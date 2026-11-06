//! Tencent Cloud real-time speech translation over WebSocket (step 4g, providers/tencent.md).
//!
//! The service recognizes and translates in one stream: drafts per `sentence_id`, a confirmed
//! pair at `sentence_end`, and `final:1` after `{"type":"end"}`.

pub mod client;
pub mod protocol;
pub mod signing;

use sheng_core::Source;
use url::Url;

pub use client::{TencentConfig, TencentSpeech};
pub use signing::TencentCredentials;

use crate::config::request::Credentials;
use crate::config::SessionPlan;
use crate::provider::{SessionError, SpeechClient};

/// Production endpoint; the AppId path segment and the signed query are appended.
pub const ENDPOINT: &str = "wss://asr.cloud.tencent.com/asr/speech_translate/";

/// Speech client for one source's lane (tencentCloud).
pub fn speech(plan: &SessionPlan, _source: Source) -> Result<Box<dyn SpeechClient>, SessionError> {
    let Credentials::TencentCloud {
        app_id,
        secret_id,
        secret_key,
    } = &plan.credentials
    else {
        return Err(SessionError::ProviderConfigurationFailed);
    };
    let config = TencentConfig {
        endpoint: Url::parse(ENDPOINT).map_err(|_| SessionError::ProviderConfigurationFailed)?,
        credentials: TencentCredentials::new(app_id, secret_id.clone(), secret_key.clone())?,
        source: plan.source_language.clone(),
        target: plan.target_language.clone(),
        proxy: plan.speech_proxy.clone(),
    };
    Ok(Box::new(TencentSpeech::new(config)))
}
