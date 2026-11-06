import SwiftUI

/// The subtitle control page: audio source, languages and service as three steps, then start.
struct LivePage: View {
    @ObservedObject var model: SessionModel
    @ObservedObject var overlay: OverlayController
    @ObservedObject var navigation: WorkspaceNavigation
    private var locked: Bool { model.active || model.isPaused }

    // The page scrolls so a window restored smaller than the content never clips the header or the start bar.
    var body: some View {
        ScrollView { content.padding(24) }
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: 14) {
            PageHeader("开始字幕", subtitle: "确认下面三项，开启后字幕会显示在浮窗中") {
                HStack(spacing: 14) {
                    HStack(spacing: 6) {
                        Circle().fill(model.isCapturing ? Color.green : Color.secondary.opacity(0.6)).frame(width: 7, height: 7)
                        Text(LocalizedStringKey(model.statusTitle)).font(.system(size: 12)).foregroundStyle(.secondary)
                    }
                    Button { overlay.toggle(model: model) } label: {
                        Label(LocalizedStringKey(overlay.visible ? "隐藏浮窗" : "显示浮窗"), systemImage: "rectangle.on.rectangle")
                    }.buttonStyle(BorderedButtonStyle())
                }
            }
            HStack(alignment: .top, spacing: 12) {
                StepCard(number: 1, title: "音源") { AudioStep(model: model, locked: locked) }
                StepCard(number: 2, title: "语言") { LanguageStep(model: model, navigation: navigation, locked: locked) }
                StepCard(number: 3, title: "服务") { ServiceStep(model: model, navigation: navigation, locked: locked) }
            }
            .fixedSize(horizontal: false, vertical: true)
            SessionActionBar(model: model)
            if hasCaptions { currentSession } else { OverlayPreviewCard(model: model, navigation: navigation) }
        }
    }

    private var hasCaptions: Bool {
        !model.presentation.captions.isEmpty || (model.snapshot?.tracks.contains { !$0.isFinal && model.presentation.shows($0) && (!$0.original.isEmpty || !$0.pendingOriginal.isEmpty) } ?? false)
    }

    private var currentSession: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("当前会话").font(.system(size: 12, weight: .semibold))
                if !model.saveHistory {
                    Text("未开启保存，仅保留最近 500 句").font(.system(size: 11)).foregroundStyle(.secondary)
                }
                Spacer()
                Button { model.exportCurrent() } label: { Label("导出", systemImage: "square.and.arrow.up") }
                    .buttonStyle(BorderedButtonStyle())
            }
            ScrollViewReader { proxy in
                ScrollView {
                    CaptionDisplay(model: model)
                    Color.clear.frame(height: 1).id("latest")
                }
                .onChange(of: model.snapshot?.revision) { _ in if model.isCapturing { proxy.scrollTo("latest", anchor: .bottom) } }
            }
            .background(ShengStyle.surface, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(ShengStyle.border.opacity(0.5)))
            .frame(height: 320)
        }
    }
}

private struct StepCard<Content: View>: View {
    let number: Int
    let title: String
    @ViewBuilder let content: () -> Content
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Text("\(number)").font(.system(size: 11, weight: .semibold)).foregroundStyle(ShengStyle.accent)
                    .frame(width: 20, height: 20).background(ShengStyle.accent.opacity(0.12), in: Circle())
                Text(LocalizedStringKey(title)).font(.system(size: 13, weight: .semibold))
            }
            content()
        }
        .padding(14)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(ShengStyle.surface, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(ShengStyle.border.opacity(0.55)))
    }
}

private struct FieldLabel: View {
    let text: String
    var body: some View { Text(LocalizedStringKey(text)).font(.system(size: 11)).foregroundStyle(.secondary) }
}

