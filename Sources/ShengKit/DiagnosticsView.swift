import SwiftUI
import AVFoundation

public struct DiagnosticSummary: Codable {
    public let schemaVersion: Int
    public let osVersion: String
    public let architecture: String
    public let captureBackend: String
    public let microphonePermission: String
    public let service: String
    public let sessionState: String
    public let reconnecting: Bool
    public let health: [CaptureHealthSnapshot]
    public let apiLatencyMs: UInt64?
    public let translationLatencyMs: UInt64?
    public let translationPending: Bool
    public let recoveryPending: Bool
}
extension SessionModel {
    public var diagnosticSummary: DiagnosticSummary {
        let permission: String
        switch AVCaptureDevice.authorizationStatus(for: .audio) { case .authorized: permission = "authorized"; case .denied: permission = "denied"; case .restricted: permission = "restricted"; default: permission = "notDetermined" }
        let backend: String
        if input == .microphone { backend = "AVAudioEngine" } else if #available(macOS 14.2, *) { backend = "CoreAudioTap" } else { backend = "ScreenCaptureKit" }
        #if arch(arm64)
        let architecture = "arm64"
        #else
        let architecture = "x86_64"
        #endif
        let values = snapshot?.diagnostics ?? []
        return DiagnosticSummary(schemaVersion: 1, osVersion: ProcessInfo.processInfo.operatingSystemVersionString, architecture: architecture, captureBackend: backend, microphonePermission: permission, service: ServiceProfile.providers.contains(where: { $0.0 == selectedService?.provider }) ? selectedService!.provider : "none", sessionState: isPaused ? "paused" : isCapturing ? "listening" : isBusy ? "busy" : "idle", reconnecting: isReconnecting, health: captureHealth, apiLatencyMs: values.compactMap(\.apiLatencyMs).max(), translationLatencyMs: values.compactMap(\.translationLatencyMs).max(), translationPending: values.contains { $0.translationPending }, recoveryPending: values.contains { $0.recovery != nil })
    }
    public var diagnosticText: String {
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted,.sortedKeys]
        return (try? String(decoding: encoder.encode(diagnosticSummary), as: UTF8.self)) ?? "{}"
    }
}
struct CaptureHealthNotice: View {
    @ObservedObject var model: SessionModel
    var body: some View {
        VStack(alignment: .leading, spacing: 4) { if model.isCapturing { ForEach(model.captureHealth) { lane in if let notice = lane.notice { Text(lane.source.title + ": " + notice).font(.caption).foregroundStyle(.orange) } } } }
    }
}
struct DiagnosticsPage: View {
    @ObservedObject var model: SessionModel
    @Binding var showGuide: Bool
    @State private var message = ""
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                PageHeader("诊断与帮助", subtitle: "摘要只包含系统、采集和连接状态，不包含字幕、音频、密钥或个人路径，也不会自动上传")
                SettingsGroup("采集状态") {
                    if model.captureHealth.isEmpty {
                        SettingRow("音源", detail: "开启字幕后显示各音源是否收到声音") { EmptyView() }
                    }
                    ForEach(model.captureHealth) { lane in
                        SettingRow(lane.source.title, detail: lane.notice) { StatusChip(text: lane.state, tone: lane.notice == nil ? .neutral : .warning) }
                    }
                }
                SettingsGroup("诊断摘要") {
                    ScrollView([.horizontal, .vertical]) {
                        Text(model.diagnosticText).font(.system(.caption, design: .monospaced)).textSelection(.enabled).padding(.vertical, 10)
                    }.frame(height: 220)
                    SettingRow("需要支持时，请把预览过的摘要提供给维护者", detail: message.isEmpty ? nil : message) {
                        HStack(spacing: 6) {
                            Button("复制诊断摘要") { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(model.diagnosticText, forType: .string); message = L10n.text("已复制。") }
                            Button("导出诊断摘要") { export() }
                        }.buttonStyle(BorderedButtonStyle())
                    }
                }
                SettingsGroup("帮助") {
                    SettingRow("入门指南", detail: "配置服务、授权和开始字幕的步骤") { Button("查看") { showGuide = true }.buttonStyle(BorderedButtonStyle()) }
                }
            }
            .padding(.horizontal, 26).padding(.vertical, 22)
        }
        .background(ShengStyle.canvas)
    }
    private func export() {
        let panel = NSSavePanel(); panel.nameFieldStringValue = model.product.rawValue + "-diagnostics.json"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do { try model.diagnosticText.write(to: url, atomically: true, encoding: .utf8); message = L10n.text("导出完成。") } catch { message = L10n.text("诊断导出失败。") }
    }
}
struct GettingStartedView: View {
    let done: () -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text(L10n.named("开始使用%@")).font(.title2)
            Text("1. 在“语音识别”和“文字翻译”中添加配置，保存后点击“设为当前”。")
            Text("2. 在字幕控制页选择音源和语言；Apple 服务需先检查并下载语言资源。")
            Text("3. 点击“开启字幕”，按系统提示授权。系统声模式不会开启麦克风。")
            Text("4. 如需完整记录，请在开始前开启保存字幕或录音；两者默认关闭。")
            Text("5. 在会话记录中搜索、阅读、播放或导出。关闭保存和清屏不会删除已有记录。")
            Text("未保存时仅保留最近 500 句／2 MiB；字幕浮窗可独立显示、收起和隐藏。")
            Button("完成／跳过") { done() }.buttonStyle(QuietButtonStyle(prominent: true))
        }.padding(28).frame(width: 560)
    }
}
