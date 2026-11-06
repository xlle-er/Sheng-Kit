//! Server error classification (providers/openai.md “错误映射”).

use super::messages::ErrorInfo;
use crate::provider::{classify_vendor_code, mentions_timeout, ProviderError};

/// Code or type → normalized error. Unknown codes are generic provider errors.
const ERROR_TABLE: &[(&str, ProviderError)] = &[
    ("invalid_api_key", ProviderError::Authentication),
    ("authentication_error", ProviderError::Authentication),
    ("authorization_error", ProviderError::Authentication),
    ("unauthorized", ProviderError::Authentication),
    ("forbidden", ProviderError::Authentication),
    ("permission_denied", ProviderError::Authentication),
    ("model_not_found", ProviderError::Configuration),
    ("insufficient_quota", ProviderError::Configuration),
    ("billing_hard_limit_reached", ProviderError::Configuration),
    ("session_expired", ProviderError::Transport),
];

/// Committing an empty (or too short) buffer is harmless: nothing was waiting for it.
pub const COMMIT_EMPTY: &str = "input_audio_buffer_commit_empty";

/// Classifies by `code`, falling back to the error `type` when the code is unknown.
pub fn classify(info: &ErrorInfo) -> ProviderError {
    [info.code.as_deref(), info.kind.as_deref()]
        .into_iter()
        .flatten()
        .map(|code| classify_vendor_code(code, ERROR_TABLE))
        .find(|error| *error != ProviderError::Provider)
        .unwrap_or_else(|| timeout_or_provider(info))
}

/// Unknown codes that name a timeout are retryable (providers/README.md “意外断开”).
fn timeout_or_provider(info: &ErrorInfo) -> ProviderError {
    let is_timeout = [info.code.as_deref(), info.kind.as_deref()]
        .into_iter()
        .flatten()
        .any(mentions_timeout);
    if is_timeout {
        ProviderError::Transport
    } else {
        ProviderError::Provider
    }
}

pub fn is_commit_empty(info: &ErrorInfo) -> bool {
    info.code.as_deref() == Some(COMMIT_EMPTY)
}
