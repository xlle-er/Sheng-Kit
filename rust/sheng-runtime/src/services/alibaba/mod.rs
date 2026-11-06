//! Alibaba Cloud Model Studio (DashScope): streaming recognition, live translation and
//! Qwen-MT text translation, plus custom DashScope recognition endpoints (step 4f,
//! providers/alibaba.md).

pub mod asr;
pub mod connection;
pub mod diagnostics;
pub mod errors;
pub mod live;
pub mod mt;

use std::sync::Arc;

use sheng_core::Source;
use url::Url;

use crate::config::request::Credentials;
use crate::config::route::SpeechRoute;
use crate::config::secret::Secret;
use crate::config::{SessionPlan, TextPlan};
use crate::provider::{SessionError, SpeechClient, TextTranslator};
use asr::client::{AsrClient, AsrConfig};
use asr::protocol::TaskSettings;
use live::client::{LiveConfig, LiveTranslateClient};
use live::protocol::{realtime_url, LIVE_TRANSLATE_MODEL};
use mt::{QwenMtTranslator, QWEN_MT_ENDPOINT};

pub const INFERENCE_ENDPOINT: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/inference";
pub const ASR_MODEL: &str = "qwen-audio-3.0-asr-flash-streaming";

fn api_key(plan: &SessionPlan) -> Result<&Secret, SessionError> {
    match &plan.credentials {
        Credentials::ApiKey { api_key } | Credentials::CustomSpeech { api_key, .. } => Ok(api_key),
        _ => Err(SessionError::ProviderConfigurationFailed),
    }
}

/// Endpoint and model of a recognition route.
fn recognition_target(plan: &SessionPlan) -> Result<(Url, String), SessionError> {
    let invalid = |_| SessionError::InvalidConfiguration;
    match (&plan.route, &plan.credentials) {
        (SpeechRoute::AlibabaAsr, _) => Ok((
            Url::parse(INFERENCE_ENDPOINT).map_err(invalid)?,
            ASR_MODEL.to_owned(),
        )),
        (
            SpeechRoute::CustomDashScope,
            Credentials::CustomSpeech {
                endpoint, model, ..
            },
        ) => {
            let endpoint = crate::config::endpoint::validate_speech_endpoint(endpoint.expose())
                .map_err(|_| SessionError::InvalidConfiguration)?;
            let model = crate::config::endpoint::normalize_model_name(model)
                .map_err(|_| SessionError::InvalidConfiguration)?;
            Ok((Url::parse(&endpoint).map_err(invalid)?, model))
        }
        _ => Err(SessionError::ProviderConfigurationFailed),
    }
}

/// Speech client for one source's lane (alibabaCloud, customDashScopeASR).
pub fn speech(plan: &SessionPlan, _source: Source) -> Result<Box<dyn SpeechClient>, SessionError> {
    if plan.route == SpeechRoute::AlibabaLiveTranslate {
        let endpoint = realtime_url(&plan.alibaba_workspace_id, LIVE_TRANSLATE_MODEL)
            .ok_or(SessionError::InvalidConfiguration)?;
        return live_translate(plan, endpoint);
    }
    let (endpoint, model) = recognition_target(plan)?;
    let config = AsrConfig {
        endpoint,
        api_key: api_key(plan)?.clone(),
        proxy: plan.speech_proxy.clone(),
        settings: TaskSettings {
            model,
            sample_rate: plan.sample_rate,
            source_language: plan.source_language.clone(),
        },
    };
    Ok(Box::new(AsrClient::new(config)))
}

/// Live-translation client for a validated `liveTranslate` plan at `endpoint` (the realtime
/// URL from `speech`; tests pass a loopback one).
pub fn live_translate(
    plan: &SessionPlan,
    endpoint: Url,
) -> Result<Box<dyn SpeechClient>, SessionError> {
    if plan.route != SpeechRoute::AlibabaLiveTranslate {
        return Err(SessionError::ProviderConfigurationFailed);
    }
    let config = LiveConfig {
        endpoint,
        api_key: api_key(plan)?.clone(),
        proxy: plan.speech_proxy.clone(),
        target_language: plan.target_language.clone(),
        sample_rate: plan.sample_rate,
    };
    Ok(Box::new(LiveTranslateClient::new(config)))
}

/// Text translator for a recognizing route (Qwen-MT when no independent translator is configured).
pub fn translator(plan: &SessionPlan) -> Result<Arc<dyn TextTranslator>, SessionError> {
    let TextPlan::QwenMt { model } = &plan.text else {
        return Err(SessionError::ProviderConfigurationFailed);
    };
    let translator =
        QwenMtTranslator::new(QWEN_MT_ENDPOINT, *model, api_key(plan)?, &plan.text_proxy)
            .map_err(|error| error.speech_session_error())?;
    Ok(Arc::new(translator))
}
