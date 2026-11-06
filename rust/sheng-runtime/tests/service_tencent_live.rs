//! Real-service smoke test for Tencent Cloud speech translation. Ignored by default; run with
//! `SHENG_LIVE_TENCENT=1 TENCENT_APP_ID=… TENCENT_SECRET_ID=… TENCENT_SECRET_KEY=…
//! cargo test -p sheng-runtime --test service_tencent_live -- --ignored`.
//! Speech comes from macOS `say` in a temporary directory; nothing is printed or committed.

use std::time::Duration;

use sheng_core::CaptionEvent;
use sheng_runtime::config::endpoint::ProxyChoice;
use sheng_runtime::config::secret::Secret;
use sheng_runtime::provider::{EventSink, ProviderEvent, SpeechClient};
use sheng_runtime::services::tencent::client::MAX_FRAME_BYTES;
use sheng_runtime::services::tencent::{
    TencentConfig, TencentCredentials, TencentSpeech, ENDPOINT,
};
use url::Url;

/// Real-time pacing: one 200 ms frame every 200 ms, as the documentation advises.
const FRAME_INTERVAL: Duration = Duration::from_millis(200);
const RESULT_WAIT: Duration = Duration::from_secs(30);
const SPOKEN: &str = "Good morning. The weather is nice today, and we are testing live captions.";

fn credentials() -> Option<TencentCredentials> {
    if std::env::var("SHENG_LIVE_TENCENT").ok()? != "1" {
        return None;
    }
    let read = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let app_id = Secret::new(read("TENCENT_APP_ID")?);
    let secret_id = Secret::new(read("TENCENT_SECRET_ID")?);
    let secret_key = Secret::new(read("TENCENT_SECRET_KEY")?);
    TencentCredentials::new(&app_id, secret_id, secret_key).ok()
}

/// 16 kHz 16-bit mono PCM of `text`, from a WAV written by `say`.
fn synthesize(text: &str) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("sheng-tencent-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("speech.wav");
    let status = std::process::Command::new("say")
        .args(["--file-format=WAVE", "--data-format=LEI16@16000", "-o"])
        .arg(&wav)
        .arg(text)
        .status()
        .expect("macOS say is available");
    assert!(status.success(), "say failed");
    let bytes = std::fs::read(&wav).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    wav_data(&bytes).to_vec()
}

/// Payload of the `data` chunk of a RIFF/WAVE file.
fn wav_data(bytes: &[u8]) -> &[u8] {
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = at + 8;
        if &bytes[at..at + 4] == b"data" {
            return &bytes[body..(body + size).min(bytes.len())];
        }
        at = body + size + (size & 1);
    }
    panic!("WAV without a data chunk");
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_TENCENT=1 and Tencent Cloud credentials"]
async fn live_session_confirms_at_least_one_translated_sentence() {
    let Some(credentials) = credentials() else {
        return;
    };
    let mut speech = TencentSpeech::new(TencentConfig {
        endpoint: Url::parse(ENDPOINT).unwrap(),
        credentials,
        source: "en".into(),
        target: "zh".into(),
        proxy: ProxyChoice::System,
    });
    let (sink, mut events) = EventSink::channel();
    speech.connect(sink).await.expect("connects");

    let mut pcm = synthesize(SPOKEN);
    pcm.extend(vec![0u8; 5 * MAX_FRAME_BYTES]); // one second of trailing silence
    for frame in pcm.chunks(MAX_FRAME_BYTES) {
        speech
            .send_audio(frame.to_vec())
            .await
            .expect("audio accepted");
        tokio::time::sleep(FRAME_INTERVAL).await;
    }
    speech.finish().await.expect("end accepted");

    let mut confirmed = 0;
    let ended = tokio::time::timeout(RESULT_WAIT, async {
        while let Some(event) = events.recv().await {
            match event {
                ProviderEvent::Caption(CaptionEvent::ConfirmedPair {
                    original,
                    translation,
                    ..
                }) if !original.trim().is_empty() && !translation.trim().is_empty() => {
                    confirmed += 1
                }
                ProviderEvent::Ended => return true,
                ProviderEvent::Failed(error) => panic!("session failed: {error}"),
                _ => {}
            }
        }
        false
    })
    .await;
    speech.disconnect().await;

    assert_eq!(ended, Ok(true), "service ended with final:1");
    assert!(
        confirmed >= 1,
        "at least one confirmed, translated sentence"
    );
}
