//! Alibaba protocol units: inference messages, error mapping, live-translation linking and
//! Qwen-MT request and stream assembly. Fixtures are hand-written from the public docs.

use serde_json::json;
use sheng_core::{CaptionEvent, CaptionReducer, ReducerConfig, Source, UtteranceId};
use sheng_runtime::provider::{ProviderError, ProviderEvent, TranslationRequest};
use sheng_runtime::services::alibaba::asr::protocol::{
    finish_task, parse, run_task, ServerEvent, TaskSettings,
};
use sheng_runtime::services::alibaba::errors::classify;
use sheng_runtime::services::alibaba::live::protocol::{
    self as live, realtime_url, session_update, LiveEvent,
};
use sheng_runtime::services::alibaba::live::state::LiveState;
use sheng_runtime::services::alibaba::mt::{assemble, languages, request_body};
use sheng_runtime::transport::{SseDecoder, SseEvent};

const TASK: &str = "task-1";

fn settings(source: &str) -> TaskSettings {
    TaskSettings {
        model: "asr-model".into(),
        sample_rate: 16_000,
        source_language: source.into(),
    }
}

#[test]
fn run_task_has_the_documented_shape() {
    let message = run_task(TASK, &settings("ja"));
    assert_eq!(
        message,
        json!({
            "header": {"action": "run-task", "task_id": TASK, "streaming": "duplex"},
            "payload": {
                "task_group": "audio", "task": "asr", "function": "recognition",
                "model": "asr-model",
                "parameters": {"format": "pcm", "sample_rate": 16000,
                    "language_hints": ["ja"], "semantic_punctuation_enabled": false},
                "input": {},
            },
        })
    );
}

#[test]
fn auto_source_sends_no_language_hint() {
    let message = run_task(TASK, &settings("auto"));
    assert!(message["payload"]["parameters"]
        .get("language_hints")
        .is_none());
}

#[test]
fn finish_task_reuses_the_task_id() {
    let message = finish_task(TASK);
    assert_eq!(message["header"]["action"], "finish-task");
    assert_eq!(message["header"]["task_id"], TASK);
    assert_eq!(message["payload"], json!({"input": {}}));
}

#[test]
fn sentence_end_decides_draft_or_final() {
    let draft = parse(include_str!("fixtures/alibaba/result_draft.json"), TASK);
    let utterance = Some(UtteranceId::Number(3));
    assert_eq!(
        draft,
        ServerEvent::Caption(CaptionEvent::OriginalDraft {
            utterance: utterance.clone(),
            text: "早上好".into()
        })
    );
    let done = parse(include_str!("fixtures/alibaba/result_final.json"), TASK);
    assert_eq!(
        done,
        ServerEvent::Caption(CaptionEvent::OriginalFinal {
            utterance,
            text: "早上好，各位。".into()
        })
    );
}

#[test]
fn empty_results_and_heartbeats_are_ignored() {
    for text in [
        include_str!("fixtures/alibaba/result_empty_begin.json"),
        include_str!("fixtures/alibaba/result_heartbeat.json"),
        "not json",
        r#"{"header": {"event": "something-new"}}"#,
    ] {
        assert_eq!(parse(text, TASK), ServerEvent::Ignored);
    }
}

#[test]
fn messages_of_another_task_are_ignored() {
    let draft = include_str!("fixtures/alibaba/result_draft.json");
    assert_eq!(parse(draft, "task-2"), ServerEvent::Ignored);
}

#[test]
fn lifecycle_events_parse() {
    let started = include_str!("fixtures/alibaba/task_started.json");
    let finished = include_str!("fixtures/alibaba/task_finished.json");
    assert_eq!(parse(started, TASK), ServerEvent::Started);
    assert_eq!(parse(finished, TASK), ServerEvent::Finished);
}

