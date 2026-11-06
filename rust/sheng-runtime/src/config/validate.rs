//! Semantic checks of a parsed request (start-request.md “校验规则”).

use thiserror::Error;

use super::endpoint::{
    normalize_azure_endpoint, normalize_deepl_key, normalize_model_name, normalize_text_endpoint,
    validate_proxy, validate_speech_endpoint, ProxyChoice, CHAT_COMPLETIONS_SUFFIX, DEEPLX_SUFFIX,
};
use super::request::{Configuration, Credentials, Input, Provider, StartRequest, TextCredentials};
use super::route::{speech_route, text_route, RouteModes, SpeechRoute, TextRoute};
use super::secret::Secret;
use crate::catalog::routes::{language_issues, language_options};

/// Why a structurally valid request is rejected. All map to `configuration_invalid`;
/// the variants exist for tests and never carry user content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("credential kind does not match the provider")]
    CredentialKind,
    #[error("a required credential is empty or too long")]
    Credential,
    #[error("endpoint is not acceptable")]
    Endpoint,
    #[error("endpoint is not an Azure OpenAI host")]
    AzureHost,
    #[error("model name is not acceptable")]
    ModelName,
    #[error("proxy configuration is not acceptable")]
    Proxy,
    #[error("provider and mode combination is not supported")]
    Route,
    #[error("this route needs text translation credentials")]
    MissingTextCredentials,
    #[error("language pair is not supported by this route")]
    Language,
    #[error("this route translates by itself and takes no text translation credentials")]
    UnexpectedTextCredentials,
    #[error("this route needs a translation target")]
    MissingTarget,
    #[error("workspace id is not acceptable")]
    WorkspaceId,
}

/// Normalized text translation settings for the pipeline.
#[derive(Debug, Clone)]
pub enum TextPlan {
    None,
    Apple,
    DeepL {
        endpoint: &'static str,
        api_key: Secret,
    },
    DeepLx {
        endpoint: String,
        token: Secret,
    },
    OpenAiCompatible {
        endpoint: String,
        model: String,
        api_key: Secret,
    },
    QwenMt {
        model: &'static str,
    },
}

/// Everything later stages need from a validated request.
#[derive(Debug, Clone)]
pub struct SessionPlan {
    pub input: Input,
    pub provider: Provider,
    pub route: SpeechRoute,
    pub text_route: TextRoute,
    pub text: TextPlan,
    pub source_language: String,
    pub target_language: String,
    pub sample_rate: u32,
    pub speech_proxy: ProxyChoice,
    pub text_proxy: ProxyChoice,
    /// Speech service credentials as validated; `Debug` shows only the kind.
    pub credentials: Credentials,
    /// Alibaba workspace id as sent (empty when not set); selects the live-translation host.
    pub alibaba_workspace_id: String,
}

fn require(secret: &Secret) -> Result<(), ValidationError> {
    if secret.is_blank() || !secret.is_within_limit() {
        return Err(ValidationError::Credential);
    }
    Ok(())
}

fn check_credentials(provider: Provider, credentials: &Credentials) -> Result<(), ValidationError> {
    use Provider as P;
    match (provider, credentials) {
        (P::AppleSpeech, Credentials::AppleSpeech) => Ok(()),
        (
            P::AlibabaCloud
            | P::VolcanoEngine
            | P::GoogleGeminiLive
            | P::OpenAiRealtime
            | P::XaiRealtime,
            Credentials::ApiKey { api_key },
        ) => require(api_key),
        (
            P::AzureOpenAiRealtime,
            Credentials::AzureOpenAi {
                endpoint,
                deployment,
                transcription_deployment,
                api_key,
            },
        ) => {
            require(api_key)?;
            normalize_azure_endpoint(endpoint.expose())?;
            normalize_model_name(deployment)?;
            normalize_model_name(transcription_deployment).map(|_| ())
        }
        (
            P::TencentCloud,
            Credentials::TencentCloud {
                app_id,
                secret_id,
                secret_key,
            },
        ) => [app_id, secret_id, secret_key]
            .into_iter()
            .try_for_each(require),
        (P::BaiduTranslate, Credentials::BaiduTranslate { app_id, app_key }) => {
            require(app_id)?;
            require(app_key)
        }
        (
            P::CustomDashScopeAsr | P::CustomOpenAiAsr,
            Credentials::CustomSpeech {
                endpoint,
                model,
                api_key,
            },
        ) => {
            require(api_key)?;
            validate_speech_endpoint(endpoint.expose())?;
            normalize_model_name(model).map(|_| ())
        }
        _ => Err(ValidationError::CredentialKind),
    }
}

