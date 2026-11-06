//! Test-only scripted speech client and text translator. They never touch the network;
//! enabled for this crate's tests and, through the `test-support` feature, for ABI tests.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use super::error::ProviderError;
use super::traits::{
    BoxFuture, EventSink, ProviderEvent, SpeechClient, TextTranslator, TranslationRequest,
};

/// What the scripted speech client does at each stage.
#[derive(Debug, Clone, Default)]
pub struct SpeechScript {
    /// Error returned by `connect`, if any.
    pub connect_error: Option<ProviderError>,
    /// Events emitted right after connecting (`Ready` is emitted first automatically).
    pub on_connect: Vec<ProviderEvent>,
    /// Events emitted after the n-th audio chunk (1-based).
    pub on_audio: Vec<(usize, Vec<ProviderEvent>)>,
    /// Events emitted on `finish`, before the automatic `Ended`.
    pub on_finish: Vec<ProviderEvent>,
    /// Every `send_audio` waits this long, to simulate a stuck transport.
    pub send_delay: Option<Duration>,
    /// `finish` emits `on_finish` but no `Ended`: a service that never confirms the end.
    pub silent_finish: bool,
}

/// What the scripted speech client observed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeechLog {
    pub audio: Vec<u8>,
    pub chunks: usize,
    pub heartbeats: usize,
    pub finished: bool,
    pub disconnected: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SharedLog(Arc<Mutex<SpeechLog>>);

impl SharedLog {
    pub fn snapshot(&self) -> SpeechLog {
        self.lock().clone()
    }

    fn lock(&self) -> MutexGuard<'_, SpeechLog> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Debug)]
pub struct ScriptedSpeech {
    script: SpeechScript,
    log: SharedLog,
    events: Option<EventSink>,
}

impl ScriptedSpeech {
    pub fn new(script: SpeechScript) -> (Self, SharedLog) {
        let log = SharedLog::default();
        let client = Self {
            script,
            log: log.clone(),
            events: None,
        };
        (client, log)
    }

    fn emit_all(&self, events: Vec<ProviderEvent>) -> Result<(), ProviderError> {
        if events.is_empty() {
            return Ok(());
        }
        let Some(sink) = &self.events else {
            return Err(ProviderError::Transport);
        };
        events
            .into_iter()
            .try_for_each(|event| sink.emit(event).map_err(|_| ProviderError::Transport))
    }
}

impl SpeechClient for ScriptedSpeech {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            if let Some(error) = self.script.connect_error {
                return Err(error);
            }
            self.events = Some(events);
            let mut initial = vec![ProviderEvent::Ready];
            initial.extend(self.script.on_connect.clone());
            self.emit_all(initial)
        })
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            if let Some(delay) = self.script.send_delay {
                tokio::time::sleep(delay).await;
            }
            let chunk = {
                let mut log = self.log.lock();
                log.audio.extend_from_slice(&pcm);
                log.chunks += 1;
                log.chunks
            };
            let due: Vec<ProviderEvent> = self
                .script
                .on_audio
                .iter()
                .filter(|(at, _)| *at == chunk)
                .flat_map(|(_, events)| events.clone())
                .collect();
            self.emit_all(due)
        })
    }

    fn heartbeat(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            self.log.lock().heartbeats += 1;
            Ok(())
        })
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            self.log.lock().finished = true;
            let mut events = self.script.on_finish.clone();
            if !self.script.silent_finish {
                events.push(ProviderEvent::Ended);
            }
            self.emit_all(events)
        })
    }

    fn disconnect(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.log.lock().disconnected = true;
            self.events = None;
        })
    }
}

/// One scripted translator reply.
#[derive(Debug, Clone)]
pub enum Reply {
    /// Translation of the request text with this prefix, e.g. `"zh:"`.
    Prefix(String),
    Fail(ProviderError),
    /// Waits, then answers with the given prefix.
    Delayed(Duration, String),
}

/// Answers translation requests from a queue of replies; when the queue is empty it echoes
/// the text with a `"tr:"` prefix. Records every request.
#[derive(Debug, Default)]
pub struct ScriptedTranslator {
    replies: Mutex<VecDeque<Reply>>,
    requests: Mutex<Vec<TranslationRequest>>,
}

impl ScriptedTranslator {
    pub fn new(replies: impl IntoIterator<Item = Reply>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        })
    }

    pub fn requests(&self) -> Vec<TranslationRequest> {
        self.requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl TextTranslator for ScriptedTranslator {
    fn translate(
        &self,
        request: TranslationRequest,
    ) -> BoxFuture<'_, Result<String, ProviderError>> {
        let reply = self
            .replies
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .unwrap_or_else(|| Reply::Prefix("tr:".into()));
        let text = request.text.clone();
        self.requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request);
        Box::pin(async move {
            match reply {
                Reply::Prefix(prefix) => Ok(format!("{prefix}{text}")),
                Reply::Fail(error) => Err(error),
                Reply::Delayed(delay, prefix) => {
                    tokio::time::sleep(delay).await;
                    Ok(format!("{prefix}{text}"))
                }
            }
        })
    }
}
