//! Wire format of the Baidu real-time speech translation WebSocket API
//! (ai.baidu.com MT/Sl9p2h5k9): START/FINISH frames, server messages and error codes.

use serde::Deserialize;
use serde_json::json;
use sheng_core::{CaptionEvent, UtteranceId};

use crate::config::secret::Secret;
use crate::provider::{classify_vendor_code, ProviderError};

/// Recommended audio packet length (“建议每40ms发送一次音频包”).
pub const FRAME_MILLIS: usize = 40;
/// Bytes per 16-bit mono sample.
const BYTES_PER_SAMPLE: usize = 2;

/// Error codes after which the service keeps working (“这一句报错，不影响后续…”).
const RECOVERABLE_CODES: &[i64] = &[20311, 20312, 20313, 20315];

/// Error code classification (providers/baidu.md “错误”); unlisted codes are provider errors.
const ERROR_TABLE: &[(&str, ProviderError)] = &[
    ("31003", ProviderError::Authentication),
    // No usable duration left on the account: the user must act (step 4 answer 4h-7).
    ("41017", ProviderError::Authentication),
    ("10001", ProviderError::Configuration),
    ("20302", ProviderError::Configuration),
    ("31004", ProviderError::Configuration),
    ("31005", ProviderError::Configuration),
];

/// Everything the START frame needs. Credentials stay secret until serialized.
#[derive(Debug, Clone)]
pub struct StartParams {
    pub from: &'static str,
    pub to: &'static str,
    pub app_id: Secret,
    pub app_key: Secret,
    pub sampling_rate: u32,
}

/// START frame text. It carries the credentials and must never be logged.
pub fn start_frame(params: &StartParams) -> String {
    json!({
        "type": "START",
        "from": params.from,
        "to": params.to,
        "app_id": params.app_id.expose(),
        "app_key": params.app_key.expose(),
        "sampling_rate": params.sampling_rate,
        "return_target_tts": false,
    })
    .to_string()
}

/// FINISH frame text.
pub fn finish_frame() -> String {
    json!({"type": "FINISH"}).to_string()
}

/// Bytes of one recommended audio packet at `sampling_rate`.
pub fn frame_bytes(sampling_rate: u32) -> usize {
    let samples = sampling_rate as usize * FRAME_MILLIS / 1000;
    (samples * BYTES_PER_SAMPLE).max(BYTES_PER_SAMPLE)
}

/// One sentence result (`data.result`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Segment {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub asr: String,
    #[serde(default)]
    pub asr_trans: String,
    #[serde(default)]
    pub sentence: String,
    #[serde(default)]
    pub sentence_trans: String,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    code: i64,
    #[serde(default)]
    data: Option<Data>,
}

#[derive(Debug, Deserialize)]
struct Data {
    #[serde(default)]
    status: String,
    #[serde(default)]
    result: Option<Segment>,
}

/// A text message from the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMessage {
    /// START accepted (`STA`).
    Started,
    /// Recognition/translation result (`TRN`).
    Result(Segment),
    /// Session finished (`END`).
    Ended,
    /// Non-zero `code`.
    Error(i64),
    /// Anything else (unknown status); ignored.
    Other,
}

/// Parses a text message; `None` when it is not the documented JSON shape.
pub fn parse_message(text: &str) -> Option<ServerMessage> {
    let envelope: Envelope = serde_json::from_str(text).ok()?;
    if envelope.code != 0 {
        return Some(ServerMessage::Error(envelope.code));
    }
    let Some(data) = envelope.data else {
        return Some(ServerMessage::Other);
    };
    Some(match (data.status.as_str(), data.result) {
        ("STA", _) => ServerMessage::Started,
        ("END", _) => ServerMessage::Ended,
        ("TRN", Some(segment)) => ServerMessage::Result(segment),
        _ => ServerMessage::Other,
    })
}

/// Whether the service keeps the session alive after this error code.
pub fn is_recoverable(code: i64) -> bool {
    RECOVERABLE_CODES.contains(&code)
}

/// Normalized error for a fatal service error code.
pub fn classify_error(code: i64) -> ProviderError {
    classify_vendor_code(&code.to_string(), ERROR_TABLE)
}

/// Assigns local utterance numbers: the service has no sentence ids, so each `FIN`
/// sentence closes the current number and anything after it belongs to the next one.
#[derive(Debug, Clone)]
pub struct Utterances {
    current: u64,
}

impl Default for Utterances {
    fn default() -> Self {
        Self { current: 1 }
    }
}

impl Utterances {
    /// Caption events for one result. A `MID` with an interim translation (`asr_trans`) is
    /// the sentence's preview pair (questions.md step 4 answer 4h-4); without one it is a
    /// draft. A `FIN` may also carry the next sentence's draft in `asr`; that draft is
    /// reported after the final, under the next number.
    pub fn map(&mut self, segment: Segment) -> Vec<CaptionEvent> {
        let mut events = Vec::new();
        if segment.kind == "FIN" && !segment.sentence.trim().is_empty() {
            events.push(self.final_event(segment.sentence, segment.sentence_trans));
            self.current += 1;
        }
        if !matches!(segment.kind.as_str(), "MID" | "FIN") || segment.asr.trim().is_empty() {
            return events;
        }
        let utterance = Some(self.id());
        if segment.kind == "MID" && !segment.asr_trans.trim().is_empty() {
            events.push(CaptionEvent::PreviewPair {
                utterance,
                original: segment.asr,
                translation: segment.asr_trans,
            });
        } else {
            events.push(CaptionEvent::OriginalDraft {
                utterance,
                text: segment.asr,
            });
        }
        events
    }

    fn final_event(&self, original: String, translation: String) -> CaptionEvent {
        let utterance = Some(self.id());
        if translation.trim().is_empty() {
            return CaptionEvent::OriginalFinal {
                utterance,
                text: original,
            };
        }
        CaptionEvent::ConfirmedPair {
            utterance,
            original,
            translation,
        }
    }

    fn id(&self) -> UtteranceId {
        UtteranceId::Number(self.current)
    }
}
