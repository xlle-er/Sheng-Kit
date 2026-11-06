import Foundation

public struct CaptureHealthSnapshot: Codable, Equatable, Identifiable {
    public var id: String { source.rawValue }
    public let source: AudioSource
    public let state: String
    public let receivedBuffers: UInt64
    public let rmsDb: Double?
    public let secondsSincePCM: Double?
    var notice: String? {
        if state == "noPCM" { return L10n.text("尚未收到音频，请检查所选音源是否正在播放。") }
        if state == "quiet" { return L10n.text("音源声音较低，请检查播放音量或麦克风。") }
        return nil
    }
    func with(state: String) -> CaptureHealthSnapshot {
        CaptureHealthSnapshot(source: source, state: state, receivedBuffers: receivedBuffers, rmsDb: rmsDb, secondsSincePCM: secondsSincePCM)
    }
    /// Counters change on every audio buffer; republishing them at poll rate redraws every observing view.
    static let counterRefreshInterval: TimeInterval = 1
    static func needsPublish(previous: [CaptureHealthSnapshot], next: [CaptureHealthSnapshot], sinceLastPublish: TimeInterval) -> Bool {
        previous.map { "\($0.source.rawValue):\($0.state)" } != next.map { "\($0.source.rawValue):\($0.state)" } || sinceLastPublish >= counterRefreshInterval
    }
}
final class CaptureHealthTracker: @unchecked Sendable {
    private struct Lane { var count: UInt64 = 0; var last: TimeInterval?; var signal: TimeInterval; var db: Double? }
    private let lock = NSLock()
    private var lanes: [AudioSource: Lane] = [:]
    private var started: TimeInterval = 0
    private var failure = false
    func begin(input: AudioInput, now: TimeInterval = ProcessInfo.processInfo.systemUptime) {
        lock.withLock { started = now; failure = false; lanes = [:]; for source in [AudioSource.system,.microphone] where input == .both || input.rawValue == source.rawValue { lanes[source] = Lane(signal: now) } }
    }
    func receive(_ data: Data, source: AudioSource, now: TimeInterval = ProcessInfo.processInfo.systemUptime) {
        var sum = 0.0; var count = 0
        data.withUnsafeBytes { bytes in
            // Bounded downsampling for health statistics; this never feeds ASR or archives.
            let raw = bytes.bindMemory(to: UInt8.self)
            for index in stride(from: 0, to: max(0, raw.count - 1), by: 32) {
                let sample = Double(Int16(bitPattern: UInt16(raw[index]) | UInt16(raw[index + 1]) << 8)) / 32768
                sum += sample * sample; count += 1
            }
        }
        guard count > 0 else { return }
        let db = max(-120, 10 * log10(max(sum / Double(count), 1e-12)))
        lock.withLock { guard var lane = lanes[source] else { return }; lane.count += 1; lane.last = now; lane.db = db; if db >= -60 { lane.signal = now }; lanes[source] = lane }
    }
    func fail() { lock.withLock { failure = true } }
    func snapshot(now: TimeInterval = ProcessInfo.processInfo.systemUptime) -> [CaptureHealthSnapshot] {
        let lanes: [CaptureHealthSnapshot] = lock.withLock { [AudioSource.system,.microphone].compactMap { source in
            guard let lane = self.lanes[source] else { return nil }
            let elapsed = lane.last.map { max(0, now - $0) }
            let state = failure ? "failed" : (elapsed ?? max(0, now - started)) >= 5 ? "noPCM" : lane.count == 0 ? "waiting" : now - lane.signal >= 10 ? "quiet" : (lane.db ?? -120) >= -60 ? "signal" : "receiving"
            return CaptureHealthSnapshot(source: source, state: state, receivedBuffers: lane.count, rmsDb: lane.db, secondsSincePCM: elapsed)
        } }
        // With two sources one is often silent on purpose (only the mic speaking); flag quiet only when nothing is heard.
        guard lanes.contains(where: { $0.state == "signal" }) else { return lanes }
        return lanes.map { $0.state == "quiet" ? $0.with(state: "receiving") : $0 }
    }
}
