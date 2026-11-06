//! DeepL `POST /v2/translate` (developers.deepl.com/api-reference/translate).

use serde_json::{json, Value};
use sheng_core::language::{AUTO, ZH_TW};

use super::response::{finish_translation, post, read_json, Headers};
use crate::config::endpoint::ProxyChoice;
use crate::config::secret::Secret;
use crate::provider::{BoxFuture, ProviderError, TextTranslator, TranslationRequest};

/// Target codes that differ from the upper-cased catalog code.
const TARGET_CODES: &[(&str, &str)] = &[
    ("zh", "ZH-HANS"),
    (ZH_TW, "ZH-HANT"),
    ("en", "EN-US"),
    ("pt", "PT-BR"),
    ("es-ES", "ES"),
    ("no", "NB"),
];

/// Source codes that differ from the upper-cased catalog code.
const SOURCE_CODES: &[(&str, &str)] = &[("no", "NB")];

fn mapped(code: &str, table: &[(&str, &str)]) -> String {
    table
        .iter()
        .find(|(catalog, _)| *catalog == code)
        .map_or_else(
            || code.to_ascii_uppercase(),
            |(_, deepl)| (*deepl).to_owned(),
        )
}

/// DeepL `target_lang` for a catalog target code.
pub fn deepl_target(code: &str) -> String {
    mapped(code, TARGET_CODES)
}

/// DeepL `source_lang`; `None` for `auto` (DeepL detects the language when it is omitted).
pub fn deepl_source(code: &str) -> Option<String> {
    (code != AUTO).then(|| mapped(code, SOURCE_CODES))
}

/// JSON body: `text` as a one-element list, `target_lang`, optional `source_lang`.
pub fn deepl_request_body(request: &TranslationRequest) -> Value {
    let mut body = json!({
        "text": [request.text],
        "target_lang": deepl_target(&request.target_language),
    });
    if let (Some(source), Some(object)) =
        (deepl_source(&request.source_language), body.as_object_mut())
    {
        object.insert("source_lang".to_owned(), Value::String(source));
    }
    body
}

/// DeepL translator. `Debug` hides the credential header.
#[derive(Debug)]
pub struct DeepLClient {
    client: reqwest::Client,
    endpoint: String,
    headers: Headers,
}

impl DeepLClient {
    /// `endpoint` is the free or pro translate URL chosen by the key suffix during validation.
    pub fn new(endpoint: String, key: Secret, proxy: &ProxyChoice) -> Result<Self, ProviderError> {
        Ok(Self::with_client(super::client(proxy)?, endpoint, &key))
    }

    /// Same as `new` with a caller-built HTTP client (tests use short timeouts).
    pub fn with_client(client: reqwest::Client, endpoint: String, key: &Secret) -> Self {
        let header = Secret::new(format!("DeepL-Auth-Key {}", key.expose().trim()));
        Self {
            client,
            endpoint,
            headers: vec![("authorization", header)],
        }
    }

    async fn run(&self, request: TranslationRequest) -> Result<String, ProviderError> {
        let body = deepl_request_body(&request);
        let response = post(&self.client, &self.endpoint, &self.headers, &body).await?;
        let value = read_json(response).await?;
        let text = value
            .pointer("/translations/0/text")
            .and_then(Value::as_str)
            .ok_or(ProviderError::Provider)?;
        finish_translation(text)
    }
}

impl TextTranslator for DeepLClient {
    fn translate(
        &self,
        request: TranslationRequest,
    ) -> BoxFuture<'_, Result<String, ProviderError>> {
        Box::pin(self.run(request))
    }
}
