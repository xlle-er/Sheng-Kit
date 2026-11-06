import SwiftUI

/// One saved session: editable title, what was captured, actions, the recording and the transcript.
struct SessionDetailView: View {
    let session: SavedSession
    let locale: Locale
    let isActive: Bool
    let exporting: Bool
    let rename: (String) -> Void
    let export: (_ markdown: Bool) -> Void
    let exportAudio: (AudioSource) -> Void
    let delete: () -> Void
    @State private var title = ""
    @FocusState private var editingTitle: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header.padding(.horizontal, 24).padding(.top, 22).padding(.bottom, 14)
            if !session.audioSources.isEmpty { SessionAudioView(session: session).padding(.horizontal, 24).padding(.bottom, 14) }
            Divider()
            if isActive {
                EmptyState(icon: "record.circle", title: "这个会话正在写入", detail: "停止字幕后即可阅读、播放和导出。")
            } else if session.hasTranscript {
                TranscriptPane(session: session, showsSource: session.input == AudioInput.both.rawValue)
            } else {
                EmptyState(icon: "waveform", title: "这个会话只保存了录音", detail: "开启“保存字幕”后，新的会话会同时保存可搜索的字幕。")
            }
        }
        .onAppear { title = session.title }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 8) {
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .firstTextBaseline, spacing: 12) { titleField; Spacer(minLength: 8); actions }
                VStack(alignment: .leading, spacing: 10) { titleField; actions }
            }
            Text(summary).font(.system(size: 12)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            if session.saveFailed { StatusChip(text: "部分内容保存失败", tone: .warning) }
        }
    }

    private var titleField: some View {
        TextField(LocalizedStringKey("未命名会话"), text: $title)
            .textFieldStyle(.plain).font(.system(size: 20, weight: .semibold))
            .focused($editingTitle)
            .onSubmit(commitTitle)
            .onChange(of: editingTitle) { focused in if !focused { commitTitle() } }
            .disabled(isActive)
            .help(Text("点击修改标题"))
            .accessibilityLabel(Text("会话标题"))
    }

    private var actions: some View {
        AdaptiveActions {
            Menu {
                Button("字幕 TXT") { export(false) }.disabled(!session.hasTranscript)
                Button("字幕 Markdown") { export(true) }.disabled(!session.hasTranscript)
                if !session.audioSources.isEmpty { Divider() }
                ForEach(session.audioSources, id: \.self) { source in Button(source.title + " WAV") { exportAudio(source) } }
            } label: { Label("导出", systemImage: "square.and.arrow.up") }
                .menuStyle(.borderlessButton).fixedSize().disabled(exporting || isActive)
            ActionButton("在访达中显示", icon: "folder") { NSWorkspace.shared.open(session.directory) }
            ActionButton("删除", icon: "trash", destructive: true) { delete() }.disabled(isActive)
        }
    }

    private func commitTitle() {
        let trimmed = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed != session.title else { return }
        if trimmed.isEmpty { title = session.title; return }
        rename(trimmed)
    }

    /// "Nov 6, 2026 at 10:50 · Chinese → English · System audio · 2.1 MB"
    private var summary: String {
        var parts = [session.date.formatted(Date.FormatStyle(date: .long, time: .shortened).locale(locale))]
        if !session.sourceLanguage.isEmpty { parts.append(name(session.sourceLanguage) + " → " + name(session.targetLanguage)) }
        if let input = AudioInput(rawValue: session.input) { parts.append(input.title) }
        parts.append(ByteCountFormatter.string(fromByteCount: session.bytes, countStyle: .file))
        return parts.joined(separator: " · ")
    }
    private func name(_ code: String) -> String {
        if code == "auto" { return L10n.text("自动") }
        if code == "original" || code.isEmpty { return L10n.text("仅原文") }
        return locale.localizedString(forIdentifier: code.replacingOccurrences(of: "_", with: "-")) ?? code
    }
}
