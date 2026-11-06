//! Live Baidu smoke test. Runs only with `SHENG_LIVE_BAIDU=1`, `SHENG_BAIDU_APP_ID` and
//! `SHENG_BAIDU_APP_KEY` set; speech is generated with macOS `say` into a temporary
//! directory. Prints nothing about the transcript, audio or credentials.

mod common;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::json;
use sheng_core::CaptionEvent;
use sheng_runtime::config::check_request;
use sheng_runtime::provider::{EventSink, ProviderEvent};
use sheng_runtime::services::baidu::speech;

const PHRASE: &str = "Good morning. The weather is very nice today.";
const FRAME: usize = 1280;
const FRAME_INTERVAL: Duration = Duration::from_millis(40);
const TRAILING_SILENCE_FRAMES: usize = 50;
const RESULT_WAIT: Duration = Duration::from_secs(20);

fn credentials() -> Option<(String, String)> {
    if std::env::var("SHENG_LIVE_BAIDU").ok()? != "1" {
        return None;
    }
    let id = std::env::var("SHENG_BAIDU_APP_ID").ok()?;
    let key = std::env::var("SHENG_BAIDU_APP_KEY").ok()?;
    Some((id, key))
}

/// 16 kHz mono 16-bit PCM of `PHRASE`, from the WAV `data` chunk `say` writes.
fn speech_pcm(dir: &Path) -> Vec<u8> {
    let wav = dir.join("phrase.wav");
    let status = Command::new("say")
        .arg("-o")
        .arg(&wav)
        .arg("--data-format=LEI16@16000")
        .arg(PHRASE)
        .status()
        .expect("say runs");
    assert!(status.success());
    let bytes = std::fs::read(&wav).unwrap();
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &bytes[offset..offset + 4] == b"data" {
            let end = (offset + 8 + size).min(bytes.len());
            return bytes[offset + 8..end].to_vec();
        }
        offset += 8 + size + (size & 1);
    }
    panic!("no data chunk");
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_BAIDU=1 and Baidu credentials"]
async fn live_session_confirms_a_translated_sentence() {
    let Some((app_id, app_key)) = credentials() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("sheng-baidu-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let pcm = speech_pcm(&dir);
    let _ = std::fs::remove_dir_all(&dir);

    let mut request = common::start_request("system", "baiduTranslate", "-", "-", "en", "zh");
    request["configuration"]["credentials"] =
        json!({"kind": "baiduTranslate", "appId": app_id, "appKey": app_key});
    let plan = check_request(&request.to_string()).unwrap();
    let mut client = speech(&plan, sheng_core::Source::System).unwrap();
    let (sink, mut events) = EventSink::channel();
    client.connect(sink).await.expect("connected");

    let silence = vec![0u8; FRAME * TRAILING_SILENCE_FRAMES];
    for frame in pcm.chunks(FRAME).chain(silence.chunks(FRAME)) {
        client.send_audio(frame.to_vec()).await.expect("audio sent");
        tokio::time::sleep(FRAME_INTERVAL).await;
    }
    client.finish().await.expect("finish sent");

    let mut confirmed = 0;
    while let Ok(Some(event)) = tokio::time::timeout(RESULT_WAIT, events.recv()).await {
        match event {
            ProviderEvent::Caption(CaptionEvent::ConfirmedPair { translation, .. })
                if !translation.trim().is_empty() =>
            {
                confirmed += 1
            }
            ProviderEvent::Ended => break,
            ProviderEvent::Failed(error) => panic!("session failed: {error}"),
            _ => {}
        }
    }
    client.disconnect().await;
    assert!(
        confirmed >= 1,
        "at least one confirmed, translated sentence"
    );
}
