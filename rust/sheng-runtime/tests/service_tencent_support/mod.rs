//! Scripted local Tencent Cloud WebSocket server (127.0.0.1 only) shared by the mock tests.
#![allow(dead_code)]

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use sheng_runtime::config::endpoint::ProxyChoice;
use sheng_runtime::config::secret::Secret;
use sheng_runtime::provider::{EventSink, ProviderEvent, SpeechClient};
use sheng_runtime::services::tencent::client::END_MESSAGE;
use sheng_runtime::services::tencent::signing::{signature, TencentCredentials};
use sheng_runtime::services::tencent::{TencentConfig, TencentSpeech};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;
use url::Url;

pub const WAIT: Duration = Duration::from_secs(5);
pub const SECRET_KEY: &str = "mock-secret-key";
pub const HANDSHAKE: &str = include_str!("../fixtures/tencent/handshake.json");
pub const DRAFT: &str = include_str!("../fixtures/tencent/draft.json");
pub const SENTENCE_END: &str = include_str!("../fixtures/tencent/sentence-end.json");
pub const FINAL: &str = include_str!("../fixtures/tencent/final.json");

/// One server action, in order.
#[derive(Clone)]
pub enum Step {
    Send(&'static str),
    /// Read audio until `{"type":"end"}`, answering pings.
    AwaitEnd,
    Pause(Duration),
    /// Send the same message this many times, stopping early if the client leaves.
    Repeat(&'static str, usize),
    Close,
    /// Drop the TCP connection without a close frame.
    Drop,
}

/// What the server saw.
#[derive(Debug, Default)]
pub struct Seen {
    pub path_and_query: String,
    pub frames: Vec<usize>,
    pub got_end: bool,
}

#[allow(clippy::result_large_err)] // the error type is fixed by tungstenite's callback signature
pub fn record_uri(
    slot: &mut String,
) -> impl FnOnce(
    &Request,
    Response,
) -> Result<Response, tokio_tungstenite::tungstenite::handshake::server::ErrorResponse>
       + '_ {
    move |request: &Request, response: Response| {
        *slot = request.uri().to_string();
        Ok(response)
    }
}

/// Serves one connection with `steps`, then keeps reading until the client leaves.
pub async fn serve(steps: Vec<Step>) -> (u16, oneshot::Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (done_tx, done_rx) = oneshot::channel();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut seen = Seen::default();
        let mut ws = tokio_tungstenite::accept_hdr_async(tcp, record_uri(&mut seen.path_and_query))
            .await
            .unwrap();
        for step in steps {
            match step {
                Step::Send(text) => ws.send(Message::text(text)).await.unwrap(),
                Step::AwaitEnd => read_until_end(&mut ws, &mut seen).await,
                Step::Pause(duration) => tokio::time::sleep(duration).await,
                Step::Repeat(text, times) => {
                    for _ in 0..times {
                        if ws.send(Message::text(text)).await.is_err() {
                            break;
                        }
                    }
                }
                Step::Close => {
                    let _ = ws.close(None).await;
                }
                Step::Drop => {
                    let _ = done_tx.send(seen);
                    return;
                }
            }
        }
        while let Some(Ok(_)) = ws.next().await {}
        let _ = done_tx.send(seen);
    });
    (port, done_rx)
}

pub async fn read_until_end<S>(ws: &mut tokio_tungstenite::WebSocketStream<S>, seen: &mut Seen)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    while let Some(Ok(message)) = ws.next().await {
        match message {
            Message::Binary(data) => seen.frames.push(data.len()),
            Message::Text(text) if text.as_str() == END_MESSAGE => {
                seen.got_end = true;
                return;
            }
            _ => {}
        }
    }
}

pub fn client(port: u16) -> TencentSpeech {
    let endpoint = format!("ws://127.0.0.1:{port}/asr/speech_translate/");
    TencentSpeech::new(TencentConfig {
        endpoint: Url::parse(&endpoint).unwrap(),
        credentials: TencentCredentials::new(
            &Secret::new("1250000000"),
            Secret::new("AKIDmock"),
            Secret::new(SECRET_KEY),
        )
        .unwrap(),
        source: "zh".into(),
        target: "en".into(),
        proxy: ProxyChoice::Direct,
    })
}

pub async fn connected(
    steps: Vec<Step>,
) -> (
    TencentSpeech,
    mpsc::Receiver<ProviderEvent>,
    oneshot::Receiver<Seen>,
) {
    let (port, seen) = serve(steps).await;
    let mut speech = client(port);
    let (sink, mut events) = EventSink::channel();
    tokio::time::timeout(WAIT, speech.connect(sink))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next(&mut events).await, ProviderEvent::Ready);
    (speech, events, seen)
}

pub async fn next(events: &mut mpsc::Receiver<ProviderEvent>) -> ProviderEvent {
    tokio::time::timeout(WAIT, events.recv())
        .await
        .unwrap()
        .unwrap()
}

/// Recomputes the signature the way the service documents it.
pub fn signature_is_valid(path_and_query: &str, port: u16) -> bool {
    let (unsigned, encoded) = path_and_query.rsplit_once("&signature=").unwrap();
    let (path, query) = unsigned.split_once('?').unwrap();
    let raw: Vec<String> = query
        .split('&')
        .map(|pair| {
            percent_encoding::percent_decode_str(pair)
                .decode_utf8()
                .unwrap()
                .into_owned()
        })
        .collect();
    let text = format!("127.0.0.1:{port}{path}?{}", raw.join("&"));
    let sent = percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .unwrap();
    sent == signature(&Secret::new(SECRET_KEY), &text)
}
