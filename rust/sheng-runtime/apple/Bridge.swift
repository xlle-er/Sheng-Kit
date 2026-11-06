import Foundation

/// Callback into Rust: `(token, status, text)`. For one-shot replies `status` is
/// `ReplyStatus`; for speech sessions it is `SpeechEvent`. `text` is valid only during the call.
public typealias ShengCallback = @convention(c) (UInt64, Int32, UnsafePointer<CChar>?) -> Void

enum ReplyStatus: Int32 {
    case ok = 0
    case failed = 1
}

enum SpeechEvent: Int32 {
    case ready = 0
    case draft = 1
    case final = 2
    case ended = 3
    case failed = 4
}

/// Failure names reported with `SpeechEvent.failed`; Rust maps them to session codes.
enum SpeechFailure: String {
    case unavailable = "service_unavailable"
    case streamClosed = "stream_closed"
}

func deliver(_ callback: ShengCallback, _ token: UInt64, _ status: Int32, _ text: String) {
    text.withCString { callback(token, status, $0) }
}

func reply(_ callback: ShengCallback, _ token: UInt64, _ status: ReplyStatus, _ text: String) {
    deliver(callback, token, status.rawValue, text)
}

func text(_ pointer: UnsafePointer<CChar>?) -> String? {
    pointer.map { String(cString: $0) }
}

func json(_ object: Any) -> String {
    guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]) else {
        return "{}"
    }
    return String(decoding: data, as: UTF8.self)
}

/// Runs `work` off the calling thread and replies exactly once.
func replyLater(
    _ callback: ShengCallback,
    _ token: UInt64,
    _ work: @escaping @Sendable () async -> (ReplyStatus, String)
) {
    Task.detached {
        let (status, payload) = await work()
        reply(callback, token, status, payload)
    }
}
