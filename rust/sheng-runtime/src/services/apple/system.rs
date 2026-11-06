//! What the Apple service needs from the system, as traits: the Swift bridge implements
//! them on macOS, tests use fakes. Nothing here downloads unless a `prepare_*` is called.

use std::sync::Arc;

use super::status::{SpeechStatus, TranslationStatus};
use crate::provider::BoxFuture;

/// Recognition locales of this machine.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpeechInventory {
    /// `SpeechTranscriber` can run here (macOS 26 or later).
    pub available: bool,
    /// BCP 47 identifier and asset status of each supported locale.
    pub locales: Vec<(String, SpeechStatus)>,
}

impl SpeechInventory {
    pub fn identifiers(&self) -> Vec<String> {
        self.locales.iter().map(|(id, _)| id.clone()).collect()
    }

    pub fn status(&self, identifier: &str) -> SpeechStatus {
        self.locales
            .iter()
            .find(|(id, _)| id == identifier)
            .map_or(SpeechStatus::Unsupported, |(_, status)| *status)
    }
}

/// Why a translation request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslateFailure {
    /// No Translation framework on this system.
    Unavailable,
    Unsupported,
    /// The pair is supported but its assets are not installed.
    AssetsMissing,
    /// The request itself failed or timed out.
    Failed,
}

impl TranslateFailure {
    pub fn from_name(name: &str) -> Self {
        match name {
            "unavailable" => Self::Unavailable,
            "unsupported" => Self::Unsupported,
            "assets_missing" => Self::AssetsMissing,
            _ => Self::Failed,
        }
    }
}

/// Resource queries and one-shot work. Methods resolve to `None` when the bridge did not
/// answer (not macOS, or no reply in time).
pub trait AppleSystem: Send + Sync {
    fn speech_inventory(&self) -> BoxFuture<'_, Option<SpeechInventory>>;
    /// Downloads the recognizer for `locale`; true once installed.
    fn prepare_speech(&self, locale: String) -> BoxFuture<'_, bool>;
    /// Maximal identifiers of the translation languages; `None` without the framework.
    fn translation_languages(&self) -> BoxFuture<'_, Option<Vec<String>>>;
    fn translation_status(
        &self,
        source: String,
        target: String,
    ) -> BoxFuture<'_, TranslationStatus>;
    /// Downloads the translation pair; true once installed.
    fn prepare_translation(&self, source: String, target: String) -> BoxFuture<'_, bool>;
    fn translate(
        &self,
        text: String,
        source: String,
        target: String,
    ) -> BoxFuture<'_, Result<String, TranslateFailure>>;
}

/// Result of handing one PCM chunk to the recognizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
    Accepted,
    Closed,
    Full,
}

impl Push {
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Accepted,
            -2 => Self::Full,
            _ => Self::Closed,
        }
    }
}

/// Recognition sessions keyed by the token from `events::register`. Events for a token go
/// to `events::deliver` until `close`.
pub trait Recognizer: Send + Sync {
    /// Starts a session; false when recognition is unavailable (no events follow).
    fn open(&self, locale: &str, token: u64) -> bool;
    fn push(&self, token: u64, pcm: &[u8]) -> Push;
    fn finish(&self, token: u64);
    fn alive(&self, token: u64) -> bool;
    fn close(&self, token: u64);
}

/// The system services used by real sessions and resource operations.
#[derive(Clone)]
pub struct Apple {
    pub system: Arc<dyn AppleSystem>,
    pub recognizer: Arc<dyn Recognizer>,
}
