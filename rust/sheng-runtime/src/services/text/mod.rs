//! Independent text translators: xAI, OpenAI-compatible, chat mock, DeepL, DeepLX (step 4b,
//! providers/text.md, providers/xai.md “文本翻译”).
//!
//! Every client sends one sentence per request and maps HTTP failures onto
//! `ProviderError` so the translation pipeline can retry or fail (translation-pipeline.md §2).
//! Endpoints come from the validated plan; tests construct clients against local servers.

mod chat;
mod deepl;
mod deeplx;
mod prompt;
mod response;

use std::sync::Arc;
use std::time::Duration;

pub use chat::{chat_request_body, thinking_switch, ChatClient, ChatDecoder};
pub use deepl::{deepl_request_body, deepl_source, deepl_target, DeepLClient};
pub use deeplx::{deeplx_request_body, DeepLxClient};
pub use response::classify_status;

use crate::config::endpoint::ProxyChoice;
use crate::config::{SessionPlan, TextPlan};
use crate::provider::{ProviderError, SessionError, TextTranslator};

/// Upper bound of one HTTP request including the streamed body. The pipeline applies its own,
/// shorter limits (12 s preview, 45 s per final sentence); this only bounds a stuck connection.
pub const TEXT_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

/// Builds the HTTP client shared by all text translators.
pub(crate) fn client(proxy: &ProxyChoice) -> Result<reqwest::Client, ProviderError> {
    crate::transport::http_client(proxy, TEXT_REQUEST_TIMEOUT)
}

/// Text translator for a recognizing route (text kinds xAI, openAICompatible, chatMock, deepL, deepLX).
pub fn translator(plan: &SessionPlan) -> Result<Arc<dyn TextTranslator>, SessionError> {
    let proxy = &plan.text_proxy;
    let built: Result<Arc<dyn TextTranslator>, ProviderError> = match &plan.text {
        TextPlan::OpenAiCompatible {
            endpoint,
            model,
            api_key,
        } => ChatClient::new(endpoint.clone(), model.clone(), api_key.clone(), proxy)
            .map(|client| Arc::new(client) as Arc<dyn TextTranslator>),
        TextPlan::DeepL { endpoint, api_key } => {
            DeepLClient::new((*endpoint).to_owned(), api_key.clone(), proxy)
                .map(|client| Arc::new(client) as Arc<dyn TextTranslator>)
        }
        TextPlan::DeepLx { endpoint, token } => {
            DeepLxClient::new(endpoint.clone(), token.clone(), proxy)
                .map(|client| Arc::new(client) as Arc<dyn TextTranslator>)
        }
        TextPlan::None | TextPlan::Apple | TextPlan::QwenMt { .. } => {
            return Err(SessionError::ProviderConfigurationFailed)
        }
    };
    built.map_err(|_| SessionError::ProviderConfigurationFailed)
}
