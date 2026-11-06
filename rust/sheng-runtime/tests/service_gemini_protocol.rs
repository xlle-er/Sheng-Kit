//! Gemini Live protocol units: setup and audio messages, server message parsing, error
//! mapping and transcript accumulation. Fixtures are hand-written from the public docs.

use serde_json::{json, Value};
use sheng_core::align::Stream;
use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::provider::{ProviderError, ProviderEvent, StreamText};
use sheng_runtime::services::gemini::errors::{classify_close, classify_status, CloseOutcome};
use sheng_runtime::services::gemini::protocol::{
    audio_message, audio_stream_end_message, parse_server_message, setup_message, ServerEvent,
    SessionKind,
};
use sheng_runtime::services::gemini::transcribe::TranscribeState;
use sheng_runtime::services::gemini::translate::{TranslateState, TurnText};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/fixtures/gemini/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).expect("fixture exists")
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn translate_setup_carries_model_target_and_both_transcriptions() {
    let kind = SessionKind::Translate {
        target: "zh-Hans".into(),
    };
    let setup = parse(&setup_message("gemini-3.5-live-translate-preview", &kind));
    assert_eq!(
        setup,
        json!({"setup": {
            "model": "models/gemini-3.5-live-translate-preview",
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "translationConfig": {"targetLanguageCode": "zh-Hans", "echoTargetLanguage": true}
            },
            "inputAudioTranscription": {},
            "outputAudioTranscription": {}
        }})
    );
}

#[test]
fn transcribe_setup_requests_text_and_optional_language_hint() {
    let auto = parse(&setup_message(
        "gemini-3.5-transcribe-live",
        &SessionKind::Transcribe { source: None },
    ));
    assert_eq!(auto["setup"]["inputAudioTranscription"], json!({}));
    assert_eq!(
        auto["setup"]["generationConfig"]["responseModalities"],
        json!(["TEXT"])
    );
    assert!(auto["setup"].get("outputAudioTranscription").is_none());
    let hinted = parse(&setup_message(
        "gemini-3.5-transcribe-live",
        &SessionKind::Transcribe {
            source: Some("ja".into()),
        },
    ));
    assert_eq!(
        hinted["setup"]["inputAudioTranscription"]["languageCodes"],
        json!(["ja"])
    );
}

#[test]
fn audio_is_base64_pcm_at_16k_and_end_is_explicit() {
    let audio = parse(&audio_message(&[0, 1, 2, 3]));
    assert_eq!(
        audio,
        json!({"realtimeInput": {"audio": {"data": "AAECAw==", "mimeType": "audio/pcm;rate=16000"}}})
    );
    let end = parse(&audio_stream_end_message());
    assert_eq!(end, json!({"realtimeInput": {"audioStreamEnd": true}}));
}

#[test]
fn parses_fixture_messages_and_ignores_model_audio() {
    assert_eq!(
        parse_server_message(&fixture("setup_complete.json")),
        vec![ServerEvent::SetupComplete]
    );
    assert_eq!(
        parse_server_message(&fixture("server_content_mixed.json")),
        vec![
            ServerEvent::Input("Hello there.".into()),
            ServerEvent::Output("你好。".into())
        ]
    );
    assert_eq!(
        parse_server_message(&fixture("interim.json")),
        vec![ServerEvent::InterimInput("good morn".into())]
    );
    assert_eq!(
        parse_server_message(&fixture("turn_flags.json")),
        vec![
            ServerEvent::Interrupted,
            ServerEvent::GenerationComplete,
            ServerEvent::TurnComplete
        ]
    );
    assert_eq!(
        parse_server_message(&fixture("go_away.json")),
        vec![ServerEvent::GoAway]
    );
    assert_eq!(
        parse_server_message(&fixture("error_permission.json")),
        vec![ServerEvent::Failed(ProviderError::Authentication)]
    );
    assert_eq!(
        parse_server_message(b"not json"),
        vec![ServerEvent::Failed(ProviderError::Provider)]
    );
}

#[test]
fn status_table_follows_the_spec() {
    use ProviderError as E;
    for (status, expected) in [
        ("UNAUTHENTICATED", E::Authentication),
        ("PERMISSION_DENIED", E::Authentication),
        ("INVALID_ARGUMENT", E::Configuration),
        ("NOT_FOUND", E::Configuration),
        ("FAILED_PRECONDITION", E::Configuration),
        ("UNIMPLEMENTED", E::Configuration),
        ("RESOURCE_EXHAUSTED", E::Provider),
        ("INTERNAL", E::Provider),
        ("permission denied for this key", E::Authentication),
    ] {
        assert_eq!(classify_status(status), expected, "{status}");
    }
}

fn frame(code: CloseCode, reason: &str) -> CloseFrame {
    CloseFrame {
        code,
        reason: reason.to_owned().into(),
    }
}

