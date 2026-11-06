//! Google Gemini Live (step 4e, providers/gemini.md).
//!
//! `simultaneous` uses the Live Translate model and reports input and output transcripts as
//! two aligned streams; `asrMt` uses the Live Transcribe model and reports originals only.

pub mod client;
pub mod config;
pub mod errors;
pub mod language;
pub mod protocol;
pub mod reader;
pub mod session;
pub mod transcribe;
pub mod translate;

use sheng_core::Source;
use url::Url;

use crate::config::SessionPlan;
use crate::provider::{SessionError, SpeechClient};

pub use client::GeminiClient;
pub use config::GeminiConfig;

/// Speech client for one source's lane (googleGeminiLive).
pub fn speech(plan: &SessionPlan, _source: Source) -> Result<Box<dyn SpeechClient>, SessionError> {
    let endpoint =
        Url::parse(config::LIVE_ENDPOINT).map_err(|_| SessionError::ProviderConfigurationFailed)?;
    speech_at(plan, endpoint)
}

/// Same as [`speech`] against another endpoint (local mock servers in tests).
pub fn speech_at(plan: &SessionPlan, endpoint: Url) -> Result<Box<dyn SpeechClient>, SessionError> {
    let config = GeminiConfig::from_plan(plan, endpoint)?;
    Ok(Box::new(GeminiClient::new(config)))
}
