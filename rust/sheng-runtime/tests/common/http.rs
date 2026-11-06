//! One HTTP/1.1 request as a local test server receives it, shared by the service mocks.
//! Framing follows RFC 9112: the head is the request line and header fields up to the first
//! empty line; the body is exactly `Content-Length` bytes (none without the field). The
//! clients under test send JSON with a known length, so chunked bodies are rejected.

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Largest head a test server accepts.
const MAX_HEAD_BYTES: usize = 64 * 1024;
const READ_CHUNK: usize = 4096;
const HEAD_END: &[u8] = b"\r\n\r\n";

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub target: String,
    headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// Value of the first header field called `name` (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(field, _)| field.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The body parsed as JSON; panics when it is not JSON, which fails the test.
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("request body is JSON")
    }
}

/// Reads one request; `None` when the peer closes early or the message is malformed.
pub async fn read_request<S: AsyncRead + Unpin>(stream: &mut S) -> Option<HttpRequest> {
    let mut received = Vec::new();
    let head_len = loop {
        if let Some(at) = received.windows(HEAD_END.len()).position(|w| w == HEAD_END) {
            break at;
        }
        if received.len() > MAX_HEAD_BYTES || !read_more(stream, &mut received).await {
            return None;
        }
    };
    let head = std::str::from_utf8(&received[..head_len]).ok()?;
    let (method, target, headers) = parse_head(head)?;
    let mut request = HttpRequest {
        method,
        target,
        headers,
        body: Vec::new(),
    };
    if request.header("transfer-encoding").is_some() {
        return None;
    }
    let length = match request.header("content-length") {
        Some(value) => value.parse::<usize>().ok()?,
        None => 0,
    };
    let mut body = received.split_off(head_len + HEAD_END.len());
    while body.len() < length {
        if !read_more(stream, &mut body).await {
            return None;
        }
    }
    body.truncate(length);
    request.body = body;
    Some(request)
}

/// Appends whatever the peer sends next; false once it has closed or failed.
async fn read_more<S: AsyncRead + Unpin>(stream: &mut S, into: &mut Vec<u8>) -> bool {
    let mut chunk = [0u8; READ_CHUNK];
    match stream.read(&mut chunk).await {
        Ok(0) | Err(_) => false,
        Ok(count) => {
            into.extend_from_slice(&chunk[..count]);
            true
        }
    }
}

type Head = (String, String, Vec<(String, String)>);

/// Request line `METHOD SP target SP version`, then `name: value` fields.
fn parse_head(head: &str) -> Option<Head> {
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_owned();
    let target = request_line.next()?.to_owned();
    request_line.next()?.strip_prefix("HTTP/1.")?;
    let headers = lines
        .map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect::<Option<Vec<_>>>()?;
    Some((method, target, headers))
}
