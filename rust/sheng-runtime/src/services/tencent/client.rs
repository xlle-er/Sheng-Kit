//! Speech client of one source's lane: signed handshake, PCM frames, `{"type":"end"}`,
//! and a reader task that turns server messages into provider events.

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use super::protocol::{self, Parsed};
use super::signing::{signed_url, ConnectionParams, TencentCredentials};
use crate::config::endpoint::ProxyChoice;
use crate::provider::{BoxFuture, EventSink, ProviderError, ProviderEvent, SpeechClient};
use crate::transport::{connect_websocket, WsRequest, WsStream, CONNECT_TIMEOUT};

/// Largest binary frame: 200 ms of 16 kHz 16-bit mono PCM, as the documentation advises.
pub const MAX_FRAME_BYTES: usize = 6400;
/// Text message that ends the audio stream.
pub const END_MESSAGE: &str = r#"{"type":"end"}"#;
/// How long a polite close may take on disconnect.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);
/// Pongs waiting for a heartbeat; extra pongs are dropped.
const PONG_QUEUE: usize = 4;

type Writer = SplitSink<WsStream, Message>;
type Reader = SplitStream<WsStream>;

/// Everything one connection needs. The endpoint is injectable for local tests; the
/// production endpoint is set only in `services::tencent::speech`.
#[derive(Debug, Clone)]
pub struct TencentConfig {
    /// Base URL ending in `/`; the AppId and query are appended.
    pub endpoint: Url,
    pub credentials: TencentCredentials,
    pub source: String,
    pub target: String,
    pub proxy: ProxyChoice,
}

/// Connection state is created by `connect` and dropped by `disconnect`.
pub struct TencentSpeech {
    config: TencentConfig,
    writer: Option<Writer>,
    reader: Option<JoinHandle<()>>,
    pongs: Option<mpsc::Receiver<()>>,
    /// Set once `{"type":"end"}` was sent; a close after it ends the session normally.
    finishing: Arc<AtomicBool>,
}

impl std::fmt::Debug for TencentSpeech {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TencentSpeech")
            .field("connected", &self.writer.is_some())
            .finish_non_exhaustive()
    }
}

impl TencentSpeech {
    pub fn new(config: TencentConfig) -> Self {
        Self {
            config,
            writer: None,
            reader: None,
            pongs: None,
            finishing: Arc::new(AtomicBool::new(false)),
        }
    }

    async fn open(&mut self, events: EventSink) -> Result<(), ProviderError> {
        let config = &self.config;
        let params = ConnectionParams::fresh(&config.source, &config.target);
        let request = WsRequest {
            url: signed_url(&config.endpoint, &config.credentials, &params)?,
            headers: Vec::new(),
        };
        let stream = connect_websocket(&request, &config.proxy, CONNECT_TIMEOUT).await?;
        let (writer, mut reader) = stream.split();
        let first = await_handshake(&mut reader).await?;
        events
            .emit(ProviderEvent::Ready)
            .map_err(|_| ProviderError::Transport)?;
        let (pong_tx, pong_rx) = mpsc::channel(PONG_QUEUE);
        self.finishing = Arc::new(AtomicBool::new(false));
        let finishing = Arc::clone(&self.finishing);
        // The handshake reply carries no result; anything it does carry is still reported.
        match deliver(first, &events) {
            ControlFlow::Continue(()) => {
                self.reader = Some(tokio::spawn(read_loop(reader, events, pong_tx, finishing)));
            }
            ControlFlow::Break(event) => {
                let _ = events.emit(event);
            }
        }
        self.writer = Some(writer);
        self.pongs = Some(pong_rx);
        Ok(())
    }

    async fn write(&mut self, message: Message) -> Result<(), ProviderError> {
        let writer = self.writer.as_mut().ok_or(ProviderError::Transport)?;
        writer
            .send(message)
            .await
            .map_err(|_| ProviderError::Transport)
    }

    async fn ping(&mut self) -> Result<(), ProviderError> {
        let pongs = self.pongs.as_mut().ok_or(ProviderError::Transport)?;
        while pongs.try_recv().is_ok() {}
        self.write(Message::Ping(Vec::new().into())).await?;
        let pongs = self.pongs.as_mut().ok_or(ProviderError::Transport)?;
        pongs.recv().await.ok_or(ProviderError::Transport)
    }

    async fn close(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
        self.pongs = None;
        if let Some(mut writer) = self.writer.take() {
            let _ = tokio::time::timeout(CLOSE_TIMEOUT, writer.close()).await;
        }
    }
}

/// Waits for the first text message: `code:0` accepts the connection.
async fn await_handshake(reader: &mut Reader) -> Result<Parsed, ProviderError> {
    loop {
        match reader.next().await {
            Some(Ok(Message::Text(text))) => return protocol::parse(text.as_str()),
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                return Err(ProviderError::Transport)
            }
            Some(Ok(_)) => {}
        }
    }
}

/// Emits the caption events of one message; `Break` carries the event that ends reading.
fn deliver(parsed: Parsed, events: &EventSink) -> ControlFlow<ProviderEvent> {
    if let Some(sentence) = parsed.sentence {
        for caption in protocol::caption_events(sentence) {
            if events.emit(ProviderEvent::Caption(caption)).is_err() {
                // The lane is gone or its queue overflowed; nothing more can be reported.
                return ControlFlow::Break(ProviderEvent::Failed(ProviderError::Transport));
            }
        }
    }
    if parsed.is_final {
        return ControlFlow::Break(ProviderEvent::Ended);
    }
    ControlFlow::Continue(())
}

fn on_message(
    message: Message,
    events: &EventSink,
    pongs: &mpsc::Sender<()>,
) -> ControlFlow<ProviderEvent> {
    match message {
        Message::Text(text) => match protocol::parse(text.as_str()) {
            Ok(parsed) => deliver(parsed, events),
            Err(error) => ControlFlow::Break(ProviderEvent::Failed(error)),
        },
        Message::Pong(_) => {
            let _ = pongs.try_send(());
            ControlFlow::Continue(())
        }
        // Synthesized speech (not requested) and pings answered by the transport.
        _ => ControlFlow::Continue(()),
    }
}

/// Reads until `final:1`. Before `{"type":"end"}` a closed connection is a retryable
/// transport failure; during the finish wait it ends the session with what arrived
/// (providers/README.md “意外断开”, “结束时不丢尾句”).
async fn read_loop(
    mut reader: Reader,
    events: EventSink,
    pongs: mpsc::Sender<()>,
    finishing: Arc<AtomicBool>,
) {
    let last = loop {
        let message = match reader.next().await {
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                break closed_event(&finishing);
            }
            Some(Ok(message)) => message,
        };
        if let ControlFlow::Break(event) = on_message(message, &events, &pongs) {
            break event;
        }
    };
    let _ = events.emit(last);
}

fn closed_event(finishing: &AtomicBool) -> ProviderEvent {
    if finishing.load(Ordering::Acquire) {
        ProviderEvent::Ended
    } else {
        ProviderEvent::Failed(ProviderError::Transport)
    }
}

impl SpeechClient for TencentSpeech {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            self.close().await;
            self.open(events).await
        })
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            for frame in pcm.chunks(MAX_FRAME_BYTES) {
                self.write(Message::Binary(frame.to_vec().into())).await?;
            }
            Ok(())
        })
    }

    fn heartbeat(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.ping())
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        self.finishing.store(true, Ordering::Release);
        Box::pin(self.write(Message::Text(END_MESSAGE.into())))
    }

    fn disconnect(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(self.close())
    }
}
