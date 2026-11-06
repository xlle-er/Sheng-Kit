//! Requests for the recorded negative validation cases.

use serde_json::{json, Value};

use super::start_request;

pub fn base() -> Value {
    start_request(
        "system",
        "openAIRealtime",
        "asrMt",
        "openAICompatible",
        "en",
        "zh",
    )
}

pub fn with(mut request: Value, path: &[&str], value: Value) -> Value {
    let (last, parents) = path.split_last().unwrap();
    let mut node = &mut request;
    for key in parents {
        node = &mut node[*key];
    }
    node[*last] = value;
    request
}

pub fn without_text(mut request: Value) -> Value {
    request["configuration"]
        .as_object_mut()
        .unwrap()
        .remove("textCredentials");
    request
}

/// Request for one named case of `golden/validate-negative.json`.
pub fn negative_case(name: &str) -> String {
    let request = match name {
        "valid-both-input" => with(base(), &["input"], json!("both")),
        "unknown-top-level-field" => with(base(), &["extra"], json!(1)),
        "unknown-configuration-field" => with(base(), &["configuration", "extra"], json!(1)),
        "unknown-input" => with(base(), &["input"], json!("speaker")),
        "unknown-provider" => with(base(), &["configuration", "provider"], json!("acme")),
        "empty-api-key" => with(
            base(),
            &["configuration", "credentials", "apiKey"],
            json!(""),
        ),
        "mismatched-credential-kind" => with(
            base(),
            &["configuration", "credentials"],
            json!({"kind": "appleSpeech"}),
        ),
        "unknown-source-language" => {
            with(base(), &["configuration", "sourceLanguage"], json!("xx"))
        }
        "unknown-target-language" => {
            with(base(), &["configuration", "targetLanguage"], json!("xx"))
        }
        "custom-proxy-without-url" => with(
            base(),
            &["configuration", "networkProxy"],
            json!({"mode": "custom"}),
        ),
        "custom-socks-proxy" => with(
            base(),
            &["configuration", "networkProxy"],
            json!({"mode": "custom", "url": "socks5://127.0.0.1:1080"}),
        ),
        "proxy-none" => with(
            base(),
            &["configuration", "networkProxy"],
            json!({"mode": "none"}),
        ),
        "asr-mt-without-text-credentials" => without_text(base()),
        "qwen-model-plus" => with(base(), &["configuration", "qwenMtModel"], json!("plus")),
        "qwen-model-turbo" => with(base(), &["configuration", "qwenMtModel"], json!("turbo")),
        "translation-mode-other" => {
            with(base(), &["configuration", "translationMode"], json!("fast"))
        }
        "custom-speech-https-endpoint" | "custom-speech-http-endpoint" => {
            let scheme = if name.ends_with("https-endpoint") {
                "https"
            } else {
                "http"
            };
            let request = start_request(
                "system",
                "customOpenAIASR",
                "asrMt",
                "openAICompatible",
                "en",
                "zh",
            );
            with(
                request,
                &["configuration", "credentials", "endpoint"],
                json!(format!("{scheme}://speech.example.com/v1")),
            )
        }
        "azure-non-azure-host" => {
            let request = start_request(
                "system",
                "azureOpenAIRealtime",
                "asrMt",
                "openAICompatible",
                "en",
                "zh",
            );
            with(
                request,
                &["configuration", "credentials", "endpoint"],
                json!("https://example.com"),
            )
        }
        "not-json" => return "{not json".to_owned(),
        "oversize-request" => {
            return with(
                base(),
                &["configuration", "alibabaWorkspaceId"],
                json!("x".repeat(70_000)),
            )
            .to_string()
        }
        other => panic!("unhandled negative case {other}"),
    };
    request.to_string()
}
