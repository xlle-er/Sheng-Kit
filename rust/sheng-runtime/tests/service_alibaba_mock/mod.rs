//! Local loopback servers for the Alibaba client tests.
#![allow(dead_code)]

use std::future::Future;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use sheng_runtime::config::endpoint::ProxyChoice;
use sheng_runtime::config::secret::Secret;
use sheng_runtime::provider::{EventSink, ProviderEvent};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use url::Url;

use crate::common::http::{read_request, HttpRequest};

pub const WAIT: Duration = Duration::from_secs(5);
pub const KEY: &str = "loopback-key";

pub type Ws = WebSocketStream<TcpStream>;

/// Accepts one WebSocket connection and runs `script` on it. The receiver yields the
/// `Authorization` header the client sent.
#[allow(clippy::result_large_err)] // the error type is fixed by tungstenite's callback
pub async fn ws_server<F, Fut>(script: F) -> (Url, oneshot::Receiver<Option<String>>)
where
    F: FnOnce(Ws) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (auth_tx, auth_rx) = oneshot::channel();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut seen = None;
        let callback = |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                        response| {
            seen = request
                .headers()
                .get("authorization")
                .map(|v| v.to_str().unwrap().to_owned());
            Ok(response)
        };
        let ws = tokio_tungstenite::accept_hdr_async(tcp, callback)
            .await
            .unwrap();
        let _ = auth_tx.send(seen);
        script(ws).await;
    });
    let url = Url::parse(&format!("ws://127.0.0.1:{port}/api-ws/v1/inference")).unwrap();
    (url, auth_rx)
}

/// Next text message as JSON; panics on anything else.
pub async fn next_json(ws: &mut Ws) -> Value {
    loop {
        match tokio::time::timeout(WAIT, ws.next()).await.unwrap() {
            Some(Ok(Message::Text(text))) => return serde_json::from_str(text.as_str()).unwrap(),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            other => panic!("expected a text message, got {other:?}"),
        }
    }
}

pub async fn next_binary(ws: &mut Ws) -> Vec<u8> {
    loop {
        match tokio::time::timeout(WAIT, ws.next()).await.unwrap() {
            Some(Ok(Message::Binary(data))) => return data.to_vec(),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            other => panic!("expected audio, got {other:?}"),
        }
    }
}

/// True when the client sends nothing (other than pings) for `quiet`.
pub async fn stays_quiet(ws: &mut Ws, quiet: Duration) -> bool {
    loop {
        match tokio::time::timeout(quiet, ws.next()).await {
            Err(_) => return true,
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
            Ok(_) => return false,
        }
    }
}

pub async fn send_text(ws: &mut Ws, text: &str) {
    ws.send(Message::Text(text.to_owned().into()))
        .await
        .unwrap();
}

/// Waits until the client closes, so the server does not drop the connection early.
pub async fn drain(mut ws: Ws) {
    while let Ok(Some(Ok(message))) = tokio::time::timeout(WAIT, ws.next()).await {
        if message.is_close() {
            break;
        }
    }
}

/// Answers every connection with a fixed HTTP status.
pub async fn status_server(status_line: &'static str) -> u16 {
    raw_http_server(move |_| {
        format!("HTTP/1.1 {status_line}\r\nRetry-After: 7\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    })
    .await
    .0
}

/// HTTP server that answers each request with `reply(request)`; requests are forwarded.
pub async fn raw_http_server<F>(reply: F) -> (u16, mpsc::UnboundedReceiver<HttpRequest>)
where
    F: Fn(&HttpRequest) -> String + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            let Some(request) = read_request(&mut tcp).await else {
                continue;
            };
            let _ = tcp.write_all(reply(&request).as_bytes()).await;
            let _ = tcp.shutdown().await;
            let _ = tx.send(request);
        }
    });
    (port, rx)
}

pub fn ws_url(port: u16) -> Url {
    Url::parse(&format!("ws://127.0.0.1:{port}/api-ws/v1/inference")).unwrap()
}

pub fn key() -> Secret {
    Secret::new(KEY)
}

pub fn direct() -> ProxyChoice {
    ProxyChoice::Direct
}

pub fn sink() -> (EventSink, mpsc::Receiver<ProviderEvent>) {
    EventSink::channel()
}

pub async fn next_event(events: &mut mpsc::Receiver<ProviderEvent>) -> ProviderEvent {
    tokio::time::timeout(WAIT, events.recv())
        .await
        .expect("event in time")
        .expect("event queue open")
}
