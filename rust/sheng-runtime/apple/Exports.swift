import Foundation

// C entry points called by `services/apple/bridge.rs`. None of them blocks: one-shot work
// replies through the callback with the caller's token; speech sessions report events
// through their callback until closed. Frameworks are touched only inside these calls.

@_cdecl("sheng_apple_speech_inventory")
public func shengAppleSpeechInventory(_ token: UInt64, _ callback: ShengCallback) {
    replyLater(callback, token) { (.ok, await SpeechAssets.inventory()) }
}

@_cdecl("sheng_apple_prepare_speech")
public func shengApplePrepareSpeech(_ locale: UnsafePointer<CChar>?, _ token: UInt64, _ callback: ShengCallback) {
    let identifier = text(locale) ?? ""
    replyLater(callback, token) {
        await SpeechAssets.prepare(identifier) ? (.ok, "") : (.failed, "")
    }
}

@_cdecl("sheng_apple_translation_languages")
public func shengAppleTranslationLanguages(_ token: UInt64, _ callback: ShengCallback) {
    replyLater(callback, token) { (.ok, await TranslationAssets.languages()) }
}

@_cdecl("sheng_apple_translation_status")
public func shengAppleTranslationStatus(
    _ source: UnsafePointer<CChar>?, _ target: UnsafePointer<CChar>?, _ token: UInt64, _ callback: ShengCallback
) {
    let (from, to) = (text(source) ?? "", text(target) ?? "")
    replyLater(callback, token) { (.ok, await TranslationAssets.status(source: from, target: to)) }
}

@_cdecl("sheng_apple_prepare_translation")
public func shengApplePrepareTranslation(
    _ source: UnsafePointer<CChar>?, _ target: UnsafePointer<CChar>?, _ token: UInt64, _ callback: ShengCallback
) {
    let (from, to) = (text(source) ?? "", text(target) ?? "")
    replyLater(callback, token) {
        await TranslationAssets.prepare(source: from, target: to) ? (.ok, "") : (.failed, "")
    }
}

@_cdecl("sheng_apple_translate")
public func shengAppleTranslate(
    _ source: UnsafePointer<CChar>?, _ target: UnsafePointer<CChar>?, _ sentence: UnsafePointer<CChar>?,
    _ token: UInt64, _ callback: ShengCallback
) {
    let (from, to, input) = (text(source) ?? "", text(target) ?? "", text(sentence) ?? "")
    replyLater(callback, token) {
        switch await TranslationAssets.translate(input, source: from, target: to) {
        case .success(let translated): return (.ok, translated)
        case .failure(let failure): return (.failed, failure.rawValue)
        }
    }
}

/// Opens a recognition session; `0` when started (events follow), `-1` when Speech is unavailable.
@_cdecl("sheng_apple_speech_open")
public func shengAppleSpeechOpen(_ locale: UnsafePointer<CChar>?, _ token: UInt64, _ callback: ShengCallback) -> Int32 {
    guard #available(macOS 26, *), let identifier = text(locale) else { return -1 }
    let session = SpeechSession(token: token, callback: callback)
    SpeechSessions.shared.insert(token, session)
    session.start(localeIdentifier: identifier)
    return 0
}

/// `0` accepted, `-1` closed or unknown, `-2` queue full. The bytes are copied.
@_cdecl("sheng_apple_speech_push")
public func shengAppleSpeechPush(_ token: UInt64, _ bytes: UnsafePointer<UInt8>?, _ length: Int) -> Int32 {
    guard #available(macOS 26, *),
          let session = SpeechSessions.shared.get(token) as? SpeechSession,
          let bytes, length > 0
    else { return PCMQueue.Push.closed.rawValue }
    return session.queue.push(Data(bytes: bytes, count: length)).rawValue
}

@_cdecl("sheng_apple_speech_finish")
public func shengAppleSpeechFinish(_ token: UInt64) {
    guard #available(macOS 26, *), let session = SpeechSessions.shared.get(token) as? SpeechSession else { return }
    session.finish()
}

/// `1` while the recognition task is running.
@_cdecl("sheng_apple_speech_alive")
public func shengAppleSpeechAlive(_ token: UInt64) -> Int32 {
    guard #available(macOS 26, *), let session = SpeechSessions.shared.get(token) as? SpeechSession else { return 0 }
    return session.running ? 1 : 0
}

/// Stops the session; no event is delivered for `token` after this returns.
@_cdecl("sheng_apple_speech_close")
public func shengAppleSpeechClose(_ token: UInt64) {
    guard #available(macOS 26, *), let session = SpeechSessions.shared.remove(token) as? SpeechSession else { return }
    session.close()
}
