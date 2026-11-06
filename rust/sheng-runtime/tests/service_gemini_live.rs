//! Real-service smoke tests for Gemini Live, both modes. Ignored by default; they run only
//! with `SHENG_LIVE_GEMINI=1` and `GEMINI_API_KEY` set, e.g.
//! `SHENG_LIVE_GEMINI=1 GEMINI_API_KEY=… cargo test -p sheng-runtime --test service_gemini_live -- --ignored`.
//! The speech sample is generated with macOS `say` into a temporary directory. Nothing
//! prints transcripts, audio or the key.

mod common;

use std::process::Command;
use std::time::Duration;

use serde_json::json;
use sheng_core::align::Stream;
use sheng_core::{CaptionEvent, Source};
use sheng_runtime::config::{check_request, SessionPlan};
use sheng_runtime::provider::{EventSink, ProviderEvent, SpeechClient};
use sheng_runtime::services::gemini;

const SAMPLE_RATE: u32 = 16_000;
/// 100 ms of 16-bit mono PCM.
const CHUNK_BYTES: usize = SAMPLE_RATE as usize / 10 * 2;
const TRAILING_SILENCE_CHUNKS: usize = 20;
const SESSION_LIMIT: Duration = Duration::from_secs(60);
const SAMPLE_TEXT: &str = "Good morning. The weather is nice today.";

fn live_key() -> Option<String> {
    let is_enabled = std::env::var("SHENG_LIVE_GEMINI").is_ok_and(|v| v == "1");
    let key = std::env::var("GEMINI_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty());
    is_enabled.then_some(key).flatten()
}

fn live_plan(mode: &str, source: &str, key: &str) -> SessionPlan {
    let text = if mode == "asrMt" { "apple" } else { "-" };
    let mut request = common::start_request("system", "googleGeminiLive", mode, text, source, "zh");
    request["configuration"]["credentials"] = json!({"kind": "apiKey", "apiKey": key});
    check_request(&request.to_string()).expect("valid gemini request")
}

/// Speaks the sample with `say` as 16 kHz PCM, followed by silence.
fn speech() -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("sheng-gemini-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sample.wav");
    let status = Command::new("say")
        .arg("-o")
        .arg(&path)
        .arg(format!("--data-format=LEI16@{SAMPLE_RATE}"))
        .arg(SAMPLE_TEXT)
        .status()
        .expect("say runs");
    assert!(status.success());
    let wav = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let data = wav
        .windows(4)
        .position(|w| w == b"data")
        .expect("data chunk")
        + 8;
    let mut pcm = wav[data..].to_vec();
    pcm.extend(std::iter::repeat_n(
        0u8,
        CHUNK_BYTES * TRAILING_SILENCE_CHUNKS,
    ));
    pcm
}

/// Streams the sample in real time, finishes and collects events until `Ended`.
async fn run(mut client: Box<dyn SpeechClient>) -> Vec<ProviderEvent> {
    let (events, mut rx) = EventSink::channel();
    client.connect(events).await.expect("connects");
    for chunk in speech().chunks(CHUNK_BYTES) {
        client.send_audio(chunk.to_vec()).await.expect("audio sent");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    client.finish().await.expect("finish sent");
    let mut seen = Vec::new();
    let collect = async {
        while let Some(event) = rx.recv().await {
            let is_last = matches!(event, ProviderEvent::Ended | ProviderEvent::Failed(_));
            seen.push(event);
            if is_last {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(SESSION_LIMIT, collect).await;
    client.disconnect().await;
    seen
}

fn has_failure(events: &[ProviderEvent]) -> bool {
    events.iter().any(|e| matches!(e, ProviderEvent::Failed(_)))
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_GEMINI=1 and GEMINI_API_KEY"]
async fn simultaneous_reports_original_and_translation() {
    let Some(key) = live_key() else { return };
    let plan = live_plan("simultaneous", "auto", &key);
    let events = run(gemini::speech(&plan, Source::System).expect("client")).await;
    let has = |wanted: Stream| {
        events.iter().any(
            |event| matches!(event, ProviderEvent::StreamText { stream, .. } if *stream == wanted),
        )
    };
    assert!(!has_failure(&events), "session failed");
    assert!(has(Stream::Original), "no original transcript");
    assert!(has(Stream::Translation), "no translation transcript");
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_GEMINI=1 and GEMINI_API_KEY"]
async fn transcribe_confirms_at_least_one_final() {
    let Some(key) = live_key() else { return };
    let plan = live_plan("asrMt", "en", &key);
    let events = run(gemini::speech(&plan, Source::Microphone).expect("client")).await;
    let finals = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                ProviderEvent::Caption(CaptionEvent::OriginalFinal { .. })
            )
        })
        .count();
    assert!(!has_failure(&events), "session failed");
    assert!(finals >= 1, "no final transcript");
}
