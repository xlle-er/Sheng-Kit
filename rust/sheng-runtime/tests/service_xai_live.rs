//! Real-service smoke tests for xAI voice and STT. Ignored by default; they run only with
//! `SHENG_LIVE_XAI=1` and `XAI_API_KEY` set, e.g.
//! `SHENG_LIVE_XAI=1 XAI_API_KEY=… cargo test -p sheng-runtime --test service_xai_live -- --ignored`.
//! The speech sample is generated with macOS `say` into a temporary directory. Nothing
//! prints transcripts, audio or the key.

#[path = "fixtures/xai/server.rs"]
mod server;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use server::{plan_with_key, sink};
use sheng_core::align::Stream;
use sheng_core::{CaptionEvent, Source};
use sheng_runtime::provider::{ProviderEvent, SpeechClient};
use sheng_runtime::services::xai;

const SAMPLE_RATE: u32 = 24_000;
/// 100 ms of 16-bit mono PCM.
const CHUNK_BYTES: usize = SAMPLE_RATE as usize / 10 * 2;
const SESSION_LIMIT: Duration = Duration::from_secs(60);

fn live_key() -> Option<String> {
    let enabled = std::env::var("SHENG_LIVE_XAI").is_ok_and(|v| v == "1");
    let key = std::env::var("XAI_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty());
    enabled.then_some(key).flatten()
}

/// Speaks `text` with `say` as 24 kHz PCM and returns the samples, followed by silence
/// so that the service detects the end of speech.
fn speech(text: &str) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("sheng-xai-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join("sample.wav");
    let status = Command::new("say")
        .arg("-o")
        .arg(&path)
        .arg(format!("--data-format=LEI16@{SAMPLE_RATE}"))
        .arg(text)
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
    pcm.extend(std::iter::repeat_n(0u8, CHUNK_BYTES * 20));
    pcm
}

/// Streams the sample in real time, finishes and collects events until `Ended`.
async fn run(mut client: Box<dyn SpeechClient>, pcm: Vec<u8>) -> Vec<ProviderEvent> {
    let (events, mut rx) = sink();
    client.connect(events).await.expect("connects");
    for chunk in pcm.chunks(CHUNK_BYTES) {
        client.send_audio(chunk.to_vec()).await.expect("audio sent");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    client.finish().await.expect("finish sent");
    let mut seen = Vec::new();
    let collect = async {
        while let Some(event) = rx.recv().await {
            let ended = matches!(event, ProviderEvent::Ended | ProviderEvent::Failed(_));
            seen.push(event);
            if ended {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(SESSION_LIMIT, collect).await;
    client.disconnect().await;
    seen
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_XAI=1 and XAI_API_KEY"]
async fn voice_interprets_at_least_one_turn() {
    let Some(key) = live_key() else { return };
    let plan = plan_with_key("voice", "en", "zh", &key, "system");
    let client = xai::speech(&plan, Source::System).expect("client");
    let events = run(client, speech("Good morning. The weather is nice today.")).await;
    let has = |wanted: Stream| {
        events.iter().any(
            |event| matches!(event, ProviderEvent::StreamText { stream, .. } if *stream == wanted),
        )
    };
    assert!(!events.iter().any(|e| matches!(e, ProviderEvent::Failed(_))));
    assert!(has(Stream::Original), "no original transcript");
    assert!(has(Stream::Translation), "no translation transcript");
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_XAI=1 and XAI_API_KEY"]
async fn stt_confirms_at_least_one_final() {
    let Some(key) = live_key() else { return };
    let plan = plan_with_key("asrMt", "en", "zh", &key, "system");
    let client = xai::speech(&plan, Source::Microphone).expect("client");
    let events = run(client, speech("Good morning. The weather is nice today.")).await;
    let finals = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                ProviderEvent::Caption(CaptionEvent::OriginalFinal { .. })
            )
        })
        .count();
    assert!(!events.iter().any(|e| matches!(e, ProviderEvent::Failed(_))));
    assert!(finals >= 1, "no final transcript");
}