#[test]
fn task_failed_codes_map_to_normalized_errors() {
    let rate = ProviderError::RateLimited { retry_after: None };
    let cases = [
        ("InvalidApiKey", ProviderError::Authentication),
        ("Unauthorized", ProviderError::Authentication),
        ("authentication.expired", ProviderError::Authentication),
        ("Throttling", rate),
        ("TOO_MANY_REQUESTS", rate),
        ("RequestTimeout", ProviderError::Transport),
        ("UNSUPPORTED_LANGUAGE", ProviderError::Configuration),
        ("CLIENT_ERROR", ProviderError::Configuration),
        ("InternalError", ProviderError::Provider),
    ];
    let template = include_str!("fixtures/alibaba/task_failed.json");
    for (code, expected) in cases {
        let message = template.replace("ERROR_CODE", code);
        assert_eq!(
            parse(&message, TASK),
            ServerEvent::Failed(expected),
            "{code}"
        );
    }
    assert_eq!(classify(None, None), ProviderError::Provider);
}

#[test]
fn realtime_url_uses_workspace_host_when_given() {
    let plain = realtime_url("", "m").unwrap();
    assert_eq!(
        plain.as_str(),
        "wss://dashscope.aliyuncs.com/api-ws/v1/realtime?model=m"
    );
    let workspace = realtime_url(" ws-123 ", "m").unwrap();
    assert_eq!(
        workspace.as_str(),
        "wss://ws-123.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime?model=m"
    );
    assert!(realtime_url("bad/host", "m").is_none());
}

#[test]
fn session_update_requests_text_only_output() {
    let update = session_update("en", 16_000);
    assert_eq!(update["type"], "session.update");
    assert_eq!(update["session"]["output_modalities"], json!(["text"]));
    assert_eq!(update["session"]["translation"]["language"], "en");
    assert_eq!(
        update["session"]["audio"]["input"]["format"],
        json!({"type": "pcm", "sample_rate": 16000})
    );
}

fn live_events(state: &mut LiveState, lines: &str) -> Vec<CaptionEvent> {
    lines
        .lines()
        .flat_map(|line| state.handle(live::parse(line)).events)
        .filter_map(|event| match event {
            ProviderEvent::Caption(caption) => Some(caption),
            _ => None,
        })
        .collect()
}

#[test]
fn translations_link_to_their_original_through_previous_item_id() {
    let mut state = LiveState::default();
    let events = live_events(
        &mut state,
        include_str!("fixtures/alibaba/live_events.jsonl"),
    );
    let source = Some(UtteranceId::Text("item_src_1".into()));
    let expected = [
        CaptionEvent::OriginalDraft {
            utterance: source.clone(),
            text: "Guten ".into(),
        },
        CaptionEvent::OriginalDraft {
            utterance: source.clone(),
            text: "Guten Morgen".into(),
        },
        CaptionEvent::TranslationDraft {
            utterance: source.clone(),
            text: "Good ".into(),
        },
        CaptionEvent::TranslationDraft {
            utterance: source.clone(),
            text: "Good morning".into(),
        },
        // The translation is final before its original; both carry the original's id.
        CaptionEvent::TranslationFinal {
            utterance: source.clone(),
            text: "Good morning".into(),
        },
        CaptionEvent::OriginalFinal {
            utterance: source,
            text: "Guten Morgen.".into(),
        },
    ];
    assert_eq!(events, expected);
}

#[test]
fn live_events_confirm_one_pair_in_the_caption_state() {
    let mut state = LiveState::default();
    let events = live_events(
        &mut state,
        include_str!("fixtures/alibaba/live_events.jsonl"),
    );
    let mut reducer = CaptionReducer::new(ReducerConfig {
        run_id: "run".into(),
        generation: 1,
        sources: vec![Source::System],
        source_language: "de".into(),
        target_language: "en".into(),
    });
    for (at, event) in events.into_iter().enumerate() {
        reducer.apply(Source::System, event, at as u64).unwrap();
    }
    let confirmed = reducer.drain_confirmed();
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].original, "Guten Morgen.");
    assert_eq!(confirmed[0].translation, "Good morning");
}

#[test]
fn unlinked_translations_are_dropped() {
    let mut state = LiveState::default();
    let lines = concat!(
        r#"{"type": "response.text.delta", "item_id": "item_x", "delta": "Orphan"}"#,
        "\n",
        r#"{"type": "response.text.done", "item_id": "item_x", "text": "Orphan"}"#,
    );
    assert!(live_events(&mut state, lines).is_empty());
}

