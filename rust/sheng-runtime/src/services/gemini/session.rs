//! Route-specific handling of server events, shared by the reader task.

use tokio::time::Instant;

use super::protocol::{ServerEvent, SessionKind};
use super::transcribe::TranscribeState;
use super::translate::TranslateState;
use crate::provider::ProviderEvent;

#[derive(Debug)]
pub enum RouteState {
    Translate(TranslateState),
    Transcribe(TranscribeState),
}

impl RouteState {
    pub fn new(kind: &SessionKind) -> Self {
        match kind {
            SessionKind::Translate { .. } => Self::Translate(TranslateState::default()),
            SessionKind::Transcribe { .. } => Self::Transcribe(TranscribeState::default()),
        }
    }

    pub fn apply(&mut self, event: &ServerEvent, now: Instant) -> Vec<ProviderEvent> {
        match self {
            Self::Translate(state) => state.apply(event),
            Self::Transcribe(state) => state.apply(event, now),
        }
    }

    /// Quiet timer of the transcription route; the aligner times the translating route.
    pub fn quiet_deadline(&self) -> Option<Instant> {
        match self {
            Self::Translate(_) => None,
            Self::Transcribe(state) => state.quiet_deadline(),
        }
    }

    /// Finalizes whatever the transcription route has accumulated.
    pub fn flush(&mut self) -> Vec<ProviderEvent> {
        match self {
            Self::Translate(_) => Vec::new(),
            Self::Transcribe(state) => state.flush(),
        }
    }

    /// Nothing is waiting for more service output.
    pub fn is_idle(&self) -> bool {
        match self {
            Self::Translate(state) => state.is_idle(),
            Self::Transcribe(state) => state.is_idle(),
        }
    }

    /// After input has ended: whether `event` delivered the last results. Only the end of
    /// the turn does; later transcripts join the last sentence (providers/gemini.md
    /// “结束与断开”).
    pub fn finish_ready(&self, event: &ServerEvent) -> bool {
        matches!(event, ServerEvent::TurnComplete) && self.is_idle()
    }
}
