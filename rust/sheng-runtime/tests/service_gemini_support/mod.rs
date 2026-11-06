//! Local Gemini Live stand-in: one scripted WebSocket connection on 127.0.0.1.
#![allow(dead_code)]

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sheng_runtime::config::{check_request, SessionPlan};
use sheng_runtime::provider::{EventSink, ProviderEvent, SpeechClient};
use sheng_runtime::services::gemini;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

pub const WAIT: Duration = Duration::from_secs(5);

/// What the stand-in sends next.
pub enum Out {
    Json(Value),
    /// JSON in a binary frame, as the service sends it.
    Binary(Value),
    Close(CloseCode, &'static str),
    /// Drops the TCP connection without a close frame.
    Drop,
}

/// Handshake facts the stand-in saw.
#[derive(Debug, Clone)]
pub struct Seen {
    pub api_key: Option<String>,
    pub query: Option<String>,
}

pub struct Mock {
    pub url: Url,
    pub seen: oneshot::Receiver<Seen>,
    /// Client messages, parsed.
    pub received: mpsc::UnboundedReceiver<Value>,
    out: mpsc::UnboundedSender<Out>,
}

impl Mock {
    pub fn send(&self, out: Out) {
        self.out.send(out).expect("mock running");
    }

    pub fn json(&self, value: Value) {
        self.send(Out::Json(value));
    }

    pub fn content(&self, content: Value) {
        self.send(Out::Binary(json!({ "serverContent": content })));
    }

    pub async fn next(&mut self) -> Value {
        tokio::time::timeout(WAIT, self.received.recv())
            .await
            .expect("client message in time")
            .expect("connection open")
    }

    /// Reads the setup message and answers `setupComplete`.
    pub async fn complete_setup(&mut self) -> Value {
        let setup = self.next().await;
        self.json(json!({ "setupComplete": {} }));
        setup
    }
}

async fn serve(
    listener: TcpListener,
    seen_tx: oneshot::Sender<Seen>,
    received: mpsc::UnboundedSender<Value>,
    mut out: mpsc::UnboundedReceiver<Out>,
) {
    let (tcp, _) = listener.accept().await.unwrap();
    let mut seen = None;
    #[allow(clippy::result_large_err)] // signature fixed by tungstenite
    let callback = |request: &Request, response: Response| {
        let header = request.headers().get("x-goog-api-key");
        seen = Some(Seen {
            api_key: header.map(|v| v.to_str().unwrap().to_owned()),
            query: request.uri().query().map(str::to_owned),
        });
        Ok(response)
    };
    let mut ws = tokio_tungstenite::accept_hdr_async(tcp, callback)
        .await
        .unwrap();
    let _ = seen_tx.send(seen.unwrap());
    loop {
        tokio::select! {
            message = ws.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    let _ = received.send(serde_json::from_str(&text).unwrap());
                }
                Some(Ok(_)) => {}
                _ => return,
            },
            command = out.recv() => match command {
                Some(Out::Json(value)) => {
                    let _ = ws.send(Message::text(value.to_string())).await;
                }
                Some(Out::Binary(value)) => {
                    let _ = ws.send(Message::Binary(value.to_string().into_bytes().into())).await;
                }
                Some(Out::Close(code, reason)) => {
                    let frame = CloseFrame { code, reason: reason.into() };
                    let _ = ws.close(Some(frame)).await;
                }
                Some(Out::Drop) | None => return,
            },
        }
    }
}

pub async fn mock() -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (seen_tx, seen) = oneshot::channel();
    let (received_tx, received) = mpsc::unbounded_channel();
    let (out, out_rx) = mpsc::unbounded_channel();
    tokio::spawn(serve(listener, seen_tx, received_tx, out_rx));
    Mock {
        url: Url::parse(&format!("ws://127.0.0.1:{port}/ws")).unwrap(),
        seen,
        received,
        out,
    }
}

/// Plan for a Gemini session as the Swift caller builds it.
pub fn plan(mode: &str, source: &str, target: &str) -> SessionPlan {
    let text = if mode == "asrMt" { "deepL" } else { "-" };
    let request =
        crate::common::start_request("system", "googleGeminiLive", mode, text, source, target);
    check_request(&request.to_string()).expect("valid gemini request")
}

pub fn client(plan: &SessionPlan, mock: &Mock) -> Box<dyn SpeechClient> {
    gemini::speech_at(plan, mock.url.clone()).expect("client")
}

/// Connects `client`, answering setup; returns the event queue.
pub async fn connected(
    client: &mut Box<dyn SpeechClient>,
    mock: &mut Mock,
) -> mpsc::Receiver<ProviderEvent> {
    let (sink, events) = EventSink::channel();
    let (connect, _) = tokio::join!(client.connect(sink), mock.complete_setup());
    connect.expect("connects");
    events
}

pub async fn next_event(events: &mut mpsc::Receiver<ProviderEvent>) -> ProviderEvent {
    tokio::time::timeout(WAIT, events.recv())
        .await
        .expect("event in time")
        .expect("queue open")
}

/// Events until (and including) the first one matching `last`.
pub async fn events_until(
    events: &mut mpsc::Receiver<ProviderEvent>,
    last: fn(&ProviderEvent) -> bool,
) -> Vec<ProviderEvent> {
    let mut seen = Vec::new();
    loop {
        let event = next_event(events).await;
        let done = last(&event);
        seen.push(event);
        if done {
            return seen;
        }
    }
}