fn text_plan(text: &TextCredentials) -> Result<TextPlan, ValidationError> {
    match text {
        TextCredentials::Apple => Ok(TextPlan::Apple),
        TextCredentials::DeepL { api_key } => {
            require(api_key)?;
            let key = normalize_deepl_key(api_key.expose())?;
            Ok(TextPlan::DeepL {
                endpoint: super::endpoint::deepl_endpoint(&key),
                api_key: Secret::new(key),
            })
        }
        TextCredentials::DeepLx { endpoint, token } => {
            if !token.is_within_limit() {
                return Err(ValidationError::Credential);
            }
            Ok(TextPlan::DeepLx {
                endpoint: normalize_text_endpoint(endpoint.expose(), DEEPLX_SUFFIX)?,
                token: token.clone(),
            })
        }
        TextCredentials::Xai { api_key, model } => {
            require(api_key)?;
            Ok(TextPlan::OpenAiCompatible {
                endpoint: crate::provider::XAI_CHAT_COMPLETIONS.to_owned(),
                model: normalize_model_name(model)?,
                api_key: api_key.clone(),
            })
        }
        TextCredentials::OpenAiCompatible {
            endpoint,
            model,
            api_key,
        }
        | TextCredentials::ChatMock {
            endpoint,
            model,
            api_key,
        } => {
            if !api_key.is_within_limit() {
                return Err(ValidationError::Credential);
            }
            Ok(TextPlan::OpenAiCompatible {
                endpoint: normalize_text_endpoint(endpoint.expose(), CHAT_COMPLETIONS_SUFFIX)?,
                model: normalize_model_name(model)?,
                api_key: api_key.clone(),
            })
        }
    }
}

fn modes(configuration: &Configuration) -> RouteModes {
    RouteModes {
        speech_translation: configuration.speech_translation_mode,
        xai_speech: configuration.xai_speech_mode,
        alibaba_realtime: configuration.alibaba_realtime_mode,
    }
}

fn resolve_text(
    route: SpeechRoute,
    configuration: &Configuration,
) -> Result<(TextRoute, TextPlan), ValidationError> {
    let configured = configuration.text_credentials.as_ref();
    let text_route = text_route(
        route,
        configured.map(TextCredentials::kind),
        configuration.qwen_mt_model,
    );
    let translating = configuration.target_language != sheng_core::language::ORIGINAL;
    match text_route {
        TextRoute::Independent(_) => match configured {
            Some(text) => Ok((text_route, text_plan(text)?)),
            None => Err(ValidationError::MissingTextCredentials),
        },
        TextRoute::QwenMt(model) => Ok((
            text_route,
            TextPlan::QwenMt {
                model: qwen_model_id(model),
            },
        )),
        TextRoute::None if translating && !route.translates() => {
            Err(ValidationError::MissingTextCredentials)
        }
        TextRoute::None => Ok((text_route, TextPlan::None)),
    }
}

fn qwen_model_id(model: super::request::QwenMtModel) -> &'static str {
    match model {
        super::request::QwenMtModel::Lite => "qwen-mt-lite",
        super::request::QwenMtModel::Plus => "qwen-mt-plus",
    }
}

/// Validates a parsed request and resolves its routes.
pub fn validate(request: &StartRequest) -> Result<SessionPlan, ValidationError> {
    let configuration = &request.configuration;
    check_credentials(configuration.provider, &configuration.credentials)?;
    let speech_proxy = validate_proxy(&configuration.network_proxy)?;
    let text_proxy = validate_proxy(&configuration.text_network_proxy)?;
    let route =
        speech_route(configuration.provider, modes(configuration)).ok_or(ValidationError::Route)?;
    if route == SpeechRoute::AlibabaLiveTranslate {
        super::alibaba_live::check(configuration)?;
    }
    let (text_route, text) = resolve_text(route, configuration)?;
    let (source, target) = (
        &configuration.source_language,
        &configuration.target_language,
    );
    let options = language_options(route, text_route, None, source, target);
    if !language_issues(&options, source, target).is_empty() {
        return Err(ValidationError::Language);
    }
    Ok(SessionPlan {
        input: request.input,
        provider: configuration.provider,
        route,
        text_route,
        text,
        source_language: source.clone(),
        target_language: target.clone(),
        sample_rate: route.sample_rate(),
        speech_proxy,
        text_proxy,
        credentials: configuration.credentials.clone(),
        alibaba_workspace_id: configuration.alibaba_workspace_id.clone(),
    })
}
