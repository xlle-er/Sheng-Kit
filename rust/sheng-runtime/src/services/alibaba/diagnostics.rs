//! Why the service ended a connection: WebSocket close code and reason, and the type, code,
//! message and parameter of the last error event. Never holds audio, captions or credentials;
//! texts are cut to a short length.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;

/// Longest close reason or error message kept, in characters.
const MAX_TEXT_CHARS: usize = 160;

/// Failure facts reported by the service; all fields are absent until it reports something.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FailureDetail {
    pub close_code: Option<u16>,
    pub close_reason: Option<String>,
    /// Server event that carried the error (`error`, `task-failed`, …).
    pub event: Option<String>,
    pub error_type: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub error_param: Option<String>,
}

impl FailureDetail {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

impl fmt::Display for FailureDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fields = [
            ("close_code", self.close_code.map(|c| c.to_string())),
            ("close_reason", self.close_reason.clone()),
            ("event", self.event.clone()),
            ("error_type", self.error_type.clone()),
            ("error_code", self.error_code.clone()),
            ("error_message", self.error_message.clone()),
            ("error_param", self.error_param.clone()),
        ];
        let shown: Vec<String> = fields
            .into_iter()
            .filter_map(|(name, value)| value.map(|v| format!("{name}={v:?}")))
            .collect();
        if shown.is_empty() {
            return f.write_str("no close frame or error event received");
        }
        f.write_str(&shown.join(" "))
    }
}

/// Shared slot the handshake and the reader task write into and the client reads.
#[derive(Debug, Clone, Default)]
pub struct FailureNote(Arc<Mutex<FailureDetail>>);

impl FailureNote {
    pub fn detail(&self) -> FailureDetail {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn clear(&self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = FailureDetail::default();
    }

    fn update(&self, change: impl FnOnce(&mut FailureDetail)) {
        change(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// Records the close frame (code and reason) or a close without one.
    pub fn closed(&self, frame: Option<&CloseFrame>) {
        self.update(|detail| match frame {
            Some(frame) => {
                detail.close_code = Some(u16::from(frame.code));
                detail.close_reason = Some(short(frame.reason.as_str())).filter(|r| !r.is_empty());
            }
            None => detail.close_code = detail.close_code.or(Some(NO_STATUS)),
        });
    }

    /// Records the error fields of a server message, if it carries any.
    pub fn message(&self, text: &str) {
        if !text.contains("error") && !text.contains("failed") {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return;
        };
        if let Some(fields) = error_fields(&value) {
            self.update(|detail| {
                *detail = FailureDetail {
                    close_code: detail.close_code,
                    close_reason: detail.close_reason.clone(),
                    ..fields
                }
            });
        }
    }
}

/// Close code reported when the peer closed without a status (RFC 6455 §7.1.5).
const NO_STATUS: u16 = 1005;

fn short(text: &str) -> String {
    text.chars().take(MAX_TEXT_CHARS).collect()
}

fn field(value: &Value, key: &str) -> Option<String> {
    match &value[key] {
        Value::String(text) => Some(short(text)),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Realtime events carry `error:{type,code,message,param}`; inference events carry
/// `header.error_code` / `header.error_message`.
fn error_fields(value: &Value) -> Option<FailureDetail> {
    let header = &value["header"];
    let event = field(value, "type").or_else(|| field(header, "event"));
    let error = &value["error"];
    let detail = if error.is_object() {
        FailureDetail {
            event,
            error_type: field(error, "type"),
            error_code: field(error, "code"),
            error_message: field(error, "message"),
            error_param: field(error, "param"),
            ..FailureDetail::default()
        }
    } else if header["error_code"].is_string() || header["error_message"].is_string() {
        FailureDetail {
            event,
            error_code: field(header, "error_code"),
            error_message: field(header, "error_message"),
            ..FailureDetail::default()
        }
    } else if field(value, "type").as_deref() == Some("response.done")
        && field(&value["response"], "status").as_deref() == Some("failed")
    {
        let error = &value["response"]["status_details"]["error"];
        FailureDetail {
            event,
            error_type: field(error, "type"),
            error_code: field(error, "code"),
            error_message: field(error, "message"),
            ..FailureDetail::default()
        }
    } else {
        return None;
    };
    Some(detail)
}
