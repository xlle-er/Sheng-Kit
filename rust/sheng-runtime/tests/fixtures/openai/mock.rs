//! Local stand-ins for the realtime service: a WebSocket server the test drives frame by
//! frame, and a server that answers the handshake with a fixed HTTP status.
#![allow(dead_code)]

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use sheng_runtime::config::endpoint::ProxyChoice;
use sheng_runtime::config::secret::Secret;
use sheng_runtime::provider::ProviderEvent;
use sheng_runtime::services::openai::{LanguageField, RealtimeConfig, SessionKind};
use sheng_runtime::transport::WsRequest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use url::Url;

/// Longest wait for any one step of a scripted exchange.
pub const STEP: Duration = Duration::from_secs(5);

pub type ServerSocket = WebSocketStream<TcpStream>;

/// Accepts one WebSocket connection and hands its server side to the test.
pub async fn listen() -> (u16, oneshot::Receiver<ServerSocket>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let _ = tx.send(socket);
    });
    (port, rx)
}

/// Answers every connection with `status_line` (e.g. `401 Unauthorized`).
pub async fn status_server(status_line: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = tcp.read(&mut buf).await;
            let reply =
                format!("HTTP/1.1 {status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = tcp.write_all(reply.as_bytes()).await;
        }
    });
    port
}

pub async fn send(socket: &mut ServerSocket, text: &str) {
    socket.send(Message::text(text)).await.unwrap();
}

/// Next text frame from the client as JSON; control frames are skipped.
pub async fn recv(socket: &mut ServerSocket) -> Value {
    let next = async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => return serde_json::from_str(&text).unwrap(),
                Some(Ok(_)) => continue,
                other => panic!("client went away: {other:?}"),
            }
        }
    };
    tokio::time::timeout(STEP, next)
        .await
        .expect("client frame")
}

/// Receives client frames until one of type `kind` arrives.
pub async fn recv_type(socket: &mut ServerSocket, kind: &str) -> Value {
    loop {
        let message = recv(socket).await;
        if message["type"] == kind {
            return message;
        }
    }
}

/// Standard handshake from the server side; returns the client's `session.update`.
pub async fn handshake(socket: &mut ServerSocket) -> Value {
    send(socket, r#"{"type":"session.created","session":{"id":"s"}}"#).await;
    let update = recv(socket).await;
    send(socket, r#"{"type":"session.updated","session":{"id":"s"}}"#).await;
    update
}

pub fn config(port: u16, kind: SessionKind, custom_endpoint: bool) -> RealtimeConfig {
    RealtimeConfig {
        request: WsRequest {
            url: Url::parse(&format!("ws://127.0.0.1:{port}/v1/realtime")).unwrap(),
            headers: vec![("authorization", Secret::new("Bearer local-test"))],
        },
        proxy: ProxyChoice::Direct,
        kind,
        custom_endpoint,
    }
}

pub fn transcribe_kind() -> SessionKind {
    SessionKind::Transcribe {
        model: "gpt-live-transcribe".into(),
        language: Some("en".into()),
        language_field: LanguageField::List,
    }
}

pub fn translate_kind() -> SessionKind {
    SessionKind::Translate {
        source_transcript_model: "gpt-realtime-whisper".into(),
        output_language: "fr".into(),
    }
}

/// `millis` of a loud square wave, PCM16 mono 24 kHz; counts as speech.
pub fn speech(millis: usize) -> Vec<u8> {
    (0..millis * 24)
        .flat_map(|i| {
            let sample: i16 = if (i / 24) % 2 == 0 { 4000 } else { -4000 };
            sample.to_le_bytes()
        })
        .collect()
}

/// Next provider event within [`STEP`].
pub async fn next_event(events: &mut mpsc::Receiver<ProviderEvent>) -> ProviderEvent {
    tokio::time::timeout(STEP, events.recv())
        .await
        .expect("event in time")
        .expect("sink open")
}

/// Events until (and including) `Ended` or `Failed`.
pub async fn until_end(events: &mut mpsc::Receiver<ProviderEvent>) -> Vec<ProviderEvent> {
    let mut seen = Vec::new();
    loop {
        let event = next_event(events).await;
        let last = matches!(event, ProviderEvent::Ended | ProviderEvent::Failed(_));
        seen.push(event);
        if last {
            return seen;
        }
    }
}