private struct AudioStep: View {
    @ObservedObject var model: SessionModel
    let locked: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            SegmentedChoice(title: "音源", selection: Binding(get: { model.input.rawValue }, set: { model.input = AudioInput(rawValue: $0) ?? .system }),
                            options: [("system", "系统声音"), ("microphone", "麦克风"), ("both", "双音源")])
                .disabled(locked)
            FieldLabel(text: "采集范围")
            ChoiceControl(title: "采集范围", selection: Binding(get: { String(model.systemProcessID) }, set: { model.systemProcessID = Int32($0) ?? 0 }), options: applications)
                .disabled(locked || model.input == .microphone)
            FieldLabel(text: "麦克风")
            ChoiceControl(title: "麦克风", selection: $model.microphoneUID, options: microphones)
                .disabled(locked || model.input == .system)
            HStack {
                health
                Spacer()
                Button { Task { await model.refreshDevices() } } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.borderless).help("刷新设备与应用").accessibilityLabel("刷新设备与应用").disabled(locked)
            }
        }
    }
    private var applications: [(String, String)] {
        [("0", "所有应用")] + model.runningApplications.map { (String($0.id), $0.name) } + model.applications.map { (String($0.processID), $0.applicationName) }
    }
    private var microphones: [(String, String)] {
        let missing = !model.microphoneUID.isEmpty && !model.microphones.contains { $0.id == model.microphoneUID }
        return [("", "系统默认")] + model.microphones.map { ($0.id, $0.name) } + (missing ? [(model.microphoneUID, "设备不可用")] : [])
    }
    @ViewBuilder private var health: some View {
        if model.isCapturing, !model.captureHealth.isEmpty {
            VStack(alignment: .leading, spacing: 2) {
                ForEach(model.captureHealth) { lane in
                    Text(lane.source.title + "：" + L10n.text(Self.describe(lane.state)))
                        .foregroundStyle(lane.notice == nil ? Color.secondary : Color.orange)
                }
            }.font(.system(size: 11))
        } else {
            Text(LocalizedStringKey(model.input == .both ? "系统声音会用来抑制麦克风里的外放回声" : "开启后在此显示是否收到声音"))
                .font(.system(size: 11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
    }
    static func describe(_ state: String) -> String {
        switch state {
        case "signal": return "正在收到声音"
        case "receiving": return "已连接，暂无明显声音"
        case "quiet": return "声音较低"
        case "noPCM": return "未收到音频"
        case "failed": return "采集中断"
        default: return "等待音频…"
        }
    }
}

private struct LanguageStep: View {
    @ObservedObject var model: SessionModel
    @ObservedObject var navigation: WorkspaceNavigation
    let locked: Bool
    /// `true` shows the profile's saved pairs; `false` shows the two free language pickers.
    @State private var usesPreset: Bool?
    /// Language choices come from the selected service's capabilities; without one there is nothing to list.
    private var noService: Bool { model.selectedService == nil }
    private var presets: [LanguagePair] { model.selectedService?.languagePresets ?? [] }
    private var currentPreset: LanguagePair? { presets.first { $0.source == model.sourceLanguage && $0.target == model.targetLanguage } }
    private var showingPresets: Bool { !presets.isEmpty && (usesPreset ?? (currentPreset != nil)) }

    private func short(_ code: String) -> String {
        if code == "auto" { return L10n.text("自动") }
        if code == "original" { return L10n.text("仅原文") }
        return model.locale.localizedString(forIdentifier: code.replacingOccurrences(of: "_", with: "-")) ?? code
    }
    private func key(_ pair: LanguagePair) -> String { pair.source + ">" + pair.target }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !presets.isEmpty {
                SegmentedChoice(title: "语言方式", selection: Binding(get: { showingPresets ? "preset" : "custom" }, set: { usesPreset = $0 == "preset" }),
                                options: [("preset", "常用"), ("custom", "自定义")])
            }
            if showingPresets { presetPicker } else { customPickers }
            if presets.isEmpty && !noService {
                Button("添加常用语言，一键切换 →") { navigation.page = "services" }
                    .buttonStyle(.borderless).font(.system(size: 11)).foregroundStyle(ShengStyle.accent)
            }
            FieldLabel(text: "显示")
            SegmentedChoice(title: "显示内容", selection: $model.displayMode, options: [("bilingual", "双语"), ("translation", "仅译文"), ("original", "仅原文")])
        }
        .onChange(of: model.selectedProfile) { _ in usesPreset = nil }
    }

    private var presetPicker: some View {
        VStack(alignment: .leading, spacing: 8) {
            FieldLabel(text: "常用语言")
            ChoiceControl(title: "常用语言", selection: Binding(get: { currentPreset.map(key) ?? "" }, set: { value in
                guard let pair = presets.first(where: { key($0) == value }) else { return }
                Task { await model.reconfigure(profileID: model.selectedProfile, source: pair.source, target: pair.target) }
            }), options: presets.map { (key($0), short($0.source) + " → " + short($0.target)) })
            .disabled(model.isBusy || model.isReconnecting)
        }
    }

    private var customPickers: some View {
        VStack(alignment: .leading, spacing: 8) {
            FieldLabel(text: "听到的语言")
            ChoiceControl(title: "输入语言", selection: $model.sourceLanguage, options: model.languageOptions(source: true)).disabled(locked || noService)
            HStack {
                FieldLabel(text: "翻译成")
                Spacer()
                Button { Task { await model.switchDirection() } } label: { Label("互换语言", systemImage: "arrow.up.arrow.down").font(.system(size: 11)) }
                    .buttonStyle(.borderless).disabled(model.isBusy || model.isReconnecting || model.swappedLanguages == nil)
            }
            ChoiceControl(title: "翻译语言", selection: $model.targetLanguage, options: model.languageOptions(source: false)).disabled(locked || noService)
        }
    }
}

