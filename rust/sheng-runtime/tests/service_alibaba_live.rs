//! Real-service smoke tests (ignored by default). Run only with `SHENG_LIVE_DASHSCOPE=1` and
//! `DASHSCOPE_API_KEY` set (live translation also reads the optional `DASHSCOPE_WORKSPACE_ID`);
//! they assert at least one final original with its translation (Qwen-MT, or the live
//! translation service) without printing transcripts, audio or credentials. Speech comes
//! from macOS `say`.
//!
//! `SHENG_LIVE_DASHSCOPE=1 DASHSCOPE_API_KEY=… cargo test -p sheng-runtime \
//!   --test service_alibaba_live -- --ignored`

use std::process::Command;
use std::time::Duration;

use sheng_core::CaptionEvent;
use sheng_runtime::config::endpoint::ProxyChoice;
use sheng_runtime::config::secret::Secret;
use sheng_runtime::config::SessionPlan;
use sheng_runtime::provider::{
    EventSink, ProviderEvent, SpeechClient, TextTranslator, TranslationRequest,
};
use sheng_runtime::services::alibaba::asr::client::{AsrClient, AsrConfig};
use sheng_runtime::services::alibaba::asr::protocol::TaskSettings;
use sheng_runtime::services::alibaba::live::client::{LiveConfig, LiveTranslateClient};
use sheng_runtime::services::alibaba::live::protocol::{realtime_url, LIVE_TRANSLATE_MODEL};
use sheng_runtime::services::alibaba::mt::{QwenMtTranslator, QWEN_MT_ENDPOINT};
use sheng_runtime::services::alibaba::{ASR_MODEL, INFERENCE_ENDPOINT};

const CHUNK_BYTES: usize = 3_200; // 100 ms of 16 kHz mono 16-bit audio
const FINISH_WAIT: Duration = Duration::from_secs(10);
/// How long a failing live test waits for the close frame before reporting.
const DETAIL_WAIT: Duration = Duration::from_secs(2);

