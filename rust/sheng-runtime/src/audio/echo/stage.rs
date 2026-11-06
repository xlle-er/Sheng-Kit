//! Echo processing on its own thread behind a bounded queue (audio.md §3).

use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use super::processor::EchoProcessor;
use super::stats::{EchoReport, EchoStats};
use crate::provider::SessionError;

/// Messages waiting for the echo thread.
pub const ECHO_QUEUE_MESSAGES: usize = 128;
/// Longer pushes are split so one message never carries more than this much audio.
pub const MAX_MESSAGE_DURATION: Duration = Duration::from_millis(250);
/// Stop must finish processing the received microphone audio within this time.
pub const ECHO_STOP_DEADLINE: Duration = Duration::from_secs(1);

/// Receives processed microphone audio; an error ends echo processing.
pub type MicOutput = Box<dyn FnMut(Vec<i16>) -> Result<(), SessionError> + Send>;

enum Message {
    Reference(Vec<i16>, Duration),
    Mic(Vec<i16>, Duration),
}

#[derive(Default)]
struct Shared {
    failure: Mutex<Option<SessionError>>,
    stats: Mutex<EchoStats>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
    /// Keeps only the first failure so it is reported once.
    fn fail(&self, error: SessionError) {
        lock(&self.failure).get_or_insert(error);
    }
}

pub struct EchoStage {
    sender: Mutex<Option<SyncSender<Message>>>,
    done: Mutex<Option<Receiver<()>>>,
    shared: Arc<Shared>,
    rate: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EchoPushError {
    /// Stage stopped; late captures cannot reopen it.
    Closed,
    /// Queue full; the session fails with `echo_processing_failed`.
    Overflow,
}

impl EchoStage {
    /// Starts the echo thread. `None` means the canceller could not be initialized.
    pub fn spawn(rate: u32, mut output: MicOutput) -> Option<Self> {
        let mut processor = EchoProcessor::new(rate)?;
        let (sender, receiver) = sync_channel::<Message>(ECHO_QUEUE_MESSAGES);
        let (done_tx, done_rx) = sync_channel(1);
        let shared = Arc::new(Shared::default());
        let worker = Arc::clone(&shared);
        thread::Builder::new()
            .name("echo".into())
            .spawn(move || {
                let mut emit = |samples: Vec<i16>| {
                    if samples.is_empty() {
                        return true;
                    }
                    output(samples).map_err(|error| worker.fail(error)).is_ok()
                };
                let mut healthy = true;
                while let Ok(message) = receiver.recv() {
                    if healthy {
                        healthy = match message {
                            Message::Reference(samples, at) => {
                                processor.push_reference(&samples, at);
                                true
                            }
                            Message::Mic(samples, at) => emit(processor.push_mic(&samples, at)),
                        };
                    }
                    *lock(&worker.stats) = processor.stats().clone();
                }
                if healthy {
                    emit(processor.finish());
                }
                *lock(&worker.stats) = processor.stats().clone();
                let _ = done_tx.send(());
            })
            .ok()?;
        Some(Self {
            sender: Mutex::new(Some(sender)),
            done: Mutex::new(Some(done_rx)),
            shared,
            rate,
        })
    }

    fn piece_len(&self) -> usize {
        (self.rate as usize * MAX_MESSAGE_DURATION.as_millis() as usize / 1000).max(1)
    }

    fn send(&self, samples: &[i16], at: Duration, mic: bool) -> Result<(), EchoPushError> {
        let guard = lock(&self.sender);
        let sender = guard.as_ref().ok_or(EchoPushError::Closed)?;
        let piece = self.piece_len();
        for (index, chunk) in samples.chunks(piece).enumerate() {
            let offset = MAX_MESSAGE_DURATION * u32::try_from(index).unwrap_or(u32::MAX);
            let message = if mic {
                Message::Mic(chunk.to_vec(), at + offset)
            } else {
                Message::Reference(chunk.to_vec(), at + offset)
            };
            match sender.try_send(message) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    self.shared.fail(SessionError::EchoProcessingFailed);
                    return Err(EchoPushError::Overflow);
                }
                Err(TrySendError::Disconnected(_)) => return Err(EchoPushError::Closed),
            }
        }
        Ok(())
    }

    /// Copies system sound as reference; never waits for any service.
    pub fn push_reference(&self, samples: &[i16], at: Duration) -> Result<(), EchoPushError> {
        self.send(samples, at, false)
    }

    pub fn push_mic(&self, samples: &[i16], at: Duration) -> Result<(), EchoPushError> {
        self.send(samples, at, true)
    }

    /// Stops accepting audio and waits for the received microphone audio to be processed.
    pub fn stop(&self) -> Result<(), SessionError> {
        lock(&self.sender).take();
        if let Some(done) = lock(&self.done).take() {
            if done.recv_timeout(ECHO_STOP_DEADLINE).is_err() {
                self.shared.fail(SessionError::EchoProcessingFailed);
            }
        }
        self.failure().map_or(Ok(()), Err)
    }

    pub fn failure(&self) -> Option<SessionError> {
        *lock(&self.shared.failure)
    }

    pub fn report(&self) -> EchoReport {
        lock(&self.shared.stats).report()
    }
}

impl Drop for EchoStage {
    fn drop(&mut self) {
        lock(&self.sender).take();
    }
}
