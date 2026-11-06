//! OpenAI Realtime protocol: session.update shape, event decoding, draft accumulation,
//! error mapping (providers/openai.md). Hand-written fixtures, no network.

#[path = "fixtures/openai/support.rs"]
mod support;

use serde_json::{json, Value};
use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::provider::{ProviderError, ProviderEvent};
use sheng_runtime::services::openai::errors::classify;
use sheng_runtime::services::openai::messages::{self, decode, ErrorInfo, ServerEvent};
use sheng_runtime::services::openai::transcript::{Mapper, Step};
use sheng_runtime::services::openai::{LanguageField, SessionKind};

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn transcribe(language: Option<&str>, field: LanguageField) -> SessionKind {
    SessionKind::Transcribe {
        model: "gpt-live-transcribe".into(),
        language: language.map(str::to_owned),
        language_field: field,
    }
}

fn translate() -> SessionKind {
    SessionKind::Translate {
        source_transcript_model: "gpt-realtime-whisper".into(),
        output_language: "fr".into(),
    }
}

/// Runs fixture lines through a fresh mapper, collecting events and the last step.
fn run(lines: &[String]) -> (Vec<ProviderEvent>, Step) {
    let mut mapper = Mapper::default();
    let mut out = Vec::new();
    let mut step = Step::Continue;
    for line in lines {
        step = mapper.on_event(decode(line).expect("fixture decodes"), &mut out);
    }
    (out, step)
}

fn item(id: &str) -> Option<UtteranceId> {
    Some(UtteranceId::Text(id.into()))
}

#[test]
fn transcription_session_update_sets_model_pcm_24k_language_and_no_turn_detection() {
    let update = parse(&messages::session_update(&transcribe(
        Some("en"),
        LanguageField::List,
    )));

    assert_eq!(
        update,
        json!({
            "type": "session.update",
            "session": {
                "type": "transcription",
                "audio": { "input": {
                    "format": { "type": "audio/pcm", "rate": 24000 },
                    "transcription": { "model": "gpt-live-transcribe", "languages": ["en"] },
                    "turn_detection": null,
                }},
            },
        })
    );
}

#[test]
fn single_language_field_and_auto_language() {
    let single = parse(&messages::session_update(&transcribe(
        Some("zh"),
        LanguageField::Single,
    )));
    let auto = parse(&messages::session_update(&transcribe(
        None,
        LanguageField::List,
    )));

    let transcription = &single["session"]["audio"]["input"]["transcription"];
    assert_eq!(transcription["language"], "zh");
    assert!(transcription.get("languages").is_none());
    let transcription = &auto["session"]["audio"]["input"]["transcription"];
    assert!(transcription.get("language").is_none() && transcription.get("languages").is_none());
}

#[test]
fn translation_session_update_sets_output_language_and_source_transcription() {
    let update = parse(&messages::session_update(&translate()));

    assert_eq!(
        update,
        json!({
            "type": "session.update",
            "session": { "audio": {
                "input": { "transcription": { "model": "gpt-realtime-whisper" } },
                "output": { "language": "fr" },
            }},
        })
    );
}

#[test]
fn audio_append_uses_the_session_kind_event_name_and_base64() {
    let pcm = [0u8, 1, 2, 3];

    let transcribe = parse(&messages::append_audio(
        &transcribe(None, LanguageField::List),
        &pcm,
    ));
    let translate = parse(&messages::append_audio(&translate(), &pcm));

    assert_eq!(transcribe["type"], "input_audio_buffer.append");
    assert_eq!(translate["type"], "session.input_audio_buffer.append");
    assert_eq!(transcribe["audio"], "AAECAw==");
    assert_eq!(
        parse(&messages::commit())["type"],
        "input_audio_buffer.commit"
    );
    assert_eq!(parse(&messages::session_close())["type"], "session.close");
}

#[test]
fn deltas_of_one_item_accumulate_into_one_draft_and_complete_once() {
    let (events, _) = run(&support::fixture("transcription_turn.jsonl"));

    let draft = |text: &str| {
        ProviderEvent::Caption(CaptionEvent::OriginalDraft {
            utterance: item("item_a"),
            text: text.into(),
        })
    };
    assert_eq!(
        events,
        vec![
            draft("Good"),
            draft("Good morning,"),
            draft("Good morning, everyone."),
            ProviderEvent::Caption(CaptionEvent::OriginalFinal {
                utterance: item("item_a"),
                text: "Good morning, everyone.".into(),
            }),
        ]
    );
}

