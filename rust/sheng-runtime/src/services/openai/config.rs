//! Per-route connection settings: endpoint, authentication and session options
//! (providers/openai.md “路由”). The official endpoints are only set here; tests build a
//! [`RealtimeConfig`] that points at a local server instead.

use url::Url;

use crate::config::endpoint::{normalize_azure_endpoint, normalize_model_name, ProxyChoice};
use crate::config::request::Credentials;
use crate::config::route::SpeechRoute;
use crate::config::secret::Secret;
use crate::config::SessionPlan;
use crate::provider::SessionError;
use crate::transport::WsRequest;

/// Input sample rate of every route served here (providers/README.md).
pub const SAMPLE_RATE: u32 = 24_000;
/// Transcription session endpoint (realtime transcription guide).
pub const OPENAI_TRANSCRIBE_URL: &str = "wss://api.openai.com/v1/realtime?intent=transcription";
/// Translation session endpoint; the model goes into the `model` query parameter.
pub const OPENAI_TRANSLATE_URL: &str = "wss://api.openai.com/v1/realtime/translations";
pub const OPENAI_TRANSCRIBE_MODEL: &str = "gpt-live-transcribe";
pub const OPENAI_TRANSLATE_MODEL: &str = "gpt-realtime-translate";
/// Source transcript model of OpenAI translation sessions (translation client events).
pub const OPENAI_SOURCE_TRANSCRIPT_MODEL: &str = "gpt-realtime-whisper";
const AZURE_REALTIME_PATH: &str = "/openai/v1/realtime";

/// How the transcription session receives the source language hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageField {
    /// `languages: [..]`, used by `gpt-live-transcribe`.
    List,
    /// `language: ".."` (ISO-639-1), accepted by the other transcription models.
    Single,
}

/// Which session protocol the connection speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionKind {
    /// Transcription session; finals come per committed audio turn.
    Transcribe {
        model: String,
        language: Option<String>,
        language_field: LanguageField,
    },
    /// Translation session with two id-less transcript streams.
    Translate {
        source_transcript_model: String,
        output_language: String,
    },
}

/// Everything one connection needs. `Debug` never shows credentials.
#[derive(Debug, Clone)]
pub struct RealtimeConfig {
    pub request: WsRequest,
    pub proxy: ProxyChoice,
    pub kind: SessionKind,
    /// Endpoints not run by the vendor answer a protocol mismatch with
    /// `invalid_configuration` (providers/openai.md “错误映射”).
    pub custom_endpoint: bool,
}

fn bearer(api_key: &Secret) -> (&'static str, Secret) {
    let value = format!("Bearer {}", api_key.expose().trim());
    ("authorization", Secret::new(value))
}

fn parse(url: &str) -> Result<Url, SessionError> {
    Url::parse(url).map_err(|_| SessionError::ProviderConfigurationFailed)
}

fn with_model(base: &str, model: &str) -> Result<Url, SessionError> {
    let mut url = parse(base)?;
    url.query_pairs_mut().append_pair("model", model);
    Ok(url)
}

fn model(raw: &str) -> Result<String, SessionError> {
    normalize_model_name(raw).map_err(|_| SessionError::ProviderConfigurationFailed)
}

/// Builds the connection settings of a validated plan.
pub fn realtime_config(plan: &SessionPlan) -> Result<RealtimeConfig, SessionError> {
    let (request, kind) = match (plan.route, &plan.credentials) {
        (SpeechRoute::OpenAiTranscribe, Credentials::ApiKey { api_key }) => {
            openai_transcribe(api_key, &plan.source_language)?
        }
        (SpeechRoute::OpenAiTranslate, Credentials::ApiKey { api_key }) => {
            openai_translate(api_key, &plan.target_language)?
        }
        (
            route @ (SpeechRoute::AzureTranscribe | SpeechRoute::AzureTranslate),
            Credentials::AzureOpenAi {
                endpoint,
                deployment,
                transcription_deployment,
                api_key,
            },
        ) => {
            let azure = Azure {
                endpoint,
                deployment,
                transcription_deployment,
                api_key,
            };
            azure.build(route == SpeechRoute::AzureTranslate, plan)?
        }
        (
            SpeechRoute::CustomOpenAi,
            Credentials::CustomSpeech {
                endpoint,
                model: name,
                api_key,
            },
        ) => custom(endpoint, name, api_key, &plan.source_language)?,
        _ => return Err(SessionError::ProviderConfigurationFailed),
    };
    Ok(RealtimeConfig {
        request,
        proxy: plan.speech_proxy.clone(),
        kind,
        custom_endpoint: plan.route == SpeechRoute::CustomOpenAi,
    })
}

