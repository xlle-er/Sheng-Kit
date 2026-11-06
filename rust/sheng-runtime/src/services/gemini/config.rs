//! Connection settings of one Gemini Live lane, derived from a validated plan.

use url::Url;

use super::language::service_code;
use super::protocol::SessionKind;
use crate::config::endpoint::ProxyChoice;
use crate::config::request::Credentials;
use crate::config::route::SpeechRoute;
use crate::config::secret::Secret;
use crate::config::SessionPlan;
use crate::provider::SessionError;
use crate::transport::WsRequest;

/// Live API WebSocket endpoint (v1beta).
pub const LIVE_ENDPOINT: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
/// Model of the `simultaneous` route.
pub const TRANSLATE_MODEL: &str = "gemini-3.5-live-translate-preview";
/// Model of the `asrMt` route.
pub const TRANSCRIBE_MODEL: &str = "gemini-3.5-transcribe-live";
/// Header carrying the API key (questions/step4e.md: the WebSocket guide shows `?key=`).
pub const API_KEY_HEADER: &str = "x-goog-api-key";

#[derive(Clone)]
pub struct GeminiConfig {
    pub endpoint: Url,
    pub api_key: Secret,
    pub proxy: ProxyChoice,
    pub model: String,
    pub kind: SessionKind,
}

impl std::fmt::Debug for GeminiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiConfig")
            .field("host", &self.endpoint.host_str())
            .field("model", &self.model)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl GeminiConfig {
    /// Settings for `plan` against `endpoint` (the production endpoint, or a local one in tests).
    pub fn from_plan(plan: &SessionPlan, endpoint: Url) -> Result<Self, SessionError> {
        let Credentials::ApiKey { api_key } = &plan.credentials else {
            return Err(SessionError::ProviderConfigurationFailed);
        };
        let (model, kind) = match plan.route {
            SpeechRoute::GeminiTranslate => {
                let target = service_code(&plan.target_language)
                    .ok_or(SessionError::ProviderConfigurationFailed)?;
                let kind = SessionKind::Translate {
                    target: target.to_owned(),
                };
                (TRANSLATE_MODEL, kind)
            }
            SpeechRoute::GeminiTranscribe => {
                let source = service_code(&plan.source_language).map(str::to_owned);
                (TRANSCRIBE_MODEL, SessionKind::Transcribe { source })
            }
            _ => return Err(SessionError::ProviderConfigurationFailed),
        };
        Ok(Self {
            endpoint,
            api_key: api_key.clone(),
            proxy: plan.speech_proxy.clone(),
            model: model.to_owned(),
            kind,
        })
    }

    pub fn ws_request(&self) -> WsRequest {
        WsRequest {
            url: self.endpoint.clone(),
            headers: vec![(API_KEY_HEADER, self.api_key.clone())],
        }
    }
}
