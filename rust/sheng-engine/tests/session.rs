//! Sessions behind the C functions with scripted service clients (snapshot.md).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use serde_json::{json, Value};
use sheng_core::{CaptionEvent, Source};
use sheng_engine::abi::engine::*;
use sheng_runtime::provider::scripted::{ScriptedSpeech, ScriptedTranslator, SpeechScript};
use sheng_runtime::provider::{ProviderEvent, SpeechClient};
use tokio::sync::Notify;

const SYSTEM: u32 = 0;
const MICROPHONE: u32 = 1;
/// 100 ms of 16 kHz PCM16.
const CHUNK: [u8; 3200] = [0; 3200];

fn pair(id: u64, original: &str, translation: &str) -> ProviderEvent {
    ProviderEvent::Caption(CaptionEvent::ConfirmedPair {
        utterance: Some(id.into()),
        original: original.into(),
        translation: translation.into(),
    })
}

fn scripted(
    script: SpeechScript,
) -> (
    Box<dyn SpeechClient>,
    sheng_runtime::provider::scripted::SharedLog,
) {
    let (client, log) = ScriptedSpeech::new(script);
    (Box::new(client), log)
}

/// One caption per audio chunk, chunk `n` carries utterance `n + offset`.
fn per_chunk(chunks: usize, offset: u64) -> SpeechScript {
    SpeechScript {
        on_audio: (1..=chunks)
            .map(|n| {
                let id = n as u64 + offset;
                (
                    n,
                    vec![pair(id, &format!("原文 {id}"), &format!("text {id}"))],
                )
            })
            .collect(),
        ..SpeechScript::default()
    }
}

fn recent_len(snapshot: &Value) -> usize {
    snapshot["recent"].as_array().unwrap().len()
}

#[test]
fn session_moves_through_connecting_listening_stopping_and_idle() {
    let (client, log) = scripted(per_chunk(1, 0));
    let handle = handle_with(TestFactory::new(vec![(Source::System, client)]));
    let fresh = snapshot(handle);

    assert_eq!(
        start(handle, &translating_request("system")),
        json!({"ok": true})
    );
    let started = snapshot(handle);
    assert!(matches!(status(&started), "connecting" | "listening"));
    assert_eq!(started["generation"], 1);
    assert_eq!(started["settled"], false);
    assert!(started["revision"].as_u64() > fresh["revision"].as_u64());
    wait_for(handle, |s| status(s) == "listening");
    assert_eq!(push(handle, SYSTEM, &CHUNK), 0);
    assert_eq!(push(handle, MICROPHONE, &CHUNK), -1);
    let shown = wait_for(handle, |s| recent_len(s) == 1);
    assert_eq!(shown["tracks"][0]["original"], "原文 1");
    assert_eq!(shown["tracks"][0]["isFinal"], true);
    sheng_engine_stop(handle);
    assert!(matches!(status(&snapshot(handle)), "stopping" | "idle"));
    assert_eq!(push(handle, SYSTEM, &CHUNK), -1);
    let done = wait_settled(handle);

    assert_eq!(status(&done), "idle");
    assert_eq!(done["error"], Value::Null);
    let log = log.snapshot();
    assert!(log.finished && log.disconnected);
    assert_eq!(log.audio.len(), CHUNK.len());
    let captions = drain(handle);
    assert_eq!(captions.len(), 1);
    let run_id = done["runId"].as_str().unwrap();
    assert_eq!(captions[0]["id"], format!("{run_id}:1:0"));
    assert_eq!(captions[0]["source"], "system");
    assert_eq!(captions[0]["sourceLanguage"], "zh");
    assert!(drain(handle).is_empty());
    sheng_engine_destroy(handle);
}

#[test]
fn listening_waits_until_both_sources_are_connected() {
    let gate = Arc::new(Notify::new());
    let (system, _) = scripted(SpeechScript::default());
    let (mic, _) = scripted(SpeechScript::default());
    let gated = Box::new(GatedSpeech {
        inner: mic,
        gate: Arc::clone(&gate),
    });
    let handle = handle_with(TestFactory::new(vec![
        (Source::System, system),
        (Source::Microphone, gated),
    ]));

    assert_eq!(start(handle, &translating_request("both"))["ok"], true);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(status(&snapshot(handle)), "connecting");
    gate.notify_one();
    let listening = wait_for(handle, |s| status(s) == "listening");

    assert!(listening["echo"].is_object());
    assert_eq!(listening["diagnostics"].as_array().unwrap().len(), 2);
    sheng_engine_destroy(handle);
}

