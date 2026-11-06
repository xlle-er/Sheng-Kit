//! Sending one text request and classifying its HTTP outcome. Service error bodies are never
//! read into errors: only the status and `Retry-After` matter.

use std::time::Duration;

use sheng_core::text::within_limit;

use crate::config::secret::Secret;
use crate::provider::ProviderError;
use crate::transport::MAX_RETRY_AFTER;

/// DeepL: the account's character quota is used up (developers.deepl.com error codes).
/// Retrying cannot help, so it is treated as a configuration problem.
const STATUS_QUOTA_EXCEEDED: u16 = 456;

/// Header value list for one request; values are credentials and stay redacted.
pub(crate) type Headers = Vec<(&'static str, Secret)>;

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let seconds: u64 = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(seconds).min(MAX_RETRY_AFTER))
}

/// Maps a failed HTTP status onto the normalized error (step4b brief “HTTP 错误分类”).
pub fn classify_status(status: u16, retry_after: Option<Duration>) -> ProviderError {
    if status == STATUS_QUOTA_EXCEEDED {
        return ProviderError::Configuration;
    }
    ProviderError::from_status(status, retry_after)
}

fn transport_failure(error: &reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Unavailable { retry_after: None }
    } else {
        ProviderError::Transport
    }
}

/// Posts `body` as JSON with `headers`; returns the response when its status is a success.
pub(crate) async fn post(
    client: &reqwest::Client,
    url: &str,
    headers: &Headers,
    body: &serde_json::Value,
) -> Result<reqwest::Response, ProviderError> {
    let mut request = client.post(url).json(body);
    for (name, value) in headers {
        request = request.header(*name, value.expose());
    }
    let response = request
        .send()
        .await
        .map_err(|error| transport_failure(&error))?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(classify_status(
        status.as_u16(),
        retry_after(response.headers()),
    ))
}

/// Reads a whole JSON response body. A body that breaks off is a temporary failure;
/// one that does not parse is a service error.
pub(crate) async fn read_json(
    response: reqwest::Response,
) -> Result<serde_json::Value, ProviderError> {
    let bytes = response
        .bytes()
        .await
        .map_err(|_| ProviderError::Unavailable { retry_after: None })?;
    serde_json::from_slice(&bytes).map_err(|_| ProviderError::Provider)
}

/// A translation is usable when it is non-empty after trimming and within the caption limit.
pub(crate) fn finish_translation(text: &str) -> Result<String, ProviderError> {
    let text = text.trim();
    if text.is_empty() || !within_limit(text) {
        return Err(ProviderError::Provider);
    }
    Ok(text.to_owned())
}

/// `Authorization` header for a bearer token; none for a blank token (local services).
pub(crate) fn bearer(token: &Secret) -> Headers {
    if token.is_blank() {
        return Vec::new();
    }
    vec![(
        "authorization",
        Secret::new(format!("Bearer {}", token.expose().trim())),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_retry_classes() {
        assert_eq!(classify_status(401, None), ProviderError::Authentication);
        assert_eq!(classify_status(403, None), ProviderError::Authentication);
        assert_eq!(classify_status(456, None), ProviderError::Configuration);
        let wait = Some(Duration::from_secs(2));
        assert_eq!(
            classify_status(429, wait),
            ProviderError::RateLimited { retry_after: wait }
        );
        assert_eq!(
            classify_status(503, None),
            ProviderError::Unavailable { retry_after: None }
        );
    }

    #[test]
    fn blank_token_sends_no_authorization() {
        assert!(bearer(&Secret::new("  ")).is_empty());
        let headers = bearer(&Secret::new(" k "));
        assert_eq!(headers[0].1.expose(), "Bearer k");
    }

    #[test]
    fn empty_or_oversized_translation_is_rejected() {
        assert_eq!(finish_translation("  \n"), Err(ProviderError::Provider));
        let long = "x".repeat(sheng_core::text::MAX_TEXT_BYTES + 1);
        assert_eq!(finish_translation(&long), Err(ProviderError::Provider));
        assert_eq!(finish_translation(" 你好 ").unwrap(), "你好");
    }
}
