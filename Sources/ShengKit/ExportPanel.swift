import SwiftUI
import UniformTypeIdentifiers

@MainActor final class ExportSelection: ObservableObject {
    @Published var options = TranscriptExportOptions()
}
struct ExportAccessory: View {
    @ObservedObject var selection: ExportSelection
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Picker("导出内容", selection: $selection.options.content) {
                Text("双语").tag(TranscriptExportOptions.Content.bilingual)
                Text("原文").tag(TranscriptExportOptions.Content.original)
                Text("译文").tag(TranscriptExportOptions.Content.translation)
            }
            Toggle("包含时间戳", isOn: $selection.options.timestamps)
        }.padding(16).frame(width: 340)
    }
}
@MainActor enum ExportPanel {
    static func choose(title: String, markdown: Bool) -> (URL, TranscriptExportOptions)? {
        let selection = ExportSelection(); selection.options.markdown = markdown
        let panel = NSSavePanel(); panel.title = title
        panel.nameFieldStringValue = L10n.named("%@-字幕") + (markdown ? ".md" : ".txt")
        panel.allowedContentTypes = markdown ? [UTType(filenameExtension: "md") ?? .plainText] : [.plainText]
        panel.accessoryView = NSHostingView(rootView: ExportAccessory(selection: selection))
        guard panel.runModal() == .OK, let url = panel.url else { return nil }
        return (url, selection.options)
    }
}
extension SessionModel {
    public func exportCurrent() {
        let alert = NSAlert(); alert.messageText = L10n.text("导出当前保留字幕")
        alert.informativeText = L10n.text("仅导出当前内存中保留的字幕，最多 500 句／2 MiB。完整记录请从已保存会话导出。")
        alert.addButton(withTitle: "TXT"); alert.addButton(withTitle: "Markdown"); alert.addButton(withTitle: L10n.text("取消"))
        let response = alert.runModal()
        guard response != .alertThirdButtonReturn, let (url, options) = ExportPanel.choose(title: L10n.text("导出当前保留字幕"), markdown: response == .alertSecondButtonReturn) else { return }
        let captions = presentation.captions
        Task { do { try await SessionLibrary.shared.export(captions: captions, to: url, options: options) } catch { self.error = L10n.text("字幕导出失败。") } }
    }
}
