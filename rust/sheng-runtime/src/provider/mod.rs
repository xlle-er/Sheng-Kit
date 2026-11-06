//! Provider abstractions: speech client and text translator traits, unified events and
//! error normalization. Real service clients are added in a later step.

pub mod error;
#[cfg(any(test, feature = "test-support"))]
pub mod scripted;
pub mod traits;

pub use error::{
    classify_vendor_code, mentions_timeout, translation_session_error, ProviderError, SessionError,
};
pub use traits::{
    BoxFuture, EventSink, OverflowFlag, ProviderEvent, SpeechClient, StreamText, TextTranslator,
    TranslationRequest,
};

/// Fixed xAI chat completions endpoint for `textCredentials.kind:"xAI"` (providers/xai.md).
pub const XAI_CHAT_COMPLETIONS: &str = "https://api.x.ai/v1/chat/completions";
