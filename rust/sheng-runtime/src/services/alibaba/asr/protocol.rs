//! Inference protocol messages for streaming recognition (providers/alibaba.md “inference 协议”).

use serde::Deserialize;
use serde_json::{json, Value};
use sheng_core::{CaptionEvent, UtteranceId};

use crate::provider::ProviderError;
use crate::services::alibaba::errors;

/// Recognition parameters sent in `run-task`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSettings {
    pub model: String,
    pub sample_rate: u32,
    /// Session source language; `auto` sends no hint.
    pub source_language: String,
}

pub fn run_task(task_id: &str, settings: &TaskSettings) -> Value {
    let mut parameters = json!({
        "format": "pcm",
        "sample_rate": settings.sample_rate,
        "semantic_punctuation_enabled": false,
    });
    if settings.source_language != sheng_core::language::AUTO {
        parameters["language_hints"] = json!([settings.source_language]);
    }
    json!({
        "header": {"action": "run-task", "task_id": task_id, "streaming": "duplex"},
        "payload": {
            "task_group": "audio",
            "task": "asr",
            "function": "recognition",
            "model": settings.model,
            "parameters": parameters,
            "input": {},
        },
    })
}

pub fn finish_task(task_id: &str) -> Value {
    json!({
        "header": {"action": "finish-task", "task_id": task_id, "streaming": "duplex"},
        "payload": {"input": {}},
    })
}

#[derive(Debug, Deserialize)]
struct Envelope {
    header: Header,
    #[serde(default)]
    payload: Option<Payload>,
}

#[derive(Debug, Deserialize)]
struct Header {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Payload {
    #[serde(default)]
    output: Option<Output>,
}

#[derive(Debug, Deserialize)]
struct Output {
    #[serde(default)]
    sentence: Option<Sentence>,
}

#[derive(Debug, Deserialize)]
struct Sentence {
    #[serde(default)]
    sentence_id: Option<u64>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    sentence_end: bool,
    #[serde(default)]
    heartbeat: bool,
}

/// A server message of this task, interpreted.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerEvent {
    Started,
    Caption(CaptionEvent),
    Finished,
    Failed(ProviderError),
    /// Messages of other tasks, heartbeats, empty results and unknown events.
    Ignored,
}

/// Parses one text message. Messages that do not parse are ignored rather than failing
/// the session; a broken connection still surfaces as a transport error.
pub fn parse(text: &str, task_id: &str) -> ServerEvent {
    let Ok(envelope) = serde_json::from_str::<Envelope>(text) else {
        return ServerEvent::Ignored;
    };
    let header = envelope.header;
    if header.task_id.as_deref().is_some_and(|id| id != task_id) {
        return ServerEvent::Ignored;
    }
    match header.event.as_deref() {
        Some("task-started") => ServerEvent::Started,
        Some("task-finished") => ServerEvent::Finished,
        Some("task-failed") => ServerEvent::Failed(errors::classify(
            header.error_code.as_deref(),
            header.error_message.as_deref(),
        )),
        Some("result-generated") => envelope
            .payload
            .and_then(|payload| payload.output)
            .and_then(|output| output.sentence)
            .map_or(ServerEvent::Ignored, caption),
        _ => ServerEvent::Ignored,
    }
}

/// `sentence_end:false` → original draft, `true` → original final; empty text is ignored.
fn caption(sentence: Sentence) -> ServerEvent {
    let text = sentence.text.unwrap_or_default();
    if sentence.heartbeat || text.trim().is_empty() {
        return ServerEvent::Ignored;
    }
    let utterance = sentence.sentence_id.map(UtteranceId::Number);
    let event = if sentence.sentence_end {
        CaptionEvent::OriginalFinal { utterance, text }
    } else {
        CaptionEvent::OriginalDraft { utterance, text }
    };
    ServerEvent::Caption(event)
}
