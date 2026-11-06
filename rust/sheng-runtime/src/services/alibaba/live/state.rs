//! Turns realtime live-translation events into caption events. Originals use the
//! transcription item id as utterance; translations use the id of the transcription item
//! they are linked to through `previous_item_id`, so the caption state pairs them
//! (captions.md §3) even when the translation is final first.

use std::collections::VecDeque;

use sheng_core::{CaptionEvent, UtteranceId};

use super::protocol::{self, LiveEvent, TextUpdate};
use crate::provider::{ProviderError, ProviderEvent};
use crate::services::alibaba::connection::{Inbound, Step};
use crate::services::alibaba::errors;

/// Active items remembered per kind; the oldest is forgotten first (captions.md §7).
pub const ITEM_LIMIT: usize = 64;

/// Small insertion-ordered map with a fixed capacity.
#[derive(Debug)]
struct Recent<V> {
    entries: VecDeque<(String, V)>,
}

impl<V> Default for Recent<V> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }
}

impl<V> Recent<V> {
    fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn insert(&mut self, key: String, value: V) {
        self.remove(&key);
        if self.entries.len() == ITEM_LIMIT {
            self.entries.pop_front();
        }
        self.entries.push_back((key, value));
    }

    fn remove(&mut self, key: &str) -> Option<V> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        self.entries.remove(index).map(|(_, v)| v)
    }
}

#[derive(Debug, Default)]
pub struct LiveState {
    originals: Recent<String>,
    translations: Recent<String>,
    links: Recent<String>,
}

fn apply(current: Option<String>, update: TextUpdate) -> String {
    match update {
        TextUpdate::Append(delta) => current.unwrap_or_default() + &delta,
        TextUpdate::Replace(text) => text,
    }
}

fn utterance(item: &str) -> Option<UtteranceId> {
    Some(UtteranceId::Text(item.to_owned()))
}

fn caption(event: CaptionEvent) -> Vec<ProviderEvent> {
    vec![ProviderEvent::Caption(event)]
}

impl LiveState {
    pub fn handle(&mut self, event: LiveEvent) -> Step {
        match event {
            LiveEvent::TranslationItem { item, previous } => {
                self.links.insert(item, previous);
                Step::default()
            }
            LiveEvent::Original { item, text } => self.original(item, text),
            LiveEvent::OriginalDone { item, text } => self.original_done(item, text),
            LiveEvent::Translation { item, text } => self.translation(item, text),
            LiveEvent::TranslationDone { item, text } => self.translation_done(item, text),
            LiveEvent::ResponseFailed => Step::last(ProviderEvent::Failed(ProviderError::Provider)),
            LiveEvent::Error { code, message } => Step::last(ProviderEvent::Failed(
                errors::classify(code.as_deref(), message.as_deref()),
            )),
            LiveEvent::Finished => Step::last(ProviderEvent::Ended),
            LiveEvent::SessionCreated | LiveEvent::SessionUpdated | LiveEvent::Other => {
                Step::default()
            }
        }
    }

    fn original(&mut self, item: String, update: TextUpdate) -> Step {
        let text = apply(self.originals.remove(&item), update);
        self.originals.insert(item.clone(), text.clone());
        if text.trim().is_empty() {
            return Step::default();
        }
        let utterance = utterance(&item);
        Step::events(caption(CaptionEvent::OriginalDraft { utterance, text }))
    }

    fn original_done(&mut self, item: String, text: String) -> Step {
        let accumulated = self.originals.remove(&item);
        let text = if text.trim().is_empty() {
            accumulated.unwrap_or_default()
        } else {
            text
        };
        if text.trim().is_empty() {
            return Step::default();
        }
        let utterance = utterance(&item);
        Step::events(caption(CaptionEvent::OriginalFinal { utterance, text }))
    }

    fn translation(&mut self, item: String, update: TextUpdate) -> Step {
        let text = apply(self.translations.remove(&item), update);
        self.translations.insert(item.clone(), text.clone());
        let Some(original) = self.links.get(&item) else {
            return Step::default();
        };
        if text.trim().is_empty() {
            return Step::default();
        }
        let utterance = utterance(original);
        Step::events(caption(CaptionEvent::TranslationDraft { utterance, text }))
    }

    /// Unlinked translations are orphans and never paired (captions.md §3.2).
    fn translation_done(&mut self, item: String, text: String) -> Step {
        let accumulated = self.translations.remove(&item);
        let Some(original) = self.links.remove(&item) else {
            return Step::default();
        };
        let text = if text.trim().is_empty() {
            accumulated.unwrap_or_default()
        } else {
            text
        };
        if text.trim().is_empty() {
            return Step::default();
        }
        let utterance = utterance(&original);
        Step::events(caption(CaptionEvent::TranslationFinal { utterance, text }))
    }
}

impl Inbound for LiveState {
    fn on_text(&mut self, text: &str) -> Step {
        self.handle(protocol::parse(text))
    }

    /// Originals without their `completed` event become finals, preceded by the
    /// translation text received for them (providers/alibaba.md “会话意外结束与恢复”).
    fn interrupted(&mut self) -> Vec<ProviderEvent> {
        let mut events = Vec::new();
        for (item, text) in std::mem::take(&mut self.originals).entries {
            if text.trim().is_empty() {
                continue;
            }
            let linked = self
                .links
                .entries
                .iter()
                .find(|(_, original)| *original == item)
                .map(|(translation, _)| translation.clone());
            let translation = linked.and_then(|t| self.translations.remove(&t));
            if let Some(translation) = translation.filter(|t| !t.trim().is_empty()) {
                events.extend(caption(CaptionEvent::TranslationFinal {
                    utterance: utterance(&item),
                    text: translation,
                }));
            }
            events.extend(caption(CaptionEvent::OriginalFinal {
                utterance: utterance(&item),
                text,
            }));
        }
        events
    }
}
