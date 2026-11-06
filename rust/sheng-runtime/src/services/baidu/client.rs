//! WebSocket speech client for one lane: START handshake, audio frames, FINISH, and a
//! reader task that turns server messages into provider events.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use super::protocol::{
    classify_error, finish_frame, frame_bytes, is_recoverable, parse_message, start_frame,
    ServerMessage, StartParams, Utterances,
};
use crate::config::endpoint::ProxyChoice;
use crate::provider::{BoxFuture, EventSink, ProviderError, ProviderEvent, SpeechClient};
use crate::transport::{connect_websocket, WsRequest, WsStream, CONNECT_TIMEOUT};

/// Longest wait for the close handshake when disconnecting.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

type Writer = SplitSink<WsStream, Message>;
type Reader = SplitStream<WsStream>;

/// Where and how to connect. The endpoint is injectable so tests can use a local server.
#[derive(Clone)]
pub struct BaiduSession {
    pub endpoint: Url,
    pub proxy: ProxyChoice,
    pub start: StartParams,
}

/// State shared between the client and its reader task.
#[derive(Debug, Default)]
struct Shared {
    /// FINISH was sent; a close after this point ends the session normally.
    finishing: AtomicBool,
    /// Signalled on every pong.
    pong: Notify,
}

pub struct BaiduSpeech {
    session: BaiduSession,
    shared: Arc<Shared>,
    writer: Option<Writer>,
    reader: Option<JoinHandle<()>>,
}

impl BaiduSpeech {
    pub fn new(session: BaiduSession) -> Self {
        Self {
            session,
            shared: Arc::default(),
            writer: None,
            reader: None,
        }
    }

    async fn open(&mut self, events: EventSink) -> Result<(), ProviderError> {
        let request = WsRequest {
            url: self.session.endpoint.clone(),
            headers: Vec::new(),
        };
        let mut stream = connect_websocket(&request, &self.session.proxy, CONNECT_TIMEOUT).await?;
        let start = start_frame(&self.session.start);
        stream
            .send(Message::text(start))
            .await
            .map_err(|_| ProviderError::Transport)?;
        await_started(&mut stream).await?;
        events
            .emit(ProviderEvent::Ready)
            .map_err(|_| ProviderError::Transport)?;
        let (writer, reader) = stream.split();
        self.writer = Some(writer);
        self.reader = Some(tokio::spawn(read_loop(
            reader,
            events,
            Arc::clone(&self.shared),
        )));
        Ok(())
    }

    fn writer(&mut self) -> Result<&mut Writer, ProviderError> {
        self.writer.as_mut().ok_or(ProviderError::Transport)
    }
}

impl Drop for BaiduSpeech {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
    }
}

/// Waits for the START confirmation; a rejection is classified by its code.
async fn await_started(stream: &mut WsStream) -> Result<(), ProviderError> {
    while let Some(message) = stream.next().await {
        let Message::Text(text) = message.map_err(|_| ProviderError::Transport)? else {
            continue;
        };
        match parse_message(&text) {
            Some(ServerMessage::Started) => return Ok(()),
            Some(ServerMessage::Error(code)) => return Err(classify_error(code)),
            Some(_) => continue,
            None => return Err(ProviderError::Provider),
        }
    }
    Err(ProviderError::Transport)
}

/// What the reader does after one message.
enum Step {
    Continue,
    Stop,
}

async fn read_loop(mut reader: Reader, events: EventSink, shared: Arc<Shared>) {
    let mut utterances = Utterances::default();
    while let Some(message) = reader.next().await {
        let step = match message {
            Ok(Message::Text(text)) => on_text(&text, &events, &mut utterances),
            Ok(Message::Pong(_)) => {
                shared.pong.notify_waiters();
                Step::Continue
            }
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => Step::Continue,
        };
        if matches!(step, Step::Stop) {
            return;
        }
    }
    // Closed without END: normal only once FINISH was sent (providers/baidu.md).
    let last = if shared.finishing.load(Ordering::Acquire) {
        ProviderEvent::Ended
    } else {
        ProviderEvent::Failed(ProviderError::Transport)
    };
    let _ = events.emit(last);
}

/// Handles one text message. A full event queue stops the reader; dropping the sink then
/// ends the lane's event stream.
fn on_text(text: &str, events: &EventSink, utterances: &mut Utterances) -> Step {
    let outgoing = match parse_message(text) {
        Some(ServerMessage::Result(segment)) => utterances
            .map(segment)
            .into_iter()
            .map(ProviderEvent::Caption)
            .collect(),
        Some(ServerMessage::Ended) => vec![ProviderEvent::Ended],
        Some(ServerMessage::Error(code)) if is_recoverable(code) => Vec::new(),
        Some(ServerMessage::Error(code)) => vec![ProviderEvent::Failed(classify_error(code))],
        Some(ServerMessage::Started | ServerMessage::Other) => Vec::new(),
        None => vec![ProviderEvent::Failed(ProviderError::Provider)],
    };
    for event in outgoing {
        let terminal = matches!(event, ProviderEvent::Ended | ProviderEvent::Failed(_));
        if events.emit(event).is_err() || terminal {
            return Step::Stop;
        }
    }
    Step::Continue
}

impl SpeechClient for BaiduSpeech {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.open(events))
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            let size = frame_bytes(self.session.start.sampling_rate);
            let writer = self.writer()?;
            for frame in pcm.chunks(size) {
                writer
                    .feed(Message::binary(frame.to_vec()))
                    .await
                    .map_err(|_| ProviderError::Transport)?;
            }
            writer.flush().await.map_err(|_| ProviderError::Transport)
        })
    }

    /// WebSocket ping; completes when the matching pong arrives (the caller bounds the wait).
    fn heartbeat(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            let shared = Arc::clone(&self.shared);
            let pong = shared.pong.notified();
            tokio::pin!(pong);
            pong.as_mut().enable();
            self.writer()?
                .send(Message::Ping(Vec::new().into()))
                .await
                .map_err(|_| ProviderError::Transport)?;
            pong.await;
            Ok(())
        })
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            self.shared.finishing.store(true, Ordering::Release);
            self.writer()?
                .send(Message::text(finish_frame()))
                .await
                .map_err(|_| ProviderError::Transport)
        })
    }

    fn disconnect(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            if let Some(reader) = self.reader.take() {
                reader.abort();
            }
            if let Some(mut writer) = self.writer.take() {
                let _ = tokio::time::timeout(CLOSE_TIMEOUT, writer.close()).await;
            }
        })
    }
}
