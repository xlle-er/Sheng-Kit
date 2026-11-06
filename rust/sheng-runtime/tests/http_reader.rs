//! The shared test-server request reader (`common/http.rs`).

mod common;

use common::http::read_request;
use tokio::io::AsyncWriteExt;

async fn read_from(parts: &[&[u8]]) -> Option<common::http::HttpRequest> {
    let (mut client, mut server) = tokio::io::duplex(64);
    let owned: Vec<Vec<u8>> = parts.iter().map(|part| part.to_vec()).collect();
    let writer = tokio::spawn(async move {
        for part in owned {
            let _ = client.write_all(&part).await;
        }
    });
    let request = read_request(&mut server).await;
    writer.await.unwrap();
    request
}

#[tokio::test]
async fn reads_head_and_a_body_split_across_writes() {
    let request = read_from(&[
        b"POST /v1/x HTTP/1.1\r\nHost: a\r\nContent-",
        b"Length: 7\r\nAuthorization: Bearer k\r\n\r\n{\"a\":",
        b"1}",
    ])
    .await
    .expect("complete request");

    assert_eq!(
        (request.method.as_str(), request.target.as_str()),
        ("POST", "/v1/x")
    );
    assert_eq!(request.header("authorization"), Some("Bearer k"));
    assert_eq!(request.json(), serde_json::json!({"a": 1}));
}

#[tokio::test]
async fn request_without_content_length_has_an_empty_body() {
    let request = read_from(&[b"GET / HTTP/1.1\r\nHost: a\r\n\r\n"])
        .await
        .unwrap();

    assert!(request.body.is_empty());
    assert_eq!(request.header("HOST"), Some("a"));
}

#[tokio::test]
async fn chunked_truncated_or_malformed_requests_are_rejected() {
    let chunked = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n";
    let truncated = b"POST / HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc";
    let malformed = b"not a request line\r\n\r\n";

    assert!(read_from(&[chunked]).await.is_none());
    assert!(read_from(&[truncated]).await.is_none());
    assert!(read_from(&[malformed]).await.is_none());
}
