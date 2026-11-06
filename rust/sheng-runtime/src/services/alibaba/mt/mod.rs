//! Qwen-MT text translation over the OpenAI-compatible chat completions endpoint
//! (providers/alibaba.md “Qwen-MT”).

pub mod languages;

use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::endpoint::ProxyChoice;
use crate::config::secret::Secret;
use crate::provider::{BoxFuture, ProviderError, TextTranslator, TranslationRequest};
use crate::services::alibaba::asr::client::bearer;
use crate::services::alibaba::errors;
use crate::transport::{http_client, post_json, read_sse};

pub const QWEN_MT_ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions";
/// Whole-request limit; the pipeline applies its own shorter limits on top.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
/// `qwen-mt-plus` streams the whole text so far in every chunk; the others stream deltas.
const SNAPSHOT_MODELS: &[&str] = &["qwen-mt-plus"];

pub struct QwenMtTranslator {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    headers: Vec<(&'static str, Secret)>,
}

impl QwenMtTranslator {
    pub fn new(
        endpoint: impl Into<String>,
        model: impl Into<String>,
        api_key: &Secret,
        proxy: &ProxyChoice,
    ) -> Result<Self, ProviderError> {
        Ok(Self {
            client: http_client(proxy, REQUEST_TIMEOUT)?,
            endpoint: endpoint.into(),
            model: model.into(),
            headers: vec![("authorization", bearer(api_key))],
        })
    }

    async fn run(&self, request: TranslationRequest) -> Result<String, ProviderError> {
        let body = request_body(&self.model, &request)?;
        let response = post_json(&self.client, &self.endpoint, &self.headers, &body).await?;
        let chunks = read_sse(response).await?;
        assemble(&chunks, SNAPSHOT_MODELS.contains(&self.model.as_str()))
    }
}

/// Chat request carrying only the text, with languages in `translation_options`.
pub fn request_body(model: &str, request: &TranslationRequest) -> Result<Value, ProviderError> {
    let target =
        languages::name(&request.target_language, model).ok_or(ProviderError::Configuration)?;
    let source = languages::source_name(&request.source_language, model);
    Ok(json!({
        "model": model,
        "stream": true,
        "messages": [{"role": "user", "content": request.text}],
        "translation_options": {"source_lang": source, "target_lang": target},
    }))
}

#[derive(Debug, Default, Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    error: Option<ChunkError>,
}

#[derive(Debug, Default, Deserialize)]
struct Choice {
    #[serde(default)]
    delta: Option<Delta>,
}

/// Only `content` is read; reasoning fields are dropped.
#[derive(Debug, Default, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ChunkError {
    #[serde(default)]
    code: Option<String>,
}

/// Joins streamed chunks into the translation. Snapshot streams keep the latest text.
/// An empty result is a provider error.
pub fn assemble(chunks: &[String], snapshots: bool) -> Result<String, ProviderError> {
    let mut text = String::new();
    for data in chunks {
        let chunk: Chunk = serde_json::from_str(data).map_err(|_| ProviderError::Provider)?;
        if let Some(error) = chunk.error {
            return Err(errors::classify(error.code.as_deref(), None));
        }
        let content = chunk
            .choices
            .into_iter()
            .filter_map(|choice| choice.delta.and_then(|delta| delta.content));
        for part in content {
            if snapshots {
                if !part.is_empty() {
                    text = part;
                }
            } else {
                text.push_str(&part);
            }
        }
    }
    if text.trim().is_empty() {
        return Err(ProviderError::Provider);
    }
    Ok(text.trim().to_owned())
}

impl TextTranslator for QwenMtTranslator {
    fn translate(
        &self,
        request: TranslationRequest,
    ) -> BoxFuture<'_, Result<String, ProviderError>> {
        Box::pin(self.run(request))
    }
}
