//! Route → connection settings: URL, authentication header, session kind
//! (providers/openai.md “路由”). Plans come from request JSON; nothing connects.

#[path = "fixtures/openai/support.rs"]
mod support;

use sheng_runtime::services::openai::{
    realtime_config, LanguageField, RealtimeConfig, SessionKind,
};

fn header(config: &RealtimeConfig, name: &str) -> Option<String> {
    config
        .request
        .headers
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.expose().to_owned())
}

#[test]
fn openai_asr_mt_uses_the_transcription_intent_bearer_and_language_list() {
    let config = realtime_config(&support::openai_plan("asrMt", "zh_tw", "en")).unwrap();

    assert_eq!(
        config.request.url.as_str(),
        "wss://api.openai.com/v1/realtime?intent=transcription"
    );
    assert_eq!(
        header(&config, "authorization"),
        Some(format!("Bearer {}", support::API_KEY))
    );
    assert_eq!(
        config.kind,
        SessionKind::Transcribe {
            model: "gpt-live-transcribe".into(),
            language: Some("zh-tw".into()),
            language_field: LanguageField::List,
        }
    );
    assert!(!config.custom_endpoint);
}

#[test]
fn openai_simultaneous_uses_the_translations_path_with_model_and_output_language() {
    let config = realtime_config(&support::openai_plan("simultaneous", "auto", "fr")).unwrap();

    assert_eq!(
        config.request.url.as_str(),
        "wss://api.openai.com/v1/realtime/translations?model=gpt-realtime-translate"
    );
    assert_eq!(
        config.kind,
        SessionKind::Translate {
            source_transcript_model: "gpt-realtime-whisper".into(),
            output_language: "fr".into(),
        }
    );
}

#[test]
fn auto_source_sends_no_language_hint() {
    let config = realtime_config(&support::openai_plan("asrMt", "auto", "original")).unwrap();

    let SessionKind::Transcribe { language, .. } = config.kind else {
        panic!("transcription route");
    };
    assert_eq!(language, None);
}

#[test]
fn azure_strips_path_and_query_and_uses_the_api_key_header_and_transcription_deployment() {
    let plan = support::azure_plan(
        "https://MyRes.openai.azure.com/openai/deployments/x?api-key=leaked",
        "zh_tw",
    );

    let config = realtime_config(&plan).unwrap();

    assert_eq!(
        config.request.url.as_str(),
        "wss://myres.openai.azure.com/openai/v1/realtime?intent=transcription"
    );
    assert!(!config.request.url.as_str().contains("leaked"));
    assert_eq!(
        header(&config, "api-key"),
        Some(support::API_KEY.to_owned())
    );
    assert_eq!(header(&config, "authorization"), None);
    assert_eq!(
        config.kind,
        SessionKind::Transcribe {
            model: "live-transcribe".into(),
            language: Some("zh".into()),
            language_field: LanguageField::Single,
        }
    );
}

#[test]
fn custom_endpoint_keeps_the_user_url_and_model_and_is_marked_custom() {
    let plan = support::custom_plan("ws://127.0.0.1:9/v1/realtime", "  my-asr  ");

    let config = realtime_config(&plan).unwrap();

    assert_eq!(config.request.url.as_str(), "ws://127.0.0.1:9/v1/realtime");
    assert_eq!(
        header(&config, "authorization"),
        Some(format!("Bearer {}", support::API_KEY))
    );
    assert_eq!(
        config.kind,
        SessionKind::Transcribe {
            model: "my-asr".into(),
            language: Some("en".into()),
            language_field: LanguageField::Single,
        }
    );
    assert!(config.custom_endpoint);
}

#[test]
fn debug_output_never_shows_the_api_key() {
    let configs = [
        realtime_config(&support::openai_plan("asrMt", "en", "fr")).unwrap(),
        realtime_config(&support::azure_plan("https://r.openai.azure.com", "en")).unwrap(),
        realtime_config(&support::custom_plan("wss://asr.example.com/v1", "m")).unwrap(),
    ];

    for config in configs {
        assert!(!format!("{config:?}").contains(support::API_KEY));
    }
}
