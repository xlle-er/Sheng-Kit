//! Text translator wire format, written from the vendors' public docs (step 4b): request JSON,
//! language codes, streamed chat decoding and client construction from a validated plan.

use serde_json::{json, Value};
use sheng_runtime::config::check_request;
use sheng_runtime::provider::{ProviderError, SessionError, TranslationRequest};
use sheng_runtime::services::text::{
    chat_request_body, deepl_request_body, deepl_source, deepl_target, deeplx_request_body,
    thinking_switch, translator, ChatDecoder,
};

fn request(source: &str, target: &str) -> TranslationRequest {
    TranslationRequest {
        text: "Good morning".into(),
        source_language: source.into(),
        target_language: target.into(),
    }
}

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/text/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).expect("fixture")
}

fn decode(bytes: &[u8], step: usize) -> Result<String, ProviderError> {
    let mut decoder = ChatDecoder::default();
    for chunk in bytes.chunks(step) {
        decoder.push(chunk)?;
    }
    decoder.finish()
}

#[test]
fn chat_body_streams_one_sentence_with_instruction() {
    let body = chat_request_body(
        "https://api.openai.com/v1/chat/completions",
        "gpt-4.1-mini",
        &request("en", "zh"),
    );
    assert_eq!(body["model"], "gpt-4.1-mini");
    assert_eq!(body["stream"], true);
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "system");
    let instruction = messages[0]["content"].as_str().unwrap();
    assert!(instruction.contains("Simplified Chinese"));
    assert!(instruction.contains("Output only the translation"));
    assert_eq!(
        messages[1],
        json!({"role": "user", "content": "Good morning"})
    );
    assert!(body.get("enable_thinking").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

/// Endpoint, model and the expected thinking switch.
type ThinkingCase = (&'static str, &'static str, Option<(&'static str, Value)>);

#[test]
fn thinking_is_switched_off_with_each_service_parameter() {
    let cases: [ThinkingCase; 6] = [
        (
            "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
            "qwen3-8b",
            Some(("enable_thinking", json!(false))),
        ),
        (
            "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
            "qwen-mt-flash",
            None,
        ),
        (
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
            "gemini-2.5-flash",
            Some(("reasoning_effort", json!("none"))),
        ),
        (
            "http://127.0.0.1:11434/v1/chat/completions",
            "qwen3:8b",
            Some(("reasoning_effort", json!("none"))),
        ),
        ("https://api.x.ai/v1/chat/completions", "grok-4.3", None),
        (
            "https://api.deepseek.com/chat/completions",
            "deepseek-chat",
            None,
        ),
    ];
    for (endpoint, model, expected) in cases {
        assert_eq!(thinking_switch(endpoint, model), expected, "{model}");
        let body = chat_request_body(endpoint, model, &request("en", "ja"));
        if let Some((name, value)) = thinking_switch(endpoint, model) {
            assert_eq!(body[name], value);
        }
    }
}

#[test]
fn deepl_codes_follow_the_documented_variants() {
    assert_eq!(deepl_target("zh"), "ZH-HANS");
    assert_eq!(deepl_target("zh_tw"), "ZH-HANT");
    assert_eq!(deepl_target("en"), "EN-US");
    assert_eq!(deepl_target("en-GB"), "EN-GB");
    assert_eq!(deepl_target("pt"), "PT-BR");
    assert_eq!(deepl_target("pt-PT"), "PT-PT");
    assert_eq!(deepl_target("es-419"), "ES-419");
    assert_eq!(deepl_target("es-ES"), "ES");
    assert_eq!(deepl_target("no"), "NB");
    assert_eq!(deepl_target("ja"), "JA");
    assert_eq!(deepl_source("auto"), None);
    assert_eq!(deepl_source("zh").as_deref(), Some("ZH"));
    assert_eq!(deepl_source("en").as_deref(), Some("EN"));
}

#[test]
fn deepl_body_lists_text_and_omits_auto_source() {
    let body = deepl_request_body(&request("en", "zh"));
    assert_eq!(
        body,
        json!({"text": ["Good morning"], "source_lang": "EN", "target_lang": "ZH-HANS"})
    );
    let body = deepl_request_body(&request("auto", "de"));
    assert_eq!(body, json!({"text": ["Good morning"], "target_lang": "DE"}));
}

#[test]
fn deeplx_body_uses_plain_text_field() {
    let body = deeplx_request_body(&request("auto", "zh"));
    assert_eq!(
        body,
        json!({"text": "Good morning", "source_lang": "auto", "target_lang": "ZH-HANS"})
    );
}

#[test]
fn chat_stream_joins_content_and_drops_thinking_fields() {
    let stream = fixture("chat_stream.txt");
    assert_eq!(decode(stream.as_bytes(), 4096).unwrap(), "早上好，世界");
}

#[test]
fn chat_stream_survives_any_chunk_boundary_inside_utf8() {
    let stream = fixture("chat_stream.txt");
    for step in 1..=7 {
        assert_eq!(
            decode(stream.as_bytes(), step).unwrap(),
            "早上好，世界",
            "step {step}"
        );
    }
}

#[test]
fn chat_stream_without_done_or_finish_reason_is_temporary() {
    let stream = fixture("chat_stream.txt");
    let stop = stream.find("\"finish_reason\":\"stop\"").unwrap();
    let cut = stream[..stop].rfind("data:").unwrap();
    assert_eq!(
        decode(&stream.as_bytes()[..cut], 64),
        Err(ProviderError::Unavailable { retry_after: None })
    );
}

#[test]
fn chat_stream_ending_after_a_finish_reason_is_complete() {
    // Step 4 answer 4b-7: `[DONE]` or a chunk with `finish_reason` followed by a normal end.
    let stream = fixture("chat_stream.txt");
    let cut = stream.find("data: [DONE]").unwrap();
    assert_eq!(
        decode(&stream.as_bytes()[..cut], 64).unwrap(),
        "早上好，世界"
    );
}

#[test]
fn chat_stream_error_object_and_empty_result_are_service_errors() {
    let error = "data: {\"error\":{\"message\":\"m\",\"type\":\"server_error\"}}\n\n";
    assert_eq!(decode(error.as_bytes(), 16), Err(ProviderError::Provider));
    let empty =
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\"}}]}\n\ndata: [DONE]\n\n";
    assert_eq!(decode(empty.as_bytes(), 16), Err(ProviderError::Provider));
}

fn plan(text: Value) -> sheng_runtime::config::SessionPlan {
    let json = json!({"input": "system", "configuration": {
        "provider": "appleSpeech", "qwenMtModel": "lite",
        "credentials": {"kind": "appleSpeech"}, "textCredentials": text,
        "sourceLanguage": "en", "targetLanguage": "zh", "translationMode": "turbo",
        "networkProxy": {"mode": "direct"}, "textNetworkProxy": {"mode": "direct"},
        "speechTranslationMode": "simultaneous", "xaiSpeechMode": "voice"}});
    check_request(&json.to_string()).expect("valid request")
}

#[test]
fn factory_builds_every_text_kind() {
    for text in [
        json!({"kind": "deepL", "apiKey": "placeholder:fx"}),
        json!({"kind": "deepLX", "endpoint": "http://127.0.0.1:1188", "token": ""}),
        json!({"kind": "xAI", "apiKey": "placeholder", "model": "grok-4.3"}),
        json!({"kind": "openAICompatible", "endpoint": "http://localhost:11434/v1",
            "model": "qwen3:8b", "apiKey": ""}),
        json!({"kind": "chatMock", "endpoint": "https://llm.example.com/v1",
            "model": "m", "apiKey": "k"}),
    ] {
        assert!(translator(&plan(text)).is_ok());
    }
}

#[test]
fn factory_rejects_plans_without_an_independent_text_service() {
    let result = translator(&plan(json!({"kind": "apple"})));
    assert!(matches!(
        result,
        Err(SessionError::ProviderConfigurationFailed)
    ));
}
