//! asrMt route: only the input transcript is used. Text is accumulated and shown as a
//! draft; on quiet (2 s) or at the end of a turn the accumulated text becomes the final
//! (providers/gemini.md §4, captions.md §5).

use std::time::Duration;

use sheng_core::align::QUIET_TIMEOUT;
use sheng_core::text::is_meaningful;
use sheng_core::{CaptionEvent, UtteranceId};
use tokio::time::Instant;

use super::protocol::ServerEvent;
use super::translate::TurnText;
use crate::provider::ProviderEvent;

/// Quiet period after which accumulated text is finalized.
pub const TRANSCRIBE_QUIET: Duration = QUIET_TIMEOUT;

/// Accumulated original text of the current utterance. Utterances get local sequence
/// numbers so that repeated sentences are confirmed once each.
#[derive(Debug, Default)]
pub struct TranscribeState {
    utterance: u64,
    /// `inputTranscription` text so far.
    settled: TurnText,
    /// Latest `interimInputTranscription` hypothesis after the settled text.
    interim: String,
    last_text_at: Option<Instant>,
}

/// Joins settled text and the interim hypothesis, adding a space only between two ASCII
/// words that the service did not separate.
fn join(settled: &str, interim: &str) -> String {
    let needs_space = matches!(
        (settled.chars().last(), interim.chars().next()),
        (Some(a), Some(b)) if a.is_ascii_graphic() && b.is_ascii_alphanumeric()
    );
    if needs_space {
        format!("{settled} {interim}")
    } else {
        format!("{settled}{interim}")
    }
}

impl TranscribeState {
    pub fn apply(&mut self, event: &ServerEvent, now: Instant) -> Vec<ProviderEvent> {
        match event {
            ServerEvent::Input(text) => {
                if self.settled.increment(text).is_none() {
                    return Vec::new();
                }
                self.interim.clear();
                self.draft(now)
            }
            ServerEvent::InterimInput(text) => {
                text.clone_into(&mut self.interim);
                self.draft(now)
            }
            ServerEvent::GenerationComplete | ServerEvent::TurnComplete => self.flush(),
            _ => Vec::new(),
        }
    }

    /// When the quiet timer should fire, if any text is waiting.
    pub fn quiet_deadline(&self) -> Option<Instant> {
        self.last_text_at.map(|at| at + TRANSCRIBE_QUIET)
    }

    pub fn is_idle(&self) -> bool {
        self.last_text_at.is_none()
    }

    /// An interim hypothesis is waiting for its finalized transcript.
    pub fn has_interim(&self) -> bool {
        !self.interim.is_empty()
    }

    /// Finalizes the accumulated text, if it has content.
    pub fn flush(&mut self) -> Vec<ProviderEvent> {
        if self.is_idle() {
            return Vec::new();
        }
        let text = join(self.settled.as_str(), &self.interim);
        self.settled.clear();
        self.interim.clear();
        self.last_text_at = None;
        if !is_meaningful(&text) {
            return Vec::new();
        }
        let utterance = Some(UtteranceId::Number(self.utterance));
        self.utterance += 1;
        vec![ProviderEvent::Caption(CaptionEvent::OriginalFinal {
            utterance,
            text,
        })]
    }

    fn draft(&mut self, now: Instant) -> Vec<ProviderEvent> {
        self.last_text_at = Some(now);
        let text = join(self.settled.as_str(), &self.interim);
        if !is_meaningful(&text) {
            return Vec::new();
        }
        vec![ProviderEvent::Caption(CaptionEvent::OriginalDraft {
            utterance: Some(UtteranceId::Number(self.utterance)),
            text,
        })]
    }
}
