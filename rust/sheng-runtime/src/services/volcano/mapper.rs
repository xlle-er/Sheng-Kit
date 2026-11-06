//! Maps AST v2 responses onto abstract caption events (captions.md §1, §6).
//!
//! The service has no utterance ids. Each `SourceSubtitleStart` opens a numbered utterance;
//! a translation belongs to the source sentence with the same start time, otherwise to the
//! oldest source sentence still waiting for its translation. The source start time is the
//! service order; only strictly increasing, non-negative start times are passed on.

use std::collections::VecDeque;

use sheng_core::{CaptionEvent, UtteranceId};

use super::message::{event, Response};
use crate::provider::{classify_vendor_code, ProviderError};

/// Source sentences waiting for a translation, oldest first.
const WAITING_LIMIT: usize = 64;
/// `ResponseMeta.StatusCode` of a successful response.
const STATUS_OK: i32 = 20_000_000;

/// Vendor status codes (document error table and `au_base.proto` `Code`).
const STATUS_TABLE: &[(&str, ProviderError)] = &[
    ("45000001", ProviderError::Configuration),
    ("45000151", ProviderError::Configuration),
    ("45000081", ProviderError::Transport),
    ("55000031", ProviderError::Unavailable { retry_after: None }),
    ("11100", ProviderError::Configuration),
    ("11500", ProviderError::Configuration),
    ("11103", ProviderError::Configuration),
    ("11200", ProviderError::Authentication),
    ("11301", ProviderError::RateLimited { retry_after: None }),
    ("11302", ProviderError::RateLimited { retry_after: None }),
    ("11303", ProviderError::Unavailable { retry_after: None }),
    ("21200", ProviderError::Transport),
];

/// What one response means for the session.
#[derive(Debug, Clone, PartialEq)]
pub enum Mapped {
    Started,
    Caption(CaptionEvent),
    Ended,
    Failed(ProviderError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sentence {
    id: u64,
    start: Option<i32>,
}

#[derive(Debug, Default)]
pub struct Mapper {
    next_id: u64,
    last_order: Option<i32>,
    source: Option<Sentence>,
    source_text: String,
    translation: Option<u64>,
    translation_text: String,
    waiting: VecDeque<Sentence>,
}

impl Mapper {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn map(&mut self, response: Response) -> Vec<Mapped> {
        if let Some(error) = status_error(&response) {
            return vec![Mapped::Failed(error)];
        }
        match response.event {
            event::SESSION_STARTED => vec![Mapped::Started],
            event::SESSION_FINISHED => vec![Mapped::Ended],
            event::SESSION_FAILED => vec![Mapped::Failed(ProviderError::Provider)],
            event::SOURCE_START => self.source_start(response.start_time),
            event::SOURCE_RESPONSE => self.source_draft(response.text),
            event::SOURCE_END => self.source_end(response.text, response.start_time),
            event::TRANSLATION_START => {
                self.translation_start(response.start_time);
                Vec::new()
            }
            event::TRANSLATION_RESPONSE => self.translation_draft(response.text),
            event::TRANSLATION_END => self.translation_end(response.text, response.start_time),
            _ => Vec::new(),
        }
    }

    fn source_start(&mut self, start: Option<i32>) -> Vec<Mapped> {
        let sentence = self.open_source(start);
        self.order(sentence).into_iter().collect()
    }

    fn open_source(&mut self, start: Option<i32>) -> Sentence {
        let sentence = Sentence {
            id: self.next_id,
            start,
        };
        self.next_id += 1;
        self.source = Some(sentence);
        self.source_text.clear();
        if self.waiting.len() >= WAITING_LIMIT {
            self.waiting.pop_front();
        }
        self.waiting.push_back(sentence);
        sentence
    }

    /// Order event for a valid service start time (captions.md §6.4).
    fn order(&mut self, sentence: Sentence) -> Option<Mapped> {
        let start = sentence.start.filter(|start| *start >= 0)?;
        if self.last_order.is_some_and(|last| start <= last) {
            return None;
        }
        self.last_order = Some(start);
        Some(Mapped::Caption(CaptionEvent::Order {
            utterance: UtteranceId::Number(sentence.id),
            sequence: i64::from(start),
        }))
    }

    fn source_draft(&mut self, text: String) -> Vec<Mapped> {
        let sentence = match self.source {
            Some(sentence) => sentence,
            None => self.open_source(None),
        };
        merge(&mut self.source_text, text);
        vec![Mapped::Caption(CaptionEvent::OriginalDraft {
            utterance: Some(UtteranceId::Number(sentence.id)),
            text: self.source_text.clone(),
        })]
    }

    fn source_end(&mut self, text: String, start: Option<i32>) -> Vec<Mapped> {
        let mut out = Vec::new();
        let sentence = match self.source.take() {
            Some(sentence) => sentence,
            None => {
                let sentence = self.open_source(start);
                self.source = None;
                out.extend(self.order(sentence));
                sentence
            }
        };
        self.source_text.clear();
        out.push(Mapped::Caption(CaptionEvent::OriginalFinal {
            utterance: Some(UtteranceId::Number(sentence.id)),
            text,
        }));
        out
    }

    fn translation_start(&mut self, start: Option<i32>) -> u64 {
        let id = self.claim_waiting(start);
        self.translation = Some(id);
        self.translation_text.clear();
        id
    }

    /// The waiting source sentence with this start time, else the oldest waiting one, else
    /// a fresh id that pairs with nothing (captions.md §3.2).
    fn claim_waiting(&mut self, start: Option<i32>) -> u64 {
        let matched = start.and_then(|start| {
            self.waiting
                .iter()
                .position(|sentence| sentence.start == Some(start))
        });
        if let Some(sentence) = matched
            .or((!self.waiting.is_empty()).then_some(0))
            .and_then(|index| self.waiting.remove(index))
        {
            return sentence.id;
        }
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn translation_draft(&mut self, text: String) -> Vec<Mapped> {
        let id = match self.translation {
            Some(id) => id,
            None => self.translation_start(None),
        };
        merge(&mut self.translation_text, text);
        vec![Mapped::Caption(CaptionEvent::TranslationDraft {
            utterance: Some(UtteranceId::Number(id)),
            text: self.translation_text.clone(),
        })]
    }

    fn translation_end(&mut self, text: String, start: Option<i32>) -> Vec<Mapped> {
        let id = match self.translation.take() {
            Some(id) => id,
            None => self.claim_waiting(start),
        };
        self.translation_text.clear();
        vec![Mapped::Caption(CaptionEvent::TranslationFinal {
            utterance: Some(UtteranceId::Number(id)),
            text,
        })]
    }
}

/// Draft text: a response that extends the current text replaces it, anything else is
/// appended (the service may send either the whole sentence so far or the next piece).
fn merge(current: &mut String, text: String) {
    if text.starts_with(current.as_str()) {
        *current = text;
    } else {
        current.push_str(&text);
    }
}

/// A non-success status code ends the session; 0 means the field was absent.
fn status_error(response: &Response) -> Option<ProviderError> {
    let code = response.status_code;
    if code == 0 || code == STATUS_OK {
        return None;
    }
    Some(classify_vendor_code(&code.to_string(), STATUS_TABLE))
}
