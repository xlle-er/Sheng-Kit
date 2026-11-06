import SwiftUI

/// Editor for one speech recognition profile. Translation lives in its own profiles (TranslationEditor).
struct ServiceEditor: View {
    @ObservedObject var model: SessionModel
    @Binding var profile: ServiceProfile
    @State private var previousProfile: ServiceProfile?
    @State private var edits: [String: String] = [:]
    @State private var present: [String: Bool] = [:]
    @State private var message = ""
    @State private var result = ""
    @State private var checking = false
    @State private var probeTask: Task<Void, Never>?
    @State private var deleting = false
    private var saved: ServiceProfile? { model.profiles.first { $0.id == profile.id } }
    private var hasChanges: Bool { saved != profile || !edits.isEmpty }
    private var realtime: Bool { ["alibabaCloud","openAIRealtime","azureOpenAIRealtime","googleGeminiLive","xAIRealtime"].contains(profile.provider) }
    private var supportsCurrentLanguages: Bool {
        (try? profile.combined(with: TranslationProfile()).capabilities(source: model.sourceLanguage, target: model.targetLanguage).issues.isEmpty) == true
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            header
            SettingsGroup("语音识别") {
                SettingRow("配置名称") { TextInput(title: "配置名称", text: $profile.name) }
                SettingRow("服务") { ChoiceControl(title: "语音识别", selection: $profile.provider, options: ServiceProfile.providers).frame(width: 220) }
                if realtime {
                    SettingRow("工作方式", detail: profile.speechMode == "simultaneous" ? "服务直接输出译文，不使用翻译配置" : "识别后由所选的翻译配置翻译") {
                        SegmentedChoice(title: "工作方式", selection: $profile.speechMode, options: [("asrMt","识别＋独立翻译"),("simultaneous","服务同声传译")])
                    }
                }
                if profile.offersServiceTranslation {
                    SettingRow("Qwen 模型", detail: "字幕页翻译选“识别服务自带”时使用") {
                        SegmentedChoice(title: "Qwen 模型", selection: $profile.qwenMtModel, options: [("lite","Lite"),("flash","Flash"),("plus","Plus")])
                    }
                }
                if profile.provider == "alibabaCloud" && profile.speechMode == "simultaneous" { SettingRow("业务空间 ID") { TextInput(title: "业务空间 ID", text: $profile.workspaceID) } }
                if ["tencentCloud","baiduTranslate"].contains(profile.provider) { SettingRow("App ID") { TextInput(title: "App ID", text: $profile.appID) } }
                if ["customDashScopeASR","customOpenAIASR","azureOpenAIRealtime"].contains(profile.provider) {
                    SettingRow("服务地址") { TextInput(title: "服务地址", text: $profile.endpoint, placeholder: "https://") }
                    if profile.provider == "azureOpenAIRealtime" {
                        SettingRow("翻译部署") { TextInput(title: "翻译部署", text: $profile.deployment) }
                        SettingRow("转写部署") { TextInput(title: "转写部署", text: $profile.transcriptionDeployment) }
                    } else { SettingRow("模型") { TextInput(title: "模型", text: $profile.model) } }
                }
                if profile.provider == "tencentCloud" { credential("Secret ID", "secretID"); credential("Secret Key", "secretKey") }
                else if profile.provider != "appleSpeech" { credential("API Key", "apiKey") }
            }
            LanguagePresetsEditor(model: model, profile: $profile)
            SettingsGroup("网络与识别范围") {
                if profile.provider.hasPrefix("custom") {
                    SwitchSetting(title: "声明识别语言范围", isOn: Binding(get: { profile.customSpeechLanguages != nil }, set: { profile.customSpeechLanguages = $0 ? [] : nil }))
                    if profile.customSpeechLanguages != nil {
                        SettingRow("语言代码") { TextInput(title: "语言代码", text: Binding(get: { profile.customSpeechLanguages?.joined(separator: ",") ?? "" }, set: { profile.customSpeechLanguages = $0.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty } }), placeholder: "zh,en") }
                    }
                }
                SettingRow("网络代理") { ChoiceControl(title: "语音代理", selection: $profile.proxyMode, options: proxyOptions).frame(width: 140) }
                if profile.proxyMode == "custom" { SettingRow("代理地址") { TextInput(title: "代理地址", text: $profile.proxyURL) } }
            }
            if !supportsCurrentLanguages {
                Label("这个服务不支持字幕页当前选择的语言；配置仍可保存，开启字幕前请换一组语言。", systemImage: "exclamationmark.triangle")
                    .font(.system(size: 12)).foregroundStyle(.orange).fixedSize(horizontal: false, vertical: true)
            }
            if profile.provider == "appleSpeech" { SettingsGroup("Apple 语音模型") { SpeechResourcesView(model: model) } }
            checkBar
            if !message.isEmpty { Text(message).font(.system(size: 12)).foregroundStyle(.secondary) }
        }
        .disabled(model.active || model.isPaused)
        .onAppear { reset() }
        .onChange(of: profile) { value in
            guard let previous = previousProfile, previous.id == value.id else { reset(); return }
            invalidateProbe()
            if previous.provider != value.provider || previous.endpoint != value.endpoint { clearSpeech() }
            previousProfile = value
        }
        .onDisappear { probeTask?.cancel() }
        .confirmationDialog("删除此服务配置？", isPresented: $deleting) {
            Button("删除", role: .destructive) {
                do {
                    try model.store.deleteCredentials(for: profile.id)
                    model.profiles.removeAll { $0.id == profile.id }; model.persistProfiles()
                    profile = model.profiles.first ?? ServiceProfile()
                } catch { message = L10n.text("服务配置保存失败，请重试。") }
            }
        }
    }

    private var header: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .center, spacing: 8) { title; Spacer(); actions }
            VStack(alignment: .leading, spacing: 10) { title; actions }
        }
    }
    private var title: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(profile.name.isEmpty ? L10n.text("未命名配置") : profile.name).font(.system(size: 20, weight: .semibold)).lineLimit(1)
            Text(LocalizedStringKey(saved == nil ? "新配置，尚未保存" : hasChanges ? "有未保存的修改" : profile.id == model.selectedProfile ? "字幕页正在使用这个配置" : "已保存"))
                .font(.system(size: 12)).foregroundStyle(.secondary)
        }
    }
    private var actions: some View {
        HStack(spacing: 6) {
            AdaptiveActions {
                ActionButton("复制", icon: "doc.on.doc") { profile = profile.duplicate(); reset() }.disabled(saved == nil)
                ActionButton("删除", icon: "trash", destructive: true) { deleting = true }.disabled(saved == nil)
                ActionButton("设为当前", icon: "checkmark.circle") { Task { await model.selectService(profile.id) } }
                    .disabled(saved == nil || hasChanges || profile.id == model.selectedProfile)
            }
            Button { save() } label: { Label("保存", systemImage: "square.and.arrow.down") }.buttonStyle(QuietButtonStyle(prominent: true))
        }
    }
    private var checkBar: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 10) { checkStatus; Spacer(); checkButtons }
            VStack(alignment: .leading, spacing: 8) { checkStatus; checkButtons }
        }
        .padding(12)
        .background(ShengStyle.surface, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(ShengStyle.border.opacity(0.55)))
    }
    private var checkStatus: some View {
        HStack(spacing: 8) {
            Text("连接检查").font(.system(size: 12.5, weight: .semibold))
            if checking { ProgressView().controlSize(.small) }
            if !result.isEmpty { StatusChip(text: result, tone: result.hasPrefix(L10n.text("连接正常")) ? .ok : .warning) }
        }
    }
    private var checkButtons: some View {
        HStack(spacing: 6) {
            ProviderHelp(provider: profile.provider)
            ActionButton("检查识别", icon: "waveform.badge.magnifyingglass") { probe() }.disabled(checking)
        }
    }
    private var proxyOptions: [(String,String)] { [("system","跟随系统"),("direct","直连"),("custom","自定义")] }
    private func credential(_ title: String, _ key: String) -> some View {
        CredentialEditor(title: title, value: Binding(get: { edits[key] }, set: { edits[key] = $0; invalidateProbe() }), present: present[key] == true,
                         reveal: { try model.store.credentials(for: profile.id).value(key) }, onSave: { save() }, onCheck: { probe() })
    }
    private func reset() {
        previousProfile = profile
        probeTask?.cancel(); checking = false; edits = [:]; result = ""; message = ""
        do { let c = try model.store.credentials(for: profile.id); present = Dictionary(uniqueKeysWithValues: ["apiKey","secretID","secretKey"].map { ($0, !c.value($0).isEmpty) }) }
        catch { present = [:]; message = L10n.text("凭据读取失败，请重试。") }
    }
    private func clearSpeech() { for key in ["apiKey","secretID","secretKey"] { edits[key] = ""; present[key] = false } }
    private func resolved() throws -> ServiceCredentials {
        var c = try model.store.credentials(for: profile.id)
        for (key, value) in edits { c.set(key, value) }; return c
    }
    private func invalidateProbe() { probeTask?.cancel(); checking = false; result = ""; message = "" }
    private func save() {
        do {
            try model.saveSpeechProfile(profile, credentials: resolved())
            reset(); message = L10n.text("配置已保存；点击“设为当前”切换。")
        } catch { message = error.localizedDescription }
    }
    /// Checks recognition with a language pair the service supports, so the check never fails just because of
    /// the subtitle page's current choice.
    private func probe() {
        let draft = profile.offersServiceTranslation ? profile.combined(with: nil) : profile.combined(with: TranslationProfile())
        let pair = probePair(draft)
        checking = true; result = ""
        probeTask = Task {
            let outcome: String
            do { outcome = try await ServiceProbe.check(profile: draft, source: pair.source, target: pair.target, text: false, secrets: resolved()) }
            catch { outcome = error.localizedDescription }
            guard !Task.isCancelled else { return }
            result = L10n.text("识别") + " " + outcome; checking = false
        }
    }
    private func probePair(_ draft: ServiceProfile) -> LanguagePair {
        let current = LanguagePair(source: model.sourceLanguage, target: model.targetLanguage)
        for pair in [current] + profile.languagePresets where (try? draft.capabilities(source: pair.source, target: pair.target).issues.isEmpty) == true { return pair }
        let caps = try? draft.capabilities(source: "auto", target: "zh")
        return LanguagePair(source: caps?.sources.first ?? "auto", target: caps?.targets.first { $0 != "original" } ?? "zh")
    }
}
