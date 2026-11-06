//! Real-service smoke test (step4-common.md §测试 3). Runs only with
//! `SHENG_LIVE_VOLCANO=1` and `SHENG_VOLCANO_API_KEY` set:
//!
//! `SHENG_LIVE_VOLCANO=1 SHENG_VOLCANO_API_KEY=... cargo test -p sheng-runtime \
//!  --test service_volcano_live -- --ignored`
//!
//! Speech is generated with macOS `say` into a temporary directory; nothing is printed
//! besides pass/fail, and no audio is kept.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use sheng_core::CaptionEvent;
use sheng_runtime::config::endpoint::ProxyChoice;
use sheng_runtime::config::secret::Secret;
use sheng_runtime::provider::{EventSink, ProviderEvent, SpeechClient};
use sheng_runtime::services::volcano::{VolcanoClient, VolcanoConfig, ENDPOINT};
use url::Url;

const CHUNK_BYTES: usize = 2560;
const CHUNK_PACE: Duration = Duration::from_millis(80);

fn live_key() -> Option<String> {
    let enabled = std::env::var("SHENG_LIVE_VOLCANO").is_ok_and(|v| v == "1");
    let key = std::env::var("SHENG_VOLCANO_API_KEY").ok()?;
    (enabled && !key.trim().is_empty()).then_some(key)
}

/// Mono PCM16 16 kHz samples of `text`, spoken by `say`.
fn speech(dir: &Path, text: &str) -> Vec<u8> {
    let aiff = dir.join("speech.aiff");
    let wav = dir.join("speech.wav");
    let said = Command::new("say").arg("-o").arg(&aiff).arg(text).status();
    assert!(said.is_ok_and(|s| s.success()), "say runs");
    let converted = Command::new("afconvert")
        .args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"])
        .arg(&aiff)
        .arg(&wav)
        .status();
    assert!(converted.is_ok_and(|s| s.success()), "afconvert runs");
    let bytes = std::fs::read(&wav).unwrap();
    let data = bytes
        .windows(4)
        .position(|w| w == b"data")
        .expect("wav data chunk");
    bytes[data + 8..].to_vec()
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_VOLCANO=1 and a real API key"]
async fn live_session_returns_a_final_pair() {
    let Some(key) = live_key() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("sheng-volcano-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pcm = speech(&dir, "Good morning. The weather is very nice today.");
    let _ = std::fs::remove_dir_all(&dir);

    let mut client = VolcanoClient::new(VolcanoConfig {
        endpoint: Url::parse(ENDPOINT).unwrap(),
        api_key: Secret::new(key),
        source_language: "en".into(),
        target_language: "zh".into(),
        proxy: ProxyChoice::System,
        connect_timeout: Duration::from_secs(30),
    });
    let (sink, mut rx) = EventSink::channel();
    client.connect(sink).await.expect("connects");
    for chunk in pcm.chunks(CHUNK_BYTES) {
        client
            .send_audio(chunk.to_vec())
            .await
            .expect("sends audio");
        tokio::time::sleep(CHUNK_PACE).await;
    }
    client.finish().await.expect("finishes");

    let (mut originals, mut translations) = (0, 0);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        match event {
            ProviderEvent::Caption(CaptionEvent::OriginalFinal { text, .. })
                if !text.is_empty() =>
            {
                originals += 1
            }
            ProviderEvent::Caption(CaptionEvent::TranslationFinal { text, .. })
                if !text.is_empty() =>
            {
                translations += 1
            }
            ProviderEvent::Ended => break,
            ProviderEvent::Failed(error) => panic!("session failed: {error}"),
            _ => {}
        }
    }
    client.disconnect().await;
    assert!(originals >= 1, "at least one original final");
    assert!(translations >= 1, "at least one translation final");
}
