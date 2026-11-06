//! Per-source bounded send queue (audio.md §2). Pushing never waits; a full queue is
//! backpressure and fails the session.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::provider::{SessionError, SpeechClient};

/// Chunks waiting per source.
pub const LANE_CAPACITY: usize = 20;
/// At most this much audio may wait, including before the connection is up.
pub const MAX_BUFFERED_AUDIO: Duration = Duration::from_secs(2);
/// One chunk taking longer than this to send is a transport error.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(5);
/// On stop, queued audio must be sent within this time.
pub const STOP_DRAIN: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushError {
    Closed,
    Backpressure,
}

/// Producer side; shared by the push path and the echo thread.
#[derive(Debug)]
pub struct AudioLane {
    sender: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
    pending: Arc<AtomicUsize>,
    byte_limit: usize,
}

/// Consumer side; hands chunks to the speech client in order.
#[derive(Debug)]
pub struct LaneReceiver {
    receiver: mpsc::Receiver<Vec<u8>>,
    pending: Arc<AtomicUsize>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Creates a lane for PCM16 mono audio at `sample_rate`.
pub fn audio_lane(sample_rate: u32) -> (AudioLane, LaneReceiver) {
    let (sender, receiver) = mpsc::channel(LANE_CAPACITY);
    let pending = Arc::new(AtomicUsize::new(0));
    let bytes_per_second = sample_rate as usize * 2;
    let lane = AudioLane {
        sender: Mutex::new(Some(sender)),
        pending: Arc::clone(&pending),
        // One maximum-size push may exceed two seconds at 16 kHz; it must still fit.
        byte_limit: (bytes_per_second * MAX_BUFFERED_AUDIO.as_secs() as usize)
            .max(crate::audio::router::MAX_PUSH_BYTES),
    };
    (lane, LaneReceiver { receiver, pending })
}

impl AudioLane {
    pub fn push(&self, pcm: Vec<u8>) -> Result<(), PushError> {
        let guard = lock(&self.sender);
        let sender = guard.as_ref().ok_or(PushError::Closed)?;
        let length = pcm.len();
        if self.pending.load(Ordering::Acquire) + length > self.byte_limit {
            return Err(PushError::Backpressure);
        }
        self.pending.fetch_add(length, Ordering::AcqRel);
        sender.try_send(pcm).map_err(|error| {
            self.pending.fetch_sub(length, Ordering::AcqRel);
            match error {
                mpsc::error::TrySendError::Full(_) => PushError::Backpressure,
                mpsc::error::TrySendError::Closed(_) => PushError::Closed,
            }
        })
    }

    /// Stops accepting audio; already queued chunks are still delivered.
    pub fn close(&self) {
        lock(&self.sender).take();
    }

    /// Bytes queued or being sent.
    pub fn pending_bytes(&self) -> usize {
        self.pending.load(Ordering::Acquire)
    }
}

/// Releases a chunk's byte count when sending ends, however it ends.
struct Release<'a> {
    pending: &'a AtomicUsize,
    length: usize,
}

impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.pending.fetch_sub(self.length, Ordering::AcqRel);
    }
}

impl LaneReceiver {
    /// Sends chunks in order until the lane is closed and empty.
    pub async fn run(&mut self, client: &mut dyn SpeechClient) -> Result<(), SessionError> {
        while let Some(chunk) = self.receiver.recv().await {
            let _release = Release {
                pending: &self.pending,
                length: chunk.len(),
            };
            match tokio::time::timeout(SEND_TIMEOUT, client.send_audio(chunk)).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(error.speech_session_error()),
                Err(_) => return Err(SessionError::TransportError),
            }
        }
        Ok(())
    }
}

impl Drop for LaneReceiver {
    fn drop(&mut self) {
        self.receiver.close();
        while let Ok(chunk) = self.receiver.try_recv() {
            self.pending.fetch_sub(chunk.len(), Ordering::AcqRel);
        }
    }
}

/// Waits for a draining sender; past `STOP_DRAIN` it is abandoned as `audio_backpressure`.
pub async fn drain_within<F>(sending: F) -> Result<(), SessionError>
where
    F: Future<Output = Result<(), SessionError>>,
{
    tokio::time::timeout(STOP_DRAIN, sending)
        .await
        .unwrap_or(Err(SessionError::AudioBackpressure))
}
