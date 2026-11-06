import SwiftUI

struct OverlaySurface: View {
    @ObservedObject var model: SessionModel
    @ObservedObject var controller: OverlayController
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var hovered = false
    @State private var controls = false
    @State private var readingHistory = false
    private var failed: Bool { model.snapshot?.status == "failed" || model.error != nil }
    private var hasContent: Bool { !model.presentation.captions.isEmpty || (model.snapshot?.tracks.contains { !$0.isFinal && model.presentation.shows($0) && (!$0.original.isEmpty || !$0.pendingOriginal.isEmpty) } ?? false) }
    var body: some View {
        GeometryReader { bounds in
        VStack(spacing: 0) {
            header
            if !model.overlayCollapsed {
                if failed { failure }
                else if hasContent { reader }
                else { empty }
                CaptureHealthNotice(model: model).padding(.horizontal, 20)
                diagnostics
                if readingHistory {
                    Button { readingHistory = false } label: { Label("返回实时字幕", systemImage: "arrow.down.to.line") }
                        .buttonStyle(QuietButtonStyle()).padding(.bottom, 10)
                }
            }
        }
        .frame(width: bounds.size.width, height: bounds.size.height, alignment: .top)
        .background {
            if !model.immersive || model.overlayCollapsed || failed {
                RoundedRectangle(cornerRadius: model.overlayCollapsed ? 27 : 20).fill(.ultraThinMaterial).opacity(0.45 * model.overlayOpacity)
                RoundedRectangle(cornerRadius: model.overlayCollapsed ? 27 : 20).fill(ShengStyle.canvas.opacity(model.overlayOpacity))
                RoundedRectangle(cornerRadius: model.overlayCollapsed ? 27 : 20).strokeBorder(.primary.opacity(0.12 * model.overlayOpacity))
            }
        }
        .overlay(alignment: .bottomTrailing) {
            if !model.overlayLocked && !model.overlayCollapsed && !model.immersive {
                Image(systemName: "arrow.up.left.and.arrow.down.right").font(.system(size: 9, weight: .medium)).foregroundStyle(.secondary.opacity(hovered ? 0.7 : 0.2))
                    .padding(10).contentShape(Rectangle()).help("拖动调整字幕窗口大小")
                    .gesture(DragGesture(coordinateSpace: .global).onChanged { controller.resize(by: $0.translation, ended: false) }.onEnded { controller.resize(by: $0.translation, ended: true) })
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: model.overlayCollapsed ? 27 : 20))
        }
        .onHover { hovered = $0 }
        .environment(\.locale, model.locale)
        .onChange(of: model.appearance) { _ in model.schedulePreferencesSave() }
        .onChange(of: model.fontSize) { _ in model.schedulePreferencesSave() }
    }
    private var sessionInfo: String? {
        guard model.showSessionInfo, let profile = model.selectedService else { return nil }
        let name: (String) -> String = { code in code == "original" ? L10n.text("仅原文") : model.locale.localizedString(forIdentifier: code.replacingOccurrences(of: "_", with: "-")) ?? code }
        return profile.name + " · " + name(model.sourceLanguage) + " → " + name(model.targetLanguage)
    }
    private var header: some View {
        HStack(spacing: 8) {
            if !model.overlayLocked {
                Image(systemName: "line.3.horizontal").font(.system(size: 11, weight: .medium)).foregroundStyle(.secondary)
                    .help("拖动移动字幕位置").accessibilityLabel("拖动移动字幕位置")
            }
            HStack(spacing: 7) {
                if failed { Image(systemName: "exclamationmark.circle").foregroundStyle(.orange) }
                else { ActivityPulse(active: model.isCapturing || model.isReconnecting, style: model.pulseStyle, animation: model.pulseAnimation) }
                Text(LocalizedStringKey(model.statusTitle)).lineLimit(1)
            }
            .font(.system(size: 11.5, weight: .medium)).padding(.horizontal, 10).frame(height: 28)
            .background(.primary.opacity(0.06), in: Capsule())
            if let sessionInfo, !model.overlayCollapsed {
                Text(sessionInfo).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
            }
            if model.recordAudio && (model.isCapturing || model.isPaused) { Image(systemName: "record.circle.fill").font(.system(size: 10)).foregroundStyle(.red).help("正在录制所选音源") }
            Spacer(minLength: 0)
            HStack(spacing: 4) {
                Button { Task { if model.isCapturing { await model.stop(pause: true) } else { await model.start() } } } label: {
                    Label(LocalizedStringKey(model.isCapturing ? "暂停" : model.isPaused ? "继续" : "开启"), systemImage: model.isCapturing ? "pause.fill" : "play.fill")
                }
                .buttonStyle(QuietButtonStyle()).disabled(model.isBusy || model.isReconnecting)
                if !model.overlayCollapsed {
                    Button { controls.toggle() } label: { Text("语言与显示") }.buttonStyle(QuietButtonStyle())
                        .popover(isPresented: $controls) { ScrollView { OverlayOptions(model: model).padding(18) }.frame(width: 370, height: min(540, (controller.screenHeight ?? 700) - 80)).background(ShengStyle.canvas) }
                }
                tool(model.overlayCollapsed ? "chevron.down" : "chevron.up", model.overlayCollapsed ? "展开字幕" : "收起字幕") { model.overlayCollapsed.toggle() }
                if !model.overlayCollapsed {
                    Menu {
                        Button("清空当前显示") { model.clearDisplayedCaptions(); readingHistory = false }.disabled(!hasContent)
                        Toggle("沉浸模式", isOn: $model.immersive)
                        Toggle("锁定位置", isOn: $model.overlayLocked)
                        Toggle("点击穿透", isOn: $model.clickThrough)
                        Button("阅读当前会话") { readingHistory = true }.disabled(model.presentation.captions.isEmpty)
                        Button("打开配置") { controller.openAppearanceSettings() }
                        Divider()
                        Button("停止字幕") { Task { await model.stop() } }.disabled(model.isBusy)
                        Button("隐藏浮窗") { controller.hide() }
                    } label: { Image(systemName: "ellipsis").frame(width: 28, height: 28) }
                        .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().accessibilityLabel("字幕操作")
                }
                tool("xmark", "停止并关闭字幕") { Task { await model.stop(); controller.hide() } }.disabled(model.isBusy)
            }
            .opacity(hovered || model.overlayCollapsed || failed || !model.isCapturing ? 1 : 0.6)
        }.padding(.horizontal, 12).frame(height: model.overlayCollapsed ? 54 : 44)
        // Empty space in the header moves the panel; buttons above it still take their own clicks.
        .background { if !model.overlayLocked { WindowDragArea() } }
    }
    @ViewBuilder private var diagnostics: some View {
        if model.isCapturing, let values = model.snapshot?.diagnostics {
            let api = values.compactMap(\.apiLatencyMs).max()
            let measured = values.filter { $0.translationLatencyMs != nil }.max { ($0.translationLatencyMs ?? 0) < ($1.translationLatencyMs ?? 0) }
            let recovery = values.compactMap(\.recovery).first
            let pending = values.contains { $0.translationPending }
            if api != nil || measured != nil || pending || recovery != nil {
                HStack(spacing: 10) {
                    if let api { Text(L10n.text("接口") + " \(api) ms").help("当前连接健康探测的往返耗时；不等于字幕端到端延迟") }
                    if model.targetLanguage != "original" {
                        if let recovery { Text(LocalizedStringKey(recovery.retryScheduled ? "翻译暂时受限，正在重试" : "翻译暂不可用")).foregroundStyle(.orange) }
                        else if pending { Text("正在翻译…") }
                        else if let measured, let latency = measured.translationLatencyMs { Text(L10n.text(measured.translationLatencyKind == "follow" ? "译文跟随" : "翻译请求") + " \(latency) ms") }
                    }
                    Spacer(minLength: 0)
                }.font(.system(size: 10, design: .monospaced)).foregroundStyle(.secondary).padding(.horizontal, 20).padding(.bottom, 10)
            }
        }
    }
    private var reader: some View {
        ScrollViewReader { proxy in
            ScrollView {
                CaptionDisplay(model: model, readingHistory: readingHistory)
                Color.clear.frame(height: 1).id("tail")
            }
            .background(ScrollIntentObserver { if !readingHistory { readingHistory = true } })
            .onAppear { proxy.scrollTo("tail", anchor: .bottom) }
            .onChange(of: model.snapshot?.revision) { _ in if !readingHistory { if model.animateCaptions && !reduceMotion { withAnimation(.easeOut(duration: 0.18)) { proxy.scrollTo("tail", anchor: .bottom) } } else { proxy.scrollTo("tail", anchor: .bottom) } } }
            .onChange(of: readingHistory) { reading in if !reading { proxy.scrollTo("tail", anchor: .bottom) } }
        }
    }
    private var empty: some View {
        VStack(spacing: 12) {
            Image(systemName: model.isCapturing ? "waveform" : "captions.bubble").font(.system(size: 24, weight: .ultraLight)).foregroundStyle(ShengStyle.accent)
            Text(LocalizedStringKey(model.isCapturing ? "正在聆听，字幕即将出现" : "准备好，听见每一句"))
                .font(.system(size: 16, weight: .medium))
            Text(LocalizedStringKey(model.isCapturing ? "原文与译文将在这里配对显示" : "在此开启字幕，或前往主窗口调整音源与语言"))
                .font(.system(size: 11)).foregroundStyle(.secondary)
        }.frame(maxWidth: .infinity, maxHeight: .infinity).padding(20)
    }
    private var failure: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label("字幕需要你的关注", systemImage: "exclamationmark.circle").font(.system(size: 14, weight: .medium))
            Text(model.error ?? EngineError(code: model.snapshot?.error ?? "provider_connection_failed").localizedDescription)
                .font(.system(size: 12)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack {
                Button("重试") { Task { await model.start() } }.buttonStyle(QuietButtonStyle(prominent: true)).disabled(model.active)
                if model.permissionFailure {
                    Button("打开权限设置") { model.openFailureSettings() }.buttonStyle(QuietButtonStyle())
                }
                Button("检查配置") { controller.openServiceSettings() }.buttonStyle(QuietButtonStyle())
            }
        }.padding(20).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
    }
    private func tool(_ icon: String, _ title: String, action: @escaping () -> Void) -> some View {
        Button(action: action) { Image(systemName: icon).font(.system(size: 11, weight: .medium)).frame(width: 17, height: 17) }
            .buttonStyle(QuietButtonStyle()).help(Text(LocalizedStringKey(title))).accessibilityLabel(Text(LocalizedStringKey(title)))
    }
}

