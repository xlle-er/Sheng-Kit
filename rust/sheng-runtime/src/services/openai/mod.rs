//! OpenAI Realtime, Azure OpenAI Realtime and custom OpenAI-compatible ASR (step 4c,
//! providers/openai.md). All three share one event codec; authentication and URLs differ.

mod client;
pub mod config;
pub mod errors;
pub mod messages;
mod reader;
pub mod transcript;
pub mod turns;

use sheng_core::Source;

use crate::config::SessionPlan;
use crate::provider::{SessionError, SpeechClient};

pub use client::{RealtimeClient, SESSION_EVENT_WAIT};
pub use config::{realtime_config, LanguageField, RealtimeConfig, SessionKind};
pub use reader::FINISH_WAIT;

/// Speech client for one source's lane (openAIRealtime, azureOpenAIRealtime, customOpenAIASR).
/// Each source gets its own connection with the same settings.
pub fn speech(plan: &SessionPlan, _source: Source) -> Result<Box<dyn SpeechClient>, SessionError> {
    let config = realtime_config(plan)?;
    Ok(Box::new(RealtimeClient::new(config)))
}
