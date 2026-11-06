import Foundation

final class WAVWriter {
    private let handle: FileHandle
    private let rate: UInt32
    private var bytes: UInt32 = 0
    init(url: URL, rate: UInt32) throws {
        self.rate = rate
        guard FileManager.default.createFile(atPath: url.path, contents: Data(repeating: 0, count: 44), attributes: [.posixPermissions: 0o600]) else { throw EngineError(code: "recording_create_failed") }
        handle = try FileHandle(forWritingTo: url)
        try writeHeader(); try handle.seekToEnd()
    }
    func append(_ data: Data) throws {
        guard UInt64(bytes) + UInt64(data.count) < 2_000_000_000 else { throw EngineError(code: "recording_size_limit") }
        try handle.write(contentsOf: data); bytes += UInt32(data.count); try writeHeader(); try handle.seekToEnd()
    }
    private func writeHeader() throws {
        var header = Data()
        func ascii(_ s: String) { header.append(contentsOf: s.utf8) }
        func u32(_ value: UInt32) { var v = value.littleEndian; withUnsafeBytes(of: &v) { header.append(contentsOf: $0) } }
        func u16(_ value: UInt16) { var v = value.littleEndian; withUnsafeBytes(of: &v) { header.append(contentsOf: $0) } }
        ascii("RIFF"); u32(bytes + 36); ascii("WAVEfmt "); u32(16); u16(1); u16(1)
        u32(rate); u32(rate * 2); u16(2); u16(16); ascii("data"); u32(bytes)
        try handle.seek(toOffset: 0); try handle.write(contentsOf: header)
    }
    func close() throws {
        try writeHeader(); try handle.synchronize(); try handle.close()
    }
}
/// All disk I/O is serialized off the audio callback and main thread. Admission is bounded.
final class SessionArchive: @unchecked Sendable {
    let directory: URL
    private let queue = DispatchQueue(label: "app.xlle.archive", qos: .utility)
    private let slots = DispatchSemaphore(value: 64)
    private var transcript: FileHandle?
    private var waves: [AudioSource: WAVWriter] = [:]
    private var audioBytes: UInt64 = 0
    private var transcriptBytes: UInt64 = 0
    private let failure: @Sendable () -> Void
    init(store: PrivateStore, rate: UInt32, input: AudioInput, history: Bool, recording: Bool, sourceLanguage: String, targetLanguage: String, failure: @escaping @Sendable () -> Void) throws {
        self.failure = failure
        directory = store.root.appendingPathComponent("Sessions/\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let metadata = ["sourceLanguage": sourceLanguage, "targetLanguage": targetLanguage, "input": input.rawValue]
        try JSONEncoder().encode(metadata).write(to: directory.appendingPathComponent("session.json"), options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: directory.appendingPathComponent("session.json").path)
        if history {
            let file = directory.appendingPathComponent("captions.jsonl")
            guard FileManager.default.createFile(atPath: file.path, contents: nil, attributes: [.posixPermissions: 0o600]) else { throw EngineError(code: "history_create_failed") }
            transcript = try FileHandle(forWritingTo: file)
        }
        if recording {
            for lane in [AudioSource.system, .microphone] where input == .both || input.rawValue == lane.rawValue {
                waves[lane] = try WAVWriter(url: directory.appendingPathComponent("\(lane.rawValue).wav"), rate: rate)
            }
        }
    }
    private func markFailure() {
        // A numeric marker only; never persist native error descriptions or content.
        queue.async { [directory] in try? Data().write(to: directory.appendingPathComponent("save-failed"), options: .atomic) }
        failure()
    }
    private func submit(_ work: @escaping @Sendable () throws -> Void) {
        guard slots.wait(timeout: .now()) == .success else { markFailure(); return }
        queue.async { [self] in defer { slots.signal() }; do { try work() } catch { markFailure() } }
    }
    func append(_ caption: Caption) {
        submit { [self] in guard let transcript else { return }; var data = try JSONEncoder().encode(caption); data.append(10); guard transcriptBytes + UInt64(data.count) <= 100 * 1024 * 1024 else { throw EngineError(code: "history_size_limit") }; try transcript.write(contentsOf: data); transcriptBytes += UInt64(data.count) }
    }
    func append(_ data: Data, source: AudioSource) { submit { [self] in guard let wave = waves[source] else { return }; guard audioBytes + UInt64(data.count) <= 2_000_000_000 else { throw EngineError(code: "recording_size_limit") }; try wave.append(data); audioBytes += UInt64(data.count) } }
    func close() async {
        await withCheckedContinuation { continuation in
            queue.async { [self] in
                do { try transcript?.synchronize(); try transcript?.close(); transcript = nil; for wave in waves.values { try wave.close() }; waves.removeAll() } catch { markFailure() }
                continuation.resume()
            }
        }
    }
}