#[test]
fn interleaved_items_keep_separate_drafts() {
    let lines = [
        r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"a","delta":"one"}"#,
        r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"b","delta":"two"}"#,
        r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"a","delta":" more"}"#,
    ]
    .map(String::from);

    let (events, _) = run(&lines);

    assert_eq!(
        events.last(),
        Some(&ProviderEvent::Caption(CaptionEvent::OriginalDraft {
            utterance: item("a"),
            text: "one more".into(),
        }))
    );
}

#[test]
fn translation_events_become_two_delta_streams_and_close_completes_the_turn() {
    use sheng_core::align::Stream;
    use sheng_runtime::provider::StreamText;

    let (events, step) = run(&support::fixture("translation_stream.jsonl"));

    let text = |stream, delta: &str| ProviderEvent::StreamText {
        stream,
        text: StreamText::Delta(delta.into()),
    };
    assert_eq!(
        events,
        vec![
            text(Stream::Original, "The train"),
            text(Stream::Translation, "Le train"),
            text(Stream::Original, " leaves at noon."),
            text(Stream::Translation, " part à midi."),
            ProviderEvent::TurnComplete,
        ]
    );
    assert_eq!(step, Step::Done);
}

#[test]
fn finish_waits_for_the_committed_turn_and_its_transcript() {
    let mut mapper = Mapper::default();
    let mut out = Vec::new();
    mapper.commit_sent();

    assert_eq!(mapper.finish(), Step::Continue);
    let committed = decode(r#"{"type":"input_audio_buffer.committed","item_id":"z"}"#).unwrap();
    assert_eq!(mapper.on_event(committed, &mut out), Step::Continue);
    let completed = decode(
        r#"{"type":"conversation.item.input_audio_transcription.completed","item_id":"z","transcript":"end"}"#,
    )
    .unwrap();
    assert_eq!(mapper.on_event(completed, &mut out), Step::Done);
}

#[test]
fn empty_commit_error_is_not_a_failure_and_settles_the_finish() {
    let mut mapper = Mapper::default();
    mapper.commit_sent();
    mapper.finish();
    let lines = support::fixture("errors.jsonl");

    let step = mapper.on_event(decode(&lines[5]).unwrap(), &mut Vec::new());

    assert_eq!(step, Step::Done);
}

#[test]
fn error_events_are_mapped_per_the_table() {
    let lines = support::fixture("errors.jsonl");
    let expected = [
        ProviderError::Authentication,
        ProviderError::Authentication,
        ProviderError::Configuration,
        ProviderError::Transport,
        ProviderError::Provider,
    ];

    for (line, expected) in lines.iter().zip(expected) {
        let (_, step) = run(std::slice::from_ref(line));
        assert_eq!(step, Step::Fail(expected), "{line}");
    }
}

#[test]
fn every_documented_code_is_classified() {
    let auth = [
        "invalid_api_key",
        "authentication_error",
        "authorization_error",
        "unauthorized",
        "forbidden",
        "permission_denied",
    ];
    let config = [
        "model_not_found",
        "insufficient_quota",
        "billing_hard_limit_reached",
    ];
    let info = |code: &str| ErrorInfo {
        code: Some(code.into()),
        kind: None,
    };

    for code in auth {
        assert_eq!(classify(&info(code)), ProviderError::Authentication);
    }
    for code in config {
        assert_eq!(classify(&info(code)), ProviderError::Configuration);
    }
    assert_eq!(classify(&info("session_expired")), ProviderError::Transport);
    for code in [
        "idle_timeout",
        "session_timeout",
        "input_audio_silence_timeout",
    ] {
        assert_eq!(classify(&info(code)), ProviderError::Transport, "{code}");
    }
    assert_eq!(classify(&ErrorInfo::default()), ProviderError::Provider);
}

#[test]
fn unknown_and_audio_events_are_ignored_and_non_json_is_not_an_event() {
    assert_eq!(
        decode(r#"{"type":"session.output_audio.delta","delta":"AAAA"}"#),
        Some(ServerEvent::Other)
    );
    assert_eq!(decode("not json"), None);
}

#[test]
fn translation_finish_waits_for_session_closed() {
    let mut mapper = Mapper::default();
    mapper.close_sent();

    assert_eq!(mapper.finish(), Step::Continue);
    let closed = decode(r#"{"type":"session.closed"}"#).unwrap();
    assert_eq!(mapper.on_event(closed, &mut Vec::new()), Step::Done);
}
