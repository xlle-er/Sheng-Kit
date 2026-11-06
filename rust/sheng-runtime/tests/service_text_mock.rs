//! Text translators against a scripted local HTTP server (step 4b): headers and bodies on the
//! wire, status classification, streaming breaks, timeouts, and retries through the pipeline.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::http::{read_request, HttpRequest};
use serde_json::json;
use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::config::endpoint::ProxyChoice;
use sheng_runtime::config::secret::Secret;
use sheng_runtime::pipeline::{
    PipelineConfig, PipelineInput, PipelineOutput, RecoveryReason, TranslationPipeline,
};
use sheng_runtime::provider::{ProviderError, SessionError, TextTranslator, TranslationRequest};
use sheng_runtime::services::text::{ChatClient, DeepLClient, DeepLxClient};
use sheng_runtime::transport::http_client;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Instant;

/// One scripted HTTP reply; the connection closes after it.
#[derive(Clone)]
struct Reply {
    status: &'static str,
    headers: &'static str,
    chunks: Vec<String>,
    /// Pause before the body, to provoke client timeouts.
    stall: Option<Duration>,
}

fn reply(status: &'static str, body: &str) -> Reply {
    Reply {
        status,
        headers: "",
        chunks: vec![body.to_owned()],
        stall: None,
    }
}

fn sse(parts: &[&str]) -> Reply {
    let mut chunks: Vec<String> = parts
        .iter()
        .map(|part| {
            let chunk = json!({"choices": [{"index": 0, "delta": {"content": part}}]});
            format!("data: {chunk}\n\n")
        })
        .collect();
    chunks.push("data: [DONE]\n\n".into());
    Reply {
        status: "200 OK",
        headers: "content-type: text/event-stream\r\n",
        chunks,
        stall: None,
    }
}

type Log = Arc<Mutex<Vec<HttpRequest>>>;

async fn serve(mut stream: TcpStream, reply: Reply, log: Log) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    log.lock().unwrap().push(request);
    let head = format!(
        "HTTP/1.1 {}\r\n{}connection: close\r\n\r\n",
        reply.status, reply.headers
    );
    if stream.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    if let Some(stall) = reply.stall {
        tokio::time::sleep(stall).await;
    }
    for chunk in reply.chunks {
        if stream.write_all(chunk.as_bytes()).await.is_err() {
            return;
        }
        let _ = stream.flush().await;
    }
    let _ = stream.shutdown().await;
}

/// Serves `replies` in order, one per connection; returns the base URL and the request log.
async fn server(replies: Vec<Reply>) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let log = Log::default();
    let recorded = log.clone();
    tokio::spawn(async move {
        for reply in replies {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(serve(stream, reply, recorded.clone()));
        }
    });
    (format!("http://127.0.0.1:{port}"), log)
}

fn http(timeout: Duration) -> reqwest::Client {
    http_client(&ProxyChoice::Direct, timeout).unwrap()
}

fn chat(base: &str, key: &str) -> ChatClient {
    let endpoint = format!("{base}/v1/chat/completions");
    ChatClient::with_client(
        http(Duration::from_secs(5)),
        endpoint,
        "m".into(),
        &Secret::new(key),
    )
}

fn sentence() -> TranslationRequest {
    TranslationRequest {
        text: "Good morning".into(),
        source_language: "en".into(),
        target_language: "zh".into(),
    }
}

#[tokio::test]
async fn chat_sends_bearer_json_and_joins_the_stream() {
    let (base, log) = server(vec![sse(&["早上", "好"])]).await;
    let result = chat(&base, "secret-key").translate(sentence()).await;
    assert_eq!(result.unwrap(), "早上好");
    let request = log.lock().unwrap()[0].clone();
    assert_eq!(
        (request.method.as_str(), request.target.as_str()),
        ("POST", "/v1/chat/completions")
    );
    assert_eq!(
        request.header("authorization").unwrap(),
        "Bearer secret-key"
    );
    assert!(request
        .header("content-type")
        .unwrap()
        .contains("application/json"));
    assert_eq!(request.json()["stream"], true);
    assert_eq!(request.json()["messages"][1]["content"], "Good morning");
}

#[tokio::test]
async fn chat_without_key_sends_no_authorization() {
    let (base, log) = server(vec![sse(&["好"])]).await;
    chat(&base, "").translate(sentence()).await.unwrap();
    assert_eq!(log.lock().unwrap()[0].header("authorization"), None);
}

#[tokio::test]
async fn deepl_sends_auth_key_header_and_reads_translations() {
    let body = r#"{"translations":[{"detected_source_language":"EN","text":"早上好"}]}"#;
    let (base, log) = server(vec![reply("200 OK", body)]).await;
    let client = DeepLClient::with_client(
        http(Duration::from_secs(5)),
        format!("{base}/v2/translate"),
        &Secret::new("k:fx"),
    );
    assert_eq!(client.translate(sentence()).await.unwrap(), "早上好");
    let request = log.lock().unwrap()[0].clone();
    assert_eq!(
        (request.method.as_str(), request.target.as_str()),
        ("POST", "/v2/translate")
    );
    assert_eq!(
        request.header("authorization").unwrap(),
        "DeepL-Auth-Key k:fx"
    );
    assert_eq!(request.json()["text"], json!(["Good morning"]));
    assert_eq!(request.json()["target_lang"], "ZH-HANS");
}

