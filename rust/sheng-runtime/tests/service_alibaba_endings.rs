//! Unexpected session endings of the inference and realtime routes (providers/alibaba.md
//! “会话意外结束与恢复”, providers/README.md “意外断开”).

mod common;
mod service_alibaba_mock;

use std::time::Duration;

use futures_util::SinkExt;
use serde_json::json;
use service_alibaba_mock as mock;
use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::provider::{ProviderError, ProviderEvent, SpeechClient};
use sheng_runtime::services::alibaba::asr::client::{AsrClient, AsrConfig};
use sheng_runtime::services::alibaba::asr::protocol::{parse, ServerEvent, TaskSettings};
use sheng_runtime::services::alibaba::live::client::{LiveConfig, LiveTranslateClient};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

/// How the stand-in ends the session before the client finished.
#[derive(Debug, Clone, Copy)]
enum Ending {
    /// The service's own end message (`task-finished` / `session.finished`).
    Finished,
    Close1000,
    Drop,
}

const ENDINGS: [Ending; 3] = [Ending::Finished, Ending::Close1000, Ending::Drop];

async fn end(mut ws: mock::Ws, ending: Ending, finished: String) {
    match ending {
        Ending::Finished => {
            mock::send_text(&mut ws, &finished).await;
            mock::drain(ws).await;
        }
        Ending::Close1000 => {
            let frame = CloseFrame {
                code: CloseCode::Normal,
                reason: "".into(),
            };
            let _ = ws.close(Some(frame)).await;
        }
        Ending::Drop => drop(ws),
    }
}

async fn events_until_last(events: &mut mpsc::Receiver<ProviderEvent>) -> Vec<ProviderEvent> {
    let mut seen = Vec::new();
    loop {
        let event = mock::next_event(events).await;
        let last = matches!(event, ProviderEvent::Ended | ProviderEvent::Failed(_));
        seen.push(event);
        if last {
            return seen;
        }
    }
}

fn asr(endpoint: Url) -> AsrClient {
    AsrClient::new(AsrConfig {
        endpoint,
        api_key: mock::key(),
        proxy: mock::direct(),
        settings: TaskSettings {
            model: "asr-model".into(),
            sample_rate: 16_000,
            source_language: "zh".into(),
        },
    })
}

fn task_event(task: &str, name: &str) -> String {
    json!({"header": {"task_id": task, "event": name, "attributes": {}}, "payload": {}}).to_string()
}

fn result(task: &str, id: u64, text: &str, end: bool) -> String {
    json!({
        "header": {"task_id": task, "event": "result-generated", "attributes": {}},
        "payload": {"output": {"sentence": {"sentence_id": id, "text": text, "sentence_end": end}}},
    })
    .to_string()
}

fn failed(code: &str, message: &str) -> String {
    json!({"header": {"task_id": "t", "event": "task-failed", "error_code": code,
        "error_message": message}, "payload": {}})
    .to_string()
}

async fn start_task(ws: &mut mock::Ws) -> String {
    let run = mock::next_json(ws).await;
    let task = run["header"]["task_id"].as_str().unwrap().to_owned();
    mock::send_text(ws, &task_event(&task, "task-started")).await;
    task
}

fn original(id: u64, text: &str, end: bool) -> ProviderEvent {
    let utterance = Some(UtteranceId::Number(id));
    let text = text.to_owned();
    ProviderEvent::Caption(if end {
        CaptionEvent::OriginalFinal { utterance, text }
    } else {
        CaptionEvent::OriginalDraft { utterance, text }
    })
}

#[tokio::test]
async fn inference_ending_mid_sentence_commits_it_once_then_fails_retryably() {
    for ending in ENDINGS {
        let (url, _) = mock::ws_server(move |mut ws| async move {
            let task = start_task(&mut ws).await;
            mock::send_text(&mut ws, &result(&task, 1, "第一句。", true)).await;
            mock::send_text(&mut ws, &result(&task, 2, "说到一", false)).await;
            mock::send_text(&mut ws, &result(&task, 2, "说到一半", false)).await;
            end(ws, ending, task_event(&task, "task-finished")).await;
        })
        .await;
        let mut client = asr(url);
        let (sink, mut events) = mock::sink();
        client.connect(sink).await.unwrap();

        let got = events_until_last(&mut events).await;

        assert_eq!(
            got,
            [
                ProviderEvent::Ready,
                original(1, "第一句。", true),
                original(2, "说到一", false),
                original(2, "说到一半", false),
                original(2, "说到一半", true),
                ProviderEvent::Failed(ProviderError::Transport),
            ],
            "{ending:?}"
        );
        client.disconnect().await;
    }
}

#[tokio::test]
async fn inference_close_after_finish_ends_with_the_tail() {
    let (url, _) = mock::ws_server(|mut ws| async move {
        let task = start_task(&mut ws).await;
        mock::send_text(&mut ws, &result(&task, 1, "最后半", false)).await;
        let finish = mock::next_json(&mut ws).await;
        assert_eq!(finish["header"]["action"], "finish-task");
        drop(ws);
    })
    .await;
    let mut client = asr(url);
    let (sink, mut events) = mock::sink();
    client.connect(sink).await.unwrap();
    assert_eq!(mock::next_event(&mut events).await, ProviderEvent::Ready);
    assert_eq!(
        mock::next_event(&mut events).await,
        original(1, "最后半", false)
    );
    client.finish().await.unwrap();

    let got = events_until_last(&mut events).await;

    assert_eq!(got, [original(1, "最后半", true), ProviderEvent::Ended]);
}

