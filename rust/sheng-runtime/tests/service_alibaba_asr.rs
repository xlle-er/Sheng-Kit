//! Recognition client against a scripted local inference server.

mod common;
mod service_alibaba_mock;

use std::time::Duration;

use futures_util::SinkExt;
use serde_json::json;
use service_alibaba_mock as mock;
use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::provider::{ProviderError, ProviderEvent, SpeechClient};
use sheng_runtime::services::alibaba::asr::client::{AsrClient, AsrConfig};
use sheng_runtime::services::alibaba::asr::protocol::TaskSettings;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

fn client(endpoint: Url) -> AsrClient {
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

fn event(task: &str, name: &str) -> String {
    json!({"header": {"task_id": task, "event": name, "attributes": {}}, "payload": {}}).to_string()
}

fn result(task: &str, id: u64, text: &str, end: bool) -> String {
    json!({
        "header": {"task_id": task, "event": "result-generated", "attributes": {}},
        "payload": {"output": {"sentence": {"sentence_id": id, "text": text, "sentence_end": end}}},
    })
    .to_string()
}

fn failed(task: &str, code: &str) -> String {
    json!({"header": {"task_id": task, "event": "task-failed", "error_code": code,
        "error_message": "detail"}, "payload": {}})
    .to_string()
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

/// Reads `run-task`, checks that no audio arrives before `task-started`, and returns the id.
async fn start_task(ws: &mut mock::Ws) -> String {
    let run = mock::next_json(ws).await;
    assert_eq!(run["header"]["action"], "run-task");
    assert_eq!(run["payload"]["model"], "asr-model");
    assert_eq!(
        run["payload"]["parameters"]["language_hints"],
        json!(["zh"])
    );
    assert!(mock::stays_quiet(ws, Duration::from_millis(150)).await);
    let task = run["header"]["task_id"].as_str().unwrap().to_owned();
    mock::send_text(ws, &event(&task, "task-started")).await;
    task
}

#[tokio::test]
async fn recognition_runs_from_task_start_to_final_after_stop() {
    let (url, auth) = mock::ws_server(|mut ws| async move {
        let task = start_task(&mut ws).await;
        assert_eq!(mock::next_binary(&mut ws).await, vec![1, 2, 3, 4]);
        mock::send_text(&mut ws, &result(&task, 1, "你好", false)).await;
        mock::send_text(&mut ws, &result(&task, 1, "", false)).await;
        mock::send_text(&mut ws, &result(&task, 1, "你好。", true)).await;
        mock::send_text(&mut ws, &result("other-task", 9, "stale", true)).await;
        let finish = mock::next_json(&mut ws).await;
        assert_eq!(finish["header"]["action"], "finish-task");
        assert_eq!(finish["header"]["task_id"], task.as_str());
        // The last sentence is only final after the client asked to finish.
        mock::send_text(&mut ws, &result(&task, 2, "再见。", true)).await;
        mock::send_text(&mut ws, &event(&task, "task-finished")).await;
        mock::drain(ws).await;
    })
    .await;
    let mut client = client(url);
    let (sink, mut events) = mock::sink();
    client.connect(sink).await.expect("connects");
    assert_eq!(auth.await.unwrap().as_deref(), Some("Bearer loopback-key"));
    assert_eq!(mock::next_event(&mut events).await, ProviderEvent::Ready);
    client.send_audio(vec![1, 2, 3, 4]).await.unwrap();
    assert_eq!(
        mock::next_event(&mut events).await,
        original(1, "你好", false)
    );
    assert_eq!(
        mock::next_event(&mut events).await,
        original(1, "你好。", true)
    );
    client.finish().await.unwrap();
    assert_eq!(
        mock::next_event(&mut events).await,
        original(2, "再见。", true)
    );
    assert_eq!(mock::next_event(&mut events).await, ProviderEvent::Ended);
    client.disconnect().await;
}

#[tokio::test]
async fn task_failed_before_start_fails_connect_with_its_mapping() {
    let (url, _) = mock::ws_server(|mut ws| async move {
        let run = mock::next_json(&mut ws).await;
        let task = run["header"]["task_id"].as_str().unwrap().to_owned();
        mock::send_text(&mut ws, &failed(&task, "InvalidApiKey")).await;
        mock::drain(ws).await;
    })
    .await;
    let (sink, _events) = mock::sink();
    let result = client(url).connect(sink).await;
    assert_eq!(result, Err(ProviderError::Authentication));
}

#[tokio::test]
async fn task_failed_while_running_is_reported_once() {
    let (url, _) = mock::ws_server(|mut ws| async move {
        let task = start_task(&mut ws).await;
        mock::send_text(&mut ws, &failed(&task, "Throttling")).await;
        mock::send_text(&mut ws, &result(&task, 1, "after failure", true)).await;
        mock::drain(ws).await;
    })
    .await;
    let mut client = client(url);
    let (sink, mut events) = mock::sink();
    client.connect(sink).await.unwrap();
    assert_eq!(mock::next_event(&mut events).await, ProviderEvent::Ready);
    let rate = ProviderError::RateLimited { retry_after: None };
    assert_eq!(
        mock::next_event(&mut events).await,
        ProviderEvent::Failed(rate)
    );
    let closed = tokio::time::timeout(mock::WAIT, events.recv())
        .await
        .unwrap();
    assert_eq!(closed, None, "nothing is read after the failure");
}

#[tokio::test]
async fn connection_closed_before_finish_is_a_transport_failure() {
    let (url, _) = mock::ws_server(|mut ws| async move {
        start_task(&mut ws).await;
        drop(ws);
    })
    .await;
    let mut client = client(url);
    let (sink, mut events) = mock::sink();
    client.connect(sink).await.unwrap();
    assert_eq!(mock::next_event(&mut events).await, ProviderEvent::Ready);
    assert_eq!(
        mock::next_event(&mut events).await,
        ProviderEvent::Failed(ProviderError::Transport)
    );
}

#[tokio::test]
async fn events_after_disconnect_are_dropped() {
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let (url, _) = mock::ws_server(|mut ws| async move {
        let task = start_task(&mut ws).await;
        let _ = release_rx.await;
        mock::send_text(&mut ws, &result(&task, 1, "late", true)).await;
    })
    .await;
    let mut client = client(url);
    let (sink, mut events) = mock::sink();
    client.connect(sink).await.unwrap();
    assert_eq!(mock::next_event(&mut events).await, ProviderEvent::Ready);
    client.disconnect().await;
    let _ = release_tx.send(());
    let after = tokio::time::timeout(mock::WAIT, events.recv())
        .await
        .unwrap();
    assert_eq!(after, None, "the reader of the old connection is gone");
    assert_eq!(
        client.send_audio(vec![0; 4]).await,
        Err(ProviderError::Transport)
    );
}

#[tokio::test]
async fn handshake_statuses_are_classified() {
    let cases = [
        ("401 Unauthorized", ProviderError::Authentication),
        ("403 Forbidden", ProviderError::Authentication),
        ("404 Not Found", ProviderError::Configuration),
        (
            "429 Too Many Requests",
            ProviderError::RateLimited {
                retry_after: Some(Duration::from_secs(7)),
            },
        ),
        (
            "503 Service Unavailable",
            ProviderError::Unavailable {
                retry_after: Some(Duration::from_secs(7)),
            },
        ),
    ];
    for (status, expected) in cases {
        let port = mock::status_server(status).await;
        let (sink, _events) = mock::sink();
        let result = client(mock::ws_url(port)).connect(sink).await;
        assert_eq!(result, Err(expected), "{status}");
    }
}

#[tokio::test]
async fn a_server_that_never_starts_the_task_keeps_connect_pending() {
    let (url, _) = mock::ws_server(|ws| async move {
        mock::drain(ws).await;
    })
    .await;
    let mut client = client(url);
    let (sink, _events) = mock::sink();
    let pending = tokio::time::timeout(Duration::from_millis(300), client.connect(sink)).await;
    assert!(pending.is_err(), "the session's connect timeout applies");
}

#[tokio::test]
async fn full_event_queue_sets_the_overflow_flag_and_stops_the_reader() {
    let (url, _auth) = mock::ws_server(|mut ws| async move {
        let task = start_task(&mut ws).await;
        let draft = result(&task, 1, "词", false);
        for _ in 0..common::overflow::FLOOD {
            if ws.send(Message::text(draft.as_str())).await.is_err() {
                break;
            }
        }
        mock::drain(ws).await;
    })
    .await;
    let mut client = client(url);
    let (sink, mut events) = mock::sink();
    let flag = sink.overflow_flag();
    client.connect(sink).await.expect("connects");

    common::overflow::assert_reader_stops_on_overflow(&flag, &mut events).await;
    client.disconnect().await;
}
