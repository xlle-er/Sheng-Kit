import Foundation

/// Bounded FIFO of PCM chunks between `push` calls from Rust and the speech analyzer.
/// After `close` it rejects new chunks; the reader drains what is left and then ends.
/// One reader at a time.
final class PCMQueue: AsyncSequence, @unchecked Sendable {
    typealias Element = Data

    enum Push: Int32 {
        case accepted = 0
        case closed = -1
        case full = -2
    }

    let capacity: Int
    private let lock = NSLock()
    private var chunks: [Data] = []
    private var isClosed = false
    private var waiter: CheckedContinuation<Data?, Never>?

    init(capacity: Int) {
        self.capacity = Swift.max(1, capacity)
    }

    func push(_ chunk: Data) -> Push {
        lock.lock()
        if isClosed {
            lock.unlock()
            return .closed
        }
        if let reader = waiter {
            // The reader only waits on an empty queue, so handing over keeps the order.
            waiter = nil
            lock.unlock()
            reader.resume(returning: chunk)
            return .accepted
        }
        guard chunks.count < capacity else {
            lock.unlock()
            return .full
        }
        chunks.append(chunk)
        lock.unlock()
        return .accepted
    }

    func close() {
        lock.lock()
        isClosed = true
        let reader = waiter
        waiter = nil
        lock.unlock()
        reader?.resume(returning: nil)
    }

    /// Next chunk without waiting: `.some(nil)` once closed and empty, `nil` when empty but open.
    func poll() -> Data?? {
        lock.lock()
        defer { lock.unlock() }
        if !chunks.isEmpty { return .some(chunks.removeFirst()) }
        return isClosed ? .some(nil) : nil
    }

    func next() async -> Data? {
        await withCheckedContinuation { (continuation: CheckedContinuation<Data?, Never>) in
            lock.lock()
            if !chunks.isEmpty {
                let chunk = chunks.removeFirst()
                lock.unlock()
                continuation.resume(returning: chunk)
            } else if isClosed {
                lock.unlock()
                continuation.resume(returning: nil)
            } else {
                waiter = continuation
                lock.unlock()
            }
        }
    }

    struct AsyncIterator: AsyncIteratorProtocol {
        let queue: PCMQueue
        mutating func next() async -> Data? { await queue.next() }
    }

    func makeAsyncIterator() -> AsyncIterator { AsyncIterator(queue: self) }
}
