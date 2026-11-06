//! DeepLX `POST /translate` (deeplx.owo.network). Same language codes as DeepL; the token goes
//! in the `Authorization: Bearer` header, never in the query string.

use serde_json::{json, Value};
use sheng_core::language::AUTO;

use super::deepl::{deepl_source, deepl_target};
use super::response::{bearer, finish_translation, post, read_json, Headers};
use crate::config::endpoint::ProxyChoice;
use crate::config::secret::Secret;
use crate::provider::{
    classify_vendor_code, BoxFuture, ProviderError, TextTranslator, TranslationRequest,
};

/// `code` values in a DeepLX reply body, mapped like HTTP statuses.
const BODY_CODES: &[(&str, ProviderError)] = &[
    ("401", ProviderError::Authentication),
    ("403", ProviderError::Authentication),
    ("429", ProviderError::RateLimited { retry_after: None }),
    ("5*", ProviderError::Unavailable { retry_after: None }),
];
const BODY_SUCCESS: i64 = 200;

/// JSON body `{"text","source_lang","target_lang"}`; an unknown source is sent as `auto`.
pub fn deeplx_request_body(request: &TranslationRequest) -> Value {
    let source = deepl_source(&request.source_language).unwrap_or_else(|| AUTO.to_owned());
    json!({
        "text": request.text,
        "source_lang": source,
        "target_lang": deepl_target(&request.target_language),
    })
}

fn body_error(value: &Value) -> Option<ProviderError> {
    let code = value.get("code")?.as_i64()?;
    if code == BODY_SUCCESS {
        return None;
    }
    Some(classify_vendor_code(&code.to_string(), BODY_CODES))
}

/// DeepLX translator. `Debug` hides the credential header.
#[derive(Debug)]
pub struct DeepLxClient {
    client: reqwest::Client,
    endpoint: String,
    headers: Headers,
}

impl DeepLxClient {
    /// `endpoint` is the normalized `/translate` URL; a blank token sends no header.
    pub fn new(
        endpoint: String,
        token: Secret,
        proxy: &ProxyChoice,
    ) -> Result<Self, ProviderError> {
        Ok(Self::with_client(super::client(proxy)?, endpoint, &token))
    }

    /// Same as `new` with a caller-built HTTP client (tests use short timeouts).
    pub fn with_client(client: reqwest::Client, endpoint: String, token: &Secret) -> Self {
        Self {
            client,
            endpoint,
            headers: bearer(token),
        }
    }

    async fn run(&self, request: TranslationRequest) -> Result<String, ProviderError> {
        let body = deeplx_request_body(&request);
        let response = post(&self.client, &self.endpoint, &self.headers, &body).await?;
        let value = read_json(response).await?;
        if let Some(error) = body_error(&value) {
            return Err(error);
        }
        let text = value
            .get("data")
            .and_then(Value::as_str)
            .ok_or(ProviderError::Provider)?;
        finish_translation(text)
    }
}

impl TextTranslator for DeepLxClient {
    fn translate(
        &self,
        request: TranslationRequest,
    ) -> BoxFuture<'_, Result<String, ProviderError>> {
        Box::pin(self.run(request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_codes_are_classified() {
        assert_eq!(body_error(&json!({"code": 200, "data": "x"})), None);
        assert_eq!(
            body_error(&json!({"code": 401})),
            Some(ProviderError::Authentication)
        );
        assert_eq!(
            body_error(&json!({"code": 503})),
            Some(ProviderError::Unavailable { retry_after: None })
        );
        assert_eq!(
            body_error(&json!({"code": 400})),
            Some(ProviderError::Provider)
        );
        assert_eq!(body_error(&json!({"data": "x"})), None);
    }
}