fn live_key() -> Option<Secret> {
    if std::env::var("SHENG_LIVE_DASHSCOPE").ok().as_deref() != Some("1") {
        return None;
    }
    std::env::var("DASHSCOPE_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .map(Secret::new)
}

/// Speaks a sentence into a temporary 16 kHz WAV and returns its PCM samples.
fn spoken_pcm() -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("sheng-alibaba-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("speech.wav");
    let status = Command::new("say")
        .args(["-v", "Tingting", "-o"])
        .arg(&path)
        .args([
            "--data-format=LEI16@16000",
            "今天天气很好，我们去公园散步吧。",
        ])
        .status()
        .expect("say runs");
    assert!(status.success());
    let wav = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    wav_data(&wav).expect("WAV data chunk").to_vec()
}

fn wav_data(wav: &[u8]) -> Option<&[u8]> {
    let mut at = 12;
    while at + 8 <= wav.len() {
        let size = u32::from_le_bytes(wav[at + 4..at + 8].try_into().ok()?) as usize;
        if &wav[at..at + 4] == b"data" {
            return wav.get(at + 8..(at + 8 + size).min(wav.len()));
        }
        at += 8 + size + size % 2;
    }
    None
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_DASHSCOPE=1 and DASHSCOPE_API_KEY"]
async fn recognition_and_qwen_mt_produce_a_translated_final() {
    let Some(key) = live_key() else {
        return;
    };
    let mut client = AsrClient::new(AsrConfig {
        endpoint: INFERENCE_ENDPOINT.parse().unwrap(),
        api_key: key.clone(),
        proxy: ProxyChoice::System,
        settings: TaskSettings {
            model: ASR_MODEL.into(),
            sample_rate: 16_000,
            source_language: "zh".into(),
        },
    });
    let (sink, mut events) = EventSink::channel();
    if let Err(error) = client.connect(sink).await {
        panic!(
            "connect failed: {error}; service reported: {}",
            client.failure_detail()
        );
    }
    for chunk in spoken_pcm().chunks(CHUNK_BYTES) {
        if let Err(error) = client.send_audio(chunk.to_vec()).await {
            panic!(
                "audio send failed: {error}; service reported: {}",
                client.failure_detail()
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    client.finish().await.expect("finish sent");
    let mut finals = Vec::new();
    while let Ok(Some(event)) = tokio::time::timeout(FINISH_WAIT, events.recv()).await {
        match event {
            ProviderEvent::Caption(CaptionEvent::OriginalFinal { text, .. }) => finals.push(text),
            ProviderEvent::Ended => break,
            ProviderEvent::Failed(error) => panic!(
                "recognition failed: {error}; service reported: {}",
                client.failure_detail()
            ),
            _ => {}
        }
    }
    client.disconnect().await;
    assert!(!finals.is_empty(), "at least one final original");

    let translator =
        QwenMtTranslator::new(QWEN_MT_ENDPOINT, "qwen-mt-lite", &key, &ProxyChoice::System)
            .unwrap();
    let request = TranslationRequest {
        text: finals.join(""),
        source_language: "zh".into(),
        target_language: "en".into(),
    };
    let translation = translator.translate(request).await.expect("translated");
    assert!(!translation.trim().is_empty(), "non-empty translation");
}

/// A validated `liveTranslate` request for the production endpoint (workspace host when
/// `DASHSCOPE_WORKSPACE_ID` is set).
fn live_translate_plan(key: &Secret) -> SessionPlan {
    let workspace = std::env::var("DASHSCOPE_WORKSPACE_ID").unwrap_or_default();
    let request = serde_json::json!({
        "input": "system",
        "configuration": {
            "provider": "alibabaCloud",
            "qwenMtModel": "lite",
            "credentials": {"kind": "apiKey", "apiKey": key.expose()},
            "sourceLanguage": "auto",
            "targetLanguage": "en",
            "translationMode": "turbo",
            "networkProxy": {"mode": "system"},
            "textNetworkProxy": {"mode": "system"},
            "speechTranslationMode": "simultaneous",
            "xaiSpeechMode": "voice",
            "alibabaRealtimeMode": "liveTranslate",
            "alibabaWorkspaceId": workspace.trim(),
        }
    });
    sheng_runtime::config::check_request(&request.to_string()).expect("valid live request")
}

/// Live-translation client for the plan, built as the factory does, so the test can read the
/// failure detail the service reported.
fn live_client(plan: &SessionPlan, key: &Secret) -> LiveTranslateClient {
    let endpoint =
        realtime_url(&plan.alibaba_workspace_id, LIVE_TRANSLATE_MODEL).expect("realtime endpoint");
    LiveTranslateClient::new(LiveConfig {
        endpoint,
        api_key: key.clone(),
        proxy: plan.speech_proxy.clone(),
        target_language: plan.target_language.clone(),
        sample_rate: plan.sample_rate,
    })
}

/// Fails with the error, which host kind was used and what the service reported (close code
/// and reason, error event type / code / message); never captions, audio or credentials.
async fn fail_live(
    what: &str,
    error: impl std::fmt::Display,
    plan: &SessionPlan,
    client: &mut LiveTranslateClient,
    events: &mut tokio::sync::mpsc::Receiver<ProviderEvent>,
) -> ! {
    // Let the reader note a close frame that is still in flight.
    while let Ok(Some(event)) = tokio::time::timeout(DETAIL_WAIT, events.recv()).await {
        if matches!(event, ProviderEvent::Failed(_) | ProviderEvent::Ended) {
            break;
        }
    }
    let detail = client.failure_detail();
    client.disconnect().await;
    let host = if plan.alibaba_workspace_id.is_empty() {
        "dashscope.aliyuncs.com (no DASHSCOPE_WORKSPACE_ID)"
    } else {
        "workspace host"
    };
    panic!("live translation {what}: {error}; model {LIVE_TRANSLATE_MODEL}; host {host}; service reported: {detail}");
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_DASHSCOPE=1 and DASHSCOPE_API_KEY"]
async fn live_translation_produces_a_final_original_and_translation_pair() {
    let Some(key) = live_key() else {
        return;
    };
    let plan = live_translate_plan(&key);
    let mut client = live_client(&plan, &key);
    let (sink, mut events) = EventSink::channel();
    if let Err(error) = client.connect(sink).await {
        fail_live("connect failed", error, &plan, &mut client, &mut events).await;
    }
    for chunk in spoken_pcm().chunks(CHUNK_BYTES) {
        if let Err(error) = client.send_audio(chunk.to_vec()).await {
            fail_live("audio send failed", error, &plan, &mut client, &mut events).await;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if let Err(error) = client.finish().await {
        fail_live("finish failed", error, &plan, &mut client, &mut events).await;
    }
    let (mut originals, mut translations, mut pairs) = (Vec::new(), Vec::new(), 0);
    while let Ok(Some(event)) = tokio::time::timeout(FINISH_WAIT, events.recv()).await {
        match event {
            ProviderEvent::Caption(CaptionEvent::OriginalFinal { utterance, text })
                if !text.trim().is_empty() =>
            {
                originals.push(utterance)
            }
            ProviderEvent::Caption(CaptionEvent::TranslationFinal { utterance, text })
                if !text.trim().is_empty() =>
            {
                translations.push(utterance)
            }
            ProviderEvent::Caption(CaptionEvent::ConfirmedPair { .. }) => pairs += 1,
            ProviderEvent::Ended => break,
            ProviderEvent::Failed(error) => {
                fail_live("failed", error, &plan, &mut client, &mut events).await
            }
            _ => {}
        }
    }
    let paired = pairs > 0 || originals.iter().any(|id| translations.contains(id));
    if !paired {
        let counts = format!(
            "{} final originals, {} final translations, none paired",
            originals.len(),
            translations.len()
        );
        fail_live("produced no pair", counts, &plan, &mut client, &mut events).await;
    }
    client.disconnect().await;
}
