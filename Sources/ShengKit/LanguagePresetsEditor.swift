import SwiftUI

/// The language pairs a speech profile offers as one-click choices on the subtitle page.
struct LanguagePresetsEditor: View {
    @ObservedObject var model: SessionModel
    @Binding var profile: ServiceProfile
    @State private var source = ""
    @State private var target = ""
    @State private var saveFailed = false

    var body: some View {
        SettingsGroup("常用语言") {
            if saveFailed { Text("常用语言保存失败，请重试。").font(.system(size: 11)).foregroundStyle(.orange) }
            if profile.languagePresets.isEmpty {
                SettingRow("还没有常用语言", detail: "添加后可在字幕页一键切换；设为当前时使用第一组") { EmptyView() }
            }
            ForEach(Array(profile.languagePresets.enumerated()), id: \.offset) { index, pair in
                SettingRow(name(pair.source) + " → " + name(pair.target), detail: issue(pair)) {
                    AdaptiveActions {
                        if index > 0 { ActionButton("设为第一组", icon: "arrow.up.to.line") { move(index) } }
                        ActionButton("删除", icon: "trash", destructive: true) { remove(index) }
                    }
                }
            }
            SettingRow("添加一组") {
                HStack(spacing: 6) {
                    ChoiceControl(title: "输入语言", selection: $source, options: options(source: true)).frame(width: 130)
                    Image(systemName: "arrow.right").foregroundStyle(.secondary).accessibilityHidden(true)
                    ChoiceControl(title: "翻译语言", selection: $target, options: options(source: false)).frame(width: 130)
                    ActionButton("添加", icon: "plus") { add() }
                        .disabled(source.isEmpty || target.isEmpty || profile.languagePresets.contains(LanguagePair(source: source, target: target)))
                }
            }
        }
        .onAppear { source = model.sourceLanguage; target = model.targetLanguage; fitChoices() }
        .onChange(of: profile.provider) { _ in fitChoices() }
        .onChange(of: profile.speechMode) { _ in fitChoices() }
        .onChange(of: source) { _ in fitChoices() }
    }

    /// Keeps the pickers on languages the service accepts instead of showing an unmatched “请选择”.
    private func fitChoices() {
        let sources = options(source: true).map(\.0)
        if !sources.contains(source), let first = sources.first { source = first }
        let targets = options(source: false).map(\.0)
        if !targets.contains(target), let first = targets.first(where: { $0 != "original" }) ?? targets.first { target = first }
    }
    private var capabilities: ProfileCapabilities? { try? profile.capabilities(source: source, target: target) }
    private func options(source isSource: Bool) -> [(String, String)] {
        let codes = (isSource ? capabilities?.sources : capabilities?.targets) ?? []
        return codes.map { ($0, name($0)) }
    }
    private func name(_ code: String) -> String {
        if code == "auto" { return L10n.text("自动") }
        if code == "original" { return L10n.text("仅原文") }
        return model.locale.localizedString(forIdentifier: code.replacingOccurrences(of: "_", with: "-")) ?? code
    }
    private func issue(_ pair: LanguagePair) -> String? {
        (try? profile.capabilities(source: pair.source, target: pair.target).issues.isEmpty) == true ? nil : "这个服务不支持这组语言"
    }
    /// Editing the list retires the single legacy preset so a removed pair cannot come back on reload.
    /// A saved profile keeps the change at once; a new one stores it with its first save.
    private func update(_ change: (inout [LanguagePair]) -> Void) {
        var presets = profile.languagePresets
        change(&presets)
        profile.languagePresets = presets
        profile.presetSource = nil; profile.presetTarget = nil
        do { try model.saveLanguagePresets(presets, for: profile.id); saveFailed = false } catch { saveFailed = true }
    }
    private func add() { update { $0.append(LanguagePair(source: source, target: target)) } }
    private func remove(_ index: Int) { update { $0.remove(at: index) } }
    private func move(_ index: Int) { update { $0.insert($0.remove(at: index), at: 0) } }
}
