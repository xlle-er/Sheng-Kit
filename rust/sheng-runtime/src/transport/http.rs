//! HTTP client with proxy settings, status classification and a server-sent events decoder.

use std::time::Duration;

use futures_util::StreamExt;

use super::retry_after;
use crate::config::endpoint::ProxyChoice;
use crate::config::secret::Secret;
use crate::provider::ProviderError;

/// Builds a client for one proxy choice. `system` is resolved per destination host with the
/// same rules as WebSocket connections (system settings, then environment).
pub fn http_client(
    proxy: &ProxyChoice,
    timeout: Duration,
) -> Result<reqwest::Client, ProviderError> {
    let builder = reqwest::Client::builder().timeout(timeout);
    let builder = match proxy {
        ProxyChoice::System => builder.proxy(reqwest::Proxy::custom(|url| {
            let host = url.host_str()?;
            match super::system_proxy::resolve_system(host) {
                ProxyChoice::Http(proxy) | ProxyChoice::Socks5(proxy) => Some(proxy),
                ProxyChoice::Direct | ProxyChoice::System => None,
            }
        })),
        ProxyChoice::Direct => builder.no_proxy(),
        ProxyChoice::Http(url) | ProxyChoice::Socks5(url) => {
            let proxy =
                reqwest::Proxy::all(url.as_str()).map_err(|_| ProviderError::Configuration)?;
            builder.proxy(proxy)
        }
    };
    builder.build().map_err(|_| ProviderError::Configuration)
}

/// Posts JSON and returns the response when its status is a success.
pub async fn post_json(
    client: &reqwest::Client,
    url: &str,
    headers: &[(&'static str, Secret)],
    body: &serde_json::Value,
) -> Result<reqwest::Response, ProviderError> {
    let mut request = client.post(url).json(body);
    for (name, value) in headers {
        request = request.header(*name, value.expose());
    }
    let response = request.send().await.map_err(|error| {
        if error.is_timeout() {
            ProviderError::Unavailable { retry_after: None }
        } else {
            ProviderError::Transport
        }
    })?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(ProviderError::from_status(
        status.as_u16(),
        retry_after(response.headers()),
    ))
}

/// One decoded server-sent event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    Data(String),
    Done,
}

/// Incremental `data:` line decoder; `[DONE]` ends the stream. Input may split anywhere.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buffer: Vec<u8>,
    data: Vec<String>,
}

/// Longest line kept while waiting for its end.
const MAX_SSE_LINE: usize = 1024 * 1024;

impl SseDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>, ProviderError> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            self.line(line.trim_end_matches(['\r', '\n']), &mut events);
        }
        if self.buffer.len() > MAX_SSE_LINE {
            return Err(ProviderError::Provider);
        }
        Ok(events)
    }

    fn line(&mut self, line: &str, events: &mut Vec<SseEvent>) {
        if line.is_empty() {
            if !self.data.is_empty() {
                let data = self.data.join("\n");
                self.data.clear();
                events.push(if data == "[DONE]" {
                    SseEvent::Done
                } else {
                    SseEvent::Data(data)
                });
            }
            return;
        }
        if let Some(value) = line.strip_prefix("data:") {
            self.data
                .push(value.strip_prefix(' ').unwrap_or(value).to_owned());
        }
    }
}

/// Reads a streaming response into decoded events until `[DONE]` or the end of the body.
/// A body that ends early is a temporary failure.
pub async fn read_sse(response: reqwest::Response) -> Result<Vec<String>, ProviderError> {
    let mut decoder = SseDecoder::default();
    let mut data = Vec::new();
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|_| ProviderError::Unavailable { retry_after: None })?;
        for event in decoder.push(&chunk)? {
            match event {
                SseEvent::Data(text) => data.push(text),
                SseEvent::Done => return Ok(data),
            }
        }
    }
    Err(ProviderError::Unavailable { retry_after: None })
}
