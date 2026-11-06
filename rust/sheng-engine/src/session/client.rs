//! A speech client shared by the send loop, the heartbeat and the lane, one call at a time.

use std::sync::Arc;
use std::time::Duration;

use sheng_runtime::provider::{BoxFuture, EventSink, ProviderError, SpeechClient};
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Heartbeat interval while running (snapshot.md).
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// A heartbeat taking longer than this is a transport error.
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct SharedClient(Arc<Mutex<Box<dyn SpeechClient>>>);

impl SharedClient {
    pub fn new(client: Box<dyn SpeechClient>) -> Self {
        Self(Arc::new(Mutex::new(client)))
    }

    /// One heartbeat; returns its round trip.
    pub async fn heartbeat_once(&self) -> Result<Duration, ProviderError> {
        let mut client = self.0.lock().await;
        let started = Instant::now();
        match tokio::time::timeout(HEARTBEAT_TIMEOUT, client.heartbeat()).await {
            Ok(Ok(())) => Ok(started.elapsed()),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(ProviderError::Transport),
        }
    }
}

impl SpeechClient for SharedClient {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move { self.0.lock().await.connect(events).await })
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move { self.0.lock().await.send_audio(pcm).await })
    }

    fn heartbeat(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move { self.0.lock().await.heartbeat().await })
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move { self.0.lock().await.finish().await })
    }

    fn disconnect(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async move { self.0.lock().await.disconnect().await })
    }
}
