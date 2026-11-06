//! State of one audio source and event dispatch.

use super::pairing::Pairing;
use super::{CaptionEvent, Confirmed};
use crate::ids::{fingerprint, BoundedSet, UtteranceId, IDENTITY_LIMIT, RETIRED_IDENTITY_LIMIT};
use crate::order::UtteranceOrder;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DisplayKind {
    #[default]
    Empty,
    Draft,
    Preview,
    Confirmed,
}

/// The pair currently shown in `original`/`translation`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Display {
    pub kind: DisplayKind,
    pub utterance: Option<UtteranceId>,
    pub original: String,
    pub translation: String,
}

impl Display {
    pub fn draft(utterance: Option<UtteranceId>, original: String, translation: String) -> Self {
        Self {
            kind: DisplayKind::Draft,
            utterance,
            original,
            translation,
        }
    }
}

/// Recognized original that has not joined the displayed pair yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingOriginal {
    pub utterance: Option<UtteranceId>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Track {
    pub needs_translation: bool,
    pub display: Display,
    pub last_confirmed: Option<Display>,
    pub pending: Option<PendingOriginal>,
    pub pairing: Pairing,
    pub order: UtteranceOrder,
    pub confirmed: BoundedSet<UtteranceId>,
    pub newest_confirmed: Option<UtteranceId>,
    /// Fingerprints of confirmed ids evicted from `confirmed`, so they never confirm again.
    pub retired: BoundedSet<u64>,
    pub seen: BoundedSet<UtteranceId>,
    pub cleared: BoundedSet<UtteranceId>,
    pub last_anonymous_final: Option<String>,
}

impl Track {
    pub fn new(needs_translation: bool) -> Self {
        Self {
            needs_translation,
            display: Display::default(),
            last_confirmed: None,
            pending: None,
            pairing: Pairing::default(),
            order: UtteranceOrder::new(),
            confirmed: BoundedSet::new(IDENTITY_LIMIT),
            newest_confirmed: None,
            retired: BoundedSet::new(RETIRED_IDENTITY_LIMIT),
            seen: BoundedSet::new(IDENTITY_LIMIT),
            cleared: BoundedSet::new(IDENTITY_LIMIT),
            last_anonymous_final: None,
        }
    }

    /// Applies one event and returns the pairs confirmed by it, oldest first.
    pub fn apply(&mut self, event: CaptionEvent) -> Vec<Confirmed> {
        match event {
            CaptionEvent::OriginalDraft { utterance, text } => {
                self.apply_original_draft(utterance, text);
                Vec::new()
            }
            CaptionEvent::TranslationDraft { utterance, text } => {
                self.apply_translation_draft(utterance, text);
                Vec::new()
            }
            CaptionEvent::PreviewPair {
                utterance,
                original,
                translation,
            } => {
                self.apply_preview(utterance, original, translation);
                Vec::new()
            }
            CaptionEvent::Retract { utterance } => {
                self.apply_retract(utterance);
                Vec::new()
            }
            CaptionEvent::OriginalFinal { utterance, text } => {
                self.apply_original_final(utterance, text)
            }
            CaptionEvent::TranslationFinal { utterance, text } => {
                self.apply_translation_final(utterance, text)
            }
            CaptionEvent::ConfirmedPair {
                utterance,
                original,
                translation,
            } => self.apply_confirmed_pair(utterance, original, translation),
            CaptionEvent::Order {
                utterance,
                sequence,
            } => {
                if !self.cleared.contains(&utterance) {
                    self.order.record_service_order(&utterance, sequence);
                }
                Vec::new()
            }
            CaptionEvent::Reset => {
                self.reset();
                Vec::new()
            }
        }
    }

    /// Drops unconfirmed content. Ids may be reused by the service after this point.
    pub fn reset(&mut self) {
        if self.display.kind != DisplayKind::Confirmed {
            self.display = Display::default();
        }
        self.pending = None;
        self.pairing.clear();
        self.order.clear();
        self.confirmed.clear();
        self.newest_confirmed = None;
        self.retired.clear();
        self.seen.clear();
        self.cleared.clear();
    }

    /// Clears the display and remembers every id seen so far so late events for them are ignored.
    pub fn clear(&mut self) {
        let seen: Vec<UtteranceId> = self.seen.iter().cloned().collect();
        for id in seen {
            self.cleared.insert(id);
        }
        self.display = Display::default();
        self.last_confirmed = None;
        self.pending = None;
        self.pairing.clear();
    }

    /// True for events of cleared, already-confirmed or older-than-confirmed utterances.
    pub fn is_stale(&self, utterance: &Option<UtteranceId>) -> bool {
        let Some(id) = utterance else {
            return false;
        };
        if self.cleared.contains(id)
            || self.confirmed.contains(id)
            || self.retired.contains(&fingerprint(id))
        {
            return true;
        }
        self.newest_confirmed
            .as_ref()
            .is_some_and(|newest| self.order.is_older(id, newest))
    }

    /// True when `utterance` is known to be older than the one currently displayed.
    pub fn is_older_than_display(&self, utterance: &Option<UtteranceId>) -> bool {
        match (utterance, &self.display.utterance) {
            (Some(candidate), Some(shown)) => self.order.is_older(candidate, shown),
            _ => false,
        }
    }

    pub fn observe(&mut self, utterance: &Option<UtteranceId>) {
        if let Some(id) = utterance {
            self.order.observe(id);
            self.seen.insert(id.clone());
        }
    }

    pub fn take_pending_of(&mut self, utterance: &Option<UtteranceId>) -> Option<PendingOriginal> {
        if self.pending.as_ref()?.utterance == *utterance {
            return self.pending.take();
        }
        None
    }
}
