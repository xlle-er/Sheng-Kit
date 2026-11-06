//! Endpoint, model-name and proxy rules (providers/text.md, providers/openai.md, start-request.md).

use std::fmt;

use url::{Host, Url};

use super::request::{NetworkProxy, ProxyMode};
use super::validate::ValidationError;

/// Model names are trimmed and limited to this many UTF-8 bytes.
pub const MAX_MODEL_NAME_BYTES: usize = 256;
pub const CHAT_COMPLETIONS_SUFFIX: &str = "/chat/completions";
pub const DEEPLX_SUFFIX: &str = "/translate";
const AZURE_HOST_SUFFIX: &str = ".openai.azure.com";

fn has_control(raw: &str) -> bool {
    raw.chars().any(char::is_control)
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

fn parse_clean(raw: &str) -> Result<Url, ValidationError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || has_control(trimmed) {
        return Err(ValidationError::Endpoint);
    }
    let url = Url::parse(trimmed).map_err(|_| ValidationError::Endpoint)?;
    let has_userinfo = !url.username().is_empty() || url.password().is_some();
    if has_userinfo || url.host().is_none() {
        return Err(ValidationError::Endpoint);
    }
    Ok(url)
}

/// Normalizes a user-entered text translation base address and appends `suffix` once.
///
/// `https://` is required, `http://` only for loopback hosts. Addresses with credentials,
/// a query string or a fragment are rejected.
pub fn normalize_text_endpoint(raw: &str, suffix: &str) -> Result<String, ValidationError> {
    let url = parse_clean(raw)?;
    let scheme_ok = match url.scheme() {
        "https" => true,
        "http" => is_loopback(&url),
        _ => false,
    };
    if !scheme_ok || url.query().is_some() || url.fragment().is_some() {
        return Err(ValidationError::Endpoint);
    }
    let base = url.as_str().trim_end_matches('/');
    if base.ends_with(suffix) {
        return Ok(base.to_owned());
    }
    Ok(format!("{base}{suffix}"))
}

/// Custom speech endpoints must be `ws://` or `wss://` without credentials or fragment.
pub fn validate_speech_endpoint(raw: &str) -> Result<String, ValidationError> {
    let url = parse_clean(raw)?;
    if !matches!(url.scheme(), "ws" | "wss") || url.fragment().is_some() {
        return Err(ValidationError::Endpoint);
    }
    Ok(url.into())
}

/// Reduces an Azure OpenAI endpoint to `https://<resource>.openai.azure.com`, dropping any
/// path or query (which may carry a pasted key). Other hosts are rejected.
pub fn normalize_azure_endpoint(raw: &str) -> Result<String, ValidationError> {
    let url = parse_clean(raw)?;
    let Some(Host::Domain(host)) = url.host() else {
        return Err(ValidationError::AzureHost);
    };
    let host = host.to_ascii_lowercase();
    let resource = host.strip_suffix(AZURE_HOST_SUFFIX).unwrap_or_default();
    let is_resource = !resource.is_empty() && !resource.contains('.');
    if url.scheme() != "https" || !is_resource || url.port().is_some() {
        return Err(ValidationError::AzureHost);
    }
    Ok(format!("https://{host}"))
}

/// Trims a model name; it must be non-empty, free of control characters and at most 256 bytes.
pub fn normalize_model_name(raw: &str) -> Result<String, ValidationError> {
    let model = raw.trim();
    if model.is_empty() || has_control(model) || model.len() > MAX_MODEL_NAME_BYTES {
        return Err(ValidationError::ModelName);
    }
    Ok(model.to_owned())
}

/// DeepL keys are trimmed; inner whitespace makes the key invalid.
pub fn normalize_deepl_key(raw: &str) -> Result<String, ValidationError> {
    let key = raw.trim();
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        return Err(ValidationError::Credential);
    }
    Ok(key.to_owned())
}

/// DeepL free-plan keys end with `:fx`.
pub fn deepl_endpoint(key: &str) -> &'static str {
    if key.ends_with(":fx") {
        "https://api-free.deepl.com/v2/translate"
    } else {
        "https://api.deepl.com/v2/translate"
    }
}

/// Proxy target of a validated network configuration. `Debug` hides the URL, which may
/// carry proxy credentials.
#[derive(Clone, PartialEq, Eq)]
pub enum ProxyChoice {
    System,
    Direct,
    Http(Url),
    Socks5(Url),
}

impl fmt::Debug for ProxyChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::System => "System",
            Self::Direct => "Direct",
            Self::Http(_) => "Http(<redacted>)",
            Self::Socks5(_) => "Socks5(<redacted>)",
        })
    }
}

/// `custom` needs an `http://` or `socks5://` URL; other modes ignore the URL.
pub fn validate_proxy(proxy: &NetworkProxy) -> Result<ProxyChoice, ValidationError> {
    match proxy.mode {
        ProxyMode::System => Ok(ProxyChoice::System),
        ProxyMode::Direct => Ok(ProxyChoice::Direct),
        ProxyMode::Custom => {
            let raw = proxy.url.as_deref().ok_or(ValidationError::Proxy)?;
            let url = Url::parse(raw.trim()).map_err(|_| ValidationError::Proxy)?;
            if url.host().is_none() || has_control(raw) {
                return Err(ValidationError::Proxy);
            }
            match url.scheme() {
                "http" => Ok(ProxyChoice::Http(url)),
                "socks5" | "socks5h" => Ok(ProxyChoice::Socks5(url)),
                _ => Err(ValidationError::Proxy),
            }
        }
    }
}