#[test]
fn timeout_like_task_failures_are_retryable() {
    let rate = ProviderError::RateLimited { retry_after: None };
    let cases = [
        ("IdleTimeout", "", ProviderError::Transport),
        ("ResponseTimeout", "", ProviderError::Transport),
        (
            "InternalError",
            "Request timeout after 23 seconds.",
            ProviderError::Transport,
        ),
        (
            "ClientError",
            "No valid audio received",
            ProviderError::Transport,
        ),
        (
            "InternalError",
            "silence detected for too long",
            ProviderError::Transport,
        ),
        (
            "InternalError",
            "exceeded the max session duration",
            ProviderError::Transport,
        ),
        (
            "InvalidApiKey",
            "request timeout",
            ProviderError::Authentication,
        ),
        ("Throttling", "timeout", rate),
        ("InternalError", "something else", ProviderError::Provider),
    ];
    for (code, message, expected) in cases {
        assert_eq!(
            parse(&failed(code, message), "t"),
            ServerEvent::Failed(expected),
            "{code} / {message}"
        );
    }
}

fn live(endpoint: Url) -> LiveTranslateClient {
    LiveTranslateClient::new(LiveConfig {
        endpoint,
        api_key: mock::key(),
        proxy: mock::direct(),
        target_language: "en".into(),
        sample_rate: 16_000,
    })
}

async fn live_handshake(ws: &mut mock::Ws) {
    mock::send_text(ws, r#"{"type": "session.created", "session": {}}"#).await;
    let _ = mock::next_json(ws).await;
    mock::send_text(ws, r#"{"type": "session.updated", "session": {}}"#).await;
}

/// Original draft of `src_1` and the finished translation linked to it.
const LIVE_HALF: [&str; 4] = [
    r#"{"type": "conversation.item.input_audio_transcription.text", "item_id": "src_1", "text": "Guten Mor", "stash": ""}"#,
    r#"{"type": "conversation.item.created", "previous_item_id": "src_1", "item": {"id": "tr_1", "role": "assistant"}}"#,
    r#"{"type": "response.text.delta", "item_id": "tr_1", "delta": "Good morn"}"#,
    r#"{"type": "response.text.done", "item_id": "tr_1", "text": "Good morning"}"#,
];

fn src(text: &str, end: bool) -> ProviderEvent {
    let utterance = Some(UtteranceId::Text("src_1".into()));
    let text = text.to_owned();
    ProviderEvent::Caption(if end {
        CaptionEvent::OriginalFinal { utterance, text }
    } else {
        CaptionEvent::OriginalDraft { utterance, text }
    })
}

#[tokio::test]
async fn realtime_ending_with_translation_but_no_original_final_confirms_the_pair_once() {
    for ending in ENDINGS {
        let (url, _) = mock::ws_server(move |mut ws| async move {
            live_handshake(&mut ws).await;
            for line in LIVE_HALF {
                mock::send_text(&mut ws, line).await;
            }
            end(ws, ending, r#"{"type": "session.finished"}"#.into()).await;
        })
        .await;
        let mut client = live(url);
        let (sink, mut events) = mock::sink();
        client.connect(sink).await.unwrap();

        let got = events_until_last(&mut events).await;

        let utterance = Some(UtteranceId::Text("src_1".into()));
        assert_eq!(
            got,
            [
                ProviderEvent::Ready,
                src("Guten Mor", false),
                ProviderEvent::Caption(CaptionEvent::TranslationDraft {
                    utterance: utterance.clone(),
                    text: "Good morn".into()
                }),
                ProviderEvent::Caption(CaptionEvent::TranslationFinal {
                    utterance,
                    text: "Good morning".into()
                }),
                src("Guten Mor", true),
                ProviderEvent::Failed(ProviderError::Transport),
            ],
            "{ending:?}"
        );
        client.disconnect().await;
    }
}

#[tokio::test]
async fn realtime_timeout_errors_are_retryable() {
    for error in [
        json!({"code": "IdleTimeout", "message": ""}),
        json!({"code": "InternalError", "message": "session timeout"}),
    ] {
        let (url, _) = mock::ws_server(move |mut ws| async move {
            live_handshake(&mut ws).await;
            let message = json!({"type": "error", "error": error}).to_string();
            mock::send_text(&mut ws, &message).await;
            mock::drain(ws).await;
        })
        .await;
        let mut client = live(url);
        let (sink, mut events) = mock::sink();
        client.connect(sink).await.unwrap();

        let got = events_until_last(&mut events).await;

        assert_eq!(
            got.last(),
            Some(&ProviderEvent::Failed(ProviderError::Transport))
        );
        client.disconnect().await;
    }
}

#[tokio::test]
async fn realtime_close_after_finish_ends_with_the_tail() {
    let (url, _) = mock::ws_server(|mut ws| async move {
        live_handshake(&mut ws).await;
        mock::send_text(&mut ws, LIVE_HALF[0]).await;
        let finish = mock::next_json(&mut ws).await;
        assert_eq!(finish, json!({"type": "session.finish"}));
        let _ = ws.send(Message::Close(None)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    })
    .await;
    let mut client = live(url);
    let (sink, mut events) = mock::sink();
    client.connect(sink).await.unwrap();
    assert_eq!(mock::next_event(&mut events).await, ProviderEvent::Ready);
    assert_eq!(mock::next_event(&mut events).await, src("Guten Mor", false));
    client.finish().await.unwrap();

    let got = events_until_last(&mut events).await;

    assert_eq!(got, [src("Guten Mor", true), ProviderEvent::Ended]);
}
