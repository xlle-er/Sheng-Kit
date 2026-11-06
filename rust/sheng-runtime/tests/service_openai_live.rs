//! Real-service smoke tests (step-4 common rules, test layer 3). Ignored by default; each
//! runs only when its `SHENG_LIVE_*` switch is `1` and its credentials are set:
//!
//! - `SHENG_LIVE_OPENAI=1`, `OPENAI_API_KEY`
//! - `SHENG_LIVE_AZURE=1`, `AZURE_OPENAI_ENDPOINT`, `AZURE_OPENAI_API_KEY`,
//!   `AZURE_OPENAI_DEPLOYMENT`, `AZURE_OPENAI_TRANSCRIPTION_DEPLOYMENT`
//!
//! `cargo test -p sheng-runtime --test service_openai_live -- --ignored`
//! Speech comes from macOS `say` into a temporary directory. Nothing printed contains
//! transcripts, audio or credentials; assertions only count results.

#[path = "fixtures/openai/mock.rs"]
mod mock;
#[path = "fixtures/openai/support.rs"]
mod support;

use std::process::Command;
use std::time::Duration;

use serde_json::json;
use sheng_core::align::Stream;
use sheng_core::CaptionEvent;
use sheng_runtime::provider::{EventSink, ProviderEvent, SpeechClient};
use sheng_runtime::services::openai::{
    realtime_config, LanguageField, RealtimeConfig, SessionKind,
};

const CHUNK_MILLIS: usize = 100;
const CHUNK_BYTES: usize = CHUNK_MILLIS * 24 * 2;
/// Trailing quiet that lets the client-side turn detector commit.
const TRAILING_SILENCE_MILLIS: usize = 1_200;

/// Value of `name` when the switch is on and every variable is set.
fn live_env(switch: &str, names: &[&str]) -> Option<Vec<String>> {
    if std::env::var(switch).as_deref() != Ok("1") {
        eprintln!("skipped: {switch} is not 1");
        return None;
    }
    let values: Option<Vec<String>> = names
        .iter()
        .map(|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty()))
        .collect();
    if values.is_none() {
        eprintln!("skipped: credentials for {switch} are not set");
    }
    values
}

/// PCM16 mono 24 kHz speech from `say`; `None` when the voice is not installed.
fn say(text: &str, voice: Option<&str>) -> Option<Vec<u8>> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!("sheng-live-{}-{stamp}.wav", std::process::id());
    let path = std::env::temp_dir().join(name);
    let mut command = Command::new("say");
    command
        .args(["--file-format=WAVE", "--data-format=LEI16@24000", "-o"])
        .arg(&path);
    if let Some(voice) = voice {
        command.args(["-v", voice]);
    }
    let status = command.arg(text).status().ok()?;
    let wav = std::fs::read(&path).ok();
    let _ = std::fs::remove_file(&path);
    if !status.success() {
        return None;
    }
    wav_data(&wav?)
}

/// Payload of the WAVE `data` chunk.
fn wav_data(wav: &[u8]) -> Option<Vec<u8>> {
    let mut offset = 12;
    while offset + 8 <= wav.len() {
        let id = &wav[offset..offset + 4];
        let size = u32::from_le_bytes(wav[offset + 4..offset + 8].try_into().ok()?) as usize;
        let body = offset + 8;
        if id == b"data" {
            return wav.get(body..body + size).map(<[u8]>::to_vec);
        }
        offset = body + size + size % 2;
    }
    None
}

/// Streams `pcm` plus trailing silence, ends input and returns every event to the end.
async fn run_session(config: RealtimeConfig, pcm: Vec<u8>) -> Vec<ProviderEvent> {
    let mut client = sheng_runtime::services::openai::RealtimeClient::new(config);
    let (sink, mut events) = EventSink::channel();
    client.connect(sink).await.expect("live connect");
    let silence = vec![0u8; TRAILING_SILENCE_MILLIS * 48];
    for chunk in pcm.chunks(CHUNK_BYTES).chain(silence.chunks(CHUNK_BYTES)) {
        client.send_audio(chunk.to_vec()).await.expect("audio sent");
        tokio::time::sleep(Duration::from_millis(CHUNK_MILLIS as u64 / 2)).await;
    }
    client.finish().await.expect("finish sent");
    let mut seen = Vec::new();
    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(20), events.recv()).await {
        let last = matches!(event, ProviderEvent::Ended | ProviderEvent::Failed(_));
        seen.push(event);
        if last {
            break;
        }
    }
    client.disconnect().await;
    seen
}

