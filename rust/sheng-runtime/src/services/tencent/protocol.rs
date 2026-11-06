//! Server messages of the speech translation WebSocket and their caption events
//! (providers/tencent.md). Service `message` texts are never read or passed on.

use serde::Deserialize;
use sheng_core::{CaptionEvent, UtteranceId};

use crate::provider::{classify_vendor_code, ProviderError};

/// Documented error codes with a specific meaning (providers/tencent.md, questions.md step 4
/// answer 4g-8); every other non-zero code is a generic provider error.
const ERROR_CODES: &[(&str, ProviderError)] = &[
    ("6001", ProviderError::Configuration),
    ("6002", ProviderError::Authentication),
    // Service not activated, resource package used up, account in arrears.
    ("6003", ProviderError::Authentication),
    ("6004", ProviderError::Authentication),
    ("6005", ProviderError::Authentication),
    // Concurrency limit reached.
    ("6006", ProviderError::Provider),
    // Connection-level failures.
    ("6009", ProviderError::Transport),
    ("6012", ProviderError::Transport),
    ("6013", ProviderError::Transport),
    ("7000", ProviderError::Transport),
];

/// `final` value that ends the translation of the audio stream.
const FINAL_TRANSLATION_DONE: i64 = 1;

#[derive(Deserialize)]
struct Wire {
    #[serde(default)]
    code: i64,
    sentence_id: Option<String>,
    result: Option<WireResult>,
    #[serde(rename = "final")]
    final_flag: Option<i64>,
}

#[derive(Deserialize)]
struct WireResult {
    sentence_id: Option<String>,
    #[serde(default)]
    source_text: String,
    #[serde(default)]
    target_text: String,
    #[serde(default)]
    sentence_end: bool,
}

/// One sentence result: a draft until `is_end`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentence {
    pub utterance: Option<UtteranceId>,
    pub original: String,
    pub translation: String,
    pub is_end: bool,
}

/// What one text message means.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Parsed {
    pub sentence: Option<Sentence>,
    /// The service finished translating all audio (`final:1`).
    pub is_final: bool,
}

/// Maps a non-zero `code` to a normalized error.
pub fn classify_code(code: i64) -> ProviderError {
    classify_vendor_code(&code.to_string(), ERROR_CODES)
}

/// Parses one text message. A non-zero `code` or malformed JSON is an error.
pub fn parse(text: &str) -> Result<Parsed, ProviderError> {
    let wire: Wire = serde_json::from_str(text).map_err(|_| ProviderError::Provider)?;
    if wire.code != 0 {
        return Err(classify_code(wire.code));
    }
    let top_level_id = wire.sentence_id;
    let sentence = wire.result.map(|result| Sentence {
        // The documented examples put `sentence_id` next to `result`; accept either place.
        utterance: top_level_id
            .or(result.sentence_id)
            .filter(|id| !id.is_empty())
            .map(UtteranceId::Text),
        original: result.source_text,
        translation: result.target_text,
        is_end: result.sentence_end,
    });
    Ok(Parsed {
        sentence,
        is_final: wire.final_flag == Some(FINAL_TRANSLATION_DONE),
    })
}

/// `sentence_end:false` → original and translation drafts; `true` → a confirmed pair.
pub fn caption_events(sentence: Sentence) -> Vec<CaptionEvent> {
    let Sentence {
        utterance,
        original,
        translation,
        is_end,
    } = sentence;
    if is_end {
        return vec![CaptionEvent::ConfirmedPair {
            utterance,
            original,
            translation,
        }];
    }
    let mut events = vec![CaptionEvent::OriginalDraft {
        utterance: utterance.clone(),
        text: original,
    }];
    if !translation.trim().is_empty() {
        events.push(CaptionEvent::TranslationDraft {
            utterance,
            text: translation,
        });
    }
    events
}
