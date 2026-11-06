//! A speech client over one WebSocket, driven by a route `Protocol`.
//!
//! `connect` performs the handshake and setup and returns once the service is ready. A
//! reader task then turns service messages into events. After `finish` the reader waits
//! for the end of the last turn, at most the protocol's finish wait, and emits `Ended`.
//! `disconnect` stops the reader, so no event of this connection arrives afterwards.

use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{oneshot, Notify};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

use super::protocol::{Inbound, Protocol};
use crate::config::endpoint::ProxyChoice;
use crate::provider::{BoxFuture, EventSink, ProviderError, ProviderEvent, SpeechClient};
use crate::transport::{connect_websocket, WsRequest, WsStream, CONNECT_TIMEOUT};

/// A heartbeat pong is awaited at most this long.
const PONG_WAIT: Duration = Duration::from_secs(5);
/// Closing the socket on disconnect is given at most this long.
const CLOSE_WAIT: Duration = Duration::from_millis(500);

type Writer = SplitSink<WsStream, Message>;
type Reader = SplitStream<WsStream>;

/// Everything needed to open the connection.
pub struct Target {
    pub request: WsRequest,
    pub proxy: ProxyChoice,
}

pub struct WsSpeechClient<P: Protocol> {
    target: Target,
    protocol: Option<P>,
    finish_wait: Duration,
    finish_message: Option<String>,
    writer: Option<Writer>,
    reader: Option<JoinHandle<()>>,
    finish_signal: Option<oneshot::Sender<()>>,
    pong: Arc<Notify>,
}

impl<P: Protocol> WsSpeechClient<P> {
    pub fn new(target: Target, protocol: P) -> Self {
        Self {
            target,
            finish_wait: protocol.finish_wait(),
            finish_message: protocol.finish(),
            protocol: Some(protocol),
            writer: None,
            reader: None,
            finish_signal: None,
            pong: Arc::new(Notify::new()),
        }
    }

    async fn open(&mut self, sink: EventSink) -> Result<(), ProviderError> {
        let mut protocol = self.protocol.take().ok_or(ProviderError::Transport)?;
        let socket =
            connect_websocket(&self.target.request, &self.target.proxy, CONNECT_TIMEOUT).await?;
        let (mut writer, mut reader) = socket.split();
        for message in protocol.setup() {
            send(&mut writer, Message::Text(message.into())).await?;
        }
        await_ready(&mut reader, &mut protocol, &sink).await?;
        emit(&sink, ProviderEvent::Ready)?;
        let (finish_tx, finish_rx) = oneshot::channel();
        let task = ReaderTask {
            reader,
            protocol,
            sink,
            pong: self.pong.clone(),
            finish_wait: self.finish_wait,
        };
        self.reader = Some(tokio::spawn(task.run(finish_rx)));
        self.finish_signal = Some(finish_tx);
        self.writer = Some(writer);
        Ok(())
    }

    fn writer(&mut self) -> Result<&mut Writer, ProviderError> {
        self.writer.as_mut().ok_or(ProviderError::Transport)
    }
}

async fn send(writer: &mut Writer, message: Message) -> Result<(), ProviderError> {
    writer
        .send(message)
        .await
        .map_err(|_| ProviderError::Transport)
}

fn emit(sink: &EventSink, event: ProviderEvent) -> Result<(), ProviderError> {
    sink.emit(event).map_err(|_| ProviderError::Transport)
}

fn emit_all(sink: &EventSink, events: Vec<ProviderEvent>) -> Result<(), ProviderError> {
    events.into_iter().try_for_each(|event| emit(sink, event))
}

/// Reads setup replies until the protocol reports ready.
async fn await_ready<P: Protocol>(
    reader: &mut Reader,
    protocol: &mut P,
    sink: &EventSink,
) -> Result<(), ProviderError> {
    loop {
        let Some(Ok(message)) = reader.next().await else {
            return Err(ProviderError::Transport);
        };
        let Message::Text(text) = message else {
            if message.is_close() {
                return Err(ProviderError::Transport);
            }
            continue;
        };
        let inbound = protocol.on_text(text.as_str());
        if let Some(error) = inbound.failure {
            return Err(error);
        }
        emit_all(sink, inbound.events)?;
        if inbound.ready {
            return Ok(());
        }
    }
}

struct ReaderTask<P: Protocol> {
    reader: Reader,
    protocol: P,
    sink: EventSink,
    pong: Arc<Notify>,
    finish_wait: Duration,
}

/// What the reader does after one step.
enum Step {
    Continue,
    End,
    Fail(ProviderError),
}

impl<P: Protocol> ReaderTask<P> {
    async fn run(mut self, mut finish: oneshot::Receiver<()>) {
        let mut deadline: Option<Instant> = None;
        let outcome = loop {
            let step = tokio::select! {
                message = self.reader.next() => self.on_message(message, deadline.is_some()),
                signal = &mut finish, if deadline.is_none() => {
                    deadline = Some(Instant::now() + self.finish_wait);
                    // A dropped sender means the client went away without finishing.
                    if signal.is_err() { Step::Fail(ProviderError::Transport) } else { Step::Continue }
                }
                _ = sleep_until(deadline), if deadline.is_some() => Step::End,
            };
            match step {
                Step::Continue => continue,
                other => break other,
            }
        };
        let last = match outcome {
            Step::Fail(error) => vec![ProviderEvent::Failed(error)],
            _ => {
                let mut events = self.protocol.flush();
                events.push(ProviderEvent::Ended);
                events
            }
        };
        let _ = emit_all(&self.sink, last);
    }

    fn on_message(
        &mut self,
        message: Option<Result<Message, impl std::error::Error>>,
        finishing: bool,
    ) -> Step {
        let closed = if finishing {
            Step::End
        } else {
            Step::Fail(ProviderError::Transport)
        };
        let message = match message {
            Some(Ok(message)) => message,
            _ => return closed,
        };
        match message {
            Message::Text(text) => {
                let inbound = self.protocol.on_text(text.as_str());
                self.on_inbound(inbound, finishing)
            }
            Message::Pong(_) => {
                self.pong.notify_one();
                Step::Continue
            }
            Message::Close(_) => closed,
            _ => Step::Continue,
        }
    }

    fn on_inbound(&mut self, inbound: Inbound, finishing: bool) -> Step {
        if let Some(error) = inbound.failure {
            return Step::Fail(error);
        }
        if emit_all(&self.sink, inbound.events).is_err() {
            return Step::Fail(ProviderError::Transport);
        }
        if inbound.done || (inbound.turn_end && finishing) {
            return Step::End;
        }
        Step::Continue
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

impl<P: Protocol> SpeechClient for WsSpeechClient<P> {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.open(events))
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move { send(self.writer()?, P::audio(&pcm)).await })
    }

    fn heartbeat(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            send(self.writer()?, Message::Ping(Vec::new().into())).await?;
            let pong = self.pong.clone();
            tokio::time::timeout(PONG_WAIT, pong.notified())
                .await
                .map_err(|_| ProviderError::Transport)
        })
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            if let Some(message) = self.finish_message.clone() {
                send(self.writer()?, Message::Text(message.into())).await?;
            }
            let signal = self.finish_signal.take().ok_or(ProviderError::Transport)?;
            signal.send(()).map_err(|_| ProviderError::Transport)
        })
    }

    fn disconnect(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            if let Some(reader) = self.reader.take() {
                reader.abort();
            }
            self.finish_signal = None;
            if let Some(mut writer) = self.writer.take() {
                let _ = tokio::time::timeout(CLOSE_WAIT, writer.close()).await;
            }
        })
    }
}