#[tokio::test]
async fn deeplx_puts_token_in_header_not_query() {
    let body = r#"{"code":200,"id":1,"data":"早上好","alternatives":[]}"#;
    let (base, log) = server(vec![reply("200 OK", body)]).await;
    let client = DeepLxClient::with_client(
        http(Duration::from_secs(5)),
        format!("{base}/translate"),
        &Secret::new("tok"),
    );
    assert_eq!(client.translate(sentence()).await.unwrap(), "早上好");
    let request = log.lock().unwrap()[0].clone();
    assert_eq!(
        (request.method.as_str(), request.target.as_str()),
        ("POST", "/translate")
    );
    assert_eq!(request.header("authorization").unwrap(), "Bearer tok");
    assert_eq!(request.json()["text"], "Good morning");
}

#[tokio::test]
async fn failure_statuses_are_classified_without_service_text() {
    let replies = vec![
        reply("401 Unauthorized", r#"{"error":{"message":"bad key"}}"#),
        Reply {
            headers: "retry-after: 2\r\n",
            ..reply("429 Too Many Requests", "{}")
        },
        reply("503 Service Unavailable", "{}"),
        reply("400 Bad Request", "{}"),
    ];
    let (base, _) = server(replies).await;
    let client = chat(&base, "k");
    let expected = [
        ProviderError::Authentication,
        ProviderError::RateLimited {
            retry_after: Some(Duration::from_secs(2)),
        },
        ProviderError::Unavailable { retry_after: None },
        ProviderError::Configuration,
    ];
    for error in expected {
        assert_eq!(client.translate(sentence()).await, Err(error));
    }
}

#[tokio::test]
async fn broken_stream_and_slow_server_are_temporary() {
    let mut cut = sse(&["早上"]);
    cut.chunks.pop();
    let slow = Reply {
        stall: Some(Duration::from_secs(3)),
        ..sse(&["好"])
    };
    let (base, _) = server(vec![cut, slow]).await;
    let temporary = Err(ProviderError::Unavailable { retry_after: None });
    assert_eq!(chat(&base, "k").translate(sentence()).await, temporary);
    let impatient = ChatClient::with_client(
        http(Duration::from_millis(300)),
        format!("{base}/v1/chat/completions"),
        "m".into(),
        &Secret::new("k"),
    );
    assert_eq!(impatient.translate(sentence()).await, temporary);
}

#[tokio::test]
async fn stream_closed_after_a_finish_reason_is_complete() {
    let mut reply = sse(&["早上", "好"]);
    reply.chunks.pop();
    let stop = json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]});
    reply.chunks.push(format!("data: {stop}\n\n"));
    let (base, _) = server(vec![reply]).await;

    assert_eq!(
        chat(&base, "k").translate(sentence()).await.unwrap(),
        "早上好"
    );
}

/// Runs one final sentence through the pipeline and collects everything until it stops.
async fn through_pipeline(client: ChatClient) -> Vec<PipelineOutput> {
    let config = PipelineConfig {
        source_language: "en".into(),
        target_language: "zh".into(),
    };
    let (pipeline, mut output, _task) = TranslationPipeline::spawn(Arc::new(client), config);
    let final_ = PipelineInput::Final {
        utterance: Some(UtteranceId::Number(1)),
        text: "Good morning".into(),
    };
    assert!(pipeline.send(final_).await);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = Vec::new();
    let mut finishing = false;
    while let Ok(Some(item)) = tokio::time::timeout_at(deadline, output.recv()).await {
        let confirmed = matches!(
            item,
            PipelineOutput::Caption(CaptionEvent::ConfirmedPair { .. })
        );
        let done = matches!(item, PipelineOutput::Finished | PipelineOutput::Failed(_));
        seen.push(item);
        if confirmed && !finishing {
            finishing = pipeline.send(PipelineInput::Finish).await;
        }
        if done {
            break;
        }
    }
    seen
}

fn recovery_reasons(outputs: &[PipelineOutput]) -> Vec<RecoveryReason> {
    outputs
        .iter()
        .filter_map(|o| match o {
            PipelineOutput::Recovery(Some(recovery)) => Some(recovery.reason),
            _ => None,
        })
        .collect()
}

fn translations(outputs: &[PipelineOutput]) -> Vec<String> {
    outputs
        .iter()
        .filter_map(|o| match o {
            PipelineOutput::Caption(CaptionEvent::ConfirmedPair { translation, .. }) => {
                Some(translation.clone())
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn unauthorized_is_not_retried() {
    let (base, log) = server(vec![reply("401 Unauthorized", "{}"), sse(&["好"])]).await;
    let outputs = through_pipeline(chat(&base, "k")).await;
    assert!(outputs.contains(&PipelineOutput::Failed(
        SessionError::CredentialAuthenticationFailed
    )));
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn rate_limit_with_retry_after_recovers() {
    let limited = Reply {
        headers: "retry-after: 1\r\n",
        ..reply("429 Too Many Requests", "{}")
    };
    let (base, log) = server(vec![limited, sse(&["早上好"])]).await;
    let outputs = through_pipeline(chat(&base, "k")).await;
    assert_eq!(recovery_reasons(&outputs), [RecoveryReason::RateLimited]);
    assert_eq!(translations(&outputs), ["早上好"]);
    assert_eq!(log.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn unavailable_is_retried_until_success() {
    let (base, log) = server(vec![reply("503 Service Unavailable", "{}"), sse(&["好"])]).await;
    let outputs = through_pipeline(chat(&base, "k")).await;
    assert_eq!(
        recovery_reasons(&outputs),
        [RecoveryReason::TemporarilyUnavailable]
    );
    assert_eq!(translations(&outputs), ["好"]);
    assert_eq!(log.lock().unwrap().len(), 2);
}
