//! Baidu client against a scripted local WebSocket server (127.0.0.1 only).

mod common;

use std::future::Future;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::config::check_request;
use sheng_runtime::provider::{EventSink, ProviderError, ProviderEvent, SpeechClient};
use sheng_runtime::services::baidu::{session_for, BaiduSpeech};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use url::Url;

type Server = WebSocketStream<TcpStream>;

const WAIT: Duration = Duration::from_secs(5);

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/baidu/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).expect("fixture exists")
}

/// Runs `script` against the first connection; returns the client and the server task.
async fn serve<F, Fut, T>(script: F) -> (BaiduSpeech, JoinHandle<T>)
where
    F: FnOnce(Server) -> Fut + Send + 'static,
    Fut: Future<Output = T> + Send,
    T: Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        script(tokio_tungstenite::accept_async(tcp).await.unwrap()).await
    });
    let request = common::start_request("system", "baiduTranslate", "-", "-", "zh", "en");
    let plan = check_request(&request.to_string()).unwrap();
    let endpoint = Url::parse(&format!("ws://127.0.0.1:{port}/")).unwrap();
    (
        BaiduSpeech::new(session_for(&plan, endpoint).unwrap()),
        task,
    )
}

/// Reads the START frame and confirms it.
async fn accept_start(ws: &mut Server) -> Value {
    let Some(Ok(Message::Text(text))) = ws.next().await else {
        panic!("expected START");
    };
    ws.send(Message::text(fixture("started"))).await.unwrap();
    serde_json::from_str(&text).unwrap()
}

async fn send(ws: &mut Server, name: &str) {
    ws.send(Message::text(fixture(name))).await.unwrap();
}

/// Next text frame, skipping audio; `None` when the connection ends.
async fn next_text(ws: &mut Server) -> Option<Value> {
    while let Some(Ok(message)) = ws.next().await {
        if let Message::Text(text) = message {
            return serde_json::from_str(&text).ok();
        }
    }
    None
}

async fn collect_until_terminal(events: &mut Receiver<ProviderEvent>) -> Vec<ProviderEvent> {
    let mut seen = Vec::new();
    loop {
        let event = tokio::time::timeout(WAIT, events.recv())
            .await
            .expect("event in time")
            .expect("event stream open");
        let terminal = matches!(event, ProviderEvent::Ended | ProviderEvent::Failed(_));
        seen.push(event);
        if terminal {
            return seen;
        }
    }
}

fn caption(event: CaptionEvent) -> ProviderEvent {
    ProviderEvent::Caption(event)
}

#[tokio::test]
async fn full_session_maps_results_and_ends_after_finish() {
    let (mut client, server) = serve(|mut ws| async move {
        let start = accept_start(&mut ws).await;
        let mut frames = Vec::new();
        while frames.iter().sum::<usize>() < 3000 {
            if let Some(Ok(Message::Binary(audio))) = ws.next().await {
                frames.push(audio.len());
            }
        }
        send(&mut ws, "mid").await;
        send(&mut ws, "fin_with_next_draft").await;
        let finish = next_text(&mut ws).await;
        send(&mut ws, "fin_untranslated").await;
        send(&mut ws, "ended").await;
        (start, frames, finish)
    })
    .await;
    let (sink, mut events) = EventSink::channel();
    client.connect(sink).await.unwrap();
    client.send_audio(vec![0; 3000]).await.unwrap();
    let mut seen = vec![events.recv().await.unwrap()];
    seen.push(events.recv().await.unwrap());
    client.finish().await.unwrap();
    seen.extend(collect_until_terminal(&mut events).await);
    client.disconnect().await;

    let n = |n| Some(UtteranceId::Number(n));
    assert_eq!(
        seen,
        vec![
            ProviderEvent::Ready,
            caption(CaptionEvent::PreviewPair {
                utterance: n(1),
                original: "早上".into(),
                translation: "Morning".into()
            }),
            caption(CaptionEvent::ConfirmedPair {
                utterance: n(1),
                original: "早上好，".into(),
                translation: "Good morning,".into()
            }),
            caption(CaptionEvent::OriginalDraft {
                utterance: n(2),
                text: "下午".into()
            }),
            caption(CaptionEvent::OriginalFinal {
                utterance: n(2),
                text: "谢谢。".into()
            }),
            ProviderEvent::Ended,
        ]
    );
    let (start, frames, finish) = server.await.unwrap();
    assert_eq!(start["type"], "START");
    assert_eq!(
        (start["from"].clone(), start["to"].clone()),
        ("zh".into(), "en".into())
    );
    assert_eq!(start["sampling_rate"], 16_000);
    assert_eq!(frames, vec![1280, 1280, 440]);
    assert_eq!(finish.unwrap()["type"], "FINISH");
}

