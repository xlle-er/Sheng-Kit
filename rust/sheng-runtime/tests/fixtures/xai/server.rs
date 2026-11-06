//! Scripted local WebSocket server and request helpers for the xAI client tests.
//! Everything listens on 127.0.0.1 only.
#![allow(dead_code)]

use std::future::Future;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sheng_runtime::config::{check_request, SessionPlan};
use sheng_runtime::provider::{EventSink, ProviderEvent};
use sheng_runtime::services::xai::Endpoints;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use url::Url;

pub type Socket = WebSocketStream<TcpStream>;
pub const WAIT: Duration = Duration::from_secs(5);

/// What the handshake carried.
#[derive(Debug, Clone)]
pub struct Seen {
    pub uri: String,
    pub authorization: Option<String>,
}

/// Accepts one WebSocket connection and runs `script` on it.
pub async fn serve<F, Fut>(script: F) -> (Endpoints, oneshot::Receiver<Seen>)
where
    F: FnOnce(Socket) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (seen_tx, seen_rx) = oneshot::channel();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut seen = None;
        #[allow(clippy::result_large_err)] // signature fixed by tungstenite
        let callback = |request: &Request, response: Response| {
            seen = Some(Seen {
                uri: request.uri().to_string(),
                authorization: request
                    .headers()
                    .get("authorization")
                    .map(|v| v.to_str().unwrap().to_owned()),
            });
            Ok(response)
        };
        let Ok(ws) = tokio_tungstenite::accept_hdr_async(tcp, callback).await else {
            return;
        };
        let _ = seen_tx.send(seen.unwrap());
        script(ws).await;
    });
    (endpoints(port), seen_rx)
}

/// Answers the handshake with a fixed HTTP status.
pub async fn status(status_line: &'static str) -> Endpoints {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let _ = tcp.read(&mut buf).await;
        let reply = format!("HTTP/1.1 {status_line}\r\nContent-Length: 0\r\n\r\n");
        let _ = tcp.write_all(reply.as_bytes()).await;
    });
    endpoints(port)
}

fn endpoints(port: u16) -> Endpoints {
    let base = |path: &str| Url::parse(&format!("ws://127.0.0.1:{port}{path}")).unwrap();
    Endpoints {
        realtime: base("/v1/realtime"),
        stt: base("/v1/stt"),
    }
}

/// Validated plan for xAIRealtime in the given mode (`voice` or `asrMt`).
pub fn plan(mode: &str, source: &str, target: &str) -> SessionPlan {
    plan_with_key(mode, source, target, " test-key ", "direct")
}

/// Validated plan using `api_key` for speech and text translation and the given proxy mode.
pub fn plan_with_key(
    mode: &str,
    source: &str,
    target: &str,
    api_key: &str,
    proxy: &str,
) -> SessionPlan {
    let mut configuration = json!({
        "provider": "xAIRealtime", "qwenMtModel": "lite",
        "credentials": {"kind": "apiKey", "apiKey": api_key},
        "sourceLanguage": source, "targetLanguage": target, "translationMode": "turbo",
        "networkProxy": {"mode": proxy}, "textNetworkProxy": {"mode": proxy},
        "speechTranslationMode": "simultaneous", "xaiSpeechMode": mode,
    });
    if mode == "asrMt" && target != "original" {
        configuration["textCredentials"] =
            json!({"kind": "xAI", "apiKey": api_key, "model": "grok-4.3"});
    }
    let request = json!({"input": "system", "configuration": configuration});
    check_request(&request.to_string()).expect("valid plan")
}

pub fn sink() -> (EventSink, mpsc::Receiver<ProviderEvent>) {
    EventSink::channel()
}

/// Next event within `WAIT`.
pub async fn next(events: &mut mpsc::Receiver<ProviderEvent>) -> ProviderEvent {
    tokio::time::timeout(WAIT, events.recv())
        .await
        .expect("event in time")
        .expect("open channel")
}

/// Next text message from the client, parsed; control frames are skipped.
pub async fn recv_json(ws: &mut Socket) -> Value {
    loop {
        match ws.next().await.expect("message").expect("frame") {
            Message::Text(text) => return serde_json::from_str(text.as_str()).unwrap(),
            Message::Binary(_) => panic!("unexpected binary frame"),
            _ => continue,
        }
    }
}

/// Next binary frame from the client.
pub async fn recv_binary(ws: &mut Socket) -> Vec<u8> {
    loop {
        match ws.next().await.expect("message").expect("frame") {
            Message::Binary(bytes) => return bytes.to_vec(),
            Message::Text(text) => panic!("unexpected text {text}"),
            _ => continue,
        }
    }
}

pub async fn send_json(ws: &mut Socket, value: Value) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

/// Sends every message, stopping quietly once the client stops reading.
pub async fn flood(ws: &mut Socket, messages: impl Iterator<Item = Value>) {
    for value in messages {
        if ws
            .send(Message::Text(value.to_string().into()))
            .await
            .is_err()
        {
            return;
        }
    }
}

/// Keeps reading until the client closes, answering pings.
pub async fn drain(mut ws: Socket) {
    while let Some(Ok(_)) = ws.next().await {}
}
