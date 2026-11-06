//! Finals, confirmation and dedupe (captions.md §2.3, §2.4, §3, §4).

use super::pairing::Paired;
use super::track::{Display, DisplayKind, Track};
use super::Confirmed;
use crate::ids::{fingerprint, UtteranceId};
use crate::text::{is_meaningful, within_limit};

impl Track {
    pub(crate) fn apply_original_final(
        &mut self,
        utterance: Option<UtteranceId>,
        text: String,
    ) -> Vec<Confirmed> {
        if !self.accepts_final(&utterance, &text) {
            return Vec::new();
        }
        self.observe(&utterance);
        if !self.needs_translation {
            return self
                .confirm(utterance, text, String::new())
                .into_iter()
                .collect();
        }
        self.show_awaiting_translation(&utterance, &text);
        let released = self.pairing.push_original(utterance, text);
        self.confirm_all(released)
    }

    pub(crate) fn apply_translation_final(
        &mut self,
        utterance: Option<UtteranceId>,
        text: String,
    ) -> Vec<Confirmed> {
        if !self.needs_translation || !self.accepts_final(&utterance, &text) {
            return Vec::new();
        }
        let released = self.pairing.push_translation(utterance, text);
        self.confirm_all(released)
    }

    pub(crate) fn apply_confirmed_pair(
        &mut self,
        utterance: Option<UtteranceId>,
        original: String,
        translation: String,
    ) -> Vec<Confirmed> {
        if !self.accepts_final(&utterance, &original) || !within_limit(&translation) {
            return Vec::new();
        }
        if self.needs_translation && !is_meaningful(&translation) {
            return Vec::new();
        }
        self.observe(&utterance);
        self.pairing.forget(&utterance);
        self.confirm(utterance, original, translation)
            .into_iter()
            .collect()
    }

    /// Unexpected end of a connection or a stop that timed out (providers/README.md): every
    /// recognized but unconfirmed original is confirmed once, oldest first, with whatever
    /// translation arrived for it (none is fine; the original is not lost).
    pub(crate) fn commit_unconfirmed(&mut self) -> Vec<Confirmed> {
        let mut candidates = self.pairing.take_waiting();
        if matches!(self.display.kind, DisplayKind::Draft | DisplayKind::Preview) {
            let shown = &self.display;
            let translation = Some(shown.translation.clone()).filter(|t| is_meaningful(t));
            candidates.push((shown.utterance.clone(), shown.original.clone(), translation));
        }
        if let Some(pending) = self.pending.take() {
            candidates.push((pending.utterance, pending.text, None));
        }
        let confirmed = candidates
            .into_iter()
            .filter_map(|(utterance, original, translation)| {
                let translation = translation
                    .or_else(|| self.shown_translation_of(&utterance))
                    .or_else(|| self.pairing.take_early(&utterance))
                    .unwrap_or_default();
                self.commit_one(utterance, original, translation)
            })
            .collect();
        self.pairing.clear();
        confirmed
    }

    fn shown_translation_of(&self, utterance: &Option<UtteranceId>) -> Option<String> {
        let shown = &self.display;
        let is_same = shown.kind != DisplayKind::Confirmed && shown.utterance == *utterance;
        (is_same && is_meaningful(&shown.translation)).then(|| shown.translation.clone())
    }

    fn commit_one(
        &mut self,
        utterance: Option<UtteranceId>,
        original: String,
        translation: String,
    ) -> Option<Confirmed> {
        if !self.accepts_final(&utterance, &original) || !within_limit(&translation) {
            return None;
        }
        self.observe(&utterance);
        self.confirm(utterance, original, translation)
    }

    /// Empty, oversized and stale finals are dropped before they can claim an identity.
    fn accepts_final(&self, utterance: &Option<UtteranceId>, text: &str) -> bool {
        within_limit(text) && is_meaningful(text) && !self.is_stale(utterance)
    }

    /// Shows a final original that still waits for its translation, unless a preview is shown
    /// (it stays until its own final pair arrives) or the display would move back to an older one.
    fn show_awaiting_translation(&mut self, utterance: &Option<UtteranceId>, text: &str) {
        if self.display.kind == DisplayKind::Preview || self.is_older_than_display(utterance) {
            return;
        }
        let translation = if self.display.utterance == *utterance {
            std::mem::take(&mut self.display.translation)
        } else {
            String::new()
        };
        self.take_pending_of(utterance);
        self.display = Display::draft(utterance.clone(), text.to_owned(), translation);
    }

    fn confirm_all(&mut self, released: Vec<Paired>) -> Vec<Confirmed> {
        released
            .into_iter()
            .filter_map(|pair| self.confirm(pair.utterance, pair.original, pair.translation))
            .collect()
    }

    fn confirm(
        &mut self,
        utterance: Option<UtteranceId>,
        original: String,
        translation: String,
    ) -> Option<Confirmed> {
        if !self.claim_identity(&utterance, &original) {
            return None;
        }
        if !self.is_older_than_display(&utterance) {
            self.take_pending_of(&utterance);
            let shown = Display {
                kind: DisplayKind::Confirmed,
                utterance,
                original: original.clone(),
                translation: translation.clone(),
            };
            self.last_confirmed = Some(shown.clone());
            self.display = shown;
        }
        Some(Confirmed {
            original,
            translation,
        })
    }

    /// Records the confirmation identity; false when it is a duplicate.
    fn claim_identity(&mut self, utterance: &Option<UtteranceId>, original: &str) -> bool {
        let Some(id) = utterance else {
            if self.last_anonymous_final.as_deref() == Some(original) {
                return false;
            }
            self.last_anonymous_final = Some(original.to_owned());
            return true;
        };
        if self.is_stale(utterance) {
            return false;
        }
        if let Some(evicted) = self.confirmed.insert(id.clone()) {
            self.retired.insert(fingerprint(&evicted));
        }
        let is_newest = self
            .newest_confirmed
            .as_ref()
            .is_none_or(|newest| !self.order.is_older(id, newest));
        if is_newest {
            self.newest_confirmed = Some(id.clone());
        }
        true
    }
}
