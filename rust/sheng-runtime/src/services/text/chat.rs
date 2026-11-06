//! OpenAI-compatible chat completions (`openAICompatible`, `chatMock`, xAI text), streamed as
//! server-sent events (platform.openai.com/docs/api-reference/chat, docs.x.ai api-reference).

use futures_util::StreamExt;
use serde_json::{json, Value};
use url::{Host, Url};

use super::response::{bearer, finish_translation, post, Headers};
use crate::config::endpoint::ProxyChoice;
use crate::config::secret::Secret;
use crate::provider::{BoxFuture, ProviderError, TextTranslator, TranslationRequest};
use crate::transport::{SseDecoder, SseEvent};

/// Alibaba Cloud Model Studio (DashScope) compatible mode hosts.
const DASHSCOPE_HOST_MARK: &str = "dashscope";
const GEMINI_HOST: &str = "generativelanguage.googleapis.com";
/// Ollama's default local port (docs.ollama.com/api/openai-compatibility).
const OLLAMA_PORT: u16 = 11434;

/// Qwen models on DashScope that think unless `enable_thinking:false` is sent.
const DASHSCOPE_THINKING: &[&str] = &["qwen3", "qwen-plus", "qwen-flash", "qwen-turbo"];
/// Gemini models whose thinking `reasoning_effort:"none"` turns off.
const GEMINI_THINKING: &[&str] = &["gemini-2.5-flash"];
/// Ollama models with a thinking switch (`reasoning_effort:"none"` requests no thinking).
const OLLAMA_THINKING: &[&str] = &["qwen3", "deepseek-r1", "gpt-oss", "magistral"];

fn starts_with_any(model: &str, prefixes: &[&str]) -> bool {
    let model = model.to_ascii_lowercase();
    prefixes.iter().any(|prefix| model.starts_with(prefix))
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// Request field that turns thinking off for models known to support the switch, using each
/// service's compatible-API parameter name. Unknown services get no extra field.
pub fn thinking_switch(endpoint: &str, model: &str) -> Option<(&'static str, Value)> {
    let url = Url::parse(endpoint).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    if host.contains(DASHSCOPE_HOST_MARK) && starts_with_any(model, DASHSCOPE_THINKING) {
        return Some(("enable_thinking", Value::Bool(false)));
    }
    if host == GEMINI_HOST && starts_with_any(model, GEMINI_THINKING) {
        return Some(("reasoning_effort", json!("none")));
    }
    let is_ollama = is_loopback(&url) && url.port() == Some(OLLAMA_PORT);
    if is_ollama && starts_with_any(model, OLLAMA_THINKING) {
        return Some(("reasoning_effort", json!("none")));
    }
    None
}

/// Streaming request body: system instruction plus the sentence; no history.
pub fn chat_request_body(endpoint: &str, model: &str, request: &TranslationRequest) -> Value {
    let instruction =
        super::prompt::system_instruction(&request.source_language, &request.target_language);
    let mut body = json!({
        "model": model,
        "stream": true,
        "messages": [
            {"role": "system", "content": instruction},
            {"role": "user", "content": request.text},
        ],
    });
    if let (Some((name, value)), Some(object)) =
        (thinking_switch(endpoint, model), body.as_object_mut())
    {
        object.insert(name.to_owned(), value);
    }
    body
}

/// Accumulates `choices[0].delta.content` from streamed chunks. Thinking fields
/// (`reasoning_content`, `reasoning`, …) are ignored. Input bytes may split anywhere,
/// including inside a multi-byte character.
#[derive(Debug, Default)]
pub struct ChatDecoder {
    sse: SseDecoder,
    text: String,
    done: bool,
    /// A chunk carried a non-null `finish_reason`.
    finished: bool,
}

impl ChatDecoder {
    /// Feeds raw body bytes. An `error` object in the stream is a service error.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), ProviderError> {
        for event in self.sse.push(bytes)? {
            match event {
                SseEvent::Done => self.done = true,
                SseEvent::Data(data) if !self.done => self.chunk(&data)?,
                SseEvent::Data(_) => {}
            }
        }
        Ok(())
    }

    fn chunk(&mut self, data: &str) -> Result<(), ProviderError> {
        let value: Value = serde_json::from_str(data).map_err(|_| ProviderError::Provider)?;
        if value.get("error").is_some_and(|error| !error.is_null()) {
            return Err(ProviderError::Provider);
        }
        let content = value
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str);
        if let Some(content) = content {
            self.text.push_str(content);
        }
        if value
            .pointer("/choices/0/finish_reason")
            .is_some_and(|reason| !reason.is_null())
        {
            self.finished = true;
        }
        Ok(())
    }

    /// True once `[DONE]` arrived.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// The translation once the body ended normally. It is complete after `[DONE]` or after a
    /// chunk with `finish_reason` (questions.md step 4 answer 4b-7); without either the
    /// stream broke off (temporary failure).
    pub fn finish(self) -> Result<String, ProviderError> {
        if !self.done && !self.finished {
            return Err(ProviderError::Unavailable { retry_after: None });
        }
        finish_translation(&self.text)
    }
}

/// Chat completions translator. `Debug` hides the credential header.
#[derive(Debug)]
pub struct ChatClient {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    headers: Headers,
}

impl ChatClient {
    /// `endpoint` is the full `/chat/completions` URL (already normalized by validation).
    pub fn new(
        endpoint: String,
        model: String,
        api_key: Secret,
        proxy: &ProxyChoice,
    ) -> Result<Self, ProviderError> {
        Ok(Self::with_client(
            super::client(proxy)?,
            endpoint,
            model,
            &api_key,
        ))
    }

    /// Same as `new` with a caller-built HTTP client (tests use short timeouts).
    pub fn with_client(
        client: reqwest::Client,
        endpoint: String,
        model: String,
        api_key: &Secret,
    ) -> Self {
        Self {
            client,
            headers: bearer(api_key),
            endpoint,
            model,
        }
    }

    async fn run(&self, request: TranslationRequest) -> Result<String, ProviderError> {
        let body = chat_request_body(&self.endpoint, &self.model, &request);
        let response = post(&self.client, &self.endpoint, &self.headers, &body).await?;
        let mut decoder = ChatDecoder::default();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ProviderError::Unavailable { retry_after: None })?;
            decoder.push(&chunk)?;
            if decoder.is_done() {
                break;
            }
        }
        decoder.finish()
    }
}

impl TextTranslator for ChatClient {
    fn translate(
        &self,
        request: TranslationRequest,
    ) -> BoxFuture<'_, Result<String, ProviderError>> {
        Box::pin(self.run(request))
    }
}
