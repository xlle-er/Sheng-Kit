//! Volcano AST v2 client against a scripted local server (127.0.0.1 only).

#[path = "fixtures/volcano/proto.rs"]
mod proto;
#[path = "fixtures/volcano/server.rs"]
mod server;

mod common;

use std::time::Duration;

use futures_util::StreamExt;
use proto::events as ev;
use server::{accept_start, next_request, send, serve, summarize, Seen, KEY};
use sheng_core::{CaptionEvent, Source};
use sheng_runtime::provider::{EventSink, ProviderError, ProviderEvent, SpeechClient};
use sheng_runtime::services::volcano::frames::FRAME_BYTES;
use sheng_runtime::services::volcano::{self, VolcanoClient, RESOURCE_ID};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const WAIT: Duration = Duration::from_secs(5);

async fn next(rx: &mut mpsc::Receiver<ProviderEvent>) -> Option<ProviderEvent> {
    tokio::time::timeout(WAIT, rx.recv())
        .await
        .expect("event in time")
}

async fn connected(client: &mut VolcanoClient) -> mpsc::Receiver<ProviderEvent> {
    let (sink, mut rx) = EventSink::channel();
    client.connect(sink).await.expect("connects");
    assert_eq!(next(&mut rx).await, Some(ProviderEvent::Ready));
    rx
}

/// Events until the terminal one (`Ended` or `Failed`), inclusive.
async fn until_terminal(rx: &mut mpsc::Receiver<ProviderEvent>) -> Vec<ProviderEvent> {
    let mut events = Vec::new();
    while let Some(event) = next(rx).await {
        let terminal = matches!(event, ProviderEvent::Ended | ProviderEvent::Failed(_));
        events.push(event);
        if terminal {
            break;
        }
    }
    events
}

#[tokio::test]
async fn handshake_carries_key_and_resource_and_waits_for_session_started() {
    let (log, mut seen) = mpsc::unbounded_channel::<Seen>();
    let (port, mut headers) = serve(1, move |mut ws, _| {
        let log = log.clone();
        async move {
            let _ = log.send(accept_start(&mut ws).await);
            while next_request(&mut ws).await.is_some() {}
        }
    })
    .await;
    let mut client = server::client(port);
    let _rx = connected(&mut client).await;
    let headers = headers.recv().await.unwrap();
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(header("x-api-key").as_deref(), Some(KEY));
    assert_eq!(header("x-api-resource-id").as_deref(), Some(RESOURCE_ID));
    assert!(header("x-api-connect-id").is_some_and(|v| !v.is_empty()));
    let start = seen.recv().await.unwrap();
    assert_eq!(start.event, 100);
    assert!(!start.session.is_empty());
    client.disconnect().await;
}

#[tokio::test]
async fn sends_80ms_frames_and_reports_last_finals_after_finish() {
    let (log, mut seen) = mpsc::unbounded_channel::<Seen>();
    let (port, _) = serve(1, move |mut ws, _| {
        let log = log.clone();
        async move {
            let _ = log.send(accept_start(&mut ws).await);
            while let Some(bytes) = next_request(&mut ws).await {
                let request = summarize(&bytes);
                let finished = request.event == 102;
                let _ = log.send(request);
                if finished {
                    send(
                        &mut ws,
                        proto::response(ev::SOURCE_START, None, Some(0), None),
                    )
                    .await;
                    let original =
                        proto::response(ev::SOURCE_END, Some("最后一句。"), Some(0), None);
                    send(&mut ws, original).await;
                    let translation =
                        proto::response(ev::TRANSLATION_END, Some("Last line."), Some(0), None);
                    send(&mut ws, translation).await;
                    let done = proto::response(ev::SESSION_FINISHED, None, None, None);
                    send(&mut ws, done).await;
                }
            }
        }
    })
    .await;
    let mut client = server::client(port);
    let mut rx = connected(&mut client).await;
    for _ in 0..3 {
        client.send_audio(vec![1u8; 2000]).await.unwrap();
    }
    client.finish().await.unwrap();
    let events = until_terminal(&mut rx).await;
    assert_eq!(events.last(), Some(&ProviderEvent::Ended));
    let finals: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            ProviderEvent::Caption(CaptionEvent::OriginalFinal { text, .. })
            | ProviderEvent::Caption(CaptionEvent::TranslationFinal { text, .. }) => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(finals, ["最后一句。", "Last line."]);

    let mut requests = Vec::new();
    while let Ok(request) = seen.try_recv() {
        requests.push(request);
    }
    let shape: Vec<(u64, usize)> = requests.iter().map(|r| (r.event, r.audio)).collect();
    let frame = FRAME_BYTES;
    assert_eq!(
        shape,
        [(100, 0), (200, frame), (200, frame), (200, frame), (102, 0)]
    );
    assert!(requests.iter().all(|r| r.session == requests[0].session));
    client.disconnect().await;
}

