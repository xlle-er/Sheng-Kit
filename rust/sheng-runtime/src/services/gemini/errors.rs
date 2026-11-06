//! Error classification for the Live API (providers/gemini.md “错误映射”).
//!
//! Only the status keyword is looked at; the service's message text is never kept.

use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;

use crate::provider::ProviderError;

/// Status keywords in the order they are searched for.
const STATUS_TABLE: &[(&str, ProviderError)] = &[
    ("unauthenticated", ProviderError::Authentication),
    ("permission_denied", ProviderError::Authentication),
    ("invalid_argument", ProviderError::Configuration),
    ("not_found", ProviderError::Configuration),
    ("failed_precondition", ProviderError::Configuration),
    ("unimplemented", ProviderError::Configuration),
];

/// Lowercases and joins words with `_`, so `PERMISSION_DENIED`, `permission denied` and
/// `Permission-Denied` all read the same.
fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            ' ' | '-' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

/// Maps a status name or a free-text reason that contains one; anything else is a
/// generic provider error.
pub fn classify_status(text: &str) -> ProviderError {
    let normalized = normalize(text);
    STATUS_TABLE
        .iter()
        .find(|(status, _)| normalized.contains(status))
        .map_or(ProviderError::Provider, |(_, error)| *error)
}

/// How a close frame ends the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    /// Ordinary close without an error: the service is done.
    Normal,
    Failed(ProviderError),
}

/// Classifies a close frame. Code 1000 is normal unless its reason names an error status;
/// other codes keep a credential or configuration status named in their reason and are
/// otherwise transport failures.
pub fn classify_close(frame: Option<&CloseFrame>) -> CloseOutcome {
    let Some(frame) = frame else {
        return CloseOutcome::Normal;
    };
    let reason = frame.reason.as_str();
    let status = classify_status(reason);
    if frame.code == CloseCode::Normal {
        return match status {
            ProviderError::Provider => CloseOutcome::Normal,
            error => CloseOutcome::Failed(error),
        };
    }
    // Any other close is an unexpected end of the connection and can be retried
    // (providers/README.md “意外断开”).
    match status {
        ProviderError::Provider => CloseOutcome::Failed(ProviderError::Transport),
        error => CloseOutcome::Failed(error),
    }
}
