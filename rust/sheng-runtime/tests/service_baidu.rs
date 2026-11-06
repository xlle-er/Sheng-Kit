//! Baidu protocol units: START frame, language codes, message parsing and result mapping.

mod common;

use serde_json::Value;
use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::catalog::languages::BAIDU_LANGUAGES;
use sheng_runtime::config::check_request;
use sheng_runtime::config::secret::Secret;
use sheng_runtime::provider::{ProviderError, SessionError};
use sheng_runtime::services::baidu::protocol::{
    classify_error, finish_frame, frame_bytes, is_recoverable, parse_message, start_frame,
    ServerMessage, Utterances,
};
use sheng_runtime::services::baidu::{baidu_code, session_for, speech, StartParams, BAIDU_CODES};
use url::Url;

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/baidu/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).expect("fixture exists")
}

fn segment(name: &str) -> sheng_runtime::services::baidu::protocol::Segment {
    match parse_message(&fixture(name)) {
        Some(ServerMessage::Result(segment)) => segment,
        other => panic!("not a result: {other:?}"),
    }
}

fn id(n: u64) -> Option<UtteranceId> {
    Some(UtteranceId::Number(n))
}

#[test]
fn start_frame_has_documented_fields() {
    let params = StartParams {
        from: "zh",
        to: "en",
        app_id: Secret::new("app-1"),
        app_key: Secret::new("key-1"),
        sampling_rate: 16_000,
    };
    let frame: Value = serde_json::from_str(&start_frame(&params)).unwrap();
    assert_eq!(frame["type"], "START");
    assert_eq!(frame["from"], "zh");
    assert_eq!(frame["to"], "en");
    assert_eq!(frame["app_id"], "app-1");
    assert_eq!(frame["app_key"], "key-1");
    assert_eq!(frame["sampling_rate"], 16_000);
    assert_eq!(frame["return_target_tts"], false);
    assert_eq!(frame.as_object().unwrap().len(), 7);
    let debug = format!("{params:?}");
    assert!(!debug.contains("key-1") && !debug.contains("app-1"));
}

#[test]
fn finish_frame_and_audio_packet_size() {
    let frame: Value = serde_json::from_str(&finish_frame()).unwrap();
    assert_eq!(frame, serde_json::json!({"type": "FINISH"}));
    assert_eq!(frame_bytes(16_000), 1280);
    assert_eq!(frame_bytes(8_000), 640);
}

#[test]
fn every_catalog_language_has_a_documented_code() {
    assert_eq!(BAIDU_CODES.len(), 45);
    assert_eq!(BAIDU_LANGUAGES.len(), BAIDU_CODES.len());
    for code in BAIDU_LANGUAGES {
        assert!(baidu_code(code).is_some(), "{code} has no Baidu code");
    }
    let mut baidu: Vec<_> = BAIDU_CODES.iter().map(|(_, b)| *b).collect();
    baidu.sort_unstable();
    baidu.dedup();
    assert_eq!(baidu.len(), 45, "Baidu codes are unique");
}

#[test]
fn language_codes_follow_the_document() {
    let expected = [
        ("ja", "jp"),
        ("ko", "kor"),
        ("fr", "fra"),
        ("es", "spa"),
        ("ar", "ara"),
        ("vi", "vie"),
        ("tl", "fil"),
        ("km", "hkm"),
        ("hy", "arm"),
        ("zh", "zh"),
        ("yue", "yue"),
    ];
    for (catalog, baidu) in expected {
        assert_eq!(baidu_code(catalog), Some(baidu), "{catalog}");
    }
    assert_eq!(baidu_code("auto"), None);
    assert_eq!(baidu_code("original"), None);
}

