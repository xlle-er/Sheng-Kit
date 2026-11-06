//! Real-service smoke tests for the text translators (step4-common.md “测试” 3). Ignored by
//! default; each runs only when `SHENG_LIVE_<SERVICE>=1` and its credentials are set.
//! Nothing from the request, the reply or the credentials is printed.
//!
//! ```text
//! SHENG_LIVE_DEEPL=1 SHENG_DEEPL_API_KEY=… cargo test -p sheng-runtime --test service_text_live -- --ignored
//! SHENG_LIVE_DEEPLX=1 SHENG_DEEPLX_ENDPOINT=… [SHENG_DEEPLX_TOKEN=…]
//! SHENG_LIVE_OPENAI_COMPAT=1 SHENG_OPENAI_COMPAT_ENDPOINT=… SHENG_OPENAI_COMPAT_MODEL=… [SHENG_OPENAI_COMPAT_API_KEY=…]
//! SHENG_LIVE_XAI_TEXT=1 SHENG_XAI_API_KEY=… [SHENG_XAI_TEXT_MODEL=grok-4.3]
//! ```

use serde_json::{json, Value};
use sheng_runtime::config::check_request;
use sheng_runtime::provider::TranslationRequest;
use sheng_runtime::services::text::translator;

fn enabled(flag: &str) -> bool {
    std::env::var(flag).is_ok_and(|value| value == "1")
}

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// Translates one sentence with the configured service and asserts a non-empty result.
async fn smoke(text_credentials: Value) {
    let json = json!({"input": "system", "configuration": {
        "provider": "appleSpeech", "qwenMtModel": "lite",
        "credentials": {"kind": "appleSpeech"}, "textCredentials": text_credentials,
        "sourceLanguage": "en", "targetLanguage": "zh", "translationMode": "turbo",
        "networkProxy": {"mode": "system"}, "textNetworkProxy": {"mode": "system"},
        "speechTranslationMode": "simultaneous", "xaiSpeechMode": "voice"}});
    let plan = check_request(&json.to_string()).expect("configuration is valid");
    let client = translator(&plan).expect("client is created");
    let request = TranslationRequest {
        text: "Good morning, everyone.".into(),
        source_language: "en".into(),
        target_language: "zh".into(),
    };
    let result = client.translate(request).await;
    assert!(
        matches!(&result, Ok(text) if !text.trim().is_empty()),
        "translation failed: {:?}",
        result.err()
    );
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_DEEPL=1 and SHENG_DEEPL_API_KEY"]
async fn deepl_translates_one_sentence() {
    let (true, Some(key)) = (enabled("SHENG_LIVE_DEEPL"), env("SHENG_DEEPL_API_KEY")) else {
        return;
    };
    smoke(json!({"kind": "deepL", "apiKey": key})).await;
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_DEEPLX=1 and SHENG_DEEPLX_ENDPOINT"]
async fn deeplx_translates_one_sentence() {
    let (true, Some(endpoint)) = (enabled("SHENG_LIVE_DEEPLX"), env("SHENG_DEEPLX_ENDPOINT"))
    else {
        return;
    };
    let token = env("SHENG_DEEPLX_TOKEN").unwrap_or_default();
    smoke(json!({"kind": "deepLX", "endpoint": endpoint, "token": token})).await;
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_OPENAI_COMPAT=1, SHENG_OPENAI_COMPAT_ENDPOINT and _MODEL"]
async fn openai_compatible_translates_one_sentence() {
    let configured = (
        enabled("SHENG_LIVE_OPENAI_COMPAT"),
        env("SHENG_OPENAI_COMPAT_ENDPOINT"),
        env("SHENG_OPENAI_COMPAT_MODEL"),
    );
    let (true, Some(endpoint), Some(model)) = configured else {
        return;
    };
    let key = env("SHENG_OPENAI_COMPAT_API_KEY").unwrap_or_default();
    smoke(
        json!({"kind": "openAICompatible", "endpoint": endpoint, "model": model,
        "apiKey": key}),
    )
    .await;
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_XAI_TEXT=1 and SHENG_XAI_API_KEY"]
async fn xai_text_translates_one_sentence() {
    let (true, Some(key)) = (enabled("SHENG_LIVE_XAI_TEXT"), env("SHENG_XAI_API_KEY")) else {
        return;
    };
    let model = env("SHENG_XAI_TEXT_MODEL").unwrap_or_else(|| "grok-4.3".into());
    smoke(json!({"kind": "xAI", "apiKey": key, "model": model})).await;
}