/// Answers every connection with a fixed HTTP status.
async fn status_server(status_line: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = tcp.read(&mut buf).await;
            let reply = format!(
                "HTTP/1.1 {status_line}\r\nRetry-After: 2\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = tcp.write_all(reply.as_bytes()).await;
        }
    });
    port
}

#[tokio::test]
async fn handshake_failures_are_classified_by_status() {
    let retry = Some(Duration::from_secs(2));
    let cases = [
        ("401 Unauthorized", ProviderError::Authentication),
        ("403 Forbidden", ProviderError::Authentication),
        ("400 Bad Request", ProviderError::Configuration),
        ("404 Not Found", ProviderError::Configuration),
        (
            "429 Too Many Requests",
            ProviderError::RateLimited { retry_after: retry },
        ),
        (
            "503 Service Unavailable",
            ProviderError::Unavailable { retry_after: retry },
        ),
    ];
    for (status, expected) in cases {
        let port = status_server(status).await;
        let (sink, _rx) = EventSink::channel();
        let result = server::client(port).connect(sink).await;
        assert_eq!(result, Err(expected), "{status}");
    }
}

#[tokio::test]
async fn session_failed_at_start_and_missing_session_started() {
    let (port, _) = serve(1, |mut ws, _| async move {
        let _ = next_request(&mut ws).await;
        send(&mut ws, proto::failure(ev::SESSION_FAILED, 45_000_001)).await;
    })
    .await;
    let (sink, _rx) = EventSink::channel();
    let result = server::client(port).connect(sink).await;
    assert_eq!(result, Err(ProviderError::Configuration));

    let (port, _) = serve(1, |mut ws, _| async move {
        while next_request(&mut ws).await.is_some() {}
    })
    .await;
    let config = server::config(port, "zh", Duration::from_millis(300));
    let (sink, _rx) = EventSink::channel();
    let result = VolcanoClient::new(config).connect(sink).await;
    assert_eq!(result, Err(ProviderError::Transport));
}

#[tokio::test]
async fn close_without_session_finished_is_a_transport_error() {
    for drop_socket in [false, true] {
        let (port, _) = serve(1, move |mut ws, _| async move {
            accept_start(&mut ws).await;
            send(
                &mut ws,
                proto::response(ev::SOURCE_RESPONSE, Some("半句"), None, None),
            )
            .await;
            if !drop_socket {
                let _ = ws.close(None).await;
            }
        })
        .await;
        let mut client = server::client(port);
        let mut rx = connected(&mut client).await;
        let events = until_terminal(&mut rx).await;
        assert!(events.len() >= 2, "the draft comes first: {events:?}");
        assert_eq!(
            events.last(),
            Some(&ProviderEvent::Failed(ProviderError::Transport))
        );
    }
}

#[tokio::test]
async fn close_after_finish_session_ends_with_the_tail() {
    let (port, _) = serve(1, |mut ws, _| async move {
        accept_start(&mut ws).await;
        send(
            &mut ws,
            proto::response(ev::SOURCE_RESPONSE, Some("最后半句"), None, None),
        )
        .await;
        // The only request after the start is FinishSession (no audio was sent).
        let _ = next_request(&mut ws).await;
        let _ = ws.close(None).await;
    })
    .await;
    let mut client = server::client(port);
    let mut rx = connected(&mut client).await;
    assert!(matches!(
        next(&mut rx).await,
        Some(ProviderEvent::Caption(CaptionEvent::OriginalDraft { .. }))
    ));
    client.finish().await.unwrap();

    assert_eq!(until_terminal(&mut rx).await, [ProviderEvent::Ended]);
}

