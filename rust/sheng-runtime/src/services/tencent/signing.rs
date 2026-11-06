//! Signed handshake URL of the speech translation WebSocket (providers/tencent.md).
//!
//! Signature: every query parameter except `signature`, sorted by name, appended to
//! `host + path` (no scheme); HMAC-SHA1 with the SecretKey, Base64, then URL-encoded.
//! The URL carries a signature and the SecretId: it is never logged and has no `Debug`.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use rand::Rng;
use sha1::Sha1;
use url::Url;

use crate::config::secret::Secret;
use crate::provider::{ProviderError, SessionError};

/// `voice_format` for raw PCM.
pub const VOICE_FORMAT_PCM: &str = "1";
/// `trans_model`: the lighter of the two documented translation models.
pub const TRANSLATION_MODEL: &str = "hunyuan-translation-lite";
/// Signature lifetime. It is checked at the handshake only, so a short window is enough.
pub const SIGNATURE_LIFETIME_SECS: u64 = 600;
/// The documented AppId is numeric; longer values are not account ids.
const MAX_APP_ID_DIGITS: usize = 20;

/// Unreserved characters stay; everything else, including `+`, `/` and `=`, is encoded.
const QUERY_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Account credentials of one session. `Debug` never shows the values.
#[derive(Clone)]
pub struct TencentCredentials {
    app_id: String,
    secret_id: Secret,
    secret_key: Secret,
}

impl std::fmt::Debug for TencentCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TencentCredentials(<redacted>)")
    }
}

impl TencentCredentials {
    /// The AppId becomes a path segment, so only a plain decimal id is accepted.
    pub fn new(
        app_id: &Secret,
        secret_id: Secret,
        secret_key: Secret,
    ) -> Result<Self, SessionError> {
        let app_id = app_id.expose().trim();
        let numeric = !app_id.is_empty()
            && app_id.len() <= MAX_APP_ID_DIGITS
            && app_id.bytes().all(|b| b.is_ascii_digit());
        if !numeric {
            return Err(SessionError::ProviderConfigurationFailed);
        }
        Ok(Self {
            app_id: app_id.to_owned(),
            secret_id,
            secret_key,
        })
    }
}

/// Per-connection parameters; every connection gets a new timestamp, nonce and voice id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionParams {
    pub timestamp: u64,
    pub expired: u64,
    pub nonce: u32,
    pub voice_id: String,
    pub source: String,
    pub target: String,
}

impl ConnectionParams {
    /// Fresh parameters for a new connection at the current time.
    pub fn fresh(source: &str, target: &str) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        Self {
            timestamp,
            expired: timestamp + SIGNATURE_LIFETIME_SECS,
            // A positive integer of at most 10 digits.
            nonce: rand::rng().random_range(1..=u32::MAX),
            voice_id: uuid::Uuid::new_v4().to_string(),
            source: source.to_owned(),
            target: target.to_owned(),
        }
    }
}

fn encode(value: &str) -> String {
    utf8_percent_encode(value, QUERY_VALUE).to_string()
}

/// Query parameters except the signature, sorted by name, with raw values.
fn sorted_pairs(
    credentials: &TencentCredentials,
    params: &ConnectionParams,
) -> [(&'static str, String); 9] {
    let mut pairs = [
        ("secretid", credentials.secret_id.expose().to_owned()),
        ("timestamp", params.timestamp.to_string()),
        ("expired", params.expired.to_string()),
        ("nonce", params.nonce.to_string()),
        ("voice_id", params.voice_id.clone()),
        ("voice_format", VOICE_FORMAT_PCM.to_owned()),
        ("source", params.source.clone()),
        ("target", params.target.clone()),
        ("trans_model", TRANSLATION_MODEL.to_owned()),
    ];
    pairs.sort_by_key(|(name, _)| *name);
    pairs
}

fn join(pairs: &[(&str, String)], value: impl Fn(&str) -> String) -> String {
    pairs
        .iter()
        .map(|(name, raw)| format!("{name}={}", value(raw)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Sorted query with raw values, as the documented string-to-sign shows it.
pub fn sorted_query(credentials: &TencentCredentials, params: &ConnectionParams) -> String {
    join(&sorted_pairs(credentials, params), str::to_owned)
}

/// `host[:port]/path/<appid>?<query>`: the text that is signed.
pub fn string_to_sign(endpoint: &Url, app_id: &str, query: &str) -> String {
    format!(
        "{}{}{app_id}?{query}",
        endpoint.authority(),
        endpoint.path()
    )
}

/// Base64 of HMAC-SHA1(`secret_key`, `text`).
pub fn signature(secret_key: &Secret, text: &str) -> String {
    let mut mac = Hmac::<Sha1>::new_from_slice(secret_key.expose().as_bytes())
        .expect("HMAC accepts keys of any length");
    mac.update(text.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// The full handshake URL. `endpoint` is the base ending in `/` (`.../speech_translate/`).
pub fn signed_url(
    endpoint: &Url,
    credentials: &TencentCredentials,
    params: &ConnectionParams,
) -> Result<Url, ProviderError> {
    if !endpoint.path().ends_with('/') {
        return Err(ProviderError::Configuration);
    }
    let pairs = sorted_pairs(credentials, params);
    let text = string_to_sign(endpoint, &credentials.app_id, &join(&pairs, str::to_owned));
    let query = join(&pairs, encode);
    let signed = encode(&signature(&credentials.secret_key, &text));
    let url = format!(
        "{}{}?{query}&signature={signed}",
        endpoint.as_str(),
        credentials.app_id
    );
    Url::parse(&url).map_err(|_| ProviderError::Configuration)
}
