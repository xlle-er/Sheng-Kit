import SwiftUI
import ScreenCaptureKit
import OSLog

@MainActor
public final class SessionModel: ObservableObject {
    @Published public var shortcuts = ShortcutPreferences()
    @Published public var guideCompleted = false
    public var onShortcutsChange: ((ShortcutPreferences) throws -> Void)?
    public let product: Product
    public let engine = Engine()
    public let store: PrivateStore
    @Published var appleResources: AppleResourceReport?
    let healthTracker = CaptureHealthTracker()
    @Published public var captureHealth: [CaptureHealthSnapshot] = []
    @Published public var profiles: [ServiceProfile] = []
    @Published public var translationProfiles: [TranslationProfile] = []
    /// A translation profile id, or `serviceTranslationSelection` for the speech service's own translation.
    @Published public var selectedTranslation = ""
    @Published public var selectedProfile = UUID()
    @Published public var input: AudioInput = .system
    @Published public var microphoneUID: String = ""
    @Published public var systemProcessID: Int32 = 0
    @Published var runningApplications: [RunningAudioApplication] = []
    @Published public var applications: [SCRunningApplication] = []
    @Published public var microphones: [MicrophoneDevice] = AudioDevices.microphones()
    @Published public var sourceLanguage = "zh"
    @Published public var targetLanguage = "en"
    @Published public var saveHistory = false
    @Published public var activeSessionID: String?
    @Published public var recordAudio = false
    @Published var presentation = CaptionPresentation()
    @Published public var subtitleAlignment = "left"
    @Published public var showDividers = false
    @Published public var microphoneHex = "F3CE78"
    @Published public var customSubtitleCount = false
    @Published public var animateCaptions = true
    @Published public var pulseStyle = "syllable"
    @Published public var pulseAnimation = "system"
    @Published public var showInDock = true
    @Published public var overlayCollapsed = false
    @Published public var overlayBackground = "dark"
    @Published public var showSourceLabel = true
    @Published public var showSessionInfo = true
    @Published public var showOverlayOnLaunch = false
    @Published public var sessionRetentionDays = 0
    public var onToggleOverlay: (() -> Void)?
    @Published public var snapshot: EngineSnapshot?
    @Published public var error: String?
    @Published var failureCode: String?
    var permissionFailure: Bool { failureCode == "system_audio_permission_denied" || failureCode == "screen_capture_permission_denied" || failureCode == "microphone_permission_denied" }
    func openFailureSettings() { CaptureFailure.openPermissions(microphone: failureCode == "microphone_permission_denied") }
    @Published public var isBusy = false
    @Published public var isPaused = false
    @Published public var isReconnecting = false
    var captureIntent = false
    var retryAttempt: UInt32 = 0
    var recoveryTask: Task<Void, Never>?
    @Published public var isCapturing = false
    @Published public var fontSize = 28.0
    @Published public var overlayOpacity = 0.85
    @Published public var visibleLines = 3
    @Published public var displayMode = "bilingual"
    @Published public var immersive = false
    @Published public var clickThrough = false
    @Published public var overlayLocked = false
    @Published public var showDrafts = true
    @Published public var showTime = false
    @Published public var foregroundHex = ""
    @Published public var fontName = ""
    @Published public var theme = "system"
    @Published public var uiLanguage = "system" { didSet { L10n.use(uiLanguage) } }
    public var colorScheme: ColorScheme? { theme == "dark" ? .dark : theme == "light" ? .light : nil }
    public var locale: Locale { Locale(identifier: uiLanguage == "system" ? (Bundle.main.preferredLocalizations.first ?? "zh-Hans") : uiLanguage) }
    public var onCaptureStarted: (() -> Void)?
    public var onCaption: ((Caption) -> Void)?
    var capture: AudioCapture?
    var archive: SessionArchive?
    let lease = CaptureLease()
    var timer: Timer?
    var lastHealthPublish = Date.distantPast
    var preferencesSaveTask: Task<Void, Never>?
    /// Engine polling runs only while a session may produce state; idle windows stay quiet.
    var isPolling: Bool { timer != nil }
    var epoch = 0
    var lastEchoReport = Date.distantPast
    var isStopping = false
    public init(product: Product, store: PrivateStore) {
        self.product = product; self.store = store
        L10n.use(product: product)
        do { profiles = try store.loadProfiles(); selectedProfile = profiles.first?.id ?? UUID() }
        catch { self.error = L10n.text("配置读取失败，请重试。") }
        loadPreferences()
        loadTranslationProfiles()
        L10n.use(uiLanguage)
        if let data = try? Data(contentsOf: store.root.appendingPathComponent("shortcuts-v1.json")), let value = try? JSONDecoder().decode(ShortcutPreferences.self, from: data) { shortcuts = value }
        guideCompleted = (try? Data(contentsOf: store.root.appendingPathComponent("guide-v1.json"))) == Data("true".utf8)
    }
    public var active: Bool { isReconnecting || isBusy || isCapturing || snapshot?.status == "connecting" || snapshot?.status == "stopping" }
    let lifecycleLog = Logger(subsystem: L10n.currentProduct.bundleID, category: "SessionLifecycle")
    public var statusTitle: String {
        if isStopping { return L10n.text("正在停止字幕") }
        if isBusy { return L10n.text("正在准备字幕") }
        if isReconnecting { return L10n.text("连接中断，正在重连") }
        if isPaused { return L10n.text("已暂停") }
        if isCapturing { return L10n.text("字幕进行中") }
        switch snapshot?.status { case "connecting": return L10n.text("正在连接"); case "stopping": return L10n.text("正在完成当前句"); case "failed": return L10n.text("会话已停止"); default: return L10n.text("就绪") }
    }
    public func refreshDevices() async {
        microphones = AudioDevices.microphones()
        if #available(macOS 14.2, *) {
            applications = []
            runningApplications = NSWorkspace.shared.runningApplications.filter { $0.processIdentifier != ProcessInfo.processInfo.processIdentifier && $0.activationPolicy == .regular }.map { RunningAudioApplication(id: $0.processIdentifier, name: $0.localizedName ?? "应用") }.sorted { $0.name < $1.name }
            return
        }
        do { applications = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true).applications.filter { $0.processID != ProcessInfo.processInfo.processIdentifier }.sorted { $0.applicationName < $1.applicationName } }
        catch { self.error = L10n.text("无法读取可采集应用，请检查系统录屏权限。") }
    }
    public func persistProfiles() {
        do { try store.saveProfiles(profiles) } catch { self.error = L10n.text("服务配置保存失败，请重试。") }
    }
    public func clearDisplayedCaptions() { presentation.clear(snapshot: snapshot) }
    public func revealSessions() { NSWorkspace.shared.open(store.root.appendingPathComponent("Sessions")) }
}
