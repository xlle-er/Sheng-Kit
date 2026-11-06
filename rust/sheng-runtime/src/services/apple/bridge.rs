//! The Swift bridge (`rust/sheng-runtime/apple/*.swift`), linked on macOS only.
//! The C functions never block; results come back through `on_reply` / `on_event`.

use std::ffi::{c_char, CStr, CString};
use std::panic::catch_unwind;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use super::events::{self, BridgeEvent, PendingReply, Reply};
use super::status::{SpeechStatus, TranslationStatus};
use super::system::{Apple, AppleSystem, Push, Recognizer, SpeechInventory, TranslateFailure};
use crate::provider::BoxFuture;

type Callback = extern "C" fn(u64, i32, *const c_char);

extern "C" {
    fn sheng_apple_speech_inventory(token: u64, callback: Callback);
    fn sheng_apple_prepare_speech(locale: *const c_char, token: u64, callback: Callback);
    fn sheng_apple_translation_languages(token: u64, callback: Callback);
    fn sheng_apple_translation_status(
        source: *const c_char,
        target: *const c_char,
        token: u64,
        callback: Callback,
    );
    fn sheng_apple_prepare_translation(
        source: *const c_char,
        target: *const c_char,
        token: u64,
        callback: Callback,
    );
    fn sheng_apple_translate(
        source: *const c_char,
        target: *const c_char,
        text: *const c_char,
        token: u64,
        callback: Callback,
    );
    fn sheng_apple_speech_open(locale: *const c_char, token: u64, callback: Callback) -> i32;
    fn sheng_apple_speech_push(token: u64, bytes: *const u8, length: usize) -> i32;
    fn sheng_apple_speech_finish(token: u64);
    fn sheng_apple_speech_alive(token: u64) -> i32;
    fn sheng_apple_speech_close(token: u64);
    fn sheng_apple_selftest() -> u32;
}

/// Queries answer within this time or count as unanswered.
const QUERY_TIMEOUT: Duration = Duration::from_secs(20);
/// Slightly longer than the Swift host's own 30 s limit for one translation.
const TRANSLATE_TIMEOUT: Duration = Duration::from_secs(35);

/// Runs the bridge's offline self-checks and returns the mask of failed checks (`0` when
/// all pass). Bits: 0 queue order, 1 queue capacity, 2 rejection after close,
/// 3 status names, 4 reply payload.
pub fn selftest() -> u32 {
    // SAFETY: no arguments; the function touches no system service.
    unsafe { sheng_apple_selftest() }
}

fn read_text(text: *const c_char) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: the bridge passes a NUL-terminated string valid for this call.
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

extern "C" fn on_reply(token: u64, status: i32, text: *const c_char) {
    let _ = catch_unwind(|| {
        let reply = Reply {
            ok: status == 0,
            text: read_text(text),
        };
        events::complete(token, reply);
    });
}

extern "C" fn on_event(token: u64, kind: i32, text: *const c_char) {
    let _ = catch_unwind(|| {
        if let Some(event) = BridgeEvent::from_raw(kind, read_text(text)) {
            events::deliver(token, event);
        }
    });
}

/// Starts a one-shot call and waits for its reply.
async fn call(timeout: Option<Duration>, start: impl FnOnce(u64)) -> Option<Reply> {
    let pending = PendingReply::new();
    start(pending.token);
    match timeout {
        Some(limit) => tokio::time::timeout(limit, pending.wait())
            .await
            .ok()
            .flatten(),
        None => pending.wait().await,
    }
}

fn c_string(text: &str) -> Option<CString> {
    CString::new(text).ok()
}

#[derive(Deserialize)]
struct InventoryJson {
    available: bool,
    locales: Vec<LocaleJson>,
}

#[derive(Deserialize)]
struct LocaleJson {
    identifier: String,
    status: String,
}

#[derive(Deserialize)]
struct LanguagesJson {
    available: bool,
    languages: Vec<String>,
}

/// The Swift bridge as `AppleSystem` and `Recognizer`.
#[derive(Debug, Default)]
pub struct Bridge;

/// The real system services.
pub fn apple() -> Option<Apple> {
    let bridge = Arc::new(Bridge);
    Some(Apple {
        system: bridge.clone(),
        recognizer: bridge,
    })
}

fn parse_inventory(text: &str) -> Option<SpeechInventory> {
    let parsed: InventoryJson = serde_json::from_str(text).ok()?;
    let locales = parsed
        .locales
        .into_iter()
        .map(|l| (l.identifier, SpeechStatus::from_name(&l.status)))
        .collect();
    Some(SpeechInventory {
        available: parsed.available,
        locales,
    })
}

