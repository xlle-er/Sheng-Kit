//! Gemini Live speech client: handshake, `setup`/`setupComplete`, audio, heartbeat, end.

use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

use super::config::GeminiConfig;
use super::errors::{classify_close, CloseOutcome};
use super::protocol::{
    audio_message, audio_stream_end_message, parse_server_message, setup_message, ServerEvent,
};
use super::reader::{Control, Reader};
use super::session::RouteState;
use crate::provider::{BoxFuture, EventSink, ProviderError, ProviderEvent, SpeechClient};
use crate::transport::{connect_websocket, WsStream, CONNECT_TIMEOUT};

/// Longest time `disconnect` waits for the close handshake.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

type Writer = SplitSink<WsStream, Message>;

pub struct GeminiClient {
    config: GeminiConfig,
    writer: Option<Writer>,
    control: Option<mpsc::Sender<Control>>,
    pong: Arc<Notify>,
    reader: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for GeminiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiClient")
            .field("config", &self.config)
            .field("connected", &self.writer.is_some())
            .finish()
    }
}

/// Waits for `setupComplete`; nothing else may be sent before it (Live API reference).
async fn await_setup(stream: &mut SplitStream<WsStream>) -> Result<(), ProviderError> {
    while let Some(message) = stream.next().await {
        let payload = match message.map_err(|_| ProviderError::Transport)? {
            Message::Text(text) => text.as_bytes().to_vec(),
            Message::Binary(bytes) => bytes.to_vec(),
            Message::Close(frame) => {
                return Err(match classify_close(frame.as_ref()) {
                    CloseOutcome::Normal => ProviderError::Transport,
                    CloseOutcome::Failed(error) => error,
                })
            }
            _ => continue,
        };
        for event in parse_server_message(&payload) {
            match event {
                ServerEvent::SetupComplete => return Ok(()),
                ServerEvent::Failed(error) => return Err(error),
                ServerEvent::GoAway => return Err(ProviderError::Transport),
                _ => {}
            }
        }
    }
    Err(ProviderError::Transport)
}

impl GeminiClient {
    pub fn new(config: GeminiConfig) -> Self {
        Self {
            config,
            writer: None,
            control: None,
            pong: Arc::new(Notify::new()),
            reader: None,
        }
    }

    async fn open(&self) -> Result<(Writer, SplitStream<WsStream>), ProviderError> {
        let request = self.config.ws_request();
        let ws = connect_websocket(&request, &self.config.proxy, CONNECT_TIMEOUT).await?;
        let (mut writer, mut stream) = ws.split();
        let setup = setup_message(&self.config.model, &self.config.kind);
        writer
            .send(Message::text(setup))
            .await
            .map_err(|_| ProviderError::Transport)?;
        await_setup(&mut stream).await?;
        Ok((writer, stream))
    }

    async fn connect_inner(&mut self, events: EventSink) -> Result<(), ProviderError> {
        let (writer, stream) = tokio::time::timeout(CONNECT_TIMEOUT, self.open())
            .await
            .map_err(|_| ProviderError::Transport)??;
        events
            .emit(ProviderEvent::Ready)
            .map_err(|_| ProviderError::Transport)?;
        let (control_tx, control_rx) = mpsc::channel(1);
        let reader = Reader {
            stream,
            state: RouteState::new(&self.config.kind),
            events,
            control: control_rx,
            pong: Arc::clone(&self.pong),
            finishing: false,
            finish_deadline: None,
        };
        self.reader = Some(tokio::spawn(reader.run()));
        self.writer = Some(writer);
        self.control = Some(control_tx);
        Ok(())
    }

    async fn send(&mut self, message: Message) -> Result<(), ProviderError> {
        let writer = self.writer.as_mut().ok_or(ProviderError::Transport)?;
        writer
            .send(message)
            .await
            .map_err(|_| ProviderError::Transport)
    }

    async fn heartbeat_inner(&mut self) -> Result<(), ProviderError> {
        let pong = Arc::clone(&self.pong);
        let notified = pong.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        self.send(Message::Ping(Vec::new().into())).await?;
        notified.await;
        Ok(())
    }

    async fn finish_inner(&mut self) -> Result<(), ProviderError> {
        self.send(Message::text(audio_stream_end_message())).await?;
        let control = self.control.as_ref().ok_or(ProviderError::Transport)?;
        control
            .send(Control::Finish)
            .await
            .map_err(|_| ProviderError::Transport)
    }

    async fn disconnect_inner(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
        self.control = None;
        if let Some(mut writer) = self.writer.take() {
            let _ = tokio::time::timeout(CLOSE_TIMEOUT, writer.close()).await;
        }
    }
}

impl SpeechClient for GeminiClient {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.connect_inner(events))
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move { self.send(Message::text(audio_message(&pcm))).await })
    }

    fn heartbeat(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.heartbeat_inner())
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.finish_inner())
    }

    fn disconnect(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(self.disconnect_inner())
    }
}

impl Drop for GeminiClient {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
    }
}
