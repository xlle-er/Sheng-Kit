import Foundation

public struct SessionMetadata: Codable, Equatable {
    public var title: String
    public init(title: String) { self.title = title }
}
public struct SavedSession: Identifiable, Equatable {
    public let id: String
    public let directory: URL
    public let date: Date
    public var title: String = ""
    public var bytes: Int64 = 0
    public var hasTranscript = false
    public var audioSources: [AudioSource] = []
    public var saveFailed = false
    public var sourceLanguage = ""
    public var targetLanguage = ""
    public var input = ""
}
/// Session list sections: today, yesterday, then one per calendar day, newest first.
struct SessionDay: Identifiable {
    let id: Date
    let title: String
    let sessions: [SavedSession]
    static func group(_ sessions: [SavedSession], now: Date = Date(), calendar: Calendar = .current, locale: Locale = .current) -> [SessionDay] {
        let days = Dictionary(grouping: sessions) { calendar.startOfDay(for: $0.date) }
        return days.keys.sorted(by: >).map { day in
            SessionDay(id: day, title: title(day, now: now, calendar: calendar, locale: locale), sessions: days[day, default: []].sorted { $0.date > $1.date })
        }
    }
    private static func title(_ day: Date, now: Date, calendar: Calendar, locale: Locale) -> String {
        if calendar.isDate(day, inSameDayAs: now) { return L10n.text("今天") }
        if let yesterday = calendar.date(byAdding: .day, value: -1, to: now), calendar.isDate(day, inSameDayAs: yesterday) { return L10n.text("昨天") }
        let sameYear = calendar.component(.year, from: day) == calendar.component(.year, from: now)
        let style = sameYear ? Date.FormatStyle.dateTime.month().day().weekday() : Date.FormatStyle.dateTime.year().month().day()
        return day.formatted(style.locale(locale))
    }
}
public struct TranscriptExportOptions: Equatable {
    public enum Content: String, CaseIterable { case bilingual, original, translation }
    public var markdown = false
    public var content: Content = .bilingual
    public var timestamps = false
    public init() {}
    func render(_ caption: Caption) -> String {
        let heading = "[\(caption.source.title)]" + (timestamps ? " \(ISO8601DateFormatter().string(from: Date(timeIntervalSince1970: Double(caption.timestampMs) / 1000)))" : "")
        let parts = [content != .translation ? caption.original : "", content != .original ? caption.translation : ""].filter { !$0.isEmpty }
        return (markdown ? "### " : "") + heading + "\n\n" + parts.joined(separator: "\n\n") + "\n\n"
    }
}
struct TranscriptSearchHit: Identifiable {
    let id: UInt64
    let caption: Caption
}
/// Streaming scanner shared by paging, search and export. Each JSONL row is bounded.
enum TranscriptLines {
    static func scan(file: URL, visit: (Caption, UInt64) throws -> Bool) throws {
        let input = try FileHandle(forReadingFrom: file); defer { try? input.close() }
        var pending = Data(), offset: UInt64 = 0
        func consume(_ line: Data) throws -> Bool {
            defer { offset += UInt64(line.count + 1) }
            guard !line.isEmpty else { return true }
            return try visit(JSONDecoder().decode(Caption.self, from: line), offset)
        }
        while true {
            try Task.checkCancellation()
            let chunk = try input.read(upToCount: 8192) ?? Data()
            pending.append(chunk)
            while let end = pending.firstIndex(of: 10) {
                let line = pending.subdata(in: pending.startIndex..<end)
                pending.removeSubrange(pending.startIndex...end)
                guard line.count < 256 * 1024 else { throw EngineError(code: "transcript_line_too_large") }
                if try !consume(line) { return }
            }
            guard pending.count < 256 * 1024 else { throw EngineError(code: "transcript_line_too_large") }
            if chunk.isEmpty { if !pending.isEmpty { _ = try consume(pending) }; return }
        }
    }
}
public enum TranscriptExport {
    public static func write(from source: URL, to destination: URL, markdown: Bool) throws {
        var options = TranscriptExportOptions(); options.markdown = markdown
        try write(from: source, to: destination, options: options)
    }
    public static func write(from source: URL, to destination: URL, options: TranscriptExportOptions) throws {
        try atomicOutput(to: destination) { output in
            try TranscriptLines.scan(file: source) { caption, _ in
                try output.write(contentsOf: Data(options.render(caption).utf8)); return true
            }
        }
    }
    static func write(captions: [Caption], to destination: URL, options: TranscriptExportOptions) throws {
        try atomicOutput(to: destination) { output in
            for caption in captions { try Task.checkCancellation(); try output.write(contentsOf: Data(options.render(caption).utf8)) }
        }
    }
    private static func atomicOutput(to destination: URL, write: (FileHandle) throws -> Void) throws {
        let temporary = destination.deletingLastPathComponent().appendingPathComponent(".sheng-export-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: temporary) }
        guard FileManager.default.createFile(atPath: temporary.path, contents: nil, attributes: [.posixPermissions: 0o600]) else { throw EngineError(code: "export_create_failed") }
        let output = try FileHandle(forWritingTo: temporary)
        do { try write(output); try output.synchronize(); try output.close() } catch { try? output.close(); throw error }
        try Task.checkCancellation()
        if FileManager.default.fileExists(atPath: destination.path) { _ = try FileManager.default.replaceItemAt(destination, withItemAt: temporary) }
        else { try FileManager.default.moveItem(at: temporary, to: destination) }
    }
}
actor SessionLibrary {
    static let shared = SessionLibrary()
    func list(root: URL, query: String = "") throws -> [SavedSession] {
        guard FileManager.default.fileExists(atPath: root.path) else { return [] }
        var result: [SavedSession] = []
        for directory in try FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: [.creationDateKey,.isDirectoryKey,.isSymbolicLinkKey]) {
            try Task.checkCancellation()
            let values = try directory.resourceValues(forKeys: [.creationDateKey,.isDirectoryKey,.isSymbolicLinkKey])
            guard values.isDirectory == true, values.isSymbolicLink != true, UUID(uuidString: directory.lastPathComponent) != nil else { continue }
            var item = SavedSession(id: directory.lastPathComponent, directory: directory, date: values.creationDate ?? .distantPast)
            let titleURL = directory.appendingPathComponent("metadata.json")
            if let data = try? boundedData(titleURL, limit: 4096), let metadata = try? JSONDecoder().decode(SessionMetadata.self, from: data) { item.title = metadata.title }
            if let data = try? boundedData(directory.appendingPathComponent("session.json"), limit: 4096), let info = try? JSONDecoder().decode([String: String].self, from: data) {
                item.sourceLanguage = info["sourceLanguage"] ?? ""; item.targetLanguage = info["targetLanguage"] ?? ""; item.input = info["input"] ?? ""
            }
            for file in try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: [.fileSizeKey,.isRegularFileKey,.isSymbolicLinkKey]) {
                let info = try file.resourceValues(forKeys: [.fileSizeKey,.isRegularFileKey,.isSymbolicLinkKey])
                guard info.isRegularFile == true, info.isSymbolicLink != true else { continue }
                item.bytes += Int64(info.fileSize ?? 0)
                if file.lastPathComponent == "captions.jsonl" { item.hasTranscript = true }
                if file.lastPathComponent == "save-failed" { item.saveFailed = true }
                if let source = AudioSource(rawValue: file.deletingPathExtension().lastPathComponent), file.pathExtension == "wav" { item.audioSources.append(source) }
            }
            if query.isEmpty || item.title.localizedStandardContains(query) || item.date.formatted().localizedStandardContains(query) { result.append(item); continue }
            if item.hasTranscript {
                var matched = false
                try TranscriptLines.scan(file: directory.appendingPathComponent("captions.jsonl")) { caption, _ in
                    matched = caption.original.localizedStandardContains(query) || caption.translation.localizedStandardContains(query); return !matched
                }
                if matched { result.append(item) }
            }
        }
        return result.sorted { $0.date > $1.date }
    }
    func rename(_ session: SavedSession, title: String) throws {
        let title = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty, title.count <= 120 else { throw EngineError(code: "invalid_session_title") }
        try JSONEncoder().encode(SessionMetadata(title: title)).write(to: session.directory.appendingPathComponent("metadata.json"), options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: session.directory.appendingPathComponent("metadata.json").path)
    }
    func delete(_ session: SavedSession) throws { try FileManager.default.removeItem(at: session.directory) }
    /// Opt-in retention: removes sessions created before the cutoff, never the one being written.
    func prune(root: URL, olderThanDays days: Int, now: Date = Date(), keeping active: String?) throws -> Int {
        guard days > 0 else { return 0 }
        let cutoff = now.addingTimeInterval(-Double(days) * 86_400)
        var removed = 0
        for session in try list(root: root) where session.date < cutoff && session.id != active {
            try delete(session); removed += 1
        }
        return removed
    }
    func usage(root: URL) throws -> Int64 { try list(root: root).reduce(0) { $0 + $1.bytes } }
    func search(_ session: SavedSession, query: String, offset: UInt64 = 0) throws -> [TranscriptSearchHit] {
        var hits: [TranscriptSearchHit] = []
        try TranscriptLines.scan(file: session.directory.appendingPathComponent("captions.jsonl")) { caption, position in
            if position >= offset, caption.original.localizedStandardContains(query) || caption.translation.localizedStandardContains(query) { hits.append(TranscriptSearchHit(id: position, caption: caption)) }
            return hits.count < 50
        }; return hits
    }
    func page(_ session: SavedSession, offset: UInt64) throws -> TranscriptPage { try TranscriptReader.page(file: session.directory.appendingPathComponent("captions.jsonl"), offset: offset) }
    func export(_ session: SavedSession, to destination: URL, options: TranscriptExportOptions) throws { try TranscriptExport.write(from: session.directory.appendingPathComponent("captions.jsonl"), to: destination, options: options) }
    func export(captions: [Caption], to destination: URL, options: TranscriptExportOptions) throws { try TranscriptExport.write(captions: captions, to: destination, options: options) }
    func exportAudio(_ session: SavedSession, source: AudioSource, to destination: URL) throws {
        let input = try FileHandle(forReadingFrom: session.directory.appendingPathComponent(source.rawValue + ".wav")); defer { try? input.close() }
        // Copy atomically through the same file protection and cleanup path.
        try copyFile(input: input, to: destination)
    }
    private func copyFile(input: FileHandle, to destination: URL) throws {
        let temporary = destination.deletingLastPathComponent().appendingPathComponent(".sheng-audio-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: temporary) }
        guard FileManager.default.createFile(atPath: temporary.path, contents: nil, attributes: [.posixPermissions: 0o600]) else { throw EngineError(code: "export_create_failed") }
        let output = try FileHandle(forWritingTo: temporary); defer { try? output.close() }
        while let data = try input.read(upToCount: 65536), !data.isEmpty { try Task.checkCancellation(); try output.write(contentsOf: data) }
        try output.synchronize(); try Task.checkCancellation()
        if FileManager.default.fileExists(atPath: destination.path) { _ = try FileManager.default.replaceItemAt(destination, withItemAt: temporary) }
        else { try FileManager.default.moveItem(at: temporary, to: destination) }
    }
    private func boundedData(_ file: URL, limit: Int) throws -> Data {
        let handle = try FileHandle(forReadingFrom: file); defer { try? handle.close() }
        let data = try handle.read(upToCount: limit + 1) ?? Data()
        guard data.count <= limit else { throw EngineError(code: "metadata_too_large") }; return data
    }
}
