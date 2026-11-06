//! Original drafts on recognizing routes are shown at once, however slow the independent
//! translator is (captions.md §2.1, translation-pipeline.md §1).

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use serde_json::Value;
use sheng_core::{CaptionEvent, Source};
use sheng_engine::abi::engine::sheng_engine_destroy;
use sheng_runtime::provider::scripted::{ScriptedSpeech, SpeechScript};
use sheng_runtime::provider::{
    BoxFuture, ProviderError, ProviderEvent, TextTranslator, TranslationRequest,
};

const SYSTEM: u32 = 0;
/// 100 ms of 16 kHz PCM16.
const CHUNK: [u8; 3200] = [0; 3200];
/// Every translation takes this long: longer than a draft may wait to be shown, short
/// enough that preview pairs come back while the sentence is still being spoken.
const TRANSLATION_DELAY: Duration = Duration::from_millis(400);
/// A draft must reach the snapshot within this time.
const DRAFT_DEADLINE: Duration = Duration::from_millis(300);

/// Answers every request after `TRANSLATION_DELAY`; counts requests.
#[derive(Default)]
struct SlowTranslator {
    requests: AtomicUsize,
}

impl TextTranslator for SlowTranslator {
    fn translate(
        &self,
        request: TranslationRequest,
    ) -> BoxFuture<'_, Result<String, ProviderError>> {
        self.requests.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move {
            tokio::time::sleep(TRANSLATION_DELAY).await;
            Ok(format!("tr:{}", request.text))
        })
    }
}

fn draft_text(n: usize) -> String {
    format!("draft words number {n}")
}

/// Chunk `n` produces a growing draft of one utterance; drafts per chunk are `burst`.
fn growing_drafts(chunks: usize, burst: usize) -> SpeechScript {
    SpeechScript {
        on_audio: (1..=chunks)
            .map(|n| {
                let events = (1..=burst)
                    .map(|b| {
                        let index = (n - 1) * burst + b;
                        ProviderEvent::Caption(CaptionEvent::OriginalDraft {
                            utterance: Some(1.into()),
                            text: draft_text(index),
                        })
                    })
                    .collect();
                (n, events)
            })
            .collect(),
        ..SpeechScript::default()
    }
}

/// The draft is the current original, not parked in `pendingOriginal` behind a preview.
fn shows(snapshot: &Value, text: &str) -> bool {
    snapshot["tracks"]
        .as_array()
        .is_some_and(|tracks| tracks.iter().any(|t| t["original"] == text))
}

/// Waits up to `DRAFT_DEADLINE` for `text`; returns how long it took, if it appeared.
fn time_until_shown(handle: u64, text: &str) -> Option<Duration> {
    let started = Instant::now();
    while started.elapsed() < DRAFT_DEADLINE {
        let current = snapshot(handle);
        assert_ne!(status(&current), "failed", "session failed: {current}");
        if shows(&current, text) {
            return Some(started.elapsed());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

fn apple_session(script: SpeechScript, translator: Arc<SlowTranslator>) -> u64 {
    let (client, _) = ScriptedSpeech::new(script);
    let factory =
        TestFactory::new(vec![(Source::System, Box::new(client))]).with_translator(translator);
    let handle = handle_with(factory);
    let request =
        common::requests::start_request("system", "appleSpeech", "-", "apple", "en", "zh");
    assert_eq!(start(handle, &request)["ok"], true);
    wait_for(handle, |s| status(s) == "listening");
    handle
}

#[test]
fn drafts_reach_tracks_without_waiting_for_a_slow_translator() {
    // About three seconds of one sentence, so a preview comes back meanwhile.
    const CHUNKS: usize = 60;
    // More drafts than the pipeline input queue holds while one translation is in flight.
    const BURST: usize = 8;
    let translator = Arc::new(SlowTranslator::default());
    let handle = apple_session(growing_drafts(CHUNKS, BURST), Arc::clone(&translator));

    for n in 1..=CHUNKS {
        assert_eq!(push(handle, SYSTEM, &CHUNK), 0);
        let latest = draft_text(n * BURST);
        let waited = time_until_shown(handle, &latest);
        assert!(
            waited.is_some(),
            "draft {} not shown within {DRAFT_DEADLINE:?}; snapshot {}",
            n * BURST,
            snapshot(handle)
        );
        std::thread::sleep(Duration::from_millis(40));
    }

    // The drafts above were checked while that preview's translation was on screen.
    let last = snapshot(handle);
    assert!(translator.requests.load(Ordering::Relaxed) >= 1);
    assert!(
        last["tracks"][0]["translation"]
            .as_str()
            .is_some_and(|t| t.starts_with("tr:")),
        "no preview shown; snapshot {last}"
    );
    assert_eq!(last["tracks"][0]["pendingOriginal"], "");
    sheng_engine_destroy(handle);
}

#[test]
fn late_preview_keeps_the_newer_draft_as_original() {
    let draft = |text: &str| {
        vec![ProviderEvent::Caption(CaptionEvent::OriginalDraft {
            utterance: Some(1.into()),
            text: text.into(),
        })]
    };
    let script = SpeechScript {
        on_audio: vec![(1, draft("good")), (2, draft("good morning everyone"))],
        ..SpeechScript::default()
    };
    let translator = Arc::new(SlowTranslator::default());
    let handle = apple_session(script, Arc::clone(&translator));

    assert_eq!(push(handle, SYSTEM, &CHUNK), 0);
    wait_for(handle, |_| translator.requests.load(Ordering::Relaxed) == 1);
    assert_eq!(push(handle, SYSTEM, &CHUNK), 0);
    let shown = wait_for(handle, |s| s["tracks"][0]["translation"] != "");

    let track = &shown["tracks"][0];
    assert_eq!(track["translation"], "tr:good");
    assert_eq!(track["original"], "good morning everyone");
    assert_eq!(track["pendingOriginal"], "");
    sheng_engine_destroy(handle);
}
