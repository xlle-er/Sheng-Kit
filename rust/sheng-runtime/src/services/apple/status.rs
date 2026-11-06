//! Status strings of Apple resources (apple.md §1) and bridge failure names.

use crate::provider::SessionError;

/// Recognition asset status of one locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechStatus {
    Installed,
    /// Can be downloaded.
    Supported,
    Downloading,
    Unsupported,
    /// A system status the bridge does not know.
    Unknown,
}

impl SpeechStatus {
    pub fn from_name(name: &str) -> Self {
        match name {
            "installed" => Self::Installed,
            "supported" => Self::Supported,
            "downloading" => Self::Downloading,
            "unsupported" => Self::Unsupported,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Supported => "supported",
            Self::Downloading => "downloading",
            Self::Unsupported => "unsupported",
            Self::Unknown => "unknown",
        }
    }
}

/// Translation asset status of a language pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslationStatus {
    Installed,
    /// Can be downloaded.
    Supported,
    Unsupported,
    /// The Translation framework is not available on this system.
    Unavailable,
}

impl TranslationStatus {
    pub fn from_name(name: &str) -> Self {
        match name {
            "installed" => Self::Installed,
            "supported" => Self::Supported,
            "unsupported" => Self::Unsupported,
            _ => Self::Unavailable,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Supported => "supported",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Session code for a failure name reported by the recognizer bridge. Unknown names mean
/// the service could not run.
pub fn speech_failure(name: &str) -> SessionError {
    match name {
        "stream_closed" => SessionError::AppleSpeechStreamClosed,
        "timeout" => SessionError::AppleSpeechRuntimeTimeout,
        "result_backlog" => SessionError::AppleSpeechResultBacklog,
        _ => SessionError::AppleSpeechRuntimeServiceUnavailable,
    }
}
