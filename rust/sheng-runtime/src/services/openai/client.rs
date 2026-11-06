//! The speech client: handshake (`session.created` → `session.update` → `session.updated`),
//! audio, WebSocket ping heartbeat and the end of input.

use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

use super::config::{RealtimeConfig, SessionKind};
use super::errors::classify;
use super::messages::{self, decode, ServerEvent};
use super::reader::{Control, Reader, CONTROL_QUEUE};
use super::turns::{TurnAction, TurnDetector};
use crate::provider::{BoxFuture, EventSink, ProviderError, ProviderEvent, SpeechClient};
use crate::transport::{connect_websocket, WsStream, CONNECT_TIMEOUT};

/// Longest wait for each session event of the handshake.
pub const SESSION_EVENT_WAIT: Duration = Duration::from_secs(10);
/// Closing the socket is best effort and never holds up the session.
const CLOSE_WAIT: Duration = Duration::from_millis(500);

type Writer = SplitSink<WsStream, Message>;

struct Connection {
    writer: Writer,
    control: mpsc::Sender<Control>,
    pongs: watch::Receiver<u64>,
    reader: JoinHandle<()>,
}

pub struct RealtimeClient {
    config: RealtimeConfig,
    connection: Option<Connection>,
    turns: TurnDetector,
    next_ping: u64,
}

impl RealtimeClient {
    pub fn new(config: RealtimeConfig) -> Self {
        Self {
            config,
            connection: None,
            turns: TurnDetector::default(),
            next_ping: 0,
        }
    }

    fn transcribes(&self) -> bool {
        matches!(self.config.kind, SessionKind::Transcribe { .. })
    }

    /// Error for a peer that does not speak the expected protocol.
    fn mismatch(&self) -> ProviderError {
        if self.config.custom_endpoint {
            ProviderError::Configuration
        } else {
            ProviderError::Provider
        }
    }

    async fn wait_for(
        &self,
        stream: &mut SplitStream<WsStream>,
        expected: &ServerEvent,
    ) -> Result<(), ProviderError> {
        let waiting = async {
            loop {
                let text = match stream.next().await {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                        return Err(ProviderError::Transport)
                    }
                    Some(Ok(_)) => continue,
                };
                match decode(text.as_str()) {
                    None => return Err(self.mismatch()),
                    Some(ServerEvent::Error(info)) => return Err(classify(&info)),
                    Some(event) if &event == expected => return Ok(()),
                    Some(_) => {}
                }
            }
        };
        tokio::time::timeout(SESSION_EVENT_WAIT, waiting)
            .await
            .unwrap_or_else(|_| Err(self.mismatch()))
    }

    async fn open(&mut self, events: EventSink) -> Result<(), ProviderError> {
        let socket =
            connect_websocket(&self.config.request, &self.config.proxy, CONNECT_TIMEOUT).await?;
        let (mut writer, mut stream) = socket.split();
        self.wait_for(&mut stream, &ServerEvent::SessionCreated)
            .await?;
        send(&mut writer, messages::session_update(&self.config.kind)).await?;
        self.wait_for(&mut stream, &ServerEvent::SessionUpdated)
            .await?;
        events
            .emit(ProviderEvent::Ready)
            .map_err(|_| ProviderError::Transport)?;
        let (control, control_rx) = mpsc::channel(CONTROL_QUEUE);
        let (pong_tx, pongs) = watch::channel(0);
        let reader = Reader {
            stream,
            events,
            control: control_rx,
            pongs: pong_tx,
        };
        self.connection = Some(Connection {
            writer,
            control,
            pongs,
            reader: tokio::spawn(reader.run()),
        });
        Ok(())
    }

    fn connection(&mut self) -> Result<&mut Connection, ProviderError> {
        self.connection.as_mut().ok_or(ProviderError::Transport)
    }

    async fn commit(&mut self) -> Result<(), ProviderError> {
        let connection = self.connection()?;
        notify(&connection.control, Control::CommitSent).await?;
        send(&mut connection.writer, messages::commit()).await
    }

    async fn append(&mut self, pcm: Vec<u8>) -> Result<(), ProviderError> {
        let frame = messages::append_audio(&self.config.kind, &pcm);
        send(&mut self.connection()?.writer, frame).await?;
        if !self.transcribes() {
            return Ok(());
        }
        match self.turns.push(&pcm) {
            TurnAction::Keep => Ok(()),
            TurnAction::Commit => self.commit().await,
            TurnAction::Clear => send(&mut self.connection()?.writer, messages::clear()).await,
        }
    }

    async fn ping(&mut self) -> Result<(), ProviderError> {
        self.next_ping += 1;
        let nonce = self.next_ping;
        let connection = self.connection()?;
        let payload = nonce.to_be_bytes().to_vec();
        connection
            .writer
            .send(Message::Ping(payload.into()))
            .await
            .map_err(|_| ProviderError::Transport)?;
        connection
            .pongs
            .wait_for(|seen| *seen >= nonce)
            .await
            .map(|_| ())
            .map_err(|_| ProviderError::Transport)
    }

    async fn end_input(&mut self) -> Result<(), ProviderError> {
        if self.transcribes() {
            if self.turns.take_final() {
                self.commit().await?;
            }
        } else {
            let connection = self.connection()?;
            notify(&connection.control, Control::CloseSent).await?;
            send(&mut connection.writer, messages::session_close()).await?;
        }
        notify(&self.connection()?.control, Control::Finish).await
    }

    async fn close(&mut self) {
        let Some(mut connection) = self.connection.take() else {
            return;
        };
        connection.reader.abort();
        let _ = tokio::time::timeout(CLOSE_WAIT, connection.writer.close()).await;
    }
}

async fn send(writer: &mut Writer, text: String) -> Result<(), ProviderError> {
    writer
        .send(Message::text(text))
        .await
        .map_err(|_| ProviderError::Transport)
}

async fn notify(control: &mpsc::Sender<Control>, message: Control) -> Result<(), ProviderError> {
    control
        .send(message)
        .await
        .map_err(|_| ProviderError::Transport)
}

impl SpeechClient for RealtimeClient {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.open(events))
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.append(pcm))
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

impl Drop for RealtimeClient {
    fn drop(&mut self) {
        if let Some(connection) = &self.connection {
            connection.reader.abort();
        }
    }
}