#[tokio::test]
async fn malformed_frame_is_a_provider_error() {
    let (port, _) = serve(1, |mut ws, _| async move {
        accept_start(&mut ws).await;
        send(&mut ws, vec![0x0b, 0x00]).await; // wire type 3 (group)
        while next_request(&mut ws).await.is_some() {}
    })
    .await;
    let mut client = server::client(port);
    let mut rx = connected(&mut client).await;
    assert_eq!(
        until_terminal(&mut rx).await,
        [ProviderEvent::Failed(ProviderError::Provider)]
    );
}

fn late_final(text: &str) -> Vec<u8> {
    proto::response(ev::SOURCE_END, Some(text), None, None)
}

#[tokio::test]
async fn events_after_disconnect_or_reconnect_are_dropped() {
    let (port, _) = serve(2, |mut ws, index| async move {
        accept_start(&mut ws).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let text = if index == 0 { "old" } else { "new" };
        send(&mut ws, late_final(text)).await;
        while next_request(&mut ws).await.is_some() {}
    })
    .await;
    let mut client = server::client(port);
    let mut first = connected(&mut client).await;
    let mut second = connected(&mut client).await;
    assert_eq!(
        next(&mut first).await,
        None,
        "old connection reports nothing"
    );
    match next(&mut second).await {
        Some(ProviderEvent::Caption(CaptionEvent::OriginalFinal { text, .. })) => {
            assert_eq!(text, "new")
        }
        other => panic!("unexpected {other:?}"),
    }
    client.disconnect().await;
    assert_eq!(next(&mut second).await, None, "nothing after disconnect");
}

#[tokio::test]
async fn heartbeat_is_a_websocket_ping() {
    let (pinged, mut ping_rx) = mpsc::unbounded_channel::<()>();
    let (port, _) = serve(1, move |mut ws, _| {
        let pinged = pinged.clone();
        async move {
            accept_start(&mut ws).await;
            while let Some(Ok(message)) = ws.next().await {
                if matches!(message, Message::Ping(_)) {
                    let _ = pinged.send(());
                }
            }
        }
    })
    .await;
    let mut client = server::client(port);
    let _rx = connected(&mut client).await;
    client.heartbeat().await.unwrap();
    tokio::time::timeout(WAIT, ping_rx.recv())
        .await
        .unwrap()
        .unwrap();
    client.disconnect().await;
}

#[tokio::test]
async fn full_event_queue_sets_the_overflow_flag_and_stops_the_reader() {
    let (port, _) = serve(1, |mut ws, _| async move {
        accept_start(&mut ws).await;
        send(
            &mut ws,
            proto::response(ev::SOURCE_START, None, Some(0), None),
        )
        .await;
        for index in 0..common::overflow::FLOOD {
            let text = format!("词{index}");
            send(
                &mut ws,
                proto::response(ev::SOURCE_RESPONSE, Some(&text), None, None),
            )
            .await;
        }
        while next_request(&mut ws).await.is_some() {}
    })
    .await;
    let mut client = server::client(port);
    let (sink, mut rx) = EventSink::channel();
    let flag = sink.overflow_flag();
    client.connect(sink).await.expect("connects");

    common::overflow::assert_reader_stops_on_overflow(&flag, &mut rx).await;
    client.disconnect().await;
}

#[tokio::test]
async fn audio_before_connect_fails_and_factory_builds_from_api_key() {
    let mut client = server::client(1);
    assert_eq!(
        client.send_audio(vec![0; 10]).await,
        Err(ProviderError::Transport)
    );
    let request = common::start_request("system", "volcanoEngine", "simultaneous", "-", "zh", "en");
    let plan = sheng_runtime::config::check_request(&request.to_string()).unwrap();
    assert!(volcano::speech(&plan, Source::System).is_ok());
    assert!(!format!("{plan:?}").contains("placeholder-key"));
}
