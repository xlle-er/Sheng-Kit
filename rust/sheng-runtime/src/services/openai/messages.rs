//! Realtime event encoding and decoding shared by all three providers
//! (OpenAI Realtime client/server events and translation events references).

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};

use super::config::{LanguageField, SessionKind, SAMPLE_RATE};

/// `session.update` for the session kind. Translation sessions only accept the output
/// language, source transcription and noise reduction (format is fixed at 24 kHz PCM16).
pub fn session_update(kind: &SessionKind) -> String {
    let event = match kind {
        SessionKind::Transcribe {
            model,
            language,
            language_field,
        } => {
            let mut transcription = json!({ "model": model });
            if let Some(code) = language {
                let (key, value) = match language_field {
                    LanguageField::List => ("languages", json!([code])),
                    LanguageField::Single => ("language", json!(code)),
                };
                transcription[key] = value;
            }
            json!({
                "type": "session.update",
                "session": {
                    "type": "transcription",
                    "audio": { "input": {
                        "format": { "type": "audio/pcm", "rate": SAMPLE_RATE },
                        "transcription": transcription,
                        "turn_detection": null,
                    }},
                },
            })
        }
        SessionKind::Translate {
            source_transcript_model,
            output_language,
        } => json!({
            "type": "session.update",
            "session": { "audio": {
                "input": { "transcription": { "model": source_transcript_model } },
                "output": { "language": output_language },
            }},
        }),
    };
    event.to_string()
}

/// Audio append event; translation sessions use their own event name.
pub fn append_audio(kind: &SessionKind, pcm: &[u8]) -> String {
    let event_type = match kind {
        SessionKind::Transcribe { .. } => "input_audio_buffer.append",
        SessionKind::Translate { .. } => "session.input_audio_buffer.append",
    };
    json!({ "type": event_type, "audio": STANDARD.encode(pcm) }).to_string()
}

pub fn commit() -> String {
    json!({ "type": "input_audio_buffer.commit" }).to_string()
}

pub fn clear() -> String {
    json!({ "type": "input_audio_buffer.clear" }).to_string()
}

pub fn session_close() -> String {
    json!({ "type": "session.close" }).to_string()
}

/// Server error details; only the classification codes are kept, never the message.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ErrorInfo {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}

/// Server events this client acts on; everything else is `Other`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerEvent {
    SessionCreated,
    SessionUpdated,
    SessionClosed,
    Committed { item_id: String },
    TranscriptDelta { item_id: String, delta: String },
    TranscriptCompleted { item_id: String, transcript: String },
    TranscriptFailed { item_id: String },
    InputTranscriptDelta(String),
    OutputTranscriptDelta(String),
    Error(ErrorInfo),
    Other,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    item_id: Option<String>,
    #[serde(default)]
    delta: Option<String>,
    #[serde(default)]
    transcript: Option<String>,
    #[serde(default)]
    error: Option<Value>,
}

/// Decodes one text frame; `None` when it is not a Realtime event at all.
pub fn decode(text: &str) -> Option<ServerEvent> {
    let raw: Raw = serde_json::from_str(text).ok()?;
    let item = || raw.item_id.clone().unwrap_or_default();
    let text_of = |value: &Option<String>| value.clone().unwrap_or_default();
    let event = match raw.kind.as_str() {
        "session.created" | "transcription_session.created" => ServerEvent::SessionCreated,
        "session.updated" | "transcription_session.updated" => ServerEvent::SessionUpdated,
        "session.closed" => ServerEvent::SessionClosed,
        "input_audio_buffer.committed" => ServerEvent::Committed { item_id: item() },
        "conversation.item.input_audio_transcription.delta" => ServerEvent::TranscriptDelta {
            item_id: item(),
            delta: text_of(&raw.delta),
        },
        "conversation.item.input_audio_transcription.completed" => {
            ServerEvent::TranscriptCompleted {
                item_id: item(),
                transcript: text_of(&raw.transcript),
            }
        }
        "conversation.item.input_audio_transcription.failed" => {
            ServerEvent::TranscriptFailed { item_id: item() }
        }
        "session.input_transcript.delta" => ServerEvent::InputTranscriptDelta(text_of(&raw.delta)),
        "session.output_transcript.delta" => {
            ServerEvent::OutputTranscriptDelta(text_of(&raw.delta))
        }
        "error" => ServerEvent::Error(
            raw.error
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default(),
        ),
        _ => ServerEvent::Other,
    };
    Some(event)
}
