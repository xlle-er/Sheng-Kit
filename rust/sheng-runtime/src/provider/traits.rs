//! Service client traits and the unified event stream.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sheng_core::align::Stream;
use sheng_core::CaptionEvent;
use tokio::sync::mpsc;

use super::error::{ProviderError, SessionError};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Provider events waiting for the session; a full queue fails the session.
pub const PROVIDER_EVENT_QUEUE: usize = 256;

/// Text of an unpaired transcript stream (captions.md §5), owned for sending across tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamText {
    Delta(String),
    Snapshot(String),
}

/// Everything a speech client reports, mapped onto sheng-core concepts.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    /// Session is ready for audio.
    Ready,
    /// Abstract caption event (captions.md §1).
    Caption(CaptionEvent),
    /// Text of one of the two id-less streams.
    StreamText {
        stream: Stream,
        text: StreamText,
    },
    /// Service cut point at the current end of `stream`.
    StreamBoundary(Stream),
    TurnComplete,
    /// Measured heartbeat round trip.
    ApiLatency(Duration),
    /// Service finished after `finish`.
    Ended,
    Failed(ProviderError),
}

/// Set once when the provider event queue overflows; never cleared. The session reads it
/// when the event channel closes to tell an overflow from a lost connection.
#[derive(Debug, Clone, Default)]
pub struct OverflowFlag(Arc<AtomicBool>);

impl OverflowFlag {
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn set(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Sender side of the bounded provider event queue.
#[derive(Debug, Clone)]
pub struct EventSink {
    sender: mpsc::Sender<ProviderEvent>,
    overflow: OverflowFlag,
}

impl EventSink {
    pub fn channel() -> (Self, mpsc::Receiver<ProviderEvent>) {
        let (sender, rx) = mpsc::channel(PROVIDER_EVENT_QUEUE);
        let sink = Self {
            sender,
            overflow: OverflowFlag::default(),
        };
        (sink, rx)
    }

    /// Read side of this queue's overflow flag; holding it does not keep the channel open.
    pub fn overflow_flag(&self) -> OverflowFlag {
        self.overflow.clone()
    }

    /// Non-blocking send; a full queue is reported as backlog overflow and sets the flag.
    pub fn emit(&self, event: ProviderEvent) -> Result<(), SessionError> {
        self.sender.try_send(event).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => {
                self.overflow.set();
                SessionError::ProviderEventBacklogOverflow
            }
            mpsc::error::TrySendError::Closed(_) => SessionError::TransportError,
        })
    }
}

/// A connected speech service. Implementations own their connection; events go to the sink
/// given at connect time.
pub trait SpeechClient: Send {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>>;
    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>>;
    fn heartbeat(&mut self) -> BoxFuture<'_, Result<(), ProviderError>>;
    /// Ends input; the client keeps reporting results until it emits `Ended`.
    fn finish(&mut self) -> BoxFuture<'_, Result<(), ProviderError>>;
    fn disconnect(&mut self) -> BoxFuture<'_, ()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationRequest {
    pub text: String,
    pub source_language: String,
    pub target_language: String,
}

/// Independent text translation. One request carries one sentence and no caption history.
pub trait TextTranslator: Send + Sync {
    fn translate(
        &self,
        request: TranslationRequest,
    ) -> BoxFuture<'_, Result<String, ProviderError>>;
}
