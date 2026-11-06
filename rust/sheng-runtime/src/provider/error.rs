//! Session failure codes (snapshot.md) and normalization of service errors.
//! Errors never carry service text, credentials or transcripts.

use std::time::Duration;

use sheng_core::policy::TranslationFailure;
use thiserror::Error;

/// Session failure codes reported in the snapshot `error` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionError {
    ProviderConfigurationFailed,
    TransportError,
    CredentialAuthenticationFailed,
    InvalidConfiguration,
    ProviderError,
    ProviderEventBacklogOverflow,
    TranslationBacklogOverflow,
    TranslationRateLimited,
    TranslationTemporarilyUnavailable,
    AppleSpeechRuntimeServiceUnavailable,
    AppleSpeechRuntimeTimeout,
    AppleSpeechStreamClosed,
    AppleSpeechResultBacklog,
    AudioBackpressure,
    EchoProcessingFailed,
    CaptionConsumerBackpressure,
}

impl SessionError {
    pub fn code(self) -> &'static str {
        match self {
            Self::ProviderConfigurationFailed => "provider_configuration_failed",
            Self::TransportError => "transport_error",
            Self::CredentialAuthenticationFailed => "credential_authentication_failed",
            Self::InvalidConfiguration => "invalid_configuration",
            Self::ProviderError => "provider_error",
            Self::ProviderEventBacklogOverflow => "provider_event_backlog_overflow",
            Self::TranslationBacklogOverflow => "translation_backlog_overflow",
            Self::TranslationRateLimited => "translation_rate_limited",
            Self::TranslationTemporarilyUnavailable => "translation_temporarily_unavailable",
            Self::AppleSpeechRuntimeServiceUnavailable => {
                "apple_speech_runtime_service_unavailable"
            }
            Self::AppleSpeechRuntimeTimeout => "apple_speech_runtime_timeout",
            Self::AppleSpeechStreamClosed => "apple_speech_stream_closed",
            Self::AppleSpeechResultBacklog => "apple_speech_result_backlog",
            Self::AudioBackpressure => "audio_backpressure",
            Self::EchoProcessingFailed => "echo_processing_failed",
            Self::CaptionConsumerBackpressure => "caption_consumer_backpressure",
        }
    }
}

/// Normalized service failure, independent of the vendor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProviderError {
    #[error("service rejected the credentials")]
    Authentication,
    #[error("service rejected the configuration")]
    Configuration,
    #[error("service is rate limiting")]
    RateLimited { retry_after: Option<Duration> },
    #[error("service is temporarily unavailable")]
    Unavailable { retry_after: Option<Duration> },
    #[error("connection failed, closed or timed out")]
    Transport,
    #[error("service reported an error")]
    Provider,
    /// On-device service failure that already has its exact session code (Apple, step 4a).
    #[error("on-device service failed")]
    Local(SessionError),
}

impl ProviderError {
    /// Classifies an HTTP status (handshake or text request), providers/README.md.
    pub fn from_status(status: u16, retry_after: Option<Duration>) -> Self {
        match status {
            401 | 403 => Self::Authentication,
            400 | 404 | 422 => Self::Configuration,
            429 => Self::RateLimited { retry_after },
            500..=599 => Self::Unavailable { retry_after },
            _ => Self::Provider,
        }
    }

    /// Session code when a speech service fails. Rate limiting of the speech service itself
    /// and 5xx during the handshake are transport problems for the session.
    pub fn speech_session_error(self) -> SessionError {
        match self {
            Self::Authentication => SessionError::CredentialAuthenticationFailed,
            Self::Configuration => SessionError::InvalidConfiguration,
            Self::RateLimited { .. } | Self::Unavailable { .. } | Self::Transport => {
                SessionError::TransportError
            }
            Self::Provider => SessionError::ProviderError,
            Self::Local(code) => code,
        }
    }

    /// Text translation view of the failure (translation-pipeline.md §1–§2).
    pub fn translation_failure(self) -> TranslationFailure {
        match self {
            Self::Authentication => TranslationFailure::Authentication,
            Self::Configuration => TranslationFailure::Configuration,
            Self::RateLimited { .. } => TranslationFailure::RateLimited,
            Self::Unavailable { .. } | Self::Transport | Self::Provider | Self::Local(_) => {
                TranslationFailure::TemporarilyUnavailable
            }
        }
    }

    pub fn retry_after(self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after } | Self::Unavailable { retry_after } => retry_after,
            _ => None,
        }
    }
}

/// Session code for a text translation failure that ends the session.
pub fn translation_session_error(failure: TranslationFailure) -> SessionError {
    match failure {
        TranslationFailure::RateLimited => SessionError::TranslationRateLimited,
        TranslationFailure::TemporarilyUnavailable => {
            SessionError::TranslationTemporarilyUnavailable
        }
        TranslationFailure::Authentication => SessionError::CredentialAuthenticationFailed,
        TranslationFailure::Configuration => SessionError::InvalidConfiguration,
    }
}

/// Maps a vendor error code through a table of `(code or prefix*, error)`; unknown codes
/// are generic provider errors. Prefix entries end with `*`.
pub fn classify_vendor_code(code: &str, table: &[(&str, ProviderError)]) -> ProviderError {
    table
        .iter()
        .find(|(pattern, _)| match pattern.strip_suffix('*') {
            Some(prefix) => code.starts_with(prefix),
            None => code.eq_ignore_ascii_case(pattern),
        })
        .map_or(ProviderError::Provider, |(_, error)| *error)
}

/// Words in an error code or description that mean the session timed out: silence, idle,
/// no usable audio or the session length limit. Such errors are retryable transport
/// failures (providers/README.md “意外断开”). The text is only matched, never kept.
const TIMEOUT_HINTS: &[&str] = &[
    "timeout",
    "timed out",
    "idle",
    "silence",
    "silent",
    "no valid audio",
    "novalidaudio",
    "max duration",
    "max_duration",
    "session duration",
    "duration limit",
    "session_expired",
];

pub fn mentions_timeout(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    TIMEOUT_HINTS.iter().any(|hint| text.contains(hint))
}