fn finals(events: &[ProviderEvent]) -> usize {
    let count = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                ProviderEvent::Caption(CaptionEvent::OriginalFinal { .. })
            )
        })
        .count();
    eprintln!("finals: {count}, failed: {:?}", failure(events));
    count
}

fn failure(events: &[ProviderEvent]) -> Option<&ProviderEvent> {
    events
        .iter()
        .find(|e| matches!(e, ProviderEvent::Failed(_)))
}

fn stream_count(events: &[ProviderEvent], wanted: Stream) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, ProviderEvent::StreamText { stream, .. } if *stream == wanted))
        .count()
}

fn openai_config(key: &str, mode: &str, source: &str, target: &str) -> RealtimeConfig {
    let credentials = json!({ "kind": "apiKey", "apiKey": key });
    let mut request = support::base("openAIRealtime", credentials, mode, source, target);
    if mode == "asrMt" && target != "original" {
        request["configuration"]["textCredentials"] = json!({ "kind": "apple" });
    }
    realtime_config(&support::plan(request)).expect("live config")
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_OPENAI=1 and OPENAI_API_KEY"]
async fn openai_transcription_returns_a_final() {
    let Some(env) = live_env("SHENG_LIVE_OPENAI", &["OPENAI_API_KEY"]) else {
        return;
    };
    let pcm = say("The weather is clear and the train leaves at noon.", None).expect("say");

    let events = run_session(openai_config(&env[0], "asrMt", "en", "original"), pcm).await;

    assert!(finals(&events) >= 1);
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_OPENAI=1 and OPENAI_API_KEY"]
async fn openai_translation_returns_source_and_translated_text() {
    let Some(env) = live_env("SHENG_LIVE_OPENAI", &["OPENAI_API_KEY"]) else {
        return;
    };
    let pcm = say("Good morning. The meeting starts in ten minutes.", None).expect("say");

    let config = openai_config(&env[0], "simultaneous", "auto", "fr");
    let events = run_session(config, pcm).await;

    eprintln!("failed: {:?}", failure(&events));
    assert!(stream_count(&events, Stream::Original) >= 1);
    assert!(stream_count(&events, Stream::Translation) >= 1);
}

/// questions.md step 2 item 3: does transcription accept `ja`? Builds the session directly
/// because the catalog does not offer `ja` yet. Skips when no Japanese voice is installed.
#[tokio::test]
#[ignore = "needs SHENG_LIVE_OPENAI=1 and OPENAI_API_KEY"]
async fn openai_transcription_accepts_japanese() {
    let Some(env) = live_env("SHENG_LIVE_OPENAI", &["OPENAI_API_KEY"]) else {
        return;
    };
    let Some(pcm) = say("今日はいい天気ですね。", Some("Kyoko")) else {
        eprintln!("skipped: Japanese voice Kyoko is not installed");
        return;
    };
    let mut config = openai_config(&env[0], "asrMt", "en", "original");
    config.kind = SessionKind::Transcribe {
        model: "gpt-live-transcribe".into(),
        language: Some("ja".into()),
        language_field: LanguageField::List,
    };

    let events = run_session(config, pcm).await;

    assert!(finals(&events) >= 1);
}

#[tokio::test]
#[ignore = "needs SHENG_LIVE_AZURE=1 and the AZURE_OPENAI_* variables"]
async fn azure_transcription_returns_a_final() {
    let names = [
        "AZURE_OPENAI_ENDPOINT",
        "AZURE_OPENAI_API_KEY",
        "AZURE_OPENAI_DEPLOYMENT",
        "AZURE_OPENAI_TRANSCRIPTION_DEPLOYMENT",
    ];
    let Some(env) = live_env("SHENG_LIVE_AZURE", &names) else {
        return;
    };
    let credentials = json!({
        "kind": "azureOpenAI",
        "endpoint": env[0],
        "apiKey": env[1],
        "deployment": env[2],
        "transcriptionDeployment": env[3],
    });
    let request = support::base(
        "azureOpenAIRealtime",
        credentials,
        "asrMt",
        "en",
        "original",
    );
    let config = realtime_config(&support::plan(request)).expect("live config");
    let pcm = say("Please confirm the delivery address by Friday.", None).expect("say");

    let events = run_session(config, pcm).await;

    assert!(finals(&events) >= 1);
}

#[test]
fn wav_data_finds_the_data_chunk_after_other_chunks() {
    let mut wav = b"RIFF\0\0\0\0WAVE".to_vec();
    wav.extend_from_slice(b"fmt \x02\0\0\0ab");
    wav.extend_from_slice(b"data\x04\0\0\0\x01\x02\x03\x04");

    assert_eq!(wav_data(&wav), Some(vec![1, 2, 3, 4]));
}
