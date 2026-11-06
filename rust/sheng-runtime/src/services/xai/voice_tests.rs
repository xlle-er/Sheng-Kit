//! Voice protocol: session.update, audio frames, stream events and response.done rules.

use base64::Engine;
use serde_json::{json, Value};
use sheng_core::align::Stream;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use super::*;
use crate::provider::{ProviderError, ProviderEvent, StreamText};

fn delta(stream: Stream, text: &str) -> ProviderEvent {
    ProviderEvent::StreamText {
        stream,
        text: StreamText::Delta(text.into()),
    }
}

fn completed(text: &str) -> String {
    json!({"type": "conversation.item.input_audio_transcription.completed",
        "item_id": "msg_1", "transcript": text})
    .to_string()
}

fn spoken(text: &str) -> String {
    json!({"type": "response.output_audio_transcript.delta", "response_id": "resp_1",
        "item_id": "msg_2", "delta": text})
    .to_string()
}

fn done(status: &str) -> String {
    json!({"type": "response.done", "response": {"id": "resp_1", "status": status}}).to_string()
}

#[test]
fn url_selects_the_voice_model() {
    let url = voice_url(&Url::parse("wss://api.x.ai/v1/realtime").unwrap());
    assert_eq!(
        url.as_str(),
        "wss://api.x.ai/v1/realtime?model=grok-voice-latest"
    );
}

#[test]
fn session_update_sets_format_turn_detection_hint_and_target() {
    let update = session_update(24_000, "ja", "en");
    assert_eq!(update["type"], "session.update");
    let session = &update["session"];
    assert_eq!(session["turn_detection"], json!({"type": "server_vad"}));
    let input = &session["audio"]["input"];
    assert_eq!(input["format"], json!({"type": "audio/pcm", "rate": 24000}));
    assert_eq!(input["transcription"], json!({"language_hint": "ja"}));
    let instructions = session["instructions"].as_str().unwrap();
    assert!(instructions.contains("into English."), "{instructions}");
    let auto = session_update(24_000, "auto", "pt-BR");
    assert_eq!(auto["session"]["audio"]["input"].get("transcription"), None);
    assert!(auto["session"]["instructions"]
        .as_str()
        .unwrap()
        .contains("Brazilian Portuguese"));
}

#[test]
fn setup_sends_the_session_update_and_session_updated_means_ready() {
    let mut p = VoiceProtocol::new(24_000, "auto", "zh");
    let setup = p.setup();
    assert_eq!(setup.len(), 1);
    let sent: Value = serde_json::from_str(&setup[0]).unwrap();
    assert_eq!(sent, session_update(24_000, "auto", "zh"));
    assert!(p.on_text(r#"{"type":"session.created","session":{}}"#) == Default::default());
    assert!(
        p.on_text(r#"{"type":"session.updated","session":{}}"#)
            .ready
    );
    assert!(p.finish().is_none());
}

#[test]
fn audio_is_a_base64_append() {
    let Message::Text(text) = VoiceProtocol::audio(&[0, 1, 2, 3]) else {
        panic!("text frame expected");
    };
    let message: Value = serde_json::from_str(text.as_str()).unwrap();
    assert_eq!(message["type"], "input_audio_buffer.append");
    let audio = base64::engine::general_purpose::STANDARD
        .decode(message["audio"].as_str().unwrap())
        .unwrap();
    assert_eq!(audio, [0, 1, 2, 3]);
}

#[test]
fn transcription_is_the_original_and_spoken_transcript_the_translation() {
    let mut p = VoiceProtocol::new(24_000, "auto", "en");
    let original = p.on_text(&completed("你好。")).events;
    assert_eq!(original, [delta(Stream::Original, "你好。")]);
    assert_eq!(
        p.on_text(&spoken("Hel")).events,
        [delta(Stream::Translation, "Hel")]
    );
    assert_eq!(
        p.on_text(&spoken("lo.")).events,
        [delta(Stream::Translation, "lo.")]
    );
    let ended = p.on_text(&done("completed"));
    assert!(ended.turn_end);
    assert_eq!(ended.events, [ProviderEvent::TurnComplete]);
    // Next turn: separated from the previous piece of the same stream.
    assert_eq!(
        p.on_text(&spoken("Bye")).events,
        [delta(Stream::Translation, " Bye")]
    );
    assert_eq!(
        p.on_text(&completed("再见")).events,
        [delta(Stream::Original, "再见")]
    );
    let audio = r#"{"type":"response.output_audio.delta","delta":"AAAA"}"#;
    assert_eq!(p.on_text(audio), Default::default());
}

#[test]
fn latin_transcripts_are_separated_between_items() {
    let mut p = VoiceProtocol::new(24_000, "auto", "zh");
    p.on_text(&completed("Hello there."));
    let next = p.on_text(&completed("How are you?")).events;
    assert_eq!(next, [delta(Stream::Original, " How are you?")]);
    assert!(p.on_text(&completed("")).events.is_empty());
}

#[test]
fn interrupted_responses_end_the_turn_and_failed_ones_fail() {
    let mut p = VoiceProtocol::new(24_000, "auto", "en");
    assert!(p.on_text(&done("cancelled")).turn_end);
    assert!(p.on_text(&done("incomplete")).turn_end);
    assert_eq!(
        p.on_text(&done("failed")).failure,
        Some(ProviderError::Provider)
    );
}

#[test]
fn error_events_are_classified() {
    let mut p = VoiceProtocol::new(24_000, "auto", "en");
    let error = |code: &str| {
        json!({"type": "error", "error": {"type": "invalid_request_error", "code": code,
            "message": "m"}})
        .to_string()
    };
    let auth = p.on_text(&error("invalid_api_key")).failure;
    assert_eq!(auth, Some(ProviderError::Authentication));
    let format = p.on_text(&error("invalid_audio_format")).failure;
    assert_eq!(format, Some(ProviderError::Provider));
    let timeout = json!({"type": "error", "error": {"type": "timeout", "message": "m"}});
    let timeout = p.on_text(&timeout.to_string()).failure;
    assert_eq!(timeout, Some(ProviderError::Transport));
    for code in [
        "idle_timeout",
        "session_max_duration",
        "input_silence_timeout",
    ] {
        let failure = p.on_text(&error(code)).failure;
        assert_eq!(failure, Some(ProviderError::Transport), "{code}");
    }
    assert_eq!(p.on_text("{").failure, Some(ProviderError::Provider));
}
