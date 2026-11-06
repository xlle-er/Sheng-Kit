//! Shared helpers of the `service_openai*` tests: session plans built from request JSON
//! and the hand-written server event fixtures in this directory.
#![allow(dead_code)]

use serde_json::{json, Value};
use sheng_runtime::config::{check_request, SessionPlan};

pub const API_KEY: &str = "sk-test-placeholder";

/// Lines of a `.jsonl` fixture in `tests/fixtures/openai/`.
pub fn fixture(name: &str) -> Vec<String> {
    let path = format!(
        "{}/tests/fixtures/openai/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path)
        .expect("fixture exists")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn base(provider: &str, credentials: Value, mode: &str, source: &str, target: &str) -> Value {
    json!({
        "input": "system",
        "configuration": {
            "provider": provider,
            "qwenMtModel": "lite",
            "credentials": credentials,
            "sourceLanguage": source,
            "targetLanguage": target,
            "translationMode": "turbo",
            "networkProxy": { "mode": "direct" },
            "textNetworkProxy": { "mode": "direct" },
            "speechTranslationMode": mode,
            "xaiSpeechMode": "voice",
        }
    })
}

pub fn plan(request: Value) -> SessionPlan {
    check_request(&request.to_string()).expect("request is valid")
}

/// `asrMt` routes with a translation target get Apple text translation as the second stage.
pub fn openai_plan(mode: &str, source: &str, target: &str) -> SessionPlan {
    let credentials = json!({ "kind": "apiKey", "apiKey": API_KEY });
    let mut request = base("openAIRealtime", credentials, mode, source, target);
    if mode == "asrMt" && target != "original" {
        request["configuration"]["textCredentials"] = json!({ "kind": "apple" });
    }
    plan(request)
}

pub fn azure_plan(endpoint: &str, source: &str) -> SessionPlan {
    let credentials = json!({
        "kind": "azureOpenAI",
        "endpoint": endpoint,
        "deployment": "realtime-translate",
        "transcriptionDeployment": "live-transcribe",
        "apiKey": API_KEY,
    });
    plan(base(
        "azureOpenAIRealtime",
        credentials,
        "asrMt",
        source,
        "original",
    ))
}

pub fn custom_plan(endpoint: &str, model: &str) -> SessionPlan {
    let credentials = json!({
        "kind": "customSpeech",
        "endpoint": endpoint,
        "model": model,
        "apiKey": API_KEY,
    });
    plan(base(
        "customOpenAIASR",
        credentials,
        "simultaneous",
        "en",
        "original",
    ))
}