#[test]
fn text_and_stash_form_the_current_draft() {
    let mut state = LiveState::default();
    let line = r#"{"type": "conversation.item.input_audio_transcription.text", "item_id": "a", "text": "Bon", "stash": "jour"}"#;
    let events = live_events(&mut state, line);
    assert_eq!(
        events,
        [CaptionEvent::OriginalDraft {
            utterance: Some(UtteranceId::Text("a".into())),
            text: "Bonjour".into()
        }]
    );
}

#[test]
fn live_failures_and_end_are_reported() {
    let mut state = LiveState::default();
    let failed = state.handle(live::parse(
        r#"{"type": "response.done", "response": {"status": "failed"}}"#,
    ));
    assert!(failed.done);
    assert_eq!(
        failed.events,
        [ProviderEvent::Failed(ProviderError::Provider)]
    );
    let error = state.handle(live::parse(
        r#"{"type": "error", "error": {"type": "x", "code": "InvalidApiKey", "message": "m"}}"#,
    ));
    assert_eq!(
        error.events,
        [ProviderEvent::Failed(ProviderError::Authentication)]
    );
    let finished = state.handle(live::parse(r#"{"type": "session.finished"}"#));
    assert_eq!(finished.events, [ProviderEvent::Ended]);
    assert_eq!(
        live::parse(r#"{"type": "session.created"}"#),
        LiveEvent::SessionCreated
    );
}

fn request(text: &str, source: &str, target: &str) -> TranslationRequest {
    TranslationRequest {
        text: text.into(),
        source_language: source.into(),
        target_language: target.into(),
    }
}

#[test]
fn qwen_mt_request_names_languages_in_translation_options() {
    let body = request_body("qwen-mt-lite", &request("早上好", "zh", "zh_tw")).unwrap();
    assert_eq!(
        body,
        json!({
            "model": "qwen-mt-lite",
            "stream": true,
            "messages": [{"role": "user", "content": "早上好"}],
            "translation_options": {"source_lang": "Chinese", "target_lang": "Traditional Chinese"},
        })
    );
    let auto = request_body("qwen-mt-plus", &request("x", "auto", "en")).unwrap();
    assert_eq!(auto["translation_options"]["source_lang"], "auto");
    assert_eq!(auto["model"], "qwen-mt-plus");
}

#[test]
fn qwen_mt_language_names_follow_the_model() {
    assert_eq!(languages::source_name("ro", "qwen-mt-lite"), "auto");
    assert_eq!(languages::source_name("ro", "qwen-mt-plus"), "Romanian");
    assert_eq!(languages::name("fa", "qwen-mt-lite"), Some("Persian"));
    assert_eq!(
        languages::name("fa", "qwen-mt-plus"),
        Some("Western Persian")
    );
    let unknown = request_body("qwen-mt-lite", &request("x", "auto", "yue"));
    assert_eq!(unknown, Err(ProviderError::Configuration));
}

fn sse_data(raw: &str) -> Vec<String> {
    let mut decoder = SseDecoder::default();
    decoder
        .push(raw.as_bytes())
        .unwrap()
        .into_iter()
        .filter_map(|event| match event {
            SseEvent::Data(data) => Some(data),
            SseEvent::Done => None,
        })
        .collect()
}

#[test]
fn incremental_streams_are_concatenated_without_reasoning() {
    let chunks = sse_data(include_str!("fixtures/alibaba/qwen_mt_lite.sse"));
    assert_eq!(assemble(&chunks, false).unwrap(), "Good morning, everyone.");
}

#[test]
fn snapshot_streams_keep_the_latest_text() {
    let chunks = sse_data(include_str!("fixtures/alibaba/qwen_mt_plus.sse"));
    assert_eq!(assemble(&chunks, true).unwrap(), "Good morning, everyone.");
}

#[test]
fn empty_or_failed_streams_are_errors() {
    assert_eq!(assemble(&[], false), Err(ProviderError::Provider));
    let throttled = vec![r#"{"error": {"code": "Throttling", "message": "m"}}"#.to_owned()];
    assert_eq!(
        assemble(&throttled, false),
        Err(ProviderError::RateLimited { retry_after: None })
    );
}
