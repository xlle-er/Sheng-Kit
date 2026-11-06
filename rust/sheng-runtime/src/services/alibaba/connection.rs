//! WebSocket plumbing shared by the inference (recognition) and realtime (live translation)
//! clients: handshake helpers, a writer half kept by the client and a reader task that turns
//! server messages into provider events.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

use super::diagnostics::FailureNote;
use crate::config::endpoint::ProxyChoice;
use crate::provider::{EventSink, ProviderError, ProviderEvent};
use crate::transport::{connect_websocket, WsRequest, WsStream, CONNECT_TIMEOUT};

pub type Writer = SplitSink<WsStream, Message>;
pub type Reader = SplitStream<WsStream>;

/// Opens the connection and splits it into writer and reader halves.
pub async fn open(
    request: &WsRequest,
    proxy: &ProxyChoice,
) -> Result<(Writer, Reader), ProviderError> {
    let stream = connect_websocket(request, proxy, CONNECT_TIMEOUT).await?;
    Ok(stream.split())
}

pub async fn send_json(
    writer: &mut Writer,
    value: &serde_json::Value,
) -> Result<(), ProviderError> {
    send(writer, Message::Text(value.to_string().into())).await
}

pub async fn send(writer: &mut Writer, message: Message) -> Result<(), ProviderError> {
    writer
        .send(message)
        .await
        .map_err(|_| ProviderError::Transport)
}

/// Next text message during the handshake phase; the connection closing is a transport error.
/// Close frames and error events are noted in `note`.
pub async fn next_text(reader: &mut Reader, note: &FailureNote) -> Result<String, ProviderError> {
    loop {
        match reader.next().await {
            Some(Ok(Message::Text(text))) => {
                note.message(text.as_str());
                return Ok(text.as_str().to_owned());
            }
            Some(Ok(Message::Close(frame))) => {
                note.closed(frame.as_ref());
                return Err(ProviderError::Transport);
            }
            Some(Err(_)) | None => return Err(ProviderError::Transport),
            Some(Ok(_)) => {}
        }
    }
}

/// What one server message produced.
#[derive(Debug, Default, PartialEq)]
pub struct Step {
    pub events: Vec<ProviderEvent>,
    /// The service ended the stream (finished or failed); nothing more is read.
    pub done: bool,
}

impl Step {
    pub fn events(events: Vec<ProviderEvent>) -> Self {
        Self {
            events,
            done: false,
        }
    }

    pub fn last(event: ProviderEvent) -> Self {
        Self {
            events: vec![event],
            done: true,
        }
    }
}

/// Protocol state that interprets server messages after the handshake.
pub trait Inbound: Send + 'static {
    fn on_text(&mut self, text: &str) -> Step;

    /// The stream ends without the service confirming everything: the text in progress,
    /// handed over once as finals (providers/README.md “意外断开”).
    fn interrupted(&mut self) -> Vec<ProviderEvent> {
        Vec::new()
    }
}

/// The event that ends a stream the service did not end by itself after `finish`: before
/// `finish` any ending is a retryable transport failure.
fn unexpected_end(finishing: &AtomicBool) -> ProviderEvent {
    if finishing.load(Ordering::Acquire) {
        ProviderEvent::Ended
    } else {
        ProviderEvent::Failed(ProviderError::Transport)
    }
}

/// Emits `events`; false once the queue no longer takes them.
fn emit_all(sink: &EventSink, events: Vec<ProviderEvent>) -> bool {
    events.into_iter().all(|event| sink.emit(event).is_ok())
}

/// Reads until the service ends the stream. Before `finish`, a closed connection or the
/// service's own end message is a transport failure; after it, a closed connection ends the
/// session. Either way the text in progress is handed over first. A full event queue stops
/// reading; dropping the sink tells the session. Close frames and error events are noted.
pub fn spawn_reader<H: Inbound>(
    mut reader: Reader,
    mut handler: H,
    sink: EventSink,
    note: FailureNote,
    finishing: Arc<AtomicBool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(message) = reader.next().await {
            let text = match message {
                Ok(Message::Text(text)) => text,
                Ok(Message::Close(frame)) => {
                    note.closed(frame.as_ref());
                    break;
                }
                Err(_) => break,
                Ok(_) => continue,
            };
            note.message(text.as_str());
            let step = handler.on_text(text.as_str());
            for event in step.events {
                let event = match event {
                    ProviderEvent::Ended => unexpected_end(&finishing),
                    other => other,
                };
                let ends_retryably = event == ProviderEvent::Failed(ProviderError::Transport)
                    || event == ProviderEvent::Ended;
                if ends_retryably && !emit_all(&sink, handler.interrupted()) {
                    return;
                }
                if sink.emit(event).is_err() {
                    return;
                }
            }
            if step.done {
                return;
            }
        }
        if emit_all(&sink, handler.interrupted()) {
            let _ = sink.emit(unexpected_end(&finishing));
        }
    })
}

/// Writer and reader task of one live connection.
#[derive(Debug, Default)]
pub struct Link {
    writer: Option<Writer>,
    reader: Option<JoinHandle<()>>,
    finishing: Arc<AtomicBool>,
}

impl Link {
    /// Starts the reader task for `handler` on a connection that completed its handshake.
    pub fn spawn<H: Inbound>(
        writer: Writer,
        reader: Reader,
        handler: H,
        sink: EventSink,
        note: &FailureNote,
    ) -> Self {
        let finishing = Arc::new(AtomicBool::new(false));
        let task = spawn_reader(reader, handler, sink, note.clone(), Arc::clone(&finishing));
        Self {
            writer: Some(writer),
            reader: Some(task),
            finishing,
        }
    }

    /// Input is ending: from now on a closing connection ends the session normally.
    pub fn mark_finishing(&self) {
        self.finishing.store(true, Ordering::Release);
    }

    pub fn writer(&mut self) -> Result<&mut Writer, ProviderError> {
        self.writer.as_mut().ok_or(ProviderError::Transport)
    }

    /// Stops the reader first so no late event of this connection reaches the session.
    pub async fn close(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
        if let Some(mut writer) = self.writer.take() {
            let _ = writer.send(Message::Close(None)).await;
            let _ = writer.close().await;
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
    }
}
