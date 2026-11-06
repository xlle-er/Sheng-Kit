//! Connection checks and the operation limit. Runs in its own process because it replaces
//! the default client factory.

mod common;

use std::ffi::c_char;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::requests::start_request;
use common::*;
use serde_json::{json, Value};
use sheng_core::Source;
use sheng_engine::abi::operations::*;
use sheng_runtime::provider::scripted::{ScriptedSpeech, ScriptedTranslator, SpeechScript};

fn probe(request: &Value, kind: u32) -> u64 {
    let text = c(&request.to_string());
    // SAFETY: a valid NUL-terminated string.
    unsafe { sheng_probe(text.as_ptr(), kind) }
}

fn finish(id: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let state = take_json(sheng_resources_poll(id));
        if state["state"] != "pending" {
            sheng_resources_cancel(id);
            return state;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn recognizing() -> Value {
    start_request(
        "system",
        "openAIRealtime",
        "asrMt",
        "openAICompatible",
        "en",
        "zh",
    )
}

#[test]
fn probes_report_availability_configuration_and_the_operation_limit() {
    // A factory without a speech client; the real one would contact the service.
    sheng_engine::testing::set_default(Some(Arc::new(TestFactory::new(Vec::new()))));
    let unconfigured = finish(probe(&recognizing(), 0));
    sheng_engine::testing::set_default(None);
    assert_eq!(
        unconfigured,
        json!({"state": "failed", "error": "provider_configuration_failed"})
    );
    let mut invalid = recognizing();
    invalid["configuration"]["sourceLanguage"] = json!("xx");
    assert_eq!(probe(&invalid, 0), 0);
    let original = start_request("system", "openAIRealtime", "asrMt", "-", "en", "original");
    assert_eq!(
        finish(probe(&original, 1)),
        json!({"state": "failed", "error": "text_translation_not_configured"})
    );

    let (speech, _) = ScriptedSpeech::new(SpeechScript::default());
    let factory = TestFactory::new(vec![(Source::System, Box::new(speech))])
        .with_translator(ScriptedTranslator::new([]));
    sheng_engine::testing::set_default(Some(Arc::new(factory)));
    let speech = finish(probe(&recognizing(), 0));
    let text = finish(probe(&recognizing(), 1));
    sheng_engine::testing::set_default(None);

    assert_eq!(speech["value"]["service"], "available");
    assert!(speech["value"]["elapsedMs"].is_u64());
    assert_eq!(text["value"]["service"], "available");

    let language = c("en");
    let start = |kind: u32| {
        let pointer: *const c_char = language.as_ptr();
        // SAFETY: valid strings.
        unsafe { sheng_resources_start(kind, pointer, pointer) }
    };
    let ids: Vec<u64> = (0..8).map(|_| start(0)).collect();
    assert!(ids.iter().all(|id| *id != 0));
    assert_eq!(start(0), 0);
    assert_eq!(probe(&recognizing(), 0), 0);
    sheng_resources_cancel(ids[0]);
    let again = start(0);
    assert_ne!(again, 0);
    for id in ids.into_iter().chain([again]) {
        sheng_resources_cancel(id);
    }
}
