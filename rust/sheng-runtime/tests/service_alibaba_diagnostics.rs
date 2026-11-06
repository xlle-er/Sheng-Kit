//! Failure details the Alibaba clients keep for diagnosis: close code and reason, and the
//! error fields of server events — never caption text.

mod common;
mod service_alibaba_mock;

use futures_util::SinkExt;
use service_alibaba_mock as mock;
use sheng_runtime::provider::{ProviderError, ProviderEvent, SpeechClient};
use sheng_runtime::services::alibaba::asr::client::{AsrClient, AsrConfig};
use sheng_runtime::services::alibaba::asr::protocol::TaskSettings;
use sheng_runtime::services::alibaba::diagnostics::{FailureDetail, FailureNote};
use sheng_runtime::services::alibaba::live::client::{LiveConfig, LiveTranslateClient};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

fn live_client(endpoint: Url) -> LiveTranslateClient {
    LiveTranslateClient::new(LiveConfig {
        endpoint,
        api_key: mock::key(),
        proxy: mock::direct(),
        target_language: "en".into(),
        sample_rate: 16_000,
    })
}

async fn handshake(ws: &mut mock::Ws) {
    mock::send_text(ws, r#"{"type": "session.created", "session": {}}"#).await;
    let _ = mock::next_json(ws).await;
    mock::send_text(ws, r#"{"type": "session.updated", "session": {}}"#).await;
}

#[tokio::test]
async fn close_frame_after_the_handshake_is_kept_with_code_and_reason() {
    let (url, _) = mock::ws_server(|mut ws| async move {
        handshake(&mut ws).await;
        let frame = CloseFrame {
            code: CloseCode::Policy,
            reason: "model not available on this host".into(),
        };
        ws.send(Message::Close(Some(frame))).await.unwrap();
        mock::drain(ws).await;
    })
    .await;
    let mut client = live_client(url);
    let (sink, mut events) = mock::sink();

    client.connect(sink).await.expect("connects");

    assert_eq!(mock::next_event(&mut events).await, ProviderEvent::Ready);
    assert_eq!(
        mock::next_event(&mut events).await,
        ProviderEvent::Failed(ProviderError::Transport)
    );
    let detail = client.failure_detail();
    assert_eq!(detail.close_code, Some(1008));
    assert_eq!(
        detail.close_reason.as_deref(),
        Some("model not available on this host")
    );
    client.disconnect().await;
}

#[tokio::test]
async fn error_event_in_the_handshake_keeps_its_type_code_message_and_param() {
    let (url, _) = mock::ws_server(|mut ws| async move {
        mock::send_text(&mut ws, r#"{"type": "session.created"}"#).await;
        let _ = mock::next_json(&mut ws).await;
        let error = r#"{"type": "error", "error": {"type": "invalid_request_error",
            "code": "invalid_value", "message": "Invalid modalities", "param": "session.modalities"}}"#;
        mock::send_text(&mut ws, error).await;
        mock::drain(ws).await;
    })
    .await;
    let mut client = live_client(url);
    let (sink, _events) = mock::sink();

    assert_eq!(client.connect(sink).await, Err(ProviderError::Provider));

    assert_eq!(
        client.failure_detail(),
        FailureDetail {
            event: Some("error".into()),
            error_type: Some("invalid_request_error".into()),
            error_code: Some("invalid_value".into()),
            error_message: Some("Invalid modalities".into()),
            error_param: Some("session.modalities".into()),
            ..FailureDetail::default()
        }
    );
}

#[tokio::test]
async fn recognition_task_failed_keeps_the_header_error_fields() {
    let (url, _) = mock::ws_server(|mut ws| async move {
        let run = mock::next_json(&mut ws).await;
        let task = run["header"]["task_id"].as_str().unwrap().to_owned();
        let failed = format!(
            r#"{{"header": {{"task_id": "{task}", "event": "task-failed",
              "error_code": "InvalidParameter", "error_message": "bad model"}}, "payload": {{}}}}"#
        );
        mock::send_text(&mut ws, &failed).await;
        mock::drain(ws).await;
    })
    .await;
    let mut client = AsrClient::new(AsrConfig {
        endpoint: url,
        api_key: mock::key(),
        proxy: mock::direct(),
        settings: TaskSettings {
            model: "m".into(),
            sample_rate: 16_000,
            source_language: "zh".into(),
        },
    });
    let (sink, _events) = mock::sink();

    assert!(client.connect(sink).await.is_err());

    let detail = client.failure_detail();
    assert_eq!(detail.event.as_deref(), Some("task-failed"));
    assert_eq!(detail.error_code.as_deref(), Some("InvalidParameter"));
    assert_eq!(detail.error_message.as_deref(), Some("bad model"));
}

#[test]
fn caption_events_never_enter_the_detail() {
    let note = FailureNote::default();

    note.message(r#"{"type": "response.text.done", "text": "the error failed us"}"#);
    note.message(
        r#"{"type": "response.done", "response": {"status": "completed",
          "output": [{"content": [{"text": "error"}]}]}}"#,
    );

    assert!(note.detail().is_empty());
    assert_eq!(
        note.detail().to_string(),
        "no close frame or error event received"
    );
}

#[test]
fn failed_response_and_long_texts_are_kept_short() {
    let note = FailureNote::default();
    let long = "x".repeat(500);

    note.message(&format!(
        r#"{{"type": "response.done", "response": {{"status": "failed",
           "status_details": {{"error": {{"code": "quota", "message": "{long}"}}}}}}}}"#
    ));

    let detail = note.detail();
    assert_eq!(detail.event.as_deref(), Some("response.done"));
    assert_eq!(detail.error_code.as_deref(), Some("quota"));
    assert_eq!(detail.error_message.map(|m| m.chars().count()), Some(160));
}

#[test]
fn display_lists_only_reported_fields() {
    let detail = FailureDetail {
        close_code: Some(1008),
        error_code: Some("invalid_value".into()),
        ..FailureDetail::default()
    };

    assert_eq!(
        detail.to_string(),
        r#"close_code="1008" error_code="invalid_value""#
    );
}
