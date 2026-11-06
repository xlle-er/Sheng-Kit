//! Builds requests the way the Swift caller does (ServiceProfile.request / capabilities),
//! with placeholder credentials that pass validation.
#![allow(dead_code)]

pub mod http;
pub mod negative;
pub mod opened;
pub mod overflow;

use serde_json::{json, Map, Value};

pub fn golden(name: &str) -> Value {
    let path = format!(
        "{}/../testdata/golden/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(path).expect("golden file exists");
    serde_json::from_str(&text).expect("golden parses")
}

const SPEECH_CHOICE: &[&str] = &["openAIRealtime", "googleGeminiLive", "azureOpenAIRealtime"];
const ASR_MT_CAPABLE: &[&str] = &[
    "alibabaCloud",
    "openAIRealtime",
    "azureOpenAIRealtime",
    "googleGeminiLive",
    "xAIRealtime",
];

pub fn uses_independent_text(provider: &str, speech_mode: &str) -> bool {
    matches!(
        provider,
        "appleSpeech" | "customDashScopeASR" | "customOpenAIASR"
    ) || (speech_mode == "asrMt" && ASR_MT_CAPABLE.contains(&provider))
}

pub fn credentials(provider: &str) -> Value {
    match provider {
        "appleSpeech" => json!({"kind": "appleSpeech"}),
        "customDashScopeASR" | "customOpenAIASR" => json!({
            "kind": "customSpeech", "endpoint": "wss://speech.example.com/v1",
            "model": "asr-model", "apiKey": "placeholder-key"}),
        "azureOpenAIRealtime" => json!({
            "kind": "azureOpenAI", "endpoint": "https://resource.openai.azure.com",
            "deployment": "translate", "transcriptionDeployment": "transcribe",
            "apiKey": "placeholder-key"}),
        "tencentCloud" => json!({
            "kind": "tencentCloud", "appId": "1250000000", "secretId": "id", "secretKey": "key"}),
        "baiduTranslate" => json!({"kind": "baiduTranslate", "appId": "app", "appKey": "key"}),
        _ => json!({"kind": "apiKey", "apiKey": "placeholder-key"}),
    }
}

pub fn text_credentials(kind: &str) -> Value {
    match kind {
        "apple" => json!({"kind": "apple"}),
        "deepL" => json!({"kind": "deepL", "apiKey": "placeholder:fx"}),
        "deepLX" => {
            json!({"kind": "deepLX", "endpoint": "https://deeplx.example.com", "token": "t"})
        }
        "xAI" => json!({"kind": "xAI", "apiKey": "placeholder-key", "model": "grok-4.3"}),
        other => json!({"kind": other, "endpoint": "https://llm.example.com/v1",
            "model": "translate-model", "apiKey": "placeholder-key"}),
    }
}

/// Mirrors `ServiceProfile.request(input:source:target:)`.
pub fn start_request(
    input: &str,
    provider: &str,
    speech_mode: &str,
    text: &str,
    source: &str,
    target: &str,
) -> Value {
    let mut configuration = Map::new();
    let speech_translation = if SPEECH_CHOICE.contains(&provider) {
        speech_mode
    } else {
        "simultaneous"
    };
    let xai = if provider == "xAIRealtime" && speech_mode == "asrMt" {
        "asrMt"
    } else {
        "voice"
    };
    for (key, value) in [
        ("provider", json!(provider)),
        ("qwenMtModel", json!("lite")),
        ("credentials", credentials(provider)),
        ("sourceLanguage", json!(source)),
        ("targetLanguage", json!(target)),
        ("translationMode", json!("turbo")),
        ("networkProxy", json!({"mode": "system"})),
        ("textNetworkProxy", json!({"mode": "system"})),
        ("speechTranslationMode", json!(speech_translation)),
        ("xaiSpeechMode", json!(xai)),
        ("alibabaWorkspaceId", json!("")),
    ] {
        configuration.insert(key.to_owned(), value);
    }
    if provider == "alibabaCloud" && speech_mode == "simultaneous" {
        configuration.insert("alibabaRealtimeMode".into(), json!("liveTranslate"));
    }
    let wants_text = target != "original" && uses_independent_text(provider, speech_mode);
    if wants_text && text != "followService" && text != "-" {
        configuration.insert("textCredentials".into(), text_credentials(text));
    }
    json!({"input": input, "configuration": configuration})
}

/// Mirrors `ServiceProfile.capabilities(source:target:)`; `speechTranslationMode` is given
/// as the start request would carry it.
pub fn capability_request(
    provider: &str,
    speech_mode: &str,
    text: &str,
    source: &str,
    target: &str,
) -> Value {
    let speech_translation = if SPEECH_CHOICE.contains(&provider) {
        speech_mode
    } else {
        "simultaneous"
    };
    raw_capability_request(
        provider,
        speech_translation,
        speech_mode,
        text,
        source,
        target,
    )
}

pub fn raw_capability_request(
    provider: &str,
    speech_translation: &str,
    speech_mode: &str,
    text: &str,
    source: &str,
    target: &str,
) -> Value {
    let mut profile = Map::new();
    profile.insert("id".into(), json!("00000000-0000-0000-0000-000000000000"));
    profile.insert("name".into(), json!("profile"));
    profile.insert("provider".into(), json!(provider));
    profile.insert("qwenMtModel".into(), json!("lite"));
    profile.insert("speechTranslationMode".into(), json!(speech_translation));
    let xai = if provider == "xAIRealtime" && speech_mode == "asrMt" {
        "asrMt"
    } else {
        "voice"
    };
    profile.insert("xaiSpeechMode".into(), json!(xai));
    let live = provider == "alibabaCloud" && speech_mode == "simultaneous";
    profile.insert(
        "alibabaRealtimeMode".into(),
        json!(if live { "liveTranslate" } else { "disabled" }),
    );
    if uses_independent_text(provider, speech_mode) && text != "-" {
        profile.insert("textTranslation".into(), json!(text));
    }
    json!({"profile": profile, "source": source, "target": target})
}

pub fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("missing {key}"))
}