#[test]
fn full_send_queue_returns_minus_two_and_fails_the_session() {
    let (client, _) = scripted(SpeechScript {
        send_delay: Some(Duration::from_secs(10)),
        ..SpeechScript::default()
    });
    let handle = handle_with(TestFactory::new(vec![(Source::System, client)]));
    let large = vec![0u8; 32_000];

    assert_eq!(start(handle, &translating_request("system"))["ok"], true);
    let results: Vec<i32> = (0..3).map(|_| push(handle, SYSTEM, &large)).collect();

    assert_eq!(results, vec![0, 0, -2]);
    let failed = snapshot(handle);
    assert_eq!(status(&failed), "failed");
    assert_eq!(failed["error"], "audio_backpressure");
    assert_eq!(push(handle, SYSTEM, &CHUNK), -1);
    let settled = wait_settled(handle);
    assert_eq!(status(&settled), "failed");
    assert_eq!(start(handle, &translating_request("system"))["ok"], true);
    assert_eq!(snapshot(handle)["generation"], 2);
    sheng_engine_destroy(handle);
}

#[test]
fn stop_with_a_stuck_transport_fails_with_audio_backpressure() {
    let (client, _) = scripted(SpeechScript {
        send_delay: Some(Duration::from_secs(5)),
        ..SpeechScript::default()
    });
    let handle = handle_with(TestFactory::new(vec![(Source::System, client)]));

    start(handle, &translating_request("system"));
    wait_for(handle, |s| status(s) == "listening");
    push(handle, SYSTEM, &CHUNK);
    push(handle, SYSTEM, &CHUNK);
    sheng_engine_stop(handle);
    let failed = wait_for(handle, |s| status(s) == "failed");

    assert_eq!(failed["error"], "audio_backpressure");
    sheng_engine_destroy(handle);
}

#[test]
fn results_during_stop_count_and_nothing_changes_after_settling() {
    let (client, _) = scripted(SpeechScript {
        on_finish: vec![pair(9, "最后一句", "last words")],
        ..SpeechScript::default()
    });
    let handle = handle_with(TestFactory::new(vec![(Source::System, client)]));

    start(handle, &translating_request("system"));
    wait_for(handle, |s| status(s) == "listening");
    sheng_engine_stop(handle);
    let settled = wait_settled(handle);
    std::thread::sleep(Duration::from_millis(300));
    let later = snapshot(handle);

    assert_eq!(recent_len(&settled), 1);
    assert_eq!(later, settled);
    assert_eq!(drain(handle)[0]["translation"], "last words");
    sheng_engine_destroy(handle);
}

#[test]
fn caption_ids_are_unique_across_handles() {
    let ids: Vec<String> = (0..2)
        .map(|_| {
            let (client, _) = scripted(per_chunk(1, 0));
            let handle = handle_with(TestFactory::new(vec![(Source::System, client)]));
            start(handle, &translating_request("system"));
            push(handle, SYSTEM, &CHUNK);
            wait_for(handle, |s| recent_len(s) == 1);
            let id = drain(handle)[0]["id"].as_str().unwrap().to_owned();
            sheng_engine_destroy(handle);
            id
        })
        .collect();

    assert_ne!(ids[0], ids[1]);
}

#[test]
fn undrained_captions_beyond_the_limit_fail_the_session_and_are_kept() {
    let script = SpeechScript {
        on_audio: (1..=10)
            .map(|n| {
                let events = (0..26)
                    .map(|k| {
                        let id = (n * 100 + k) as u64;
                        pair(id, &format!("句子 {id}"), &format!("sentence {id}"))
                    })
                    .collect();
                (n, events)
            })
            .collect(),
        ..SpeechScript::default()
    };
    let (client, _) = scripted(script);
    let handle = handle_with(TestFactory::new(vec![(Source::System, client)]));

    start(handle, &translating_request("system"));
    for _ in 0..10 {
        push(handle, SYSTEM, &CHUNK);
        std::thread::sleep(Duration::from_millis(20));
    }
    let failed = wait_for(handle, |s| status(s) == "failed");

    assert_eq!(failed["error"], "caption_consumer_backpressure");
    assert_eq!(drain(handle).len(), 256);
    sheng_engine_destroy(handle);
}

