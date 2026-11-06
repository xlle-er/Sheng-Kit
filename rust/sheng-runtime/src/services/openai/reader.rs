//! Receive loop of one connection: frames → provider events, pongs → heartbeat, and the
//! bounded wait for the last results after input ends.

use std::time::Duration;

use futures_util::stream::SplitStream;
use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

use super::messages::decode;
use super::transcript::{Mapper, Step};
use crate::provider::{EventSink, ProviderError, ProviderEvent};
use crate::transport::WsStream;

/// Longest wait for the last transcripts after input ends (providers/openai.md step 5).
pub const FINISH_WAIT: Duration = Duration::from_secs(2);
/// Control messages waiting for the reader.
pub const CONTROL_QUEUE: usize = 16;

/// Client → reader notifications that change what the reader waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    CommitSent,
    CloseSent,
    Finish,
}

pub struct Reader {
    pub stream: SplitStream<WsStream>,
    pub events: EventSink,
    pub control: mpsc::Receiver<Control>,
    pub pongs: watch::Sender<u64>,
}

enum Flow {
    Continue,
    Stop,
}

impl Reader {
    /// Runs until the service fails, the connection ends or the finish wait is over.
    /// Dropping the event sink on return tells the lane nothing more will come.
    pub async fn run(mut self) {
        let mut mapper = Mapper::default();
        let mut deadline: Option<Instant> = None;
        loop {
            let flow = tokio::select! {
                biased;
                control = self.control.recv() => self.on_control(control, &mut mapper, &mut deadline),
                frame = self.stream.next() => self.on_frame(frame, &mut mapper, deadline.is_some()),
                _ = sleep_until(deadline), if deadline.is_some() => self.ended(),
            };
            if matches!(flow, Flow::Stop) {
                return;
            }
        }
    }

    fn on_control(
        &self,
        control: Option<Control>,
        mapper: &mut Mapper,
        deadline: &mut Option<Instant>,
    ) -> Flow {
        match control {
            Some(Control::CommitSent) => mapper.commit_sent(),
            Some(Control::CloseSent) => mapper.close_sent(),
            Some(Control::Finish) => {
                *deadline = Some(Instant::now() + FINISH_WAIT);
                if mapper.finish() == Step::Done {
                    return self.ended();
                }
            }
            None => return Flow::Stop,
        }
        Flow::Continue
    }

    fn on_frame(
        &self,
        frame: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
        mapper: &mut Mapper,
        finishing: bool,
    ) -> Flow {
        let text = match frame {
            Some(Ok(Message::Text(text))) => text,
            Some(Ok(Message::Pong(payload))) => {
                if let Ok(bytes) = <[u8; 8]>::try_from(payload.as_ref()) {
                    self.pongs.send_replace(u64::from_be_bytes(bytes));
                }
                return Flow::Continue;
            }
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) if finishing => return self.ended(),
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                return self.fail(ProviderError::Transport)
            }
            Some(Ok(_)) => return Flow::Continue,
        };
        let Some(event) = decode(text.as_str()) else {
            return Flow::Continue;
        };
        let mut out = Vec::new();
        let step = mapper.on_event(event, &mut out);
        for event in out {
            if self.events.emit(event).is_err() {
                return Flow::Stop;
            }
        }
        match step {
            Step::Continue => Flow::Continue,
            Step::Fail(error) => self.fail(error),
            Step::Done => self.ended(),
        }
    }

    fn ended(&self) -> Flow {
        let _ = self.events.emit(ProviderEvent::Ended);
        Flow::Stop
    }

    fn fail(&self, error: ProviderError) -> Flow {
        let _ = self.events.emit(ProviderEvent::Failed(error));
        Flow::Stop
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}
