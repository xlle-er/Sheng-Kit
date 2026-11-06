import SwiftUI
import ServiceManagement

struct GeneralPage: View {
    @ObservedObject var model: SessionModel
    @Binding var showGuide: Bool
    @State private var loginItem = SMAppService.mainApp.status
    @State private var loginMessage = ""
    @State private var usage: Int64?
    @State private var pendingRetention: Int?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                PageHeader("通用")
                SettingsGroup("界面") {
                    SettingRow("界面语言") {
                        ChoiceControl(title: "语言", selection: $model.uiLanguage, options: [("system", "跟随系统"), ("zh-Hans", "简体中文"), ("zh-Hant", "繁體中文"), ("en", "English"), ("ja", "日本語"), ("de", "Deutsch"), ("fr", "Français"), ("ko", "한국어"), ("th", "ภาษาไทย")]).frame(width: 180)
                    }
                    SettingRow("外观") { SegmentedChoice(title: "外观", selection: $model.theme, options: [("system", "跟随系统"), ("light", "浅色"), ("dark", "深色")]) }
                    SwitchSetting(title: "在 Dock 中显示", isOn: $model.showInDock)
                }
                SettingsGroup("启动") {
                    SettingRow(L10n.named("登录时打开%@"), detail: loginDetail) {
                        ToggleSwitch(title: L10n.named("登录时打开%@"), isOn: Binding(get: { loginItem == .enabled || loginItem == .requiresApproval }, set: setLoginItem))
                    }
                    SwitchSetting(title: "打开时显示浮窗", detail: "不会自动开始采音；开启字幕始终需要你手动操作", isOn: $model.showOverlayOnLaunch)
                }
                SettingsGroup("快捷键") {
                    ShortcutSetting(model: model, title: "开启／暂停字幕", keyPath: \.session)
                    ShortcutSetting(model: model, title: "切换显示内容", keyPath: \.display)
                    ShortcutSetting(model: model, title: "切换沉浸模式", keyPath: \.immersive)
                    ShortcutSetting(model: model, title: "显示／隐藏浮窗", keyPath: \.overlay)
                }
                storage
                SettingsGroup("关于") {
                    SettingRow("版本") { Text(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "—").foregroundStyle(.secondary) }
                    UpdateSettings(updater: model.updater, sessionActive: model.active)
                    SettingRow("入门指南") { Button("重新查看") { showGuide = true }.buttonStyle(BorderedButtonStyle()) }
                }
            }
            .padding(.horizontal, 26).padding(.vertical, 22)
        }
        .background(ShengStyle.canvas)
        .task { await refreshUsage() }
        .onChange(of: model.activeSessionID) { _ in Task { await refreshUsage() } }
        .confirmationDialog(L10n.named("开启后，%@每次启动会删除早于这个期限的会话记录和录音，删除后无法恢复。"), isPresented: Binding(get: { pendingRetention != nil }, set: { if !$0 { pendingRetention = nil } })) {
            Button("开启自动删除", role: .destructive) {
                if let days = pendingRetention { model.sessionRetentionDays = days; model.savePreferences() }
                pendingRetention = nil
            }
        }
    }

    private var storage: some View {
        SettingsGroup("存储") {
            SettingRow("会话记录位置", detail: "~/Library/Application Support/" + model.product.bundleID + "/Sessions") {
                Button("在访达中显示") { model.revealSessions() }.buttonStyle(BorderedButtonStyle())
            }
            SettingRow("已用空间") {
                Text(usage.map(Self.format) ?? "—").foregroundStyle(.secondary)
            }
            SettingRow("自动删除旧会话", detail: "默认从不删除") {
                ChoiceControl(title: "自动删除旧会话", selection: Binding(get: { String(model.sessionRetentionDays) }, set: { value in
                    let days = Int(value) ?? 0
                    if days == 0 { model.sessionRetentionDays = 0; model.savePreferences() } else { pendingRetention = days }
                }), options: [("0", "从不"), ("30", "30 天后"), ("90", "90 天后"), ("365", "一年后")]).frame(width: 140)
            }
        }
    }

    private var loginDetail: String? {
        if !loginMessage.isEmpty { return loginMessage }
        return loginItem == .requiresApproval ? "需要在系统设置 → 通用 → 登录项中允许" : nil
    }

    private func setLoginItem(_ on: Bool) {
        do {
            if on { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
            loginMessage = ""
        } catch {
            loginMessage = L10n.text("登录项设置失败，请在系统设置中检查。")
        }
        loginItem = SMAppService.mainApp.status
        if loginItem == .requiresApproval { SMAppService.openSystemSettingsLoginItems() }
    }

    private static func format(_ bytes: Int64) -> String {
        let formatter = ByteCountFormatter()
        formatter.countStyle = .file
        formatter.allowsNonnumericFormatting = false
        return formatter.string(fromByteCount: bytes)
    }
    private func refreshUsage() async {
        usage = try? await SessionLibrary.shared.usage(root: model.sessionsRoot)
    }
}

/// Click, then press the chord; Esc cancels. Each change is registered immediately and kept only if the system accepts it.
private struct ShortcutSetting: View {
    @ObservedObject var model: SessionModel
    let title: String
    let keyPath: WritableKeyPath<ShortcutPreferences, ShortcutBinding?>
    @State private var recording = false
    @State private var monitor: Any?
    @State private var message = ""

    var body: some View {
        SettingRow(title, detail: message.isEmpty ? nil : message) {
            HStack(spacing: 6) {
                Button { recording ? stop() : start() } label: {
                    Text(label).font(.system(size: 12, weight: .medium)).frame(minWidth: 110).frame(height: 26)
                        .foregroundStyle(recording ? ShengStyle.accent : model.shortcuts[keyPath: keyPath] == nil ? Color.secondary : Color.primary)
                        .background(recording ? ShengStyle.accent.opacity(0.1) : ShengStyle.field, in: RoundedRectangle(cornerRadius: 6))
                        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(recording ? ShengStyle.accent : ShengStyle.border.opacity(0.7)))
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Text(LocalizedStringKey(title)))
                .accessibilityValue(Text(label))
                Button("清除") { apply(nil) }.buttonStyle(BorderedButtonStyle()).disabled(model.shortcuts[keyPath: keyPath] == nil)
            }
        }
        .onDisappear(perform: stop)
    }

    private var label: String {
        if recording { return L10n.text("请按下组合键…") }
        return model.shortcuts[keyPath: keyPath]?.symbols.joined(separator: " ") ?? L10n.text("未设置")
    }

    private func start() {
        message = ""; recording = true
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            if event.keyCode == 53 { stop(); return nil }
            guard let binding = ShortcutBinding(keyCode: event.keyCode, flags: event.modifierFlags.intersection(.deviceIndependentFlagsMask)) else {
                message = L10n.text("需要包含 ⌘、⌥ 或 ⌃，并使用字母或空格键。")
                return nil
            }
            stop(); apply(binding)
            return nil
        }
    }

    private func stop() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil; recording = false
    }

    private func apply(_ binding: ShortcutBinding?) {
        var next = model.shortcuts
        next[keyPath: keyPath] = binding
        do { try model.applyShortcuts(next); message = "" }
        catch { message = L10n.text("快捷键注册失败，已保留原设置。") }
    }
}