private struct ServiceStep: View {
    @ObservedObject var model: SessionModel
    @ObservedObject var navigation: WorkspaceNavigation
    let locked: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if model.profiles.isEmpty {
                Text("还没有服务配置").font(.system(size: 12)).foregroundStyle(.secondary)
                Button("添加服务配置") { navigation.page = "services" }.buttonStyle(QuietButtonStyle(prominent: true))
            } else {
                FieldLabel(text: "识别")
                ChoiceControl(title: "识别", selection: Binding(get: { model.selectedProfile.uuidString }, set: { value in
                    if let id = UUID(uuidString: value) { Task { await model.selectService(id) } }
                }), options: model.profiles.map { ($0.id.uuidString, $0.name) }).disabled(locked)
                FieldLabel(text: "翻译")
                if let speech = model.selectedService, !speech.usesIndependentText {
                    Text("服务同声传译，无需翻译配置").font(.system(size: 11.5)).foregroundStyle(.secondary).frame(height: 28, alignment: .leading)
                } else {
                    ChoiceControl(title: "翻译", selection: $model.selectedTranslation, options: translationOptions).disabled(locked)
                }
                if let speech = model.selectedService {
                    let readiness = model.readiness(of: speech)
                    StatusChip(text: readiness.title, tone: readiness == .ready ? .ok : .warning)
                }
                AdaptiveActions {
                    ActionButton("编辑识别", icon: "waveform") { navigation.page = "services" }
                    ActionButton("编辑翻译", icon: "character.bubble") { navigation.page = "translation" }
                }
            }
        }
    }
    private var translationOptions: [(String, String)] {
        let own = model.selectedService?.offersServiceTranslation == true ? [(serviceTranslationSelection, "识别服务自带")] : []
        return own + model.translationProfiles.map { ($0.id.uuidString, $0.name) }
    }
}

private struct SessionActionBar: View {
    @ObservedObject var model: SessionModel
    var body: some View {
        HStack(spacing: 18) {
            labeledSwitch("保存字幕", isOn: $model.saveHistory)
            labeledSwitch("录制音频", isOn: $model.recordAudio)
            Text(LocalizedStringKey(model.saveHistory || model.recordAudio ? "保存在本机，可在“会话记录”中查看" : "默认关闭；未保存时只保留最近 500 句"))
                .font(.system(size: 11.5)).foregroundStyle(.secondary).lineLimit(2)
            Spacer(minLength: 8)
            if model.isCapturing {
                Button { Task { await model.stop(pause: true) } } label: { Label("暂停", systemImage: "pause.fill") }.buttonStyle(BorderedButtonStyle())
            }
            if model.active || model.isPaused {
                Button { Task { await model.stop() } } label: { Label("停止", systemImage: "stop.fill") }.buttonStyle(BorderedButtonStyle())
            }
            if !model.isCapturing { startButton }
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
        .background(ShengStyle.surface, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(ShengStyle.border.opacity(0.55)))
    }
    private var startButton: some View {
        Button { Task { await model.start() } } label: {
            HStack(spacing: 9) {
                if model.isBusy || model.isReconnecting { ProgressView().controlSize(.small).tint(.white) }
                else { Image(systemName: "play.fill") }
                Text(LocalizedStringKey(model.isPaused ? "继续字幕" : "开启字幕"))
                if let shortcut = model.shortcuts.session {
                    Text(shortcut.symbols.joined()).font(.system(size: 10.5, weight: .medium))
                        .padding(.horizontal, 5).padding(.vertical, 1).background(.white.opacity(0.22), in: RoundedRectangle(cornerRadius: 4))
                }
            }
            .font(.system(size: 13, weight: .semibold)).foregroundStyle(.white)
            .padding(.horizontal, 16).frame(height: 34)
            .background(ShengStyle.button, in: RoundedRectangle(cornerRadius: 9))
        }
        .buttonStyle(.plain).disabled(model.active)
    }
    private func labeledSwitch(_ title: String, isOn: Binding<Bool>) -> some View {
        HStack(spacing: 8) {
            ToggleSwitch(title: title, isOn: isOn)
            Text(LocalizedStringKey(title)).font(.system(size: 12.5))
        }.disabled(model.active || model.isPaused)
    }
}

private struct OverlayPreviewCard: View {
    @ObservedObject var model: SessionModel
    @ObservedObject var navigation: WorkspaceNavigation
    var body: some View {
        HStack(alignment: .top, spacing: 16) {
            VStack(alignment: .leading, spacing: 6) {
                Text("浮窗预览").font(.system(size: 13, weight: .semibold))
                Text("按当前样式显示，文字为示例").font(.system(size: 11.5)).foregroundStyle(.secondary)
                Spacer()
                Button("调整字幕样式 →") { navigation.page = "appearance" }.buttonStyle(.borderless).foregroundStyle(ShengStyle.accent)
            }.frame(width: 190, alignment: .leading)
            OverlaySample(model: model).frame(maxWidth: .infinity)
        }
        .padding(14)
        .frame(minHeight: 260)
        .background(ShengStyle.surface, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(ShengStyle.border.opacity(0.55)))
    }
}
