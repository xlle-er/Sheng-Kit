//! WebSocket connections with proxy support and handshake failure classification.

use std::time::Duration;

use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue, Response};
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

use super::retry_after;
use crate::config::endpoint::ProxyChoice;
use crate::config::secret::Secret;
use crate::provider::ProviderError;

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Handshake target. Header values are secrets because they may carry credentials;
/// the URL may carry a signature and is never logged.
#[derive(Clone)]
pub struct WsRequest {
    pub url: Url,
    pub headers: Vec<(&'static str, Secret)>,
}

impl std::fmt::Debug for WsRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsRequest")
            .field("host", &self.url.host_str())
            .field("headers", &self.headers.len())
            .finish()
    }
}

fn classify_handshake<T>(response: &Response<T>) -> ProviderError {
    ProviderError::from_status(response.status().as_u16(), retry_after(response.headers()))
}

/// Connects within `timeout`; failures are normalized per providers/README.md.
pub async fn connect_websocket(
    request: &WsRequest,
    proxy: &ProxyChoice,
    timeout: Duration,
) -> Result<WsStream, ProviderError> {
    tokio::time::timeout(timeout, connect_inner(request, proxy))
        .await
        .unwrap_or(Err(ProviderError::Transport))
}

async fn connect_inner(
    request: &WsRequest,
    proxy: &ProxyChoice,
) -> Result<WsStream, ProviderError> {
    let url = &request.url;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err(ProviderError::Configuration);
    }
    let host = url.host_str().ok_or(ProviderError::Configuration)?;
    let port = url
        .port_or_known_default()
        .ok_or(ProviderError::Configuration)?;
    let mut handshake = url
        .as_str()
        .into_client_request()
        .map_err(|_| ProviderError::Configuration)?;
    for (name, value) in &request.headers {
        let value =
            HeaderValue::from_str(value.expose()).map_err(|_| ProviderError::Configuration)?;
        handshake
            .headers_mut()
            .insert(HeaderName::from_static(name), value);
    }
    let tcp = super::proxy::open_tcp(host.trim_matches(['[', ']']), port, proxy)
        .await
        .map_err(|_| ProviderError::Transport)?;
    match tokio_tungstenite::client_async_tls_with_config(handshake, tcp, None, None).await {
        Ok((stream, _)) => Ok(stream),
        Err(WsError::Http(response)) => Err(classify_handshake(&response)),
        Err(_) => Err(ProviderError::Transport),
    }
}
