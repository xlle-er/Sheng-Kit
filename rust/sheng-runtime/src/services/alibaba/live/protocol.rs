//! Realtime live-translation messages (providers/alibaba.md “realtime 同传协议”).

use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use url::Url;

/// Live-translation model used when the mode is opened (current documented realtime model).
pub const LIVE_TRANSLATE_MODEL: &str = "qwen3.8-livetranslate-flash-realtime";
const DEFAULT_HOST: &str = "dashscope.aliyuncs.com";
const WORKSPACE_HOST_SUFFIX: &str = ".cn-beijing.maas.aliyuncs.com";

/// Realtime endpoint; a workspace id selects the workspace host. `None` when the workspace
/// id cannot form a host name.
pub fn realtime_url(workspace_id: &str, model: &str) -> Option<Url> {
    let workspace = workspace_id.trim();
    let host = if workspace.is_empty() {
        DEFAULT_HOST.to_owned()
    } else if workspace
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        format!("{}{WORKSPACE_HOST_SUFFIX}", workspace.to_ascii_lowercase())
    } else {
        return None;
    };
    let mut url = Url::parse(&format!("wss://{host}/api-ws/v1/realtime")).ok()?;
    url.query_pairs_mut().append_pair("model", model);
    Some(url)
}

/// Text-only output, 16 kHz PCM input, translation target.
pub fn session_update(target_language: &str, sample_rate: u32) -> Value {
    json!({
        "type": "session.update",
        "session": {
            "output_modalities": ["text"],
            "audio": {"input": {"format": {"type": "pcm", "sample_rate": sample_rate}}},
            "translation": {"language": target_language},
        },
    })
}

pub fn audio_append(pcm: &[u8]) -> Value {
    let audio = base64::engine::general_purpose::STANDARD.encode(pcm);
    json!({"type": "input_audio_buffer.append", "audio": audio})
}

pub fn session_finish() -> Value {
    json!({"type": "session.finish"})
}

#[derive(Debug, Default, Deserialize)]
struct Raw {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    item_id: Option<String>,
    #[serde(default)]
    previous_item_id: Option<String>,
    #[serde(default)]
    item: Option<RawItem>,
    #[serde(default)]
    delta: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    stash: Option<String>,
    #[serde(default)]
    transcript: Option<String>,
    #[serde(default)]
    response: Option<RawResponse>,
    #[serde(default)]
    error: Option<RawError>,
}

#[derive(Debug, Default, Deserialize)]
struct RawItem {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    role: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawResponse {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawError {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// One realtime server event, reduced to what captions need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveEvent {
    SessionCreated,
    SessionUpdated,
    /// Translation item `item` belongs to the transcription item `previous`.
    TranslationItem {
        item: String,
        previous: String,
    },
    /// Original text: `Append` adds a delta, `Replace` sets the whole current text.
    Original {
        item: String,
        text: TextUpdate,
    },
    OriginalDone {
        item: String,
        text: String,
    },
    Translation {
        item: String,
        text: TextUpdate,
    },
    TranslationDone {
        item: String,
        text: String,
    },
    ResponseFailed,
    /// `message` is only matched for timeout words, never passed on.
    Error {
        code: Option<String>,
        message: Option<String>,
    },
    Finished,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextUpdate {
    Append(String),
    Replace(String),
}

fn update(raw: &mut Raw) -> Option<TextUpdate> {
    if let Some(delta) = raw.delta.take() {
        return Some(TextUpdate::Append(delta));
    }
    let text = raw.text.take()?;
    let stash = raw.stash.take().unwrap_or_default();
    Some(TextUpdate::Replace(text + &stash))
}

pub fn parse(text: &str) -> LiveEvent {
    let Ok(mut raw) = serde_json::from_str::<Raw>(text) else {
        return LiveEvent::Other;
    };
    let item = raw.item_id.take();
    match (raw.kind.as_str(), item) {
        ("session.created", _) => LiveEvent::SessionCreated,
        ("session.updated", _) => LiveEvent::SessionUpdated,
        ("session.finished", _) => LiveEvent::Finished,
        ("error", _) => {
            let error = raw.error.unwrap_or_default();
            LiveEvent::Error {
                code: error.code,
                message: error.message,
            }
        }
        ("response.done", _) => match raw.response.and_then(|r| r.status).as_deref() {
            Some("failed") => LiveEvent::ResponseFailed,
            _ => LiveEvent::Other,
        },
        ("conversation.item.created", _) => item_created(raw),
        (
            "conversation.item.input_audio_transcription.delta"
            | "conversation.item.input_audio_transcription.text",
            Some(item),
        ) => update(&mut raw).map_or(LiveEvent::Other, |text| LiveEvent::Original { item, text }),
        ("conversation.item.input_audio_transcription.completed", Some(item)) => {
            LiveEvent::OriginalDone {
                item,
                text: raw.transcript.unwrap_or_default(),
            }
        }
        ("response.text.delta" | "response.text.text", Some(item)) => {
            update(&mut raw).map_or(LiveEvent::Other, |text| LiveEvent::Translation {
                item,
                text,
            })
        }
        ("response.text.done", Some(item)) => LiveEvent::TranslationDone {
            item,
            text: raw.text.unwrap_or_default(),
        },
        _ => LiveEvent::Other,
    }
}

/// Assistant items created with a `previous_item_id` are translations of that item.
fn item_created(raw: Raw) -> LiveEvent {
    let Some(item) = raw.item else {
        return LiveEvent::Other;
    };
    match (item.role.as_deref(), item.id, raw.previous_item_id) {
        (Some("assistant"), Some(item), Some(previous)) => {
            LiveEvent::TranslationItem { item, previous }
        }
        _ => LiveEvent::Other,
    }
}
