//! `appleSpeech` recognition client (apple.md §2). Audio goes to the Swift bridge's bounded
//! queue; volatile and final results come back as original drafts and finals with an
//! increasing utterance number. Translation is done by the independent text pipeline.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use sheng_core::{CaptionEvent, UtteranceId};
use tokio::sync::oneshot;

use super::events::{self, BridgeEvent};
use super::locale::map_locale;
use super::status::{speech_failure, SpeechStatus};
use super::system::{Apple, Push};
use crate::provider::{
    BoxFuture, EventSink, ProviderError, ProviderEvent, SessionError, SpeechClient,
};

/// The recognizer must be ready within this time after `connect` starts it.
pub const START_TIMEOUT: Duration = Duration::from_secs(10);

fn local(code: SessionError) -> ProviderError {
    ProviderError::Local(code)
}

/// State shared with the bridge's event callback.
struct Lane {
    sink: EventSink,
    utterance: AtomicU64,
    /// Results could not be queued; the session fails with `apple_speech_result_backlog`.
    overflow: AtomicBool,
    closed: AtomicBool,
    /// Input has ended; the recognizer stopping is then expected.
    finishing: AtomicBool,
    ready: Mutex<Option<oneshot::Sender<Result<(), SessionError>>>>,
}

impl Lane {
    fn on_event(&self, event: BridgeEvent) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let pending = self
            .ready
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(ready) = pending {
            let outcome = match event {
                BridgeEvent::Ready => Ok(()),
                BridgeEvent::Failed(name) => Err(speech_failure(&name)),
                _ => Err(SessionError::AppleSpeechStreamClosed),
            };
            let _ = ready.send(outcome);
            return;
        }
        let mapped = match event {
            BridgeEvent::Ready => return,
            BridgeEvent::Draft(text) => ProviderEvent::Caption(CaptionEvent::OriginalDraft {
                utterance: Some(self.current()),
                text,
            }),
            BridgeEvent::Final(text) => {
                let utterance = Some(self.current());
                self.utterance.fetch_add(1, Ordering::AcqRel);
                ProviderEvent::Caption(CaptionEvent::OriginalFinal { utterance, text })
            }
            BridgeEvent::Ended => ProviderEvent::Ended,
            BridgeEvent::Failed(name) => ProviderEvent::Failed(local(speech_failure(&name))),
        };
        self.emit(mapped);
    }

    fn current(&self) -> UtteranceId {
        UtteranceId::Number(self.utterance.load(Ordering::Acquire))
    }

    fn emit(&self, event: ProviderEvent) {
        if self.sink.emit(event).is_err() {
            self.overflow.store(true, Ordering::Release);
        }
    }

    fn check(&self) -> Result<(), ProviderError> {
        if self.overflow.load(Ordering::Acquire) {
            return Err(local(SessionError::AppleSpeechResultBacklog));
        }
        Ok(())
    }
}

/// Speech client for one source's lane.
pub struct AppleSpeech {
    apple: Apple,
    source_language: String,
    session: Option<(u64, Arc<Lane>)>,
}

impl AppleSpeech {
    pub fn new(apple: Apple, source_language: impl Into<String>) -> Self {
        Self {
            apple,
            source_language: source_language.into(),
            session: None,
        }
    }

    /// Installed locale for the source language. Sessions never download (apple.md §1).
    async fn locale(&self) -> Result<String, ProviderError> {
        let unavailable = local(SessionError::AppleSpeechRuntimeServiceUnavailable);
        let inventory = self.apple.system.speech_inventory().await;
        let inventory = inventory.filter(|i| i.available).ok_or(unavailable)?;
        let identifiers = inventory.identifiers();
        let locale = map_locale(&self.source_language, &identifiers).ok_or(unavailable)?;
        if inventory.status(locale) != SpeechStatus::Installed {
            return Err(unavailable);
        }
        Ok(locale.to_owned())
    }

    async fn start(&mut self, sink: EventSink, locale: &str) -> Result<(), ProviderError> {
        let (ready, started) = oneshot::channel();
        let lane = Arc::new(Lane {
            sink: sink.clone(),
            utterance: AtomicU64::new(1),
            overflow: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            finishing: AtomicBool::new(false),
            ready: Mutex::new(Some(ready)),
        });
        let handler = Arc::clone(&lane);
        let token = events::register(move |event| handler.on_event(event));
        self.session = Some((token, Arc::clone(&lane)));
        if !self.apple.recognizer.open(locale, token) {
            self.close();
            return Err(local(SessionError::AppleSpeechRuntimeServiceUnavailable));
        }
        let outcome = match tokio::time::timeout(START_TIMEOUT, started).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => Err(SessionError::AppleSpeechStreamClosed),
            Err(_) => Err(SessionError::AppleSpeechRuntimeTimeout),
        };
        if let Err(code) = outcome {
            self.close();
            return Err(local(code));
        }
        sink.emit(ProviderEvent::Ready)
            .map_err(|_| local(SessionError::AppleSpeechResultBacklog))
    }

    fn close(&mut self) {
        if let Some((token, lane)) = self.session.take() {
            lane.closed.store(true, Ordering::Release);
            events::unregister(token);
            self.apple.recognizer.close(token);
        }
    }

    fn running(&self) -> Result<(u64, &Lane), ProviderError> {
        let (token, lane) = self
            .session
            .as_ref()
            .ok_or(local(SessionError::AppleSpeechStreamClosed))?;
        lane.check()?;
        Ok((*token, lane))
    }
}

impl SpeechClient for AppleSpeech {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            self.close();
            let locale = self.locale().await?;
            self.start(events, &locale).await
        })
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            let (token, _) = self.running()?;
            match self.apple.recognizer.push(token, &pcm) {
                Push::Accepted => Ok(()),
                Push::Full => Err(local(SessionError::AudioBackpressure)),
                Push::Closed => Err(local(SessionError::AppleSpeechStreamClosed)),
            }
        })
    }

    /// No network round trip: the check is that the recognition task still runs.
    fn heartbeat(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            let (token, lane) = self.running()?;
            let finishing = lane.finishing.load(Ordering::Acquire);
            if !finishing && !self.apple.recognizer.alive(token) {
                // Reported as an event so the session fails with the Apple code.
                lane.emit(ProviderEvent::Failed(local(
                    SessionError::AppleSpeechStreamClosed,
                )));
            }
            Ok(())
        })
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            let (token, lane) = self.running()?;
            lane.finishing.store(true, Ordering::Release);
            self.apple.recognizer.finish(token);
            Ok(())
        })
    }

    fn disconnect(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async move { self.close() })
    }
}

impl Drop for AppleSpeech {
    fn drop(&mut self) {
        self.close();
    }
}
