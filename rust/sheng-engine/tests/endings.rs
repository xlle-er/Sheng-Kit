//! Unexpected provider endings and the stop tail (providers/README.md “意外断开”,
//! “结束时不丢尾句”), with scripted service clients behind the C functions.

mod common;

use common::*;
use serde_json::Value;
use sheng_core::align::Stream;
use sheng_core::{CaptionEvent, Source};
use sheng_engine::abi::engine::*;
use sheng_runtime::provider::scripted::{ScriptedSpeech, ScriptedTranslator, SpeechScript};
use sheng_runtime::provider::{ProviderError, ProviderEvent, StreamText};

const SYSTEM: u32 = 0;
const CHUNK: [u8; 3200] = [0; 3200];

fn draft(id: u64, text: &str) -> ProviderEvent {
    ProviderEvent::Caption(CaptionEvent::OriginalDraft {
        utterance: Some(id.into()),
        text: text.into(),
    })
}

fn translation_draft(id: u64, text: &str) -> ProviderEvent {
    ProviderEvent::Caption(CaptionEvent::TranslationDraft {
        utterance: Some(id.into()),
        text: text.into(),
    })
}

fn stream(stream: Stream, text: &str) -> ProviderEvent {
    ProviderEvent::StreamText {
        stream,
        text: StreamText::Delta(text.into()),
    }
}

fn pairs(snapshot: &Value) -> Vec<(String, String)> {
    snapshot["recent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["original"].as_str().unwrap().to_owned(),
                c["translation"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn owned(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(o, t)| ((*o).to_owned(), (*t).to_owned()))
        .collect()
}

/// Starts `request` with `script` (and a translator for recognizing routes), pushes one chunk.
fn run(script: SpeechScript, request: &Value) -> u64 {
    let (client, _) = ScriptedSpeech::new(script);
    let factory = TestFactory::new(vec![(Source::System, Box::new(client))])
        .with_translator(ScriptedTranslator::new([]));
    let handle = handle_with(factory);
    let started = start(handle, request);
    assert_eq!(started["ok"], true, "{started}");
    wait_for(handle, |s| status(s) == "listening");
    assert_eq!(push(handle, SYSTEM, &CHUNK), 0);
    handle
}

fn failing_after(events: Vec<ProviderEvent>, last: ProviderEvent) -> SpeechScript {
    let mut events = events;
    events.push(last);
    SpeechScript {
        on_audio: vec![(1, events)],
        ..SpeechScript::default()
    }
}

fn gemini_request() -> Value {
    requests::start_request(
        "system",
        "googleGeminiLive",
        "simultaneous",
        "-",
        "auto",
        "zh",
    )
}

fn pipeline_request() -> Value {
    requests::start_request(
        "system",
        "openAIRealtime",
        "asrMt",
        "openAICompatible",
        "en",
        "zh",
    )
}

#[test]
fn closed_connection_commits_the_draft_with_its_translation_then_fails_retryably() {
    for last in [
        ProviderEvent::Failed(ProviderError::Transport),
        ProviderEvent::Ended,
    ] {
        let script = failing_after(
            vec![draft(1, "说到一半"), translation_draft(1, "half way")],
            last,
        );
        let handle = run(script, &translating_request("system"));
        let done = wait_settled(handle);

        assert_eq!(done["error"], "transport_error");
        assert_eq!(pairs(&done), owned(&[("说到一半", "half way")]));
        assert_eq!(drain(handle).len(), 1);
        sheng_engine_destroy(handle);
    }
}

#[test]
fn non_retryable_failure_does_not_commit_the_draft() {
    let script = failing_after(
        vec![draft(1, "说到一半")],
        ProviderEvent::Failed(ProviderError::Authentication),
    );
    let handle = run(script, &translating_request("system"));
    let done = wait_settled(handle);

    assert_eq!(done["error"], "credential_authentication_failed");
    assert!(pairs(&done).is_empty());
    sheng_engine_destroy(handle);
}

#[test]
fn closed_stream_route_commits_both_unconfirmed_streams() {
    let script = failing_after(
        vec![
            stream(Stream::Original, "half a sentence"),
            stream(Stream::Translation, "半句"),
        ],
        ProviderEvent::Failed(ProviderError::Transport),
    );
    let handle = run(script, &gemini_request());
    let done = wait_settled(handle);

    assert_eq!(done["error"], "transport_error");
    assert_eq!(pairs(&done), owned(&[("half a sentence", "半句")]));
    sheng_engine_destroy(handle);
}

#[test]
fn closed_recognizing_route_commits_the_shown_draft() {
    let script = failing_after(
        vec![draft(1, "Good morn")],
        ProviderEvent::Failed(ProviderError::Transport),
    );
    let handle = run(script, &pipeline_request());
    let done = wait_settled(handle);

    assert_eq!(done["error"], "transport_error");
    let committed = pairs(&done);
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].0, "Good morn");
    sheng_engine_destroy(handle);
}

#[test]
fn stop_tail_without_a_final_is_committed_before_ending() {
    let script = SpeechScript {
        on_finish: vec![draft(9, "最后半句"), translation_draft(9, "last half")],
        ..SpeechScript::default()
    };
    let handle = run(script, &translating_request("system"));
    sheng_engine_stop(handle);
    let done = wait_settled(handle);

    assert_eq!(done["status"], "idle");
    assert_eq!(done["error"], Value::Null);
    assert_eq!(pairs(&done), owned(&[("最后半句", "last half")]));
    sheng_engine_destroy(handle);
}

#[test]
fn stop_tail_of_a_stream_route_keeps_the_one_sided_original() {
    let script = SpeechScript {
        on_finish: vec![stream(Stream::Original, "tail words")],
        ..SpeechScript::default()
    };
    let handle = run(script, &gemini_request());
    sheng_engine_stop(handle);
    let done = wait_settled(handle);

    assert_eq!(done["error"], Value::Null);
    assert_eq!(pairs(&done), owned(&[("tail words", "")]));
    sheng_engine_destroy(handle);
}

#[test]
fn stop_tail_of_a_recognizing_route_is_translated_when_the_service_never_ends() {
    let script = SpeechScript {
        on_finish: vec![draft(4, "Good night")],
        silent_finish: true,
        ..SpeechScript::default()
    };
    let handle = run(script, &pipeline_request());
    sheng_engine_stop(handle);
    let done = wait_settled(handle);

    assert_eq!(done["error"], Value::Null);
    assert_eq!(pairs(&done), owned(&[("Good night", "tr:Good night")]));
    sheng_engine_destroy(handle);
}
