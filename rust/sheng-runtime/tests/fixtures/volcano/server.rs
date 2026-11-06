//! Local scripted AST v2 server on 127.0.0.1 for the Volcano client tests.
#![allow(dead_code)]

use std::future::Future;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use sheng_runtime::config::endpoint::ProxyChoice;
use sheng_runtime::config::secret::Secret;
use sheng_runtime::services::volcano::wire::Reader;
use sheng_runtime::services::volcano::{VolcanoClient, VolcanoConfig};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use url::Url;

use super::proto;

pub const KEY: &str = "loopback-key";
pub type Ws = WebSocketStream<TcpStream>;

/// Handshake headers the server saw (lower-case names).
pub type Headers = Vec<(String, String)>;

pub fn config(port: u16, source: &str, timeout: Duration) -> VolcanoConfig {
    VolcanoConfig {
        endpoint: Url::parse(&format!("ws://127.0.0.1:{port}/api/v4/ast/v2/translate")).unwrap(),
        api_key: Secret::new(KEY),
        source_language: source.into(),
        target_language: "en".into(),
        proxy: ProxyChoice::Direct,
        connect_timeout: timeout,
    }
}

pub fn client(port: u16) -> VolcanoClient {
    VolcanoClient::new(config(port, "zh", Duration::from_secs(5)))
}

/// Serves `connections` WebSocket connections, each running `script`; headers go to the
/// returned channel.
pub async fn serve<F, Fut>(connections: usize, script: F) -> (u16, mpsc::UnboundedReceiver<Headers>)
where
    F: Fn(Ws, usize) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        for index in 0..connections {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            #[allow(clippy::result_large_err)]
            let callback = |request: &Request, response: Response| {
                seen = request
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap().to_owned()))
                    .collect();
                Ok(response)
            };
            let ws = tokio_tungstenite::accept_hdr_async(tcp, callback)
                .await
                .unwrap();
            let _ = tx.send(seen);
            tokio::spawn(script(ws, index));
        }
    });
    (port, rx)
}

/// Request summary: event number, audio byte count, session id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub event: u64,
    pub audio: usize,
    pub session: String,
}

pub fn summarize(bytes: &[u8]) -> Seen {
    let mut seen = Seen {
        event: 0,
        audio: 0,
        session: String::new(),
    };
    let mut reader = Reader::new(bytes).unwrap();
    while let Some((number, value)) = reader.next_field().unwrap() {
        match number {
            1 => {
                let mut meta = Reader::new(value.bytes().unwrap()).unwrap();
                while let Some((n, v)) = meta.next_field().unwrap() {
                    if n == 6 {
                        seen.session = v.string().unwrap().to_owned();
                    }
                }
            }
            2 => seen.event = value.varint().unwrap(),
            4 => {
                let mut audio = Reader::new(value.bytes().unwrap()).unwrap();
                while let Some((n, v)) = audio.next_field().unwrap() {
                    if n == 14 {
                        seen.audio = v.bytes().unwrap().len();
                    }
                }
            }
            _ => {}
        }
    }
    seen
}

/// Next binary request, skipping control frames; `None` once the client is gone.
pub async fn next_request(ws: &mut Ws) -> Option<Vec<u8>> {
    while let Some(Ok(message)) = ws.next().await {
        if let Message::Binary(bytes) = message {
            return Some(bytes.to_vec());
        }
    }
    None
}

pub async fn send(ws: &mut Ws, bytes: Vec<u8>) {
    let _ = ws.send(Message::Binary(bytes.into())).await;
}

/// Reads StartSession and answers SessionStarted.
pub async fn accept_start(ws: &mut Ws) -> Seen {
    let start = summarize(&next_request(ws).await.unwrap());
    send(
        ws,
        proto::response(proto::events::SESSION_STARTED, None, None, None),
    )
    .await;
    start
}
