//! Baidu real-time speech translation (step 4h, providers/baidu.md).
//!
//! One WebSocket per lane. Credentials (`app_id`, `app_key`) travel in the START frame;
//! the service recognizes and translates, so results become caption events directly.

pub mod client;
pub mod languages;
pub mod protocol;

use sheng_core::Source;
use url::Url;

pub use client::{BaiduSession, BaiduSpeech};
pub use languages::{baidu_code, BAIDU_CODES};
pub use protocol::StartParams;

use crate::config::request::Credentials;
use crate::config::SessionPlan;
use crate::provider::{SessionError, SpeechClient};

/// Official endpoint (“请求URL”). Only the factory entry below uses it.
pub const BAIDU_ENDPOINT: &str = "wss://aip.baidubce.com/ws/realtime_speech_trans";

/// Speech client for one source's lane (baiduTranslate).
pub fn speech(plan: &SessionPlan, _source: Source) -> Result<Box<dyn SpeechClient>, SessionError> {
    let endpoint =
        Url::parse(BAIDU_ENDPOINT).map_err(|_| SessionError::ProviderConfigurationFailed)?;
    let session = session_for(plan, endpoint)?;
    Ok(Box::new(BaiduSpeech::new(session)))
}

/// Connection settings for `plan` against `endpoint`.
pub fn session_for(plan: &SessionPlan, endpoint: Url) -> Result<BaiduSession, SessionError> {
    let Credentials::BaiduTranslate { app_id, app_key } = &plan.credentials else {
        return Err(SessionError::ProviderConfigurationFailed);
    };
    let language = |code: &str| baidu_code(code).ok_or(SessionError::ProviderConfigurationFailed);
    let start = StartParams {
        from: language(&plan.source_language)?,
        to: language(&plan.target_language)?,
        app_id: app_id.clone(),
        app_key: app_key.clone(),
        sampling_rate: plan.sample_rate,
    };
    Ok(BaiduSession {
        endpoint,
        proxy: plan.speech_proxy.clone(),
        start,
    })
}
