//! Per-source caption state machine (captions.md §1–§4, §6–§7).
//!
//! Service clients translate wire messages into [`CaptionEvent`]s. The reducer turns them into
//! the visible `tracks[]`, the `recent[]` list and the queue of confirmed captions.

mod draft;
mod final_;
mod pairing;
mod reducer;
mod track;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ids::{Source, UtteranceId};

pub use reducer::{CaptionReducer, ReducerConfig};

/// Confirmed captions kept in `recent[]`; the oldest is evicted first.
pub const RECENT_LIMIT: usize = 10;
/// Confirmed captions waiting for `drain_confirmed`.
pub const CONFIRMED_QUEUE_LIMIT: usize = 256;

/// Abstract caption events. Text fields always carry the full current text, never a delta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CaptionEvent {
    OriginalDraft {
        utterance: Option<UtteranceId>,
        text: String,
    },
    OriginalFinal {
        utterance: Option<UtteranceId>,
        text: String,
    },
    TranslationDraft {
        utterance: Option<UtteranceId>,
        text: String,
    },
    TranslationFinal {
        utterance: Option<UtteranceId>,
        text: String,
    },
    ConfirmedPair {
        utterance: Option<UtteranceId>,
        original: String,
        translation: String,
    },
    PreviewPair {
        utterance: Option<UtteranceId>,
        original: String,
        translation: String,
    },
    Retract {
        utterance: Option<UtteranceId>,
    },
    Order {
        utterance: UtteranceId,
        sequence: i64,
    },
    Reset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Caption {
    pub id: String,
    pub source: Source,
    pub original: String,
    pub translation: String,
    pub timestamp_ms: u64,
    pub source_language: String,
    pub target_language: String,
}

/// One entry of the snapshot `tracks[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackView {
    pub source: Source,
    pub original: String,
    pub translation: String,
    pub pending_original: String,
    pub is_final: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CaptionError {
    #[error("caller is not draining confirmed captions fast enough")]
    ConsumerBackpressure,
}

impl CaptionError {
    /// Session failure code from snapshot.md.
    pub fn code(self) -> &'static str {
        match self {
            Self::ConsumerBackpressure => "caption_consumer_backpressure",
        }
    }
}

/// An original/translation pair that passed dedupe and becomes one caption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Confirmed {
    pub original: String,
    pub translation: String,
}
