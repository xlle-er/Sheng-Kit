//! Transport against local loopback servers and proxies only.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use sheng_runtime::config::endpoint::ProxyChoice;
use sheng_runtime::config::secret::Secret;
use sheng_runtime::provider::{ProviderError, SessionError};
use sheng_runtime::transport::proxy::base64;
use sheng_runtime::transport::{
    connect_websocket, http_client, post_json, read_sse, SseDecoder, SseEvent, WsRequest,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use url::Url;

const TIMEOUT: Duration = Duration::from_secs(5);

#[allow(clippy::result_large_err)] // the error type is fixed by tungstenite's callback signature
/// WebSocket echo server; returns its port. Records the `x-test-key` header it saw.
async fn echo_server() -> (u16, tokio::sync::oneshot::Receiver<Option<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut seen = None;
        let callback = |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                        response| {
            seen = request
                .headers()
                .get("x-test-key")
                .map(|v| v.to_str().unwrap().to_owned());
            Ok(response)
        };
        let mut ws = tokio_tungstenite::accept_hdr_async(tcp, callback)
            .await
            .unwrap();
        let _ = seen_tx.send(seen);
        while let Some(Ok(message)) = ws.next().await {
            if message.is_text() || message.is_binary() {
                ws.send(message).await.unwrap();
            }
        }
    });
    (port, seen_rx)
}

/// Answers every connection with a fixed HTTP status.
async fn status_server(status_line: &'static str, extra: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = tcp.read(&mut buf).await;
            let reply = format!(
                "HTTP/1.1 {status_line}\r\n{extra}Content-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = tcp.write_all(reply.as_bytes()).await;
        }
    });
    port
}

fn request(port: u16) -> WsRequest {
    WsRequest {
        url: Url::parse(&format!("ws://127.0.0.1:{port}/v1")).unwrap(),
        headers: vec![("x-test-key", Secret::new("loopback-secret"))],
    }
}

async fn round_trip(port: u16, proxy: &ProxyChoice) {
    let mut ws = connect_websocket(&request(port), proxy, TIMEOUT)
        .await
        .expect("connects");
    ws.send(Message::Binary(vec![1u8, 2, 3].into()))
        .await
        .unwrap();
    let echoed = ws.next().await.unwrap().unwrap();
    assert_eq!(echoed.into_data().as_ref(), &[1, 2, 3]);
}

#[tokio::test]
async fn websocket_connects_directly_and_sends_handshake_headers() {
    let (port, seen) = echo_server().await;

    round_trip(port, &ProxyChoice::Direct).await;

    assert_eq!(seen.await.unwrap().as_deref(), Some("loopback-secret"));
}

#[tokio::test]
async fn handshake_status_is_classified() {
    let cases = [
        ("401 Unauthorized", "", ProviderError::Authentication),
        ("403 Forbidden", "", ProviderError::Authentication),
        ("404 Not Found", "", ProviderError::Configuration),
        (
            "429 Too Many Requests",
            "Retry-After: 5\r\n",
            ProviderError::RateLimited {
                retry_after: Some(Duration::from_secs(5)),
            },
        ),
        (
            "503 Service Unavailable",
            "",
            ProviderError::Unavailable { retry_after: None },
        ),
    ];

    for (status, extra, expected) in cases {
        let port = status_server(status, extra).await;

        let error = connect_websocket(&request(port), &ProxyChoice::Direct, TIMEOUT)
            .await
            .unwrap_err();

        assert_eq!(error, expected, "{status}");
    }
    let unavailable = ProviderError::Unavailable { retry_after: None };
    assert_eq!(
        unavailable.speech_session_error(),
        SessionError::TransportError
    );
    assert_eq!(
        ProviderError::Authentication.speech_session_error().code(),
        "credential_authentication_failed"
    );
}

#[tokio::test]
async fn silent_server_times_out_as_transport_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let error = connect_websocket(
        &request(port),
        &ProxyChoice::Direct,
        Duration::from_millis(200),
    )
    .await
    .unwrap_err();

    assert_eq!(error, ProviderError::Transport);
    drop(listener);
}

async fn pipe(a: TcpStream, b: TcpStream) {
    let (mut ar, mut aw) = a.into_split();
    let (mut br, mut bw) = b.into_split();
    tokio::spawn(async move { tokio::io::copy(&mut ar, &mut bw).await });
    tokio::spawn(async move { tokio::io::copy(&mut br, &mut aw).await });
}

/// Minimal HTTP CONNECT proxy; checks the Basic credentials when `auth` is set.
async fn connect_proxy(auth: Option<&'static str>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut client, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(client.read_u8().await.unwrap());
        }
        let head = String::from_utf8(head).unwrap();
        if let Some(expected) = auth {
            if !head.contains(&format!(
                "Proxy-Authorization: Basic {}",
                base64(expected.as_bytes())
            )) {
                client
                    .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                    .await
                    .unwrap();
                return;
            }
        }
        let target = head.split_whitespace().nth(1).unwrap().to_owned();
        let upstream = TcpStream::connect(target).await.unwrap();
        client
            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .await
            .unwrap();
        pipe(client, upstream).await;
    });
    port
}

