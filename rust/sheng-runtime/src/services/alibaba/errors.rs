//! DashScope error codes mapped to normalized failures (providers/alibaba.md).

use crate::provider::{classify_vendor_code, mentions_timeout, ProviderError};

const RATE_LIMITED: ProviderError = ProviderError::RateLimited { retry_after: None };

const TABLE: &[(&str, ProviderError)] = &[
    ("InvalidApiKey", ProviderError::Authentication),
    ("Unauthorized", ProviderError::Authentication),
    ("authentication.*", ProviderError::Authentication),
    ("Throttling", RATE_LIMITED),
    ("Throttling.*", RATE_LIMITED),
    ("TOO_MANY_REQUESTS", RATE_LIMITED),
    ("RequestTimeout", ProviderError::Transport),
    ("UNSUPPORTED_LANGUAGE", ProviderError::Configuration),
    ("CLIENT_ERROR", ProviderError::Configuration),
];

/// Maps a `task-failed` or realtime `error` code. Codes without a specific meaning whose
/// code or description say the session timed out are retryable transport failures; other
/// unknown or missing codes are provider errors. The description is only matched, never kept.
pub fn classify(code: Option<&str>, message: Option<&str>) -> ProviderError {
    let mapped = code.map_or(ProviderError::Provider, |code| {
        classify_vendor_code(code, TABLE)
    });
    if mapped != ProviderError::Provider {
        return mapped;
    }
    if [code, message].into_iter().flatten().any(mentions_timeout) {
        return ProviderError::Transport;
    }
    ProviderError::Provider
}