impl AppleSystem for Bridge {
    fn speech_inventory(&self) -> BoxFuture<'_, Option<SpeechInventory>> {
        Box::pin(async {
            // SAFETY: the callback is a valid function; the bridge replies once.
            let reply = call(Some(QUERY_TIMEOUT), |token| unsafe {
                sheng_apple_speech_inventory(token, on_reply)
            })
            .await?;
            parse_inventory(&reply.text)
        })
    }

    fn prepare_speech(&self, locale: String) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            let Some(locale) = c_string(&locale) else {
                return false;
            };
            // SAFETY: `locale` outlives the call; the bridge copies it.
            let reply = call(None, |token| unsafe {
                sheng_apple_prepare_speech(locale.as_ptr(), token, on_reply)
            })
            .await;
            reply.is_some_and(|r| r.ok)
        })
    }

    fn translation_languages(&self) -> BoxFuture<'_, Option<Vec<String>>> {
        Box::pin(async {
            // SAFETY: as above.
            let reply = call(Some(QUERY_TIMEOUT), |token| unsafe {
                sheng_apple_translation_languages(token, on_reply)
            })
            .await?;
            let parsed: LanguagesJson = serde_json::from_str(&reply.text).ok()?;
            parsed.available.then_some(parsed.languages)
        })
    }

    fn translation_status(
        &self,
        source: String,
        target: String,
    ) -> BoxFuture<'_, TranslationStatus> {
        Box::pin(async move {
            let (Some(source), Some(target)) = (c_string(&source), c_string(&target)) else {
                return TranslationStatus::Unsupported;
            };
            // SAFETY: both strings outlive the call; the bridge copies them.
            let reply = call(Some(QUERY_TIMEOUT), |token| unsafe {
                sheng_apple_translation_status(source.as_ptr(), target.as_ptr(), token, on_reply)
            })
            .await;
            reply.map_or(TranslationStatus::Unavailable, |r| {
                TranslationStatus::from_name(&r.text)
            })
        })
    }

    fn prepare_translation(&self, source: String, target: String) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            let (Some(source), Some(target)) = (c_string(&source), c_string(&target)) else {
                return false;
            };
            // SAFETY: as above.
            let reply = call(None, |token| unsafe {
                sheng_apple_prepare_translation(source.as_ptr(), target.as_ptr(), token, on_reply)
            })
            .await;
            reply.is_some_and(|r| r.ok)
        })
    }

    fn translate(
        &self,
        text: String,
        source: String,
        target: String,
    ) -> BoxFuture<'_, Result<String, TranslateFailure>> {
        Box::pin(async move {
            let strings = (c_string(&source), c_string(&target), c_string(&text));
            let (Some(source), Some(target), Some(text)) = strings else {
                return Err(TranslateFailure::Failed);
            };
            // SAFETY: all strings outlive the call; the bridge copies them.
            let reply = call(Some(TRANSLATE_TIMEOUT), |token| unsafe {
                let (s, t, x) = (source.as_ptr(), target.as_ptr(), text.as_ptr());
                sheng_apple_translate(s, t, x, token, on_reply)
            })
            .await
            .ok_or(TranslateFailure::Failed)?;
            if reply.ok {
                Ok(reply.text)
            } else {
                Err(TranslateFailure::from_name(&reply.text))
            }
        })
    }
}

impl Recognizer for Bridge {
    fn open(&self, locale: &str, token: u64) -> bool {
        let Some(locale) = c_string(locale) else {
            return false;
        };
        // SAFETY: `locale` outlives the call; events for `token` go to `on_event`.
        unsafe { sheng_apple_speech_open(locale.as_ptr(), token, on_event) == 0 }
    }

    fn push(&self, token: u64, pcm: &[u8]) -> Push {
        // SAFETY: `pcm` is valid for its length; the bridge copies it before returning.
        Push::from_code(unsafe { sheng_apple_speech_push(token, pcm.as_ptr(), pcm.len()) })
    }

    fn finish(&self, token: u64) {
        // SAFETY: unknown tokens are ignored by the bridge.
        unsafe { sheng_apple_speech_finish(token) }
    }

    fn alive(&self, token: u64) -> bool {
        // SAFETY: as above.
        unsafe { sheng_apple_speech_alive(token) == 1 }
    }

    fn close(&self, token: u64) {
        // SAFETY: as above.
        unsafe { sheng_apple_speech_close(token) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inventory_json() {
        let json = r#"{"available":true,"locales":[{"identifier":"en-US","status":"installed"},
            {"identifier":"fr-FR","status":"weird"}]}"#;
        let inventory = parse_inventory(json).expect("valid json");
        assert!(inventory.available);
        assert_eq!(inventory.status("en-US"), SpeechStatus::Installed);
        assert_eq!(inventory.status("fr-FR"), SpeechStatus::Unknown);
        assert_eq!(inventory.status("de-DE"), SpeechStatus::Unsupported);
        assert!(parse_inventory("not json").is_none());
    }
}