/// Minimal SOCKS5 proxy without authentication, domain or IPv4 targets.
async fn socks_proxy() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut client, _) = listener.accept().await.unwrap();
        let mut greeting = [0u8; 3];
        client.read_exact(&mut greeting).await.unwrap();
        client.write_all(&[5, 0]).await.unwrap();
        let mut header = [0u8; 4];
        client.read_exact(&mut header).await.unwrap();
        let host = if header[3] == 3 {
            let len = client.read_u8().await.unwrap();
            let mut name = vec![0u8; usize::from(len)];
            client.read_exact(&mut name).await.unwrap();
            String::from_utf8(name).unwrap()
        } else {
            let mut ip = [0u8; 4];
            client.read_exact(&mut ip).await.unwrap();
            std::net::Ipv4Addr::from(ip).to_string()
        };
        let target_port = client.read_u16().await.unwrap();
        let upstream = TcpStream::connect((host.as_str(), target_port))
            .await
            .unwrap();
        client
            .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
            .await
            .unwrap();
        pipe(client, upstream).await;
    });
    port
}

#[tokio::test]
async fn websocket_tunnels_through_http_connect_proxy_with_credentials() {
    let (port, _) = echo_server().await;
    let proxy_port = connect_proxy(Some("user:pa ss")).await;
    let proxy = ProxyChoice::Http(
        Url::parse(&format!("http://user:pa%20ss@127.0.0.1:{proxy_port}")).unwrap(),
    );

    round_trip(port, &proxy).await;
}

#[tokio::test]
async fn rejected_proxy_tunnel_is_a_transport_error() {
    let (port, _) = echo_server().await;
    let proxy_port = connect_proxy(Some("user:secret")).await;
    let proxy = ProxyChoice::Http(Url::parse(&format!("http://127.0.0.1:{proxy_port}")).unwrap());

    let error = connect_websocket(&request(port), &proxy, TIMEOUT)
        .await
        .unwrap_err();

    assert_eq!(error, ProviderError::Transport);
}

#[tokio::test]
async fn websocket_tunnels_through_socks5_proxy() {
    let (port, _) = echo_server().await;
    let proxy_port = socks_proxy().await;
    let proxy =
        ProxyChoice::Socks5(Url::parse(&format!("socks5://127.0.0.1:{proxy_port}")).unwrap());

    round_trip(port, &proxy).await;
}

#[test]
fn proxy_choice_debug_hides_credentials() {
    let proxy = ProxyChoice::Http(Url::parse("http://user:secret@127.0.0.1:8080").unwrap());

    let debug = format!("{proxy:?} {:?}", request(1));

    assert!(!debug.contains("secret"));
}

#[tokio::test]
async fn http_post_classifies_failure_statuses() {
    let client = http_client(&ProxyChoice::Direct, TIMEOUT).unwrap();
    let cases = [
        ("401 Unauthorized", "", ProviderError::Authentication),
        ("422 Unprocessable Entity", "", ProviderError::Configuration),
        (
            "429 Too Many Requests",
            "Retry-After: 3\r\n",
            ProviderError::RateLimited {
                retry_after: Some(Duration::from_secs(3)),
            },
        ),
        (
            "502 Bad Gateway",
            "",
            ProviderError::Unavailable { retry_after: None },
        ),
    ];

    for (status, extra, expected) in cases {
        let port = status_server(status, extra).await;
        let url = format!("http://127.0.0.1:{port}/v1/chat/completions");

        let error = post_json(&client, &url, &[], &serde_json::json!({}))
            .await
            .unwrap_err();

        assert_eq!(error, expected, "{status}");
    }
}

#[tokio::test]
async fn streamed_response_is_read_until_done() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let _ = tcp.read(&mut buf).await;
        let body = "data: {\"a\":1}\n\ndata: {\"a\":2}\n\ndata: [DONE]\n\n";
        let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        tcp.write_all(reply.as_bytes()).await.unwrap();
    });
    let client = http_client(&ProxyChoice::Direct, TIMEOUT).unwrap();

    let response = post_json(
        &client,
        &format!("http://127.0.0.1:{port}/"),
        &[],
        &serde_json::json!({}),
    )
    .await
    .unwrap();
    let data = read_sse(response).await.unwrap();

    assert_eq!(data, ["{\"a\":1}", "{\"a\":2}"]);
}

#[test]
fn sse_decoder_handles_split_chunks_and_multiline_data() {
    let mut decoder = SseDecoder::default();
    let mut events = Vec::new();

    for chunk in [
        "da",
        "ta: one\r\n",
        "data: two\n",
        "\n: comment\n\ndata: [DO",
        "NE]\n\n",
    ] {
        events.extend(decoder.push(chunk.as_bytes()).unwrap());
    }

    assert_eq!(events, [SseEvent::Data("one\ntwo".into()), SseEvent::Done]);
}

#[test]
fn base64_matches_reference_vectors() {
    let vectors = [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foobar", "Zm9vYmFy"),
    ];

    for (input, expected) in vectors {
        assert_eq!(base64(input.as_bytes()), expected);
    }
}