#[test]
fn close_frames_are_classified_by_reason() {
    use CloseOutcome::{Failed, Normal};
    assert_eq!(classify_close(None), Normal);
    assert_eq!(classify_close(Some(&frame(CloseCode::Normal, ""))), Normal);
    assert_eq!(
        classify_close(Some(&frame(CloseCode::Normal, "session done"))),
        Normal
    );
    assert_eq!(
        classify_close(Some(&frame(
            CloseCode::Policy,
            "API key not valid. PERMISSION_DENIED"
        ))),
        Failed(ProviderError::Authentication)
    );
    assert_eq!(
        classify_close(Some(&frame(
            CloseCode::Invalid,
            "Request contains an invalid argument."
        ))),
        Failed(ProviderError::Configuration)
    );
    assert_eq!(
        classify_close(Some(&frame(CloseCode::Error, "Internal error occurred."))),
        Failed(ProviderError::Transport)
    );
    assert_eq!(
        classify_close(Some(&frame(CloseCode::Away, ""))),
        Failed(ProviderError::Transport)
    );
}

#[test]
fn turn_text_mixes_snapshots_and_increments_without_duplicates() {
    let mut turn = TurnText::default();
    assert_eq!(turn.increment("Hello"), Some("Hello".into()));
    assert_eq!(turn.increment("Hello world"), Some(" world".into()));
    assert_eq!(turn.increment(" again"), Some(" again".into()));
    assert_eq!(turn.increment("Hello world again"), None);
    assert_eq!(turn.as_str(), "Hello world again");
}

#[test]
fn turn_text_keeps_repeated_words() {
    let mut turn = TurnText::default();
    assert_eq!(turn.increment("la"), Some("la".into()));
    assert_eq!(turn.increment("la"), Some("la".into()));
    assert_eq!(turn.increment(" la"), Some(" la".into()));
    assert_eq!(turn.as_str(), "lala la");
}

fn delta(stream: Stream, text: &str) -> ProviderEvent {
    ProviderEvent::StreamText {
        stream,
        text: StreamText::Delta(text.into()),
    }
}

#[test]
fn interrupted_discards_unconfirmed_translation_and_late_output() {
    let mut state = TranslateState::default();
    state.apply(&ServerEvent::Input("Hi".into()));
    assert_eq!(
        state.apply(&ServerEvent::Output("你".into())),
        vec![delta(Stream::Translation, "你")]
    );
    assert_eq!(
        state.apply(&ServerEvent::Interrupted),
        vec![ProviderEvent::StreamText {
            stream: Stream::Translation,
            text: StreamText::Snapshot(String::new())
        }]
    );
    assert!(state.apply(&ServerEvent::Output("好".into())).is_empty());
    assert_eq!(
        state.apply(&ServerEvent::TurnComplete),
        vec![ProviderEvent::TurnComplete]
    );
    assert_eq!(
        state.apply(&ServerEvent::Output("新".into())),
        vec![delta(Stream::Translation, "新")]
    );
}

#[test]
fn turn_end_is_reported_once_per_turn() {
    let mut state = TranslateState::default();
    assert!(state.apply(&ServerEvent::TurnComplete).is_empty());
    state.apply(&ServerEvent::Input("One.".into()));
    assert_eq!(
        state.apply(&ServerEvent::GenerationComplete),
        vec![ProviderEvent::TurnComplete]
    );
    assert!(state.apply(&ServerEvent::TurnComplete).is_empty());
    assert!(state.is_idle());
}

fn final_event(id: u64, text: &str) -> ProviderEvent {
    ProviderEvent::Caption(CaptionEvent::OriginalFinal {
        utterance: Some(UtteranceId::Number(id)),
        text: text.into(),
    })
}

#[test]
fn transcription_accumulates_until_turn_end() {
    let now = Instant::now();
    let mut state = TranscribeState::default();
    let draft = state.apply(&ServerEvent::InterimInput("good morn".into()), now);
    assert_eq!(
        draft,
        vec![ProviderEvent::Caption(CaptionEvent::OriginalDraft {
            utterance: Some(UtteranceId::Number(0)),
            text: "good morn".into()
        })]
    );
    state.apply(&ServerEvent::Input("Good morning.".into()), now);
    state.apply(&ServerEvent::InterimInput("How".into()), now);
    assert_eq!(
        state.quiet_deadline(),
        Some(now + std::time::Duration::from_secs(2))
    );
    assert_eq!(
        state.apply(&ServerEvent::TurnComplete, now),
        vec![final_event(0, "Good morning. How")]
    );
    assert!(state.is_idle());
    state.apply(&ServerEvent::Input("Good morning.".into()), now);
    assert_eq!(state.flush(), vec![final_event(1, "Good morning.")]);
    assert!(state.flush().is_empty());
}
