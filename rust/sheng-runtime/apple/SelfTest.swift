import Foundation

/// Offline checks of the bridge's own logic, run by the Rust test suite. Touches no
/// framework service, requests no permission and downloads nothing. Returns a bit mask of
/// failed checks (`0` when all pass); bits are listed in `services/apple/bridge.rs`.
@_cdecl("sheng_apple_selftest")
public func shengAppleSelfTest() -> UInt32 {
    var failed: UInt32 = 0
    if !queueKeepsOrder() { failed |= 1 << 0 }
    if !queueIsBounded() { failed |= 1 << 1 }
    if !queueRejectsAfterClose() { failed |= 1 << 2 }
    if !statusNames() { failed |= 1 << 3 }
    if !replyCarriesPayload() { failed |= 1 << 4 }
    return failed
}

private func chunk(_ byte: UInt8) -> Data { Data([byte, 0]) }

private func queueKeepsOrder() -> Bool {
    let queue = PCMQueue(capacity: 4)
    let pushed = (1...3).map { queue.push(chunk(UInt8($0))) }
    let read = (1...3).compactMap { _ in queue.poll() ?? nil }
    return pushed.allSatisfy { $0 == .accepted } && read == (1...3).map { chunk(UInt8($0)) }
}

private func queueIsBounded() -> Bool {
    let queue = PCMQueue(capacity: 2)
    let results = (1...3).map { queue.push(chunk(UInt8($0))) }
    guard results == [.accepted, .accepted, .full] else { return false }
    _ = queue.poll()
    return queue.push(chunk(4)) == .accepted
}

private func queueRejectsAfterClose() -> Bool {
    let queue = PCMQueue(capacity: 2)
    _ = queue.push(chunk(1))
    queue.close()
    guard queue.push(chunk(2)) == .closed else { return false }
    // What was queued before closing is still delivered, then the queue ends.
    guard case .some(.some(let first)) = queue.poll(), first == chunk(1) else { return false }
    guard case .some(.none) = queue.poll() else { return false }
    return true
}

private func statusNames() -> Bool {
    guard #available(macOS 26, *) else { return true }
    let speech = [
        SpeechAssets.name(.installed), SpeechAssets.name(.supported),
        SpeechAssets.name(.downloading), SpeechAssets.name(.unsupported),
    ]
    let translation = [
        TranslationAssets.name(.installed), TranslationAssets.name(.supported),
        TranslationAssets.name(.unsupported),
    ]
    return speech == ["installed", "supported", "downloading", "unsupported"]
        && translation == ["installed", "supported", "unsupported"]
}

private nonisolated(unsafe) var lastReply: (UInt64, Int32, String)?

private func replyCarriesPayload() -> Bool {
    let record: ShengCallback = { token, status, pointer in
        lastReply = (token, status, pointer.map { String(cString: $0) } ?? "")
    }
    reply(record, 7, .failed, "ç")
    guard let (token, status, payload) = lastReply else { return false }
    return token == 7 && status == ReplyStatus.failed.rawValue && payload == "ç"
}
