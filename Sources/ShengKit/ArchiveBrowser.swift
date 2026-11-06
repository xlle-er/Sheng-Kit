import SwiftUI

/// Saved sessions: a day-grouped list on the left, the selected session's details and transcript on the right.
public struct ArchiveBrowser: View {
    @ObservedObject private var model: SessionModel
    private let openLive: (() -> Void)?
    @State private var sessions: [SavedSession] = []
    @State private var query = ""
    @State private var selectedID: String?
    @State private var deleting: SavedSession?
    @State private var message = ""
    @State private var work: Task<Void, Never>?
    @State private var exporting = false
    @State private var exportTask: Task<Void, Never>?
    public init(model: SessionModel, openLive: (() -> Void)? = nil) { self.model = model; self.openLive = openLive }

    private var selected: SavedSession? { sessions.first { $0.id == selectedID } }

    public var body: some View {
        HStack(spacing: 0) {
            SessionListColumn(days: SessionDay.group(sessions, locale: model.locale), query: $query, selected: selectedID,
                              activeID: model.activeSessionID, refresh: { reload() }, select: { selectedID = $0 })
            Divider()
            VStack(spacing: 0) {
                detail
                if exporting || !message.isEmpty { statusBar }
            }
        }
        .background(ShengStyle.canvas)
        .onAppear { reload() }.onChange(of: query) { _ in reload(debounce: true) }
        .onChange(of: model.activeSessionID) { _ in reload() }
        .onDisappear { work?.cancel(); exportTask?.cancel() }
        .confirmationDialog("删除此会话的字幕及全部录音？", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } })) {
            Button("删除", role: .destructive) { if let item = deleting { delete(item) } }
        }
    }

    @ViewBuilder private var detail: some View {
        if let session = selected {
            SessionDetailView(session: session, locale: model.locale, isActive: session.id == model.activeSessionID, exporting: exporting,
                              rename: { rename(session, $0) }, export: { export(session, markdown: $0) },
                              exportAudio: { exportAudio(session, source: $0) }, delete: { deleting = session })
                .id(session.id)
        } else if sessions.isEmpty && query.isEmpty {
            EmptyState(icon: "clock.arrow.circlepath", title: "还没有保存的会话", detail: "在字幕控制页开启“保存字幕”或“录制所选音源”后，之后的会话会保存在这里。两者默认关闭。",
                       actionTitle: openLive == nil ? nil : "前往字幕控制", action: openLive)
        } else {
            EmptyState(icon: sessions.isEmpty ? "magnifyingglass" : "sidebar.left", title: sessions.isEmpty ? "没有找到匹配的会话" : "选择一个会话",
                       detail: sessions.isEmpty ? "换个关键词试试；搜索会查找标题、原文和译文。" : "在左侧选择会话，即可阅读字幕、播放录音或导出。")
        }
    }

    private var statusBar: some View {
        HStack(spacing: 10) {
            if exporting { ProgressView().controlSize(.small); Text("正在导出…"); Button("取消") { exportTask?.cancel() }.buttonStyle(QuietButtonStyle()) }
            else { Text(message) }
            Spacer()
        }
        .font(.system(size: 12)).foregroundStyle(.secondary).padding(.horizontal, 24).padding(.vertical, 10).background(ShengStyle.surface)
    }

    private func reload(debounce: Bool = false) {
        work?.cancel(); let text = query, root = model.store.root.appendingPathComponent("Sessions")
        work = Task {
            do {
                if debounce { try await Task.sleep(nanoseconds: 250_000_000) }
                let values = try await SessionLibrary.shared.list(root: root, query: text); try Task.checkCancellation()
                sessions = values
                if !values.contains(where: { $0.id == selectedID }) { selectedID = values.first?.id }
            }
            catch is CancellationError {} catch { message = L10n.text("会话列表读取失败。") }
        }
    }
    private func rename(_ item: SavedSession, _ title: String) {
        Task {
            do { try await SessionLibrary.shared.rename(item, title: title); message = ""; reload() }
            catch { message = L10n.text("会话标题无效或保存失败。") }
        }
    }
    private func delete(_ item: SavedSession) {
        deleting = nil
        guard item.id != model.activeSessionID else { return }
        Task {
            do { try await SessionLibrary.shared.delete(item); message = ""; reload() }
            catch { message = L10n.text("删除失败，请重试。") }
        }
    }
    private func export(_ item: SavedSession, markdown: Bool) {
        guard item.hasTranscript else { message = L10n.text("此会话未开启字幕保存。"); return }
        guard let (url, options) = ExportPanel.choose(title: L10n.text("导出完整已保存会话"), markdown: markdown) else { return }
        run { try await SessionLibrary.shared.export(item, to: url, options: options) }
    }
    private func exportAudio(_ item: SavedSession, source: AudioSource) {
        let panel = NSSavePanel(); panel.nameFieldStringValue = source.rawValue + ".wav"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        run { try await SessionLibrary.shared.exportAudio(item, source: source, to: url) }
    }
    private func run(_ operation: @escaping () async throws -> Void) {
        exporting = true; message = ""
        exportTask = Task {
            do { try await operation(); message = L10n.text("导出完成。") }
            catch is CancellationError { message = L10n.text("已取消导出。") } catch { message = L10n.text("导出失败，请检查保存位置和磁盘空间。") }
            exporting = false
        }
    }
}

/// Left column: search and the sessions grouped by day.
struct SessionListColumn: View {
    let days: [SessionDay]
    @Binding var query: String
    let selected: String?
    let activeID: String?
    let refresh: () -> Void
    let select: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("会话记录").font(.system(size: 15, weight: .semibold)).lineLimit(1)
                Spacer(minLength: 4)
                ActionButton("刷新", icon: "arrow.clockwise") { refresh() }.labelStyle(.iconOnly)
            }.padding(.horizontal, 4)
            TextField(LocalizedStringKey("搜索标题、原文或译文"), text: $query).textFieldStyle(.roundedBorder)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 6) {
                    ForEach(days) { day in
                        Text(day.title).font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary).padding(.horizontal, 4).padding(.top, 8)
                        ForEach(day.sessions) { row($0) }
                    }
                }
            }
        }
        .padding(14)
        .frame(width: 250)
        .background(ShengStyle.sidebar.opacity(0.5))
    }

    private func row(_ session: SavedSession) -> some View {
        let isSelected = session.id == selected
        return Button { select(session.id) } label: {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(session.title.isEmpty ? L10n.text("未命名会话") : session.title).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                    Spacer(minLength: 4)
                    if session.id == activeID { StatusChip(text: "正在写入", tone: .accent) }
                    else if session.saveFailed { StatusChip(text: "保存失败", tone: .warning) }
                }
                HStack(spacing: 6) {
                    Text(session.date, style: .time)
                    if session.hasTranscript { Image(systemName: "text.bubble").accessibilityLabel(Text("字幕")) }
                    if !session.audioSources.isEmpty { Image(systemName: "waveform").accessibilityLabel(Text("音频")) }
                    Spacer(minLength: 0)
                    Text(ByteCountFormatter.string(fromByteCount: session.bytes, countStyle: .file))
                }.font(.system(size: 11)).foregroundStyle(.secondary)
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(ShengStyle.surface, in: RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(isSelected ? ShengStyle.accent : ShengStyle.border.opacity(0.6), lineWidth: isSelected ? 1.5 : 1))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}
