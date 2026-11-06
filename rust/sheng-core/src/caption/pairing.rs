//! Pairing originals and translations that arrive separately (captions.md §3).
//!
//! Original finals queue in the order the service produced them. Each waits for the
//! translation of its own utterance; pairs are released strictly from the front.

use std::collections::VecDeque;

use crate::ids::{BoundedMap, UtteranceId, IDENTITY_LIMIT};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Paired {
    pub utterance: Option<UtteranceId>,
    pub original: String,
    pub translation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Waiting {
    utterance: Option<UtteranceId>,
    original: String,
    translation: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Pairing {
    queue: VecDeque<Waiting>,
    early_translations: BoundedMap<UtteranceId, String>,
}

impl Default for Pairing {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            early_translations: BoundedMap::new(IDENTITY_LIMIT),
        }
    }
}

impl Pairing {
    pub fn push_original(
        &mut self,
        utterance: Option<UtteranceId>,
        original: String,
    ) -> Vec<Paired> {
        let existing = if utterance.is_some() {
            self.position_of(&utterance)
        } else {
            None
        };
        match existing {
            Some(index) => self.queue[index].original = original,
            None => {
                let translation = utterance
                    .as_ref()
                    .and_then(|id| self.early_translations.remove(id));
                self.queue.push_back(Waiting {
                    utterance,
                    original,
                    translation,
                });
                if self.queue.len() > IDENTITY_LIMIT {
                    self.queue.pop_front();
                }
            }
        }
        self.release()
    }

    /// Attaches a translation to its own original. A translation with an id waits for that
    /// original; one without an id and without a waiting original is dropped.
    pub fn push_translation(
        &mut self,
        utterance: Option<UtteranceId>,
        translation: String,
    ) -> Vec<Paired> {
        let target = match &utterance {
            Some(_) => self.position_of(&utterance),
            None => self
                .queue
                .iter()
                .position(|w| w.utterance.is_none() && w.translation.is_none()),
        };
        match (target, utterance) {
            (Some(index), _) => self.queue[index].translation = Some(translation),
            (None, Some(id)) => self.early_translations.insert(id, translation),
            (None, None) => {}
        }
        self.release()
    }

    /// Forgets anything waiting for `utterance`, e.g. after it was confirmed as a whole pair.
    pub fn forget(&mut self, utterance: &Option<UtteranceId>) {
        let Some(id) = utterance else {
            return;
        };
        self.queue.retain(|w| w.utterance.as_ref() != Some(id));
        self.early_translations.remove(id);
    }

    /// Removes every waiting original, oldest first, with its translation if it has one.
    pub fn take_waiting(&mut self) -> Vec<(Option<UtteranceId>, String, Option<String>)> {
        self.queue
            .drain(..)
            .map(|w| (w.utterance, w.original, w.translation))
            .collect()
    }

    /// Removes a translation that arrived before its original.
    pub fn take_early(&mut self, utterance: &Option<UtteranceId>) -> Option<String> {
        self.early_translations.remove(utterance.as_ref()?)
    }

    pub fn clear(&mut self) {
        self.queue.clear();
        self.early_translations.clear();
    }

    fn position_of(&self, utterance: &Option<UtteranceId>) -> Option<usize> {
        self.queue.iter().position(|w| w.utterance == *utterance)
    }

    fn release(&mut self) -> Vec<Paired> {
        let mut released = Vec::new();
        while self.queue.front().is_some_and(|w| w.translation.is_some()) {
            let Some(Waiting {
                utterance,
                original,
                translation: Some(translation),
            }) = self.queue.pop_front()
            else {
                break;
            };
            released.push(Paired {
                utterance,
                original,
                translation,
            });
        }
        released
    }
}
