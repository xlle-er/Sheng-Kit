import SwiftUI

/// Editor for one text translation profile; its key is stored under the profile's own id.
struct TranslationEditor: View {
    @ObservedObject var model: SessionModel
    @Binding var profile: TranslationProfile
    @State private var edit: String?
    @State private var present = false
    @State private var message = ""
    @State private var result = ""
    @State private var checking = false
    @State private var task: Task<Void, Never>?
    @State private var deleting = false
    @State private var pendingPreset: String?
    private let catalog = ProviderCatalog.bundled
    private var saved: TranslationProfile? { model.translationProfiles.first { $0.id == profile.id } }
    private var hasChanges: Bool { saved != profile || edit != nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            header
            SettingsGroup("文字翻译") {
                SettingRow("配置名称") { TextInput(title: "配置名称", text: $profile.name) }
                SettingRow("服务") { ChoiceControl(title: "文字翻译", selection: $profile.kind, options: TranslationProfile.kinds).frame(width: 220) }
                if profile.kind == "openAICompatible" {
                    SettingRow("服务预设", detail: "填入常见兼容服务的地址和模型") {
                        Menu("选择预设…") { ForEach(catalog.presets) { preset in Button(preset.label) { pendingPreset = preset.id } } }.menuStyle(.borderlessButton).fixedSize()
                    }
                }
                if profile.hasEndpoint { SettingRow("翻译地址") { TextInput(title: "翻译地址", text: $profile.endpoint, placeholder: "https://") } }
                if profile.hasModel { SettingRow("翻译模型") { TextInput(title: "翻译模型", text: $profile.model) } }
                if profile.kind != "apple" {
                    CredentialEditor(title: profile.needsKey ? "API Key" : "Token（可选）", value: $edit, present: present,
                                     reveal: { try model.store.credentials(for: profile.id).apiKey }, onSave: { save() }, onCheck: { probe() })
                }
            }
            SettingsGroup("网络") {
                SettingRow("网络代理") { ChoiceControl(title: "文字代理", selection: $profile.proxyMode, options: [("system","跟随系统"),("direct","直连"),("custom","自定义")]).frame(width: 140) }
                if profile.proxyMode == "custom" { SettingRow("代理地址") { TextInput(title: "代理地址", text: $profile.proxyURL) } }
            }
            if profile.kind == "apple" { SettingsGroup("Apple 翻译模型") { TranslationResourcesView(model: model) } }
            checkBar
            if !message.isEmpty { Text(message).font(.system(size: 12)).foregroundStyle(.secondary) }
        }
        .disabled(model.active || model.isPaused)
        .onAppear { reset() }
        .onChange(of: profile.id) { _ in reset() }
        .onChange(of: profile.kind) { _ in result = "" }
        .onDisappear { task?.cancel() }
        .confirmationDialog("替换当前翻译地址、模型及密钥草稿？", isPresented: Binding(get: { pendingPreset != nil }, set: { if !$0 { pendingPreset = nil } })) {
            Button("应用预设") {
                if let preset = catalog.presets.first(where: { $0.id == pendingPreset }) { profile.endpoint = preset.endpoint ?? ""; profile.model = preset.model ?? ""; edit = "" }
                pendingPreset = nil
            }
        }
        .confirmationDialog("删除此翻译配置？", isPresented: $deleting) {
            Button("删除", role: .destructive) {
                do { try model.deleteTranslationProfile(profile); profile = model.translationProfiles.first ?? TranslationProfile() }
                catch { message = L10n.text("服务配置保存失败，请重试。") }
            }
        }
    }

    private var header: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 8) { title; Spacer(); actions }
            VStack(alignment: .leading, spacing: 10) { title; actions }
        }
    }
    private var title: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(profile.name.isEmpty ? L10n.text("未命名配置") : profile.name).font(.system(size: 20, weight: .semibold)).lineLimit(1)
            Text(LocalizedStringKey(saved == nil ? "新配置，尚未保存" : hasChanges ? "有未保存的修改" : profile.id.uuidString == model.selectedTranslation ? "字幕页正在使用这个配置" : "已保存"))
                .font(.system(size: 12)).foregroundStyle(.secondary)
        }
    }
    private var actions: some View {
        HStack(spacing: 6) {
            AdaptiveActions {
                ActionButton("复制", icon: "doc.on.doc") { profile = profile.duplicate(); reset() }.disabled(saved == nil)
                ActionButton("删除", icon: "trash", destructive: true) { deleting = true }.disabled(saved == nil)
                ActionButton("设为当前", icon: "checkmark.circle") { model.selectedTranslation = profile.id.uuidString; model.savePreferences() }
                    .disabled(saved == nil || hasChanges || profile.id.uuidString == model.selectedTranslation)
            }
            Button { save() } label: { Label("保存", systemImage: "square.and.arrow.down") }.buttonStyle(QuietButtonStyle(prominent: true))
        }
    }
    private var checkBar: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 10) { status; Spacer(); buttons }
            VStack(alignment: .leading, spacing: 8) { status; buttons }
        }
        .padding(12)
        .background(ShengStyle.surface, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(ShengStyle.border.opacity(0.55)))
    }
    private var status: some View {
        HStack(spacing: 8) {
            Text("连接检查").font(.system(size: 12.5, weight: .semibold))
            if checking { ProgressView().controlSize(.small) }
            if !result.isEmpty { StatusChip(text: result, tone: result.hasPrefix(L10n.text("连接正常")) ? .ok : .warning) }
        }
    }
    private var buttons: some View {
        HStack(spacing: 6) {
            ProviderHelp(provider: "", textProvider: profile.kind, textEndpoint: profile.endpoint)
            ActionButton("检查翻译", icon: "character.bubble") { probe() }.disabled(checking)
        }
    }

    private func key() throws -> String { try edit ?? model.store.credentials(for: profile.id).apiKey }
    private func reset() {
        task?.cancel(); checking = false; edit = nil; result = ""; message = ""
        present = !((try? model.store.credentials(for: profile.id).apiKey) ?? "").isEmpty
    }
    private func save() {
        do { try model.saveTranslationProfile(profile, key: key()); reset(); message = L10n.text("配置已保存；点击“设为当前”切换。") }
        catch { message = error.localizedDescription }
    }
    /// Translation is checked on its own, paired with on-device recognition that needs no key.
    private func probe() {
        let draft = ServiceProfile().combined(with: profile)
        let source = model.sourceLanguage == "auto" ? "en" : model.sourceLanguage
        let target = model.targetLanguage == "original" ? "zh" : model.targetLanguage
        checking = true; result = ""
        task = Task {
            let outcome: String
            do {
                var secrets = ServiceCredentials(); secrets.textKey = try key()
                outcome = try await ServiceProbe.check(profile: draft, source: source, target: target, text: true, secrets: secrets)
            } catch { outcome = error.localizedDescription }
            guard !Task.isCancelled else { return }
            result = L10n.text("翻译") + " " + outcome; checking = false
        }
    }
}