private struct OverlayOptions: View {
    @ObservedObject var model: SessionModel
    private func languageBinding(source: Bool) -> Binding<String> {
        Binding(get: { source ? model.sourceLanguage : model.targetLanguage }, set: { value in Task { await model.reconfigure(profileID: model.selectedProfile, source: source ? value : model.sourceLanguage, target: source ? model.targetLanguage : value) } })
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("语言与字幕").font(.system(size: 14, weight: .semibold))
            VStack(spacing: 12) {
                SelectRow(title: "服务", selection: Binding(get: { model.selectedProfile.uuidString }, set: { value in if let id = UUID(uuidString: value) { Task { await model.selectService(id) } } }), options: model.profiles.map { ($0.id.uuidString,$0.name) })
                SelectRow(title: "输入语言", selection: languageBinding(source: true), options: model.languageOptions(source: true))
                SelectRow(title: "翻译语言", selection: languageBinding(source: false), options: model.languageOptions(source: false))
                Button { Task { await model.switchDirection() } } label: { Label("互换输入与翻译语言", systemImage: "arrow.left.arrow.right") }.buttonStyle(QuietButtonStyle()).disabled(model.swappedLanguages == nil)
            }.disabled(model.isBusy || model.isReconnecting)
            Divider()
            SelectRow(title: "显示内容", selection: $model.displayMode, options: [("bilingual","双语"),("original","原文"),("translation","译文")])
            SelectRow(title: "对齐方式", selection: $model.subtitleAlignment, options: [("left","左对齐"),("center","居中"),("right","右对齐")])
            HStack { Text("字号").font(.system(size: 12)); Slider(value: $model.fontSize, in: 14...64); Text("\(Int(model.fontSize))").font(.system(size: 11, design: .monospaced)).frame(width: 24) }
            Divider()
            SwitchRow(title: "更早显示草稿", value: $model.showDrafts)
            SwitchRow(title: "显示确认时间", value: $model.showTime)
            SwitchRow(title: "显示句间分隔线", value: $model.showDividers)
            SwitchRow(title: "沉浸模式", value: $model.immersive)
            SwitchRow(title: "锁定位置", value: $model.overlayLocked)
            Text("点击穿透后，可从菜单栏或主窗口恢复操作。")
                .font(.system(size: 10)).foregroundStyle(.secondary).lineSpacing(3)
        }.tint(ShengStyle.accent)
    }
}