#[test]
fn parses_control_messages() {
    assert_eq!(
        parse_message(&fixture("started")),
        Some(ServerMessage::Started)
    );
    assert_eq!(parse_message(&fixture("ended")), Some(ServerMessage::Ended));
    assert_eq!(
        parse_message(&fixture("auth_error")),
        Some(ServerMessage::Error(31003))
    );
    assert_eq!(parse_message("not json"), None);
    assert_eq!(
        parse_message(r#"{"code":0,"data":{"status":"NEW"}}"#),
        Some(ServerMessage::Other)
    );
}

#[test]
fn error_codes_are_classified() {
    assert_eq!(classify_error(31003), ProviderError::Authentication);
    assert_eq!(classify_error(41017), ProviderError::Authentication);
    assert_eq!(classify_error(20302), ProviderError::Configuration);
    assert_eq!(classify_error(10001), ProviderError::Configuration);
    assert_eq!(classify_error(10000), ProviderError::Provider);
    assert_eq!(classify_error(41015), ProviderError::Provider);
    for code in [20311, 20312, 20313, 20315] {
        assert!(is_recoverable(code));
    }
    assert!(!is_recoverable(20314));
}

#[test]
fn mid_with_a_translation_is_a_preview_pair_of_the_current_sentence() {
    // Step 4 answer 4h-4: `asr_trans` is shown as the sentence's preview (captions.md §2.2).
    let mut utterances = Utterances::default();
    let events = utterances.map(segment("mid"));
    assert_eq!(
        events,
        vec![CaptionEvent::PreviewPair {
            utterance: id(1),
            original: "早上".into(),
            translation: "Morning".into()
        }]
    );
}

#[test]
fn mid_without_a_translation_is_a_draft_of_the_current_sentence() {
    let mut utterances = Utterances::default();
    let events = utterances.map(segment("mid_untranslated"));
    assert_eq!(
        events,
        vec![CaptionEvent::OriginalDraft {
            utterance: id(1),
            text: "早上".into()
        }]
    );
}

#[test]
fn fin_confirms_and_carries_the_next_draft_separately() {
    let mut utterances = Utterances::default();
    utterances.map(segment("mid"));
    let events = utterances.map(segment("fin_with_next_draft"));
    assert_eq!(
        events,
        vec![
            CaptionEvent::ConfirmedPair {
                utterance: id(1),
                original: "早上好，".into(),
                translation: "Good morning,".into()
            },
            CaptionEvent::OriginalDraft {
                utterance: id(2),
                text: "下午".into()
            },
        ]
    );
}

#[test]
fn untranslated_fin_is_an_original_final_only() {
    let mut utterances = Utterances::default();
    let events = utterances.map(segment("fin_untranslated"));
    assert_eq!(
        events,
        vec![CaptionEvent::OriginalFinal {
            utterance: id(1),
            text: "谢谢。".into()
        }]
    );
    let next = utterances.map(segment("mid"));
    assert!(matches!(
        &next[0],
        CaptionEvent::PreviewPair { utterance, .. } if *utterance == id(2)
    ));
}

fn plan(source: &str, target: &str) -> sheng_runtime::config::SessionPlan {
    let request = common::start_request("system", "baiduTranslate", "-", "-", source, target);
    check_request(&request.to_string()).expect("valid Baidu request")
}

#[test]
fn session_maps_plan_languages_and_credentials() {
    let endpoint = Url::parse("ws://127.0.0.1:9/").unwrap();
    let session = session_for(&plan("ja", "zh"), endpoint).unwrap();
    assert_eq!(session.start.from, "jp");
    assert_eq!(session.start.to, "zh");
    assert_eq!(session.start.sampling_rate, 16_000);
    assert_eq!(session.start.app_id.expose(), "app");
    assert_eq!(session.start.app_key.expose(), "key");
}

#[test]
fn factory_entry_creates_a_client_without_connecting() {
    assert!(speech(&plan("en", "zh"), sheng_core::Source::System).is_ok());
}

#[test]
fn other_credentials_are_a_configuration_failure() {
    let mut plan = plan("en", "zh");
    let tencent = common::start_request("system", "tencentCloud", "-", "-", "en", "zh");
    plan.credentials = check_request(&tencent.to_string()).unwrap().credentials;
    let endpoint = Url::parse("ws://127.0.0.1:9/").unwrap();
    assert!(matches!(
        session_for(&plan, endpoint),
        Err(SessionError::ProviderConfigurationFailed)
    ));
}
