//! xAI speech (step 4d, providers/xai.md): Grok Voice simultaneous interpretation over
//! the realtime API and streaming speech-to-text for the recognize-then-translate route.
//! Text translation through xAI is part of `services::text`.

mod connection;
mod language;
mod protocol;
pub mod stt;
pub mod voice;

use sheng_core::Source;
use url::Url;

use self::connection::{Target, WsSpeechClient};
use self::stt::{stt_url, SttProtocol};
use self::voice::{voice_url, VoiceProtocol};
use crate::config::request::Credentials;
use crate::config::route::SpeechRoute;
use crate::config::secret::Secret;
use crate::config::SessionPlan;
use crate::provider::{SessionError, SpeechClient};
use crate::transport::WsRequest;

pub const REALTIME_ENDPOINT: &str = "wss://api.x.ai/v1/realtime";
pub const STT_ENDPOINT: &str = "wss://api.x.ai/v1/stt";

/// Service addresses; production uses `Endpoints::production`, tests point at loopback.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub realtime: Url,
    pub stt: Url,
}

impl Endpoints {
    pub fn production() -> Result<Self, SessionError> {
        let parse =
            |raw: &str| Url::parse(raw).map_err(|_| SessionError::ProviderConfigurationFailed);
        Ok(Self {
            realtime: parse(REALTIME_ENDPOINT)?,
            stt: parse(STT_ENDPOINT)?,
        })
    }
}

/// Speech client for one source's lane (xAIRealtime).
pub fn speech(plan: &SessionPlan, source: Source) -> Result<Box<dyn SpeechClient>, SessionError> {
    speech_at(plan, source, &Endpoints::production()?)
}

/// Speech client using the given endpoints. Both sources use identical connections.
pub fn speech_at(
    plan: &SessionPlan,
    _source: Source,
    endpoints: &Endpoints,
) -> Result<Box<dyn SpeechClient>, SessionError> {
    let Credentials::ApiKey { api_key } = &plan.credentials else {
        return Err(SessionError::ProviderConfigurationFailed);
    };
    let request = |url: Url| Target {
        request: WsRequest {
            url,
            headers: vec![("authorization", bearer(api_key))],
        },
        proxy: plan.speech_proxy.clone(),
    };
    let (source_language, target_language) = (&plan.source_language, &plan.target_language);
    match plan.route {
        SpeechRoute::XaiVoice => {
            let target = request(voice_url(&endpoints.realtime));
            let protocol = VoiceProtocol::new(plan.sample_rate, source_language, target_language);
            Ok(Box::new(WsSpeechClient::new(target, protocol)))
        }
        SpeechRoute::XaiStt => {
            let url = stt_url(&endpoints.stt, plan.sample_rate, source_language);
            Ok(Box::new(WsSpeechClient::new(
                request(url),
                SttProtocol::new(),
            )))
        }
        _ => Err(SessionError::ProviderConfigurationFailed),
    }
}

fn bearer(api_key: &Secret) -> Secret {
    Secret::new(format!("Bearer {}", api_key.expose().trim()))
}