#[test]
fn start_requires_drained_captions_and_refuses_while_running() {
    let (first, _) = scripted(per_chunk(1, 0));
    let handle = handle_with(TestFactory::new(vec![(Source::System, first)]));

    start(handle, &translating_request("system"));
    assert_eq!(
        start(handle, &translating_request("system"))["error"],
        "already_running"
    );
    push(handle, SYSTEM, &CHUNK);
    wait_for(handle, |s| recent_len(s) == 1);
    sheng_engine_clear(handle);
    assert_eq!(recent_len(&snapshot(handle)), 1);
    sheng_engine_stop(handle);
    wait_settled(handle);

    assert_eq!(
        start(handle, &translating_request("system"))["error"],
        "drain_required"
    );
    sheng_engine_clear(handle);
    let cleared = snapshot(handle);
    assert_eq!(cleared["recent"], json!([]));
    assert_eq!(cleared["tracks"], json!([]));
    assert_eq!(drain(handle).len(), 1);
    let (second, _) = scripted(SpeechScript::default());
    sheng_engine::testing::set_factory(
        handle,
        Arc::new(TestFactory::new(vec![(Source::System, second)])),
    );
    assert_eq!(start(handle, &translating_request("system"))["ok"], true);
    assert_eq!(snapshot(handle)["generation"], 2);
    sheng_engine_destroy(handle);
}

#[test]
fn quiet_source_keeps_its_queued_caption_while_the_other_fills_recent() {
    let (mic, _) = scripted(per_chunk(1, 1000));
    let (system, _) = scripted(per_chunk(20, 0));
    let handle = handle_with(TestFactory::new(vec![
        (Source::System, system),
        (Source::Microphone, mic),
    ]));

    start(handle, &translating_request("both"));
    wait_for(handle, |s| status(s) == "listening");
    push(handle, MICROPHONE, &CHUNK);
    wait_for(handle, |s| recent_len(s) == 1);
    for expected in 1..=20 {
        push(handle, SYSTEM, &CHUNK[..320]);
        wait_for(handle, |s| {
            s["recent"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["original"] == format!("原文 {expected}"))
        });
    }
    let current = snapshot(handle);

    let recent = current["recent"].as_array().unwrap();
    assert_eq!(recent.len(), 10);
    assert!(recent.iter().all(|c| c["source"] == "system"));
    let queued = drain(handle);
    assert_eq!(queued.len(), 21);
    assert_eq!(queued[0]["source"], "microphone");
    assert!(queued[1..].iter().all(|c| c["source"] == "system"));
    sheng_engine_destroy(handle);
}

#[test]
fn service_client_creation_failure_fails_the_session_honestly() {
    // A factory with no clients refuses every speech client, offline and independent
    // of which services are implemented.
    let handle = handle_with(TestFactory::default());

    assert_eq!(start(handle, &translating_request("system"))["ok"], true);
    let settled = wait_for(handle, |s| s["settled"] == true && status(s) == "failed");

    assert_eq!(settled["error"], "provider_configuration_failed");
    sheng_engine_destroy(handle);
}

#[test]
fn recognizing_route_translates_finals_through_the_pipeline() {
    let script = SpeechScript {
        on_audio: vec![(
            1,
            vec![ProviderEvent::Caption(CaptionEvent::OriginalFinal {
                utterance: Some(1.into()),
                text: "Good morning".into(),
            })],
        )],
        ..SpeechScript::default()
    };
    let (client, _) = scripted(script);
    let translator = ScriptedTranslator::new([]);
    let factory = TestFactory::new(vec![(Source::System, client)]).with_translator(translator);
    let handle = handle_with(factory);
    let request = common::requests::start_request(
        "system",
        "openAIRealtime",
        "asrMt",
        "openAICompatible",
        "en",
        "zh",
    );

    assert_eq!(start(handle, &request)["ok"], true);
    assert_eq!(snapshot(handle)["sampleRate"], 24_000);
    push(handle, SYSTEM, &CHUNK);
    let shown = wait_for(handle, |s| recent_len(s) == 1);

    assert_eq!(shown["recent"][0]["translation"], "tr:Good morning");
    assert_eq!(shown["diagnostics"][0]["translationLatencyKind"], "request");
    sheng_engine_destroy(handle);
}

#[test]
fn destroy_is_idempotent_and_invalidates_the_handle() {
    let (client, _) = scripted(SpeechScript::default());
    let handle = handle_with(TestFactory::new(vec![(Source::System, client)]));
    start(handle, &translating_request("system"));

    sheng_engine_destroy(handle);
    sheng_engine_destroy(handle);

    assert_eq!(snapshot(handle), json!({"error": "invalid_handle"}));
    assert_eq!(push(handle, SYSTEM, &CHUNK), -1);
    assert_eq!(start(handle, &json!({}))["error"], "invalid_handle");
}