#[tokio::test]
async fn close_before_finish_is_a_transport_error() {
    let (mut client, _server) = serve(|mut ws| async move {
        accept_start(&mut ws).await;
        send(&mut ws, "mid").await;
        ws.close(None).await.unwrap();
    })
    .await;
    let (sink, mut events) = EventSink::channel();
    client.connect(sink).await.unwrap();
    let seen = collect_until_terminal(&mut events).await;
    assert_eq!(
        seen.last(),
        Some(&ProviderEvent::Failed(ProviderError::Transport))
    );
    assert_eq!(seen.len(), 3, "Ready, draft, failure");
}

#[tokio::test]
async fn close_after_finish_without_end_ends_normally() {
    let (mut client, _server) = serve(|mut ws| async move {
        accept_start(&mut ws).await;
        next_text(&mut ws).await;
        send(&mut ws, "fin_with_next_draft").await;
        ws.close(None).await.unwrap();
    })
    .await;
    let (sink, mut events) = EventSink::channel();
    client.connect(sink).await.unwrap();
    client.finish().await.unwrap();
    let seen = collect_until_terminal(&mut events).await;
    assert!(seen.iter().any(|e| matches!(
        e,
        ProviderEvent::Caption(CaptionEvent::ConfirmedPair { .. })
    )));
    assert_eq!(seen.last(), Some(&ProviderEvent::Ended));
}

async fn rejected_start(name: &'static str) -> ProviderError {
    let (mut client, _server) = serve(move |mut ws| async move {
        ws.next().await;
        send(&mut ws, name).await;
        let _ = ws.close(None).await;
    })
    .await;
    let (sink, _events) = EventSink::channel();
    client.connect(sink).await.unwrap_err()
}

#[tokio::test]
async fn rejected_start_is_classified() {
    assert_eq!(
        rejected_start("auth_error").await,
        ProviderError::Authentication
    );
    assert_eq!(
        rejected_start("language_error").await,
        ProviderError::Configuration
    );
}

#[tokio::test]
async fn recoverable_errors_are_skipped_and_fatal_errors_fail() {
    let (mut client, _server) = serve(|mut ws| async move {
        accept_start(&mut ws).await;
        send(&mut ws, "recoverable_error").await;
        send(&mut ws, "mid").await;
        ws.send(Message::text(r#"{"code":10000,"msg":"x"}"#))
            .await
            .unwrap();
        next_text(&mut ws).await;
    })
    .await;
    let (sink, mut events) = EventSink::channel();
    client.connect(sink).await.unwrap();
    let seen = collect_until_terminal(&mut events).await;
    assert_eq!(seen.len(), 3, "Ready, draft, failure: {seen:?}");
    assert_eq!(seen[2], ProviderEvent::Failed(ProviderError::Provider));
}

#[tokio::test]
async fn heartbeat_waits_for_the_pong() {
    let (mut client, _server) = serve(|mut ws| async move {
        accept_start(&mut ws).await;
        next_text(&mut ws).await;
    })
    .await;
    let (sink, _events) = EventSink::channel();
    client.connect(sink).await.unwrap();
    tokio::time::timeout(WAIT, client.heartbeat())
        .await
        .expect("pong in time")
        .unwrap();
    client.disconnect().await;
}

#[tokio::test]
async fn slow_confirmation_still_connects() {
    let (mut client, _server) = serve(|mut ws| async move {
        ws.next().await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        send(&mut ws, "started").await;
        next_text(&mut ws).await;
    })
    .await;
    let (sink, mut events) = EventSink::channel();
    client.connect(sink).await.unwrap();
    assert_eq!(events.recv().await, Some(ProviderEvent::Ready));
}

#[tokio::test]
async fn events_after_disconnect_are_dropped() {
    let (mut client, _server) = serve(|mut ws| async move {
        accept_start(&mut ws).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        for _ in 0..5 {
            if ws.send(Message::text(fixture("mid"))).await.is_err() {
                break;
            }
        }
    })
    .await;
    let (sink, mut events) = EventSink::channel();
    client.connect(sink).await.unwrap();
    client.disconnect().await;
    assert_eq!(events.recv().await, Some(ProviderEvent::Ready));
    let rest = tokio::time::timeout(WAIT, events.recv()).await.unwrap();
    assert_eq!(rest, None, "no events once the client is disconnected");
}

#[tokio::test]
async fn full_event_queue_sets_the_overflow_flag_and_stops_the_reader() {
    let (mut client, _server) = serve(|mut ws| async move {
        accept_start(&mut ws).await;
        for _ in 0..common::overflow::FLOOD {
            if ws.send(Message::text(fixture("mid"))).await.is_err() {
                break;
            }
        }
        while next_text(&mut ws).await.is_some() {}
    })
    .await;
    let (sink, mut events) = EventSink::channel();
    let flag = sink.overflow_flag();
    client.connect(sink).await.unwrap();

    common::overflow::assert_reader_stops_on_overflow(&flag, &mut events).await;
    client.disconnect().await;
}
