//! Volcano Engine Doubao simultaneous interpretation 2.0 (AST v2), speech-to-text mode
//! (docs/spec/providers/volcano.md).

pub mod client;
pub mod frames;
pub mod language;
pub mod mapper;
pub mod message;
pub mod wire;

use sheng_core::Source;
use url::Url;

pub use client::{VolcanoClient, VolcanoConfig, RESOURCE_ID};

use crate::config::request::Credentials;
use crate::config::SessionPlan;
use crate::provider::{SessionError, SpeechClient};
use crate::transport::CONNECT_TIMEOUT;

/// Production endpoint; tests inject their own through `VolcanoConfig`.
pub const ENDPOINT: &str = "wss://openspeech.bytedance.com/api/v4/ast/v2/translate";

/// Speech client for one source's lane (volcanoEngine).
pub fn speech(plan: &SessionPlan, _source: Source) -> Result<Box<dyn SpeechClient>, SessionError> {
    let Credentials::ApiKey { api_key } = &plan.credentials else {
        return Err(SessionError::ProviderConfigurationFailed);
    };
    let endpoint = Url::parse(ENDPOINT).map_err(|_| SessionError::ProviderConfigurationFailed)?;
    let config = VolcanoConfig {
        endpoint,
        api_key: api_key.clone(),
        source_language: plan.source_language.clone(),
        target_language: plan.target_language.clone(),
        proxy: plan.speech_proxy.clone(),
        connect_timeout: CONNECT_TIMEOUT,
    };
    Ok(Box::new(VolcanoClient::new(config)))
}