fn openai_transcribe(
    api_key: &Secret,
    source: &str,
) -> Result<(WsRequest, SessionKind), SessionError> {
    let request = WsRequest {
        url: parse(OPENAI_TRANSCRIBE_URL)?,
        headers: vec![bearer(api_key)],
    };
    let kind = SessionKind::Transcribe {
        model: OPENAI_TRANSCRIBE_MODEL.to_owned(),
        language: transcription_language(source, LanguageField::List),
        language_field: LanguageField::List,
    };
    Ok((request, kind))
}

fn openai_translate(
    api_key: &Secret,
    target: &str,
) -> Result<(WsRequest, SessionKind), SessionError> {
    let request = WsRequest {
        url: with_model(OPENAI_TRANSLATE_URL, OPENAI_TRANSLATE_MODEL)?,
        headers: vec![bearer(api_key)],
    };
    let kind = SessionKind::Translate {
        source_transcript_model: OPENAI_SOURCE_TRANSCRIPT_MODEL.to_owned(),
        output_language: output_language(target),
    };
    Ok((request, kind))
}

struct Azure<'a> {
    endpoint: &'a Secret,
    deployment: &'a str,
    transcription_deployment: &'a str,
    api_key: &'a Secret,
}

impl Azure<'_> {
    fn build(
        &self,
        translate: bool,
        plan: &SessionPlan,
    ) -> Result<(WsRequest, SessionKind), SessionError> {
        let https = normalize_azure_endpoint(self.endpoint.expose())
            .map_err(|_| SessionError::ProviderConfigurationFailed)?;
        let base = https.replacen("https://", "wss://", 1) + AZURE_REALTIME_PATH;
        let transcription = model(self.transcription_deployment)?;
        let (url, kind) = if translate {
            let url = with_model(&format!("{base}/translations"), &model(self.deployment)?)?;
            let kind = SessionKind::Translate {
                source_transcript_model: transcription,
                output_language: output_language(&plan.target_language),
            };
            (url, kind)
        } else {
            let url = parse(&format!("{base}?intent=transcription"))?;
            (url, transcribe_kind(transcription, &plan.source_language))
        };
        let header = ("api-key", Secret::new(self.api_key.expose().trim()));
        Ok((
            WsRequest {
                url,
                headers: vec![header],
            },
            kind,
        ))
    }
}

fn custom(
    endpoint: &Secret,
    name: &str,
    api_key: &Secret,
    source: &str,
) -> Result<(WsRequest, SessionKind), SessionError> {
    let request = WsRequest {
        url: parse(endpoint.expose().trim())?,
        headers: vec![bearer(api_key)],
    };
    Ok((request, transcribe_kind(model(name)?, source)))
}

/// Deployments and custom models may be any transcription model; the single-language
/// field is the one every model in the session reference accepts.
fn transcribe_kind(model: String, source: &str) -> SessionKind {
    SessionKind::Transcribe {
        model,
        language: transcription_language(source, LanguageField::Single),
        language_field: LanguageField::Single,
    }
}

/// Source language hint; `auto` sends none.
pub fn transcription_language(source: &str, field: LanguageField) -> Option<String> {
    let code = match (source, field) {
        ("auto", _) => return None,
        ("zh_tw", LanguageField::List) => "zh-tw",
        ("yue", LanguageField::List) => "yue",
        ("zh_tw" | "yue" | "zh_en", LanguageField::Single) => "zh",
        (other, _) => other,
    };
    Some(code.to_owned())
}

/// Target language of translation output; catalog codes are already ISO-639-1.
pub fn output_language(target: &str) -> String {
    target.replace('_', "-")
}
