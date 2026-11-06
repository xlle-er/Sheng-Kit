//! Generic network transport: TCP through proxies, WebSocket and HTTP clients.
//! No service-specific behavior; nothing here connects anywhere on its own.

pub mod http;
pub mod proxy;
pub mod system_proxy;
pub mod websocket;

use std::time::Duration;

/// Handshake/connect timeout for speech services (snapshot.md).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest `Retry-After` honored, so a service cannot stall a session indefinitely.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

/// Parses a `Retry-After` header given in seconds.
pub fn retry_after(headers: &tokio_tungstenite::tungstenite::http::HeaderMap) -> Option<Duration> {
    let seconds: u64 = headers
        .get("retry-after")?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(seconds).min(MAX_RETRY_AFTER))
}

pub use http::{http_client, post_json, read_sse, SseDecoder, SseEvent};
pub use proxy::open_tcp;
pub use websocket::{connect_websocket, WsRequest, WsStream};
