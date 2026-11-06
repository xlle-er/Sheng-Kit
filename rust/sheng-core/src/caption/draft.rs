//! Drafts, previews and retractions (captions.md §2.1, §2.2, §2.5).

use super::track::{Display, DisplayKind, PendingOriginal, Track};
use crate::ids::UtteranceId;
use crate::text::{is_meaningful, within_limit};

impl Track {
    pub(crate) fn apply_original_draft(&mut self, utterance: Option<UtteranceId>, text: String) {
        if !within_limit(&text) || self.is_stale(&utterance) {
            return;
        }
        if self.is_older_than_display(&utterance) {
            return;
        }
        self.observe(&utterance);
        let shows_pair = matches!(
            self.display.kind,
            DisplayKind::Preview | DisplayKind::Confirmed
        );
        if !is_meaningful(&text) && shows_pair {
            return;
        }
        if self.display.kind == DisplayKind::Preview {
            if utterance.is_some() && self.display.utterance == utterance {
                // A revision of the previewed sentence is shown at once; its translation
                // stays until the next preview (questions.md, draft during preview).
                self.display.original = text;
                return;
            }
            self.pending = Some(PendingOriginal { utterance, text });
            return;
        }
        let translation =
            if self.display.kind == DisplayKind::Draft && self.display.utterance == utterance {
                std::mem::take(&mut self.display.translation)
            } else {
                String::new()
            };
        self.pending = None;
        self.display = Display::draft(utterance, text, translation);
    }

    /// A translation draft only updates the unconfirmed display of the same utterance.
    pub(crate) fn apply_translation_draft(&mut self, utterance: Option<UtteranceId>, text: String) {
        if !within_limit(&text) || !is_meaningful(&text) || self.is_stale(&utterance) {
            return;
        }
        if self.display.kind == DisplayKind::Draft && self.display.utterance == utterance {
            self.display.translation = text;
        }
    }

    pub(crate) fn apply_preview(
        &mut self,
        utterance: Option<UtteranceId>,
        original: String,
        translation: String,
    ) {
        let is_complete = within_limit(&original)
            && within_limit(&translation)
            && is_meaningful(&original)
            && is_meaningful(&translation);
        if !is_complete || self.is_stale(&utterance) || self.is_older_than_display(&utterance) {
            return;
        }
        self.observe(&utterance);
        self.take_pending_of(&utterance);
        self.display = Display {
            kind: DisplayKind::Preview,
            utterance,
            original,
            translation,
        };
    }

    /// Removes the preview of `utterance` only; anything else stays as it is.
    pub(crate) fn apply_retract(&mut self, utterance: Option<UtteranceId>) {
        let is_matching_preview =
            self.display.kind == DisplayKind::Preview && self.display.utterance == utterance;
        if !is_matching_preview {
            return;
        }
        self.display = match self.pending.take() {
            Some(pending) => Display::draft(pending.utterance, pending.text, String::new()),
            None => self.last_confirmed.clone().unwrap_or_default(),
        };
    }
}
