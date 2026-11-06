import Foundation

struct Preferences: Codable {
    var selectedProfile: UUID
    var microphoneUID: String?
    var input: AudioInput
    var sourceLanguage: String
    var targetLanguage: String
    var fontSize: Double
    var overlayOpacity: Double
    var visibleLines: Int
    var appearance: Appearance?
    var selectedTranslation: String?
}
extension SessionModel {
    func loadPreferences() {
        let file = store.root.appendingPathComponent("preferences-v1.json")
        guard FileManager.default.fileExists(atPath: file.path) else { return }
        do {
            let value = try JSONDecoder().decode(Preferences.self, from: Data(contentsOf: file))
            if profiles.contains(where: { $0.id == value.selectedProfile }) { selectedProfile = value.selectedProfile }
            microphoneUID = value.microphoneUID ?? ""
            selectedTranslation = value.selectedTranslation ?? ""
            input = value.input; sourceLanguage = value.sourceLanguage; targetLanguage = value.targetLanguage
            fontSize = min(max(value.fontSize.isFinite ? value.fontSize : 28, 14), 64)
            overlayOpacity = min(max(value.overlayOpacity.isFinite ? value.overlayOpacity : 0.85, 0), 1)
            visibleLines = min(max(value.visibleLines, 1), 10)
            if let a = value.appearance { pulseStyle = a.pulseStyle ?? "syllable"; pulseAnimation = a.pulseAnimation ?? "system"; microphoneHex = a.microphoneHex ?? "F3CE78"; customSubtitleCount = a.customSubtitleCount ?? false; animateCaptions = a.animateCaptions ?? true; showInDock = a.showInDock ?? true; subtitleAlignment = a.subtitleAlignment ?? "left"; showDividers = a.showDividers ?? false; overlayBackground = a.overlayBackground ?? "dark"; showSourceLabel = a.showSourceLabel ?? true; showSessionInfo = a.showSessionInfo ?? true; showOverlayOnLaunch = a.showOverlayOnLaunch ?? false; sessionRetentionDays = max(0, a.sessionRetentionDays ?? 0); displayMode = a.displayMode; immersive = a.immersive; clickThrough = a.clickThrough; overlayLocked = a.overlayLocked; showDrafts = a.showDrafts; showTime = a.showTime; foregroundHex = a.foregroundHex; fontName = a.fontName; theme = a.theme; uiLanguage = a.uiLanguage }
        } catch { self.error = L10n.text("偏好设置读取失败，已使用默认值。") }
    }
    static let preferencesSaveDelay: UInt64 = 300_000_000
    /// Coalesces continuous edits (sliders, several views observing the same change) into one write.
    public func schedulePreferencesSave() {
        preferencesSaveTask?.cancel()
        preferencesSaveTask = Task { [weak self] in
            do { try await Task.sleep(nanoseconds: Self.preferencesSaveDelay) } catch { return }
            self?.savePreferences()
        }
    }
    public func flushPreferences() {
        guard preferencesSaveTask != nil else { return }
        savePreferences()
    }
    public func savePreferences() {
        preferencesSaveTask?.cancel(); preferencesSaveTask = nil
        do {
            let value = Preferences(selectedProfile: selectedProfile, microphoneUID: microphoneUID, input: input, sourceLanguage: sourceLanguage, targetLanguage: targetLanguage, fontSize: fontSize, overlayOpacity: overlayOpacity, visibleLines: visibleLines, appearance: appearance, selectedTranslation: selectedTranslation)
            try store.write(JSONEncoder().encode(value), name: "preferences-v1.json")
        } catch { self.error = L10n.text("偏好设置保存失败，请重试。") }
    }
}

public struct Appearance: Codable, Equatable {
    var displayMode: String, immersive: Bool, clickThrough: Bool, overlayLocked: Bool
    var pulseStyle: String?
    var pulseAnimation: String?
    var microphoneHex: String?
    var customSubtitleCount: Bool?
    var animateCaptions: Bool?
    var showInDock: Bool?
    var subtitleAlignment: String?
    var showDividers: Bool?
    var overlayBackground: String?
    var showSourceLabel: Bool?
    var showSessionInfo: Bool?
    var showOverlayOnLaunch: Bool?
    var sessionRetentionDays: Int?
    var showDrafts: Bool, showTime: Bool, foregroundHex: String, fontName: String, theme: String, uiLanguage: String
}
extension SessionModel {
    public var appearance: Appearance { Appearance(displayMode: displayMode, immersive: immersive, clickThrough: clickThrough, overlayLocked: overlayLocked, pulseStyle: pulseStyle, pulseAnimation: pulseAnimation, microphoneHex: microphoneHex, customSubtitleCount: customSubtitleCount, animateCaptions: animateCaptions, showInDock: showInDock, subtitleAlignment: subtitleAlignment, showDividers: showDividers, overlayBackground: overlayBackground, showSourceLabel: showSourceLabel, showSessionInfo: showSessionInfo, showOverlayOnLaunch: showOverlayOnLaunch, sessionRetentionDays: sessionRetentionDays, showDrafts: showDrafts, showTime: showTime, foregroundHex: foregroundHex, fontName: fontName, theme: theme, uiLanguage: uiLanguage) }
    public func cycleDisplayMode() { displayMode = displayMode == "bilingual" ? "translation" : displayMode == "translation" ? "original" : "bilingual"; savePreferences() }
}
