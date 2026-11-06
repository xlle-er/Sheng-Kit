//! STT protocol: URL, audio frames, partial/final rules and errors.

use serde_json::json;
use sheng_core::{CaptionEvent, UtteranceId};
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use super::*;
use crate::provider::{ProviderError, ProviderEvent};

fn partial(text: &str, is_final: bool, speech_final: bool) -> String {
    json!({"type": "transcript.partial", "text": text, "words": [], "is_final": is_final,
        "speech_final": speech_final, "start": 0.0, "duration": 1.0})
    .to_string()
}

fn draft(n: u64, text: &str) -> ProviderEvent {
    ProviderEvent::Caption(CaptionEvent::OriginalDraft {
        utterance: Some(UtteranceId::Number(n)),
        text: text.into(),
    })
}

fn last(n: u64, text: &str) -> ProviderEvent {
    ProviderEvent::Caption(CaptionEvent::OriginalFinal {
        utterance: Some(UtteranceId::Number(n)),
        text: text.into(),
    })
}

#[test]
fn url_carries_the_stream_configuration() {
    let endpoint = Url::parse("wss://api.x.ai/v1/stt").unwrap();
    let url = stt_url(&endpoint, 24_000, "ja");
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let expected = [
        ("model", STT_MODEL),
        ("encoding", "pcm"),
        ("sample_rate", "24000"),
        ("interim_results", "true"),
        ("language", "ja"),
    ];
    let expected: Vec<(String, String)> = expected
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    assert_eq!(pairs, expected);
    let auto = stt_url(&endpoint, 24_000, "auto");
    assert!(!auto.query_pairs().any(|(key, _)| key == "language"));
}

#[test]
fn audio_is_sent_as_raw_binary_frames() {
    let pcm = vec![1u8, 2, 3, 4];
    assert_eq!(SttProtocol::audio(&pcm), Message::Binary(pcm.into()));
    let protocol = SttProtocol::new();
    assert!(protocol.setup().is_empty());
    assert_eq!(
        protocol.finish().as_deref(),
        Some(r#"{"type":"audio.done"}"#)
    );
}

#[test]
fn created_means_ready() {
    let mut protocol = SttProtocol::new();
    let inbound = protocol.on_text(r#"{"type":"transcript.created","id":"s-1"}"#);
    assert!(inbound.ready && inbound.events.is_empty());
}

#[test]
fn interim_and_chunk_finals_build_one_draft_and_speech_final_confirms_it() {
    let mut p = SttProtocol::new();
    assert_eq!(
        p.on_text(&partial("hello", false, false)).events,
        [draft(1, "hello")]
    );
    let locked = p.on_text(&partial("hello there", true, false)).events;
    assert_eq!(locked, [draft(1, "hello there")]);
    let more = p.on_text(&partial("friend", false, false)).events;
    assert_eq!(more, [draft(1, "hello there friend")]);
    let done = p
        .on_text(&partial("hello there friend.", true, true))
        .events;
    assert_eq!(done, [last(1, "hello there friend.")]);
    assert_eq!(
        p.on_text(&partial("next", false, false)).events,
        [draft(2, "next")]
    );
}

#[test]
fn speech_final_without_text_confirms_the_locked_chunks() {
    let mut p = SttProtocol::new();
    p.on_text(&partial("你好", true, false));
    p.on_text(&partial("世界", true, false));
    assert_eq!(
        p.on_text(&partial("", true, true)).events,
        [last(1, "你好世界")]
    );
}

#[test]
fn empty_results_neither_draft_nor_use_an_utterance_number() {
    let mut p = SttProtocol::new();
    assert!(p.on_text(&partial("  ", false, false)).events.is_empty());
    assert!(p.on_text(&partial("", true, true)).events.is_empty());
    assert_eq!(p.on_text(&partial("a", true, true)).events, [last(1, "a")]);
}

#[test]
fn done_confirms_only_the_unconfirmed_remainder_and_ends() {
    let mut p = SttProtocol::new();
    p.on_text(&partial("first.", true, true));
    p.on_text(&partial("second", false, false));
    let done = json!({"type": "transcript.done", "text": "first. second", "words": [],
        "duration": 2.0});
    let inbound = p.on_text(&done.to_string());
    assert!(inbound.done);
    assert_eq!(inbound.events, [last(2, "second")]);
    let quiet = SttProtocol::new().on_text(&done.to_string());
    assert!(quiet.done && quiet.events.is_empty());
}

#[test]
fn errors_and_malformed_messages_fail() {
    let mut p = SttProtocol::new();
    let error = p.on_text(r#"{"type":"error","message":"stream timeout"}"#);
    assert_eq!(error.failure, Some(ProviderError::Provider));
    let auth = p.on_text(r#"{"type":"error","code":"invalid_api_key","message":"x"}"#);
    assert_eq!(auth.failure, Some(ProviderError::Authentication));
    assert_eq!(p.on_text("not json").failure, Some(ProviderError::Provider));
    let partial = p.on_text(r#"{"type":"transcript.partial","text":"x"}"#);
    assert_eq!(partial.failure, Some(ProviderError::Provider));
    assert_eq!(p.on_text(r#"{"type":"something.new"}"#), Default::default());
}
