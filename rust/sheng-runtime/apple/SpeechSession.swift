import Foundation
import Speech

/// One recognition session (apple.md §2). Audio arrives through a bounded `PCMQueue`;
/// volatile and final results go back to Rust through the event callback. After `close`
/// no further events are delivered.
@available(macOS 26, *)
final class SpeechSession: @unchecked Sendable {
    /// PCM chunks waiting for the analyzer.
    static let queueCapacity = 64

    let queue = PCMQueue(capacity: queueCapacity)
    private let token: UInt64
    private let callback: ShengCallback
    private let lock = NSLock()
    private var analyzer: SpeechAnalyzer?
    private var tasks: [Task<Void, Never>] = []
    private var isClosed = false
    private var isFinishing = false
    private var isRunning = false

    init(token: UInt64, callback: ShengCallback) {
        self.token = token
        self.callback = callback
    }

    var running: Bool { locked { isRunning && !isClosed } }

    func start(localeIdentifier: String) {
        track(Task { await self.run(localeIdentifier) })
    }

    /// Ends input; the remaining results are delivered, then `ended`.
    func finish() {
        locked { isFinishing = true }
        queue.close()
        let analyzer = locked { self.analyzer }
        track(Task {
            do {
                try await analyzer?.finalizeAndFinishThroughEndOfInput()
            } catch {
                self.fail(.streamClosed)
            }
        })
    }

    func close() {
        let (analyzer, tasks) = locked { () -> (SpeechAnalyzer?, [Task<Void, Never>]) in
            isClosed = true
            return (self.analyzer, self.tasks)
        }
        queue.close()
        tasks.forEach { $0.cancel() }
        if let analyzer {
            Task { await analyzer.cancelAndFinishNow() }
        }
    }

    private func run(_ identifier: String) async {
        let module = SpeechAssets.transcriber(Locale(identifier: identifier))
        guard SpeechTranscriber.isAvailable,
              await AssetInventory.status(forModules: [module]) == .installed
        else { return fail(.unavailable) }
        do {
            let format = await SpeechAnalyzer.bestAvailableAudioFormat(
                compatibleWith: [module], considering: AudioInput.sourceFormat
            )
            let input = AudioInput(analyzerFormat: format)
            let analyzer = SpeechAnalyzer(modules: [module])
            try await analyzer.prepareToAnalyze(in: format)
            guard adopt(analyzer) else { return await analyzer.cancelAndFinishNow() }
            track(Task { await self.read(module) })
            try await analyzer.start(inputSequence: queue.compactMap { input.input($0) })
            locked { isRunning = true }
            send(.ready, "")
        } catch {
            fail(.unavailable)
        }
    }

    private func read(_ module: SpeechTranscriber) async {
        do {
            for try await result in module.results {
                send(result.isFinal ? .final : .draft, String(result.text.characters))
            }
            let finishing = locked { isFinishing }
            finishing ? send(.ended, "") : fail(.streamClosed)
        } catch {
            fail(.streamClosed)
        }
        locked { isRunning = false }
    }

    private func adopt(_ analyzer: SpeechAnalyzer) -> Bool {
        locked {
            guard !isClosed else { return false }
            self.analyzer = analyzer
            return true
        }
    }

    private func fail(_ failure: SpeechFailure) {
        locked { isRunning = false }
        send(.failed, failure.rawValue)
    }

    private func send(_ event: SpeechEvent, _ text: String) {
        guard !locked({ isClosed }) else { return }
        deliver(callback, token, event.rawValue, text)
    }

    private func track(_ task: Task<Void, Never>) {
        let cancelled = locked { () -> Bool in
            if isClosed { return true }
            tasks.append(task)
            return false
        }
        if cancelled { task.cancel() }
    }

    private func locked<T>(_ body: () -> T) -> T {
        lock.lock()
        defer { lock.unlock() }
        return body()
    }
}

/// Live sessions by token.
final class SpeechSessions: @unchecked Sendable {
    static let shared = SpeechSessions()
    private let lock = NSLock()
    private var sessions: [UInt64: AnyObject] = [:]

    func insert(_ token: UInt64, _ session: AnyObject) {
        lock.lock()
        sessions[token] = session
        lock.unlock()
    }

    func get(_ token: UInt64) -> AnyObject? {
        lock.lock()
        defer { lock.unlock() }
        return sessions[token]
    }

    func remove(_ token: UInt64) -> AnyObject? {
        lock.lock()
        defer { lock.unlock() }
        return sessions.removeValue(forKey: token)
    }
}
