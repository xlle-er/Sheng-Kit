//! WebSocket session: StartSession → 80 ms TaskRequests → FinishSession.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use super::frames::Framer;
use super::language::wire_pair;
use super::mapper::{Mapped, Mapper};
use super::message::{decode_response, finish_session, start_session, task_request};
use crate::config::endpoint::ProxyChoice;
use crate::config::secret::Secret;
use crate::provider::{BoxFuture, EventSink, ProviderError, ProviderEvent, SpeechClient};
use crate::transport::{connect_websocket, WsRequest, WsStream};

/// Fixed resource id of the simultaneous interpretation service.
pub const RESOURCE_ID: &str = "volc.service_type.10053";
/// Closing the socket on disconnect is given at most this long.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

type Writer = SplitSink<WsStream, Message>;
type Reader = SplitStream<WsStream>;

/// Everything one lane's client needs. The endpoint is injectable for local tests.
#[derive(Debug, Clone)]
pub struct VolcanoConfig {
    pub endpoint: Url,
    pub api_key: Secret,
    pub source_language: String,
    pub target_language: String,
    pub proxy: ProxyChoice,
    /// Bound on the handshake and on waiting for `SessionStarted`, each.
    pub connect_timeout: Duration,
}

pub struct VolcanoClient {
    config: VolcanoConfig,
    session_id: String,
    framer: Framer,
    writer: Option<Writer>,
    reader: Option<JoinHandle<()>>,
    /// Set once FinishSession was sent; a close after it ends the session normally.
    finishing: Arc<AtomicBool>,
}

impl VolcanoClient {
    pub fn new(config: VolcanoConfig) -> Self {
        Self {
            config,
            session_id: String::new(),
            framer: Framer::new(),
            writer: None,
            reader: None,
            finishing: Arc::new(AtomicBool::new(false)),
        }
    }

    fn request(&self) -> WsRequest {
        WsRequest {
            url: self.config.endpoint.clone(),
            headers: vec![
                ("x-api-key", self.config.api_key.clone()),
                ("x-api-resource-id", Secret::new(RESOURCE_ID)),
                (
                    "x-api-connect-id",
                    Secret::new(uuid::Uuid::new_v4().to_string()),
                ),
            ],
        }
    }

    async fn open(&mut self, events: EventSink) -> Result<(), ProviderError> {
        self.close().await;
        let timeout = self.config.connect_timeout;
        let stream = connect_websocket(&self.request(), &self.config.proxy, timeout).await?;
        let (mut writer, mut reader) = stream.split();
        self.session_id = uuid::Uuid::new_v4().to_string();
        self.framer = Framer::new();
        let (source, target) =
            wire_pair(&self.config.source_language, &self.config.target_language);
        let start = start_session(&self.session_id, source, target);
        writer
            .send(Message::Binary(start.into()))
            .await
            .map_err(|_| ProviderError::Transport)?;
        let mut mapper = Mapper::new();
        tokio::time::timeout(timeout, await_started(&mut reader, &mut mapper))
            .await
            .unwrap_or(Err(ProviderError::Transport))?;
        events
            .emit(ProviderEvent::Ready)
            .map_err(|_| ProviderError::Transport)?;
        self.writer = Some(writer);
        self.finishing = Arc::new(AtomicBool::new(false));
        let finishing = Arc::clone(&self.finishing);
        self.reader = Some(tokio::spawn(read_loop(reader, mapper, events, finishing)));
        Ok(())
    }

    async fn send(&mut self, payload: Vec<u8>) -> Result<(), ProviderError> {
        let writer = self.writer.as_mut().ok_or(ProviderError::Transport)?;
        writer
            .send(Message::Binary(payload.into()))
            .await
            .map_err(|_| ProviderError::Transport)
    }

    async fn ping(&mut self) -> Result<(), ProviderError> {
        let writer = self.writer.as_mut().ok_or(ProviderError::Transport)?;
        writer
            .send(Message::Ping(Vec::new().into()))
            .await
            .map_err(|_| ProviderError::Transport)
    }

    async fn send_frames(&mut self, pcm: &[u8]) -> Result<(), ProviderError> {
        if self.writer.is_none() {
            return Err(ProviderError::Transport);
        }
        for frame in self.framer.push(pcm) {
            let payload = task_request(&self.session_id, &frame);
            self.send(payload).await?;
        }
        Ok(())
    }

    async fn end_input(&mut self) -> Result<(), ProviderError> {
        if let Some(frame) = self.framer.flush() {
            let payload = task_request(&self.session_id, &frame);
            self.send(payload).await?;
        }
        let finish = finish_session(&self.session_id);
        self.finishing.store(true, Ordering::Release);
        self.send(finish).await
    }

    /// Stops the reader first, so a closing connection reports nothing more.
    async fn close(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
        if let Some(mut writer) = self.writer.take() {
            let _ = tokio::time::timeout(CLOSE_TIMEOUT, writer.close()).await;
        }
    }
}

impl Drop for VolcanoClient {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
    }
}

/// Reads until `SessionStarted`. Captions cannot arrive earlier and are dropped.
async fn await_started(reader: &mut Reader, mapper: &mut Mapper) -> Result<(), ProviderError> {
    loop {
        let bytes = match reader.next().await {
            Some(Ok(Message::Binary(bytes))) => bytes,
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                return Err(ProviderError::Transport)
            }
            Some(Ok(_)) => continue,
        };
        let response = decode_response(&bytes).map_err(|_| ProviderError::Provider)?;
        for mapped in mapper.map(response) {
            match mapped {
                Mapped::Started => return Ok(()),
                Mapped::Failed(error) => return Err(error),
                Mapped::Ended => return Err(ProviderError::Transport),
                Mapped::Caption(_) => {}
            }
        }
    }
}

/// Forwards results until the session finishes, fails or the connection closes. A close
/// before FinishSession is a retryable transport failure; after it, the session ends with
/// what arrived (providers/README.md “意外断开”, “结束时不丢尾句”).
async fn read_loop(
    mut reader: Reader,
    mut mapper: Mapper,
    events: EventSink,
    finishing: Arc<AtomicBool>,
) {
    let last = loop {
        let bytes = match reader.next().await {
            Some(Ok(Message::Binary(bytes))) => bytes,
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                break if finishing.load(Ordering::Acquire) {
                    ProviderEvent::Ended
                } else {
                    ProviderEvent::Failed(ProviderError::Transport)
                };
            }
            Some(Ok(_)) => continue,
        };
        let Ok(response) = decode_response(&bytes) else {
            break ProviderEvent::Failed(ProviderError::Provider);
        };
        if let Some(last) = forward(mapper.map(response), &events) {
            break last;
        }
    };
    let _ = events.emit(last);
}

/// Emits captions; returns the terminal event, if this batch ends the session.
fn forward(batch: Vec<Mapped>, events: &EventSink) -> Option<ProviderEvent> {
    for mapped in batch {
        let event = match mapped {
            Mapped::Started => continue,
            Mapped::Caption(caption) => ProviderEvent::Caption(caption),
            Mapped::Ended => return Some(ProviderEvent::Ended),
            Mapped::Failed(error) => return Some(ProviderEvent::Failed(error)),
        };
        if events.emit(event).is_err() {
            return Some(ProviderEvent::Failed(ProviderError::Transport));
        }
    }
    None
}

impl SpeechClient for VolcanoClient {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.open(events))
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move { self.send_frames(&pcm).await })
    }

    fn heartbeat(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.ping())
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.end_input())
    }

    fn disconnect(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(self.close())
    }
}
