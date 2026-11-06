//! Reader task: turns server messages into provider events until the connection ends.

use std::sync::Arc;

use futures_util::stream::SplitStream;
use futures_util::StreamExt;
use std::time::Duration;
use tokio::sync::{mpsc, Notify};

use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

use super::errors::{classify_close, CloseOutcome};
use super::protocol::{parse_server_message, ServerEvent};
use super::session::RouteState;
use crate::provider::{EventSink, ProviderError, ProviderEvent};
use crate::transport::WsStream;

/// After `audioStreamEnd`, late transcripts are awaited at most this long.
pub const FINISH_WAIT: Duration = Duration::from_millis(4500);

/// Requests from the client to its reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Input has ended; report `Ended` once the last results are in.
    Finish,
}

/// Whether the reader keeps going.
enum Flow {
    Continue,
    Stop,
}

pub struct Reader {
    pub stream: SplitStream<WsStream>,
    pub state: RouteState,
    pub events: EventSink,
    pub control: mpsc::Receiver<Control>,
    pub pong: Arc<Notify>,
    pub finishing: bool,
    /// Set once input has ended: the reader ends with what it has at this time.
    pub finish_deadline: Option<Instant>,
}

async fn sleep_until_some(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

impl Reader {
    pub async fn run(mut self) {
        loop {
            // While finishing, late text joins the last sentence instead of a quiet cut.
            let quiet = self.finish_deadline.or_else(|| self.state.quiet_deadline());
            let flow = tokio::select! {
                message = self.stream.next() => self.on_message(message),
                control = self.control.recv() => self.on_control(control),
                () = sleep_until_some(quiet) => self.on_quiet(),
            };
            if matches!(flow, Flow::Stop) {
                return;
            }
        }
    }

    /// Emits events; a full or closed queue stops the reader (the session sees the
    /// queue end and fails).
    fn emit(&self, events: Vec<ProviderEvent>) -> Flow {
        for event in events {
            if self.events.emit(event).is_err() {
                return Flow::Stop;
            }
        }
        Flow::Continue
    }

    /// A retryable failure first hands over the accumulated transcript as a final
    /// (providers/README.md “意外断开”).
    fn fail(&mut self, error: ProviderError) -> Flow {
        let mut events = if error == ProviderError::Transport {
            self.state.flush()
        } else {
            Vec::new()
        };
        events.push(ProviderEvent::Failed(error));
        let _ = self.emit(events);
        Flow::Stop
    }

    fn end(&mut self) -> Flow {
        let mut events = self.state.flush();
        events.push(ProviderEvent::Ended);
        let _ = self.emit(events);
        Flow::Stop
    }

    fn on_control(&mut self, control: Option<Control>) -> Flow {
        match control {
            Some(Control::Finish) => {
                self.finishing = true;
                self.finish_deadline = Some(Instant::now() + FINISH_WAIT);
                if self.state.is_idle() {
                    return self.end();
                }
                Flow::Continue
            }
            // The client is gone; nobody reads the events any more.
            None => Flow::Stop,
        }
    }

    fn on_quiet(&mut self) -> Flow {
        if self.finishing {
            return self.end();
        }
        let events = self.state.flush();
        self.emit(events)
    }

    fn on_message(
        &mut self,
        message: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
    ) -> Flow {
        let message = match message {
            Some(Ok(message)) => message,
            // Dropped connection: after input ended keep what arrived, otherwise fail.
            Some(Err(_)) | None if self.finishing => return self.end(),
            Some(Err(_)) | None => return self.fail(ProviderError::Transport),
        };
        match message {
            Message::Text(text) => self.on_payload(text.as_bytes()),
            Message::Binary(bytes) => self.on_payload(&bytes),
            Message::Pong(_) => {
                self.pong.notify_waiters();
                Flow::Continue
            }
            Message::Close(frame) => match classify_close(frame.as_ref()) {
                CloseOutcome::Normal if self.finishing => self.end(),
                CloseOutcome::Normal => self.fail(ProviderError::Transport),
                CloseOutcome::Failed(error) => self.fail(error),
            },
            Message::Ping(_) | Message::Frame(_) => Flow::Continue,
        }
    }

    fn on_payload(&mut self, payload: &[u8]) -> Flow {
        let now = Instant::now();
        for event in parse_server_message(payload) {
            match event {
                // goAway: the service is about to drop the session (providers/gemini.md §5).
                ServerEvent::GoAway => return self.fail(ProviderError::Transport),
                ServerEvent::Failed(error) => return self.fail(error),
                ServerEvent::SetupComplete => continue,
                _ => {}
            }
            let events = self.state.apply(&event, now);
            if matches!(self.emit(events), Flow::Stop) {
                return Flow::Stop;
            }
            if self.finishing && self.state.finish_ready(&event) {
                return self.end();
            }
        }
        Flow::Continue
    }
}
