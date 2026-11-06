import SwiftUI

public struct SessionView: View {
    @ObservedObject private var model: SessionModel
    @ObservedObject private var overlay: OverlayController
    @ObservedObject private var navigation: WorkspaceNavigation
    private var page: String { navigation.page }
    @State private var showingGuide = false
    @State private var serviceDraft = ServiceProfile()
    @State private var loadedDraft = false
    @State private var launched = false
    @Environment(\.openWindow) private var openWindow
    public init(model: SessionModel, navigation: WorkspaceNavigation, overlay: OverlayController) {
        self.model = model; self.navigation = navigation; self.overlay = overlay
    }

    public var body: some View {
        HStack(spacing: 0) {
            sidebar
            Divider()
            content.frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .background(ShengStyle.canvas)
        .tint(ShengStyle.accent)
        .frame(minWidth: 900, minHeight: 620)
        .alert("操作未完成", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) {
            if model.permissionFailure {
                Button("打开权限设置") { model.openFailureSettings() }
            }
            Button("好", role: .cancel) { model.error = nil }
        } message: { Text(model.error ?? "") }
        .sheet(isPresented: $showingGuide) { GettingStartedView { showingGuide = false; model.completeGuide() } }
        .onAppear(perform: appear)
        .onChange(of: model.showInDock) { show in NSApp.setActivationPolicy(show ? .regular : .accessory) }
        .onChange(of: model.input) { _ in model.recordAudio = false; model.schedulePreferencesSave() }
        .onChange(of: model.microphoneUID) { _ in model.recordAudio = false; model.schedulePreferencesSave() }
        .onChange(of: model.systemProcessID) { _ in model.recordAudio = false }
        .onChange(of: model.selectedProfile) { _ in model.schedulePreferencesSave() }
        .onChange(of: model.selectedTranslation) { _ in model.schedulePreferencesSave() }
        .onChange(of: model.sourceLanguage) { _ in model.schedulePreferencesSave() }
        .onChange(of: model.targetLanguage) { _ in model.schedulePreferencesSave() }
        .onChange(of: model.appearance) { _ in model.schedulePreferencesSave() }
        .onChange(of: model.fontSize) { _ in model.schedulePreferencesSave() }
        .onChange(of: model.overlayOpacity) { _ in model.schedulePreferencesSave() }
        .onChange(of: model.visibleLines) { _ in model.schedulePreferencesSave() }
    }

    @ViewBuilder private var content: some View {
        switch page {
        case "history": ArchiveBrowser(model: model, openLive: { navigation.page = "live" })
        case "services": ServicesPage(model: model, editing: $serviceDraft)
        case "translation": TranslationPage(model: model)
        case "appearance": AppearancePage(model: model, overlay: overlay)
        case "general": GeneralPage(model: model, showGuide: $showingGuide)
        case "diagnostics": DiagnosticsPage(model: model, showGuide: $showingGuide)
        default: LivePage(model: model, overlay: overlay, navigation: navigation)
        }
    }

    private func appear() {
        model.updater.checkIfDue()
        showingGuide = !model.guideCompleted
        if !loadedDraft { serviceDraft = model.selectedService ?? ServiceProfile(); loadedDraft = true }
        let opener = openWindow
        overlay.openConfiguration = { [weak destination = navigation] page in
            guard let destination else { return }
            destination.page = page
            if !destination.revealMainWindow() { opener(id: "main") }
        }
        model.onToggleOverlay = { [weak model, weak overlay] in
            guard let model, let overlay else { return }
            overlay.toggle(model: model)
        }
        if !launched {
            launched = true
            if model.showOverlayOnLaunch { overlay.show(model: model) }
            model.applySessionRetention()
        }
    }

    /// The app's own sidebar mark (`SidebarMark` in its bundle, 3:2); other apps fall back to their icon.
    @ViewBuilder private var brandMark: some View {
        if let mark = Bundle.main.image(forResource: "SidebarMark") {
            Image(nsImage: mark).resizable().interpolation(.high).aspectRatio(contentMode: .fit).frame(width: 36, height: 24).accessibilityHidden(true)
        } else {
            Image(nsImage: NSApp.applicationIconImage).resizable().interpolation(.high).frame(width: 30, height: 30).accessibilityHidden(true)
        }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 9) {
                brandMark
                Text(model.product.name).font(.system(size: 15, weight: .semibold))
            }.padding(.horizontal, 8).padding(.top, 16).padding(.bottom, 18)
            nav("live", "字幕控制", "captions.bubble")
            nav("services", "语音识别", "waveform")
            nav("translation", "文字翻译", "character.bubble")
            nav("appearance", "字幕与浮窗", "slider.horizontal.3")
            nav("history", "会话记录", "clock.arrow.circlepath")
            Spacer(minLength: 0)
            UpdateBadge(updater: model.updater) { navigation.page = "general" }
            nav("general", "通用", "gearshape")
            nav("diagnostics", "诊断与帮助", "stethoscope")
        }
        .padding(.horizontal, 10).padding(.bottom, 12)
        .frame(width: 200)
        .background(ShengStyle.sidebar)
    }

    private func nav(_ id: String, _ title: String, _ icon: String) -> some View {
        let selected = page == id
        return Button { navigation.page = id } label: {
            HStack(spacing: 10) { Image(systemName: icon).frame(width: 18); Text(LocalizedStringKey(title)); Spacer() }
                .font(.system(size: 13, weight: selected ? .semibold : .regular))
                .foregroundStyle(selected ? ShengStyle.accent : Color.primary.opacity(0.8))
                .padding(.horizontal, 10).frame(height: 34)
                .background(selected ? ShengStyle.accent.opacity(0.12) : .clear, in: RoundedRectangle(cornerRadius: 7))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

public enum LanguageCatalog {
    private static let codes: [String: [String]] = Engine.languageCatalog()
    private static func names(_ codes: [String]) -> [(String, String)] {
        let locale = Locale(identifier: "zh-Hans")
        return codes.filter { $0 != "original" }.map { code in
            if code == "auto" { return (code, "自动（服务支持时）") }
            return (code, locale.localizedString(forIdentifier: code.replacingOccurrences(of: "_", with: "-")) ?? code)
        }
    }
    public static let sources = names(codes["sources"] ?? ["zh", "en"])
    public static let targets = names(codes["targets"] ?? ["zh", "en"])
    /// Target language codes without the “original only” choice, for pickers that name languages themselves.
    public static let targetCodes = (codes["targets"] ?? ["zh", "en"]).filter { $0 != "original" }
}
