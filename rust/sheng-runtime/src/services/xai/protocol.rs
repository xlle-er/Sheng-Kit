//! What the two xAI WebSocket routes have in common: the per-route protocol interface the
//! connection drives, error classification and joining of transcript pieces.

use std::time::Duration;

use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

use crate::provider::{classify_vendor_code, mentions_timeout, ProviderError, ProviderEvent};

/// Result of one text message from the service.
#[derive(Debug, Default, PartialEq)]
pub struct Inbound {
    pub events: Vec<ProviderEvent>,
    /// The service is ready for audio.
    pub ready: bool,
    /// A turn ended; ends the session when input is already finished.
    pub turn_end: bool,
    /// The service finished everything and will close the connection.
    pub done: bool,
    pub failure: Option<ProviderError>,
}

impl Inbound {
    pub fn events(events: Vec<ProviderEvent>) -> Self {
        Self {
            events,
            ..Self::default()
        }
    }

    pub fn failure(error: ProviderError) -> Self {
        Self {
            failure: Some(error),
            ..Self::default()
        }
    }
}

/// One route's wire protocol. The connection owns the socket; the protocol only turns
/// messages into events and builds what is sent.
pub trait Protocol: Send + 'static {
    /// Text messages sent right after the handshake.
    fn setup(&self) -> Vec<String>;
    /// One audio chunk as a WebSocket message.
    fn audio(pcm: &[u8]) -> Message
    where
        Self: Sized;
    /// Message that ends input, if the route has one.
    fn finish(&self) -> Option<String>;
    /// How long the last results are awaited after `finish`.
    fn finish_wait(&self) -> Duration;
    /// Handles a text message. Malformed messages are protocol failures.
    fn on_text(&mut self, text: &str) -> Inbound;
    /// Events for whatever is still unconfirmed when the session ends.
    fn flush(&mut self) -> Vec<ProviderEvent>;
}

/// Vendor error codes (providers/xai.md “错误”); everything else is a provider error.
const ERROR_CODES: &[(&str, ProviderError)] = &[
    ("invalid_api_key", ProviderError::Authentication),
    ("authentication_error", ProviderError::Authentication),
    ("unauthorized", ProviderError::Authentication),
    ("timeout", ProviderError::Transport),
    ("max_duration", ProviderError::Transport),
];

/// Classifies an `error` message. The realtime route nests `code`/`type` in `error`;
/// the STT route documents only `message`, which is never read.
pub fn classify_error(message: &Value) -> ProviderError {
    let nested = message.get("error");
    let code = [
        nested.and_then(|e| e.get("code")),
        nested.and_then(|e| e.get("type")),
    ]
    .into_iter()
    .chain([message.get("code")])
    .flatten()
    .filter_map(Value::as_str)
    .map(|code| match classify_vendor_code(code, ERROR_CODES) {
        // Unknown codes that name a timeout are retryable (providers/README.md).
        ProviderError::Provider if mentions_timeout(code) => ProviderError::Transport,
        error => error,
    })
    .find(|error| *error != ProviderError::Provider);
    code.unwrap_or(ProviderError::Provider)
}

/// Parses a JSON text message and returns it with its `type`.
pub fn parse(text: &str) -> Option<(Value, String)> {
    let value: Value = serde_json::from_str(text).ok()?;
    let kind = value.get("type")?.as_str()?.to_owned();
    Some((value, kind))
}

/// Reads a string field; missing fields are empty.
pub fn text_field<'a>(value: &'a Value, field: &str) -> &'a str {
    value.get(field).and_then(Value::as_str).unwrap_or_default()
}

fn is_spaced_script(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | '\'' | '"')
}

/// Separator between two consecutive pieces of one transcript. Pieces arrive without the
/// space between them; scripts written without spaces (CJK) get none.
pub fn separator(previous: &str, next: &str) -> &'static str {
    match (previous.chars().next_back(), next.chars().next()) {
        (Some(a), Some(b)) if is_spaced_script(a) && b.is_ascii_alphanumeric() => " ",
        _ => "",
    }
}

/// `previous` and `next` joined with the separator between them.
pub fn join(previous: &str, next: &str) -> String {
    format!("{previous}{}{next}", separator(previous, next))
}

#[cfg(test)]
mod tests {
    use super::join;

    #[test]
    fn pieces_join_with_a_space_only_between_spaced_scripts() {
        assert_eq!(join("Hello.", "Bye."), "Hello. Bye.");
        assert_eq!(join("こんにちは。", "元気"), "こんにちは。元気");
        assert_eq!(join("你好", "world"), "你好world");
        assert_eq!(join("Hello ", "world"), "Hello world");
        assert_eq!(join("", "world"), "world");
    }
}
