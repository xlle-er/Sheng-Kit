import SwiftUI

struct TranscriptPage {
    let captions: [Caption]
    let nextOffset: UInt64
    let hasMore: Bool
}
enum TranscriptReader {
    static func page(file: URL, offset: UInt64) throws -> TranscriptPage {
        let handle = try FileHandle(forReadingFrom: file)
        defer { try? handle.close() }
        try handle.seek(toOffset: offset)
        let size = try handle.seekToEnd(); try handle.seek(toOffset: offset)
        var pending = Data(), captions: [Caption] = [], consumed: UInt64 = 0
        var end = false
        while captions.count < 50 {
            if let newline = pending.firstIndex(of: 10) {
                let line = pending.subdata(in: pending.startIndex..<newline)
                consumed += UInt64(line.count + 1); pending.removeSubrange(pending.startIndex...newline)
                if !line.isEmpty { captions.append(try JSONDecoder().decode(Caption.self, from: line)) }
                continue
            }
            if end {
                if !pending.isEmpty { captions.append(try JSONDecoder().decode(Caption.self, from: pending)); consumed += UInt64(pending.count) }
                break
            }
            let data = try handle.read(upToCount: 8192) ?? Data()
            if data.isEmpty { end = true } else { pending.append(data) }
            guard pending.count < 256 * 1024 else { throw EngineError(code: "transcript_line_too_large") }
        }
        return TranscriptPage(captions: captions, nextOffset: offset + consumed, hasMore: offset + consumed < size)
    }
}
/// A session's transcript, loaded 50 lines at a time, with in-session search.
struct TranscriptPane: View {
    let session: SavedSession
    /// Labels each line with its audio source; only useful when both were captured.
    let showsSource: Bool
    @State private var captions: [Caption] = []
    @State private var startOffset: UInt64 = 0
    @State private var nextOffset: UInt64 = 0
    @State private var hasMore = false
    @State private var loading = false
    @State private var error: String?
    @State private var query = ""
    @State private var hits: [TranscriptSearchHit] = []
    @State private var searchTask: Task<Void, Never>?
    @State private var searchOffset: UInt64 = 0

    private var searching: Bool { !query.isEmpty }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                TextField(LocalizedStringKey("搜索此会话"), text: $query).textFieldStyle(.roundedBorder).frame(maxWidth: 320)
                if searching { Text(L10n.text("找到") + " \(hits.count)" + (hits.count >= 50 ? "+" : "")).font(.system(size: 11)).foregroundStyle(.secondary) }
                Spacer()
            }.padding(.horizontal, 24).padding(.vertical, 12)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if !searching && startOffset > 0 {
                        Button("从开头查看") { reset(from: 0) }.buttonStyle(QuietButtonStyle()).padding(.horizontal, 12).padding(.bottom, 8)
                    }
                    ForEach(searching ? hits.map(\.caption) : captions) { caption in
                        row(caption, hit: searching ? hits.first { $0.caption.id == caption.id } : nil)
                        Divider().opacity(0.5).padding(.leading, 82)
                    }
                    if loading { ProgressView().controlSize(.small).frame(maxWidth: .infinity).padding(16) }
                    if !searching && hasMore && !loading {
                        Button("加载更多") { load() }.buttonStyle(QuietButtonStyle()).frame(maxWidth: .infinity).padding(16)
                    }
                    if searching && hits.count >= 50 {
                        Button("更多结果") { searchOffset = (hits.last?.id ?? 0) + 1; search() }.buttonStyle(QuietButtonStyle()).frame(maxWidth: .infinity).padding(16)
                    }
                    if searching && hits.isEmpty && searchTask == nil {
                        Text("没有匹配的字幕").font(.system(size: 12)).foregroundStyle(.secondary).frame(maxWidth: .infinity).padding(24)
                    }
                    if let error { Text(error).font(.system(size: 12)).foregroundStyle(.orange).padding(16) }
                }.padding(.horizontal, 12).padding(.bottom, 12)
            }
        }
        .onAppear { if captions.isEmpty { load() } }
        .onChange(of: query) { _ in searchOffset = 0; search() }
        .onDisappear { searchTask?.cancel() }
    }

    private func row(_ caption: Caption, hit: TranscriptSearchHit?) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 14) {
            Text(Date(timeIntervalSince1970: Double(caption.timestampMs) / 1000), style: .time)
                .font(.system(size: 11)).monospacedDigit().foregroundStyle(.secondary).frame(width: 56, alignment: .leading)
            VStack(alignment: .leading, spacing: 5) {
                if showsSource { Label(caption.source.title, systemImage: caption.source == .system ? "speaker.wave.2" : "mic").font(.system(size: 11)).foregroundStyle(.secondary) }
                Text(caption.original).font(.system(size: 14)).textSelection(.enabled)
                if !caption.translation.isEmpty { Text(caption.translation).font(.system(size: 14)).foregroundStyle(.secondary).textSelection(.enabled) }
                if let hit { Button("在全文中定位") { query = ""; reset(from: hit.id) }.buttonStyle(.borderless).font(.system(size: 11)).foregroundStyle(ShengStyle.accent) }
            }
            .lineSpacing(4)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 12).padding(.vertical, 12)
    }

    private func reset(from offset: UInt64) { captions = []; startOffset = offset; nextOffset = offset; hasMore = false; load() }
    private func load() {
        guard !loading else { return }
        loading = true
        let offset = nextOffset
        Task {
            do {
                let page = try await SessionLibrary.shared.page(session, offset: offset)
                captions += page.captions; nextOffset = page.nextOffset; hasMore = page.hasMore; error = nil
            } catch { self.error = L10n.text("此会话无字幕，或字幕文件读取失败。") }
            loading = false
        }
    }
    private func search() {
        searchTask?.cancel(); let text = query, offset = searchOffset
        guard !text.isEmpty else { hits = []; searchTask = nil; return }
        searchTask = Task {
            do {
                try await Task.sleep(nanoseconds: 250_000_000)
                let result = try await SessionLibrary.shared.search(session, query: text, offset: offset); try Task.checkCancellation()
                hits = offset == 0 ? result : hits + result; searchTask = nil
            }
            catch is CancellationError {} catch { self.error = L10n.text("搜索失败，请重试。"); searchTask = nil }
        }
    }
}
