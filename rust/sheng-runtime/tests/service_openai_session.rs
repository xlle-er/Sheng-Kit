//! The realtime client against a local scripted server: handshake order, captions,
//! finish with the bounded last-result wait, heartbeat, and failures (step 4c).

mod common;

#[path = "fixtures/openai/mock.rs"]
mod mock;

use std::time::{Duration, Instant};

use futures_util::SinkExt;

use sheng_core::align::Stream;
use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::provider::{EventSink, ProviderError, ProviderEvent, SpeechClient, StreamText};
use sheng_runtime::services::openai::{RealtimeClient, FINISH_WAIT};
use tokio_tungstenite::tungstenite::Message;

use mock::ServerSocket;

/// Connects a client to a fresh mock server that runs the standard handshake.
async fn connected(
    kind: sheng_runtime::services::openai::SessionKind,
) -> (
    RealtimeClient,
    ServerSocket,
    tokio::sync::mpsc::Receiver<ProviderEvent>,
    serde_json::Value,
) {
    let (port, accepted) = mock::listen().await;
    let mut client = RealtimeClient::new(mock::config(port, kind, false));
    let (sink, mut events) = EventSink::channel();
    let server = async {
        let mut socket = accepted.await.unwrap();
        let update = mock::handshake(&mut socket).await;
        (socket, update)
    };
    let (result, (socket, update)) = tokio::join!(client.connect(sink), server);
    result.expect("connects");
    assert_eq!(mock::next_event(&mut events).await, ProviderEvent::Ready);
    (client, socket, events, update)
}

/// Connect result when the server answers the handshake with `frames`.
async fn connect_against(frames: &'static [&'static str], custom: bool) -> ProviderError {
    let (port, accepted) = mock::listen().await;
    let mut client = RealtimeClient::new(mock::config(port, mock::transcribe_kind(), custom));
    let (sink, _events) = EventSink::channel();
    let server = async {
        let mut socket = accepted.await.unwrap();
        for frame in frames {
            mock::send(&mut socket, frame).await;
        }
        socket
    };
    let (result, _socket) = tokio::join!(client.connect(sink), server);
    result.expect_err("handshake fails")
}

fn item(id: &str) -> Option<UtteranceId> {
    Some(UtteranceId::Text(id.into()))
}

#[tokio::test]
async fn session_update_comes_first_and_audio_only_after_session_updated() {
    let (mut client, mut socket, _events, update) = connected(mock::transcribe_kind()).await;

    client.send_audio(mock::speech(100)).await.unwrap();

    assert_eq!(update["type"], "session.update");
    assert_eq!(update["session"]["audio"]["input"]["format"]["rate"], 24000);
    let append = mock::recv(&mut socket).await;
    assert_eq!(append["type"], "input_audio_buffer.append");
}

#[tokio::test]
async fn finish_commits_the_rest_and_still_delivers_the_last_final() {
    let (mut client, mut socket, mut events, _) = connected(mock::transcribe_kind()).await;
    client.send_audio(mock::speech(300)).await.unwrap();

    client.finish().await.unwrap();
    mock::recv_type(&mut socket, "input_audio_buffer.commit").await;
    for frame in [
        r#"{"type":"input_audio_buffer.committed","item_id":"i1"}"#,
        r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"i1","delta":"last"}"#,
        r#"{"type":"conversation.item.input_audio_transcription.completed","item_id":"i1","transcript":"last words"}"#,
    ] {
        mock::send(&mut socket, frame).await;
    }

    assert_eq!(
        mock::until_end(&mut events).await,
        vec![
            ProviderEvent::Caption(CaptionEvent::OriginalDraft {
                utterance: item("i1"),
                text: "last".into(),
            }),
            ProviderEvent::Caption(CaptionEvent::OriginalFinal {
                utterance: item("i1"),
                text: "last words".into(),
            }),
            ProviderEvent::Ended,
        ]
    );
}

#[tokio::test]
async fn finish_without_an_answer_ends_after_the_bounded_wait() {
    let (mut client, mut socket, mut events, _) = connected(mock::transcribe_kind()).await;
    client.send_audio(mock::speech(300)).await.unwrap();
    let started = Instant::now();

    client.finish().await.unwrap();
    mock::recv_type(&mut socket, "input_audio_buffer.commit").await;

    assert_eq!(
        mock::until_end(&mut events).await,
        vec![ProviderEvent::Ended]
    );
    let waited = started.elapsed();
    assert!(
        waited >= FINISH_WAIT - Duration::from_millis(100),
        "{waited:?}"
    );
    assert!(waited < FINISH_WAIT + Duration::from_secs(1), "{waited:?}");
}

#[tokio::test]
async fn translation_route_streams_both_texts_and_ends_on_session_closed() {
    let (mut client, mut socket, mut events, update) = connected(mock::translate_kind()).await;
    client.send_audio(mock::speech(100)).await.unwrap();
    let append = mock::recv(&mut socket).await;

    client.finish().await.unwrap();
    mock::recv_type(&mut socket, "session.close").await;
    for frame in [
        r#"{"type":"session.input_transcript.delta","delta":"Hello."}"#,
        r#"{"type":"session.output_transcript.delta","delta":"Bonjour."}"#,
        r#"{"type":"session.output_audio.delta","delta":"AAAA"}"#,
        r#"{"type":"session.closed"}"#,
    ] {
        mock::send(&mut socket, frame).await;
    }

    assert_eq!(update["session"]["audio"]["output"]["language"], "fr");
    assert_eq!(append["type"], "session.input_audio_buffer.append");
    let seen = mock::until_end(&mut events).await;
    let text = |stream, delta: &str| ProviderEvent::StreamText {
        stream,
        text: StreamText::Delta(delta.into()),
    };
    assert_eq!(seen[0], text(Stream::Original, "Hello."));
    assert_eq!(seen[1], text(Stream::Translation, "Bonjour."));
    assert_eq!(seen.last(), Some(&ProviderEvent::Ended));
}

#[tokio::test]
async fn heartbeat_is_a_websocket_ping_answered_by_pong() {
    let (mut client, mut socket, _events, _) = connected(mock::transcribe_kind()).await;
    let server = async {
        // Reading lets the server answer the ping; no text frame follows.
        let _ = tokio::time::timeout(Duration::from_millis(500), mock::recv(&mut socket)).await;
    };

    let (result, ()) = tokio::join!(tokio::time::timeout(mock::STEP, client.heartbeat()), server);

    assert_eq!(result.expect("pong in time"), Ok(()));
}

#[tokio::test]
async fn error_event_mid_session_fails_with_the_mapped_error() {
    let (_client, mut socket, mut events, _) = connected(mock::transcribe_kind()).await;

    mock::send(
        &mut socket,
        r#"{"type":"error","error":{"type":"invalid_request_error","code":"invalid_api_key","message":"x"}}"#,
    )
    .await;

    assert_eq!(
        mock::until_end(&mut events).await,
        vec![ProviderEvent::Failed(ProviderError::Authentication)]
    );
}

#[tokio::test]
async fn server_closing_mid_session_is_a_transport_error() {
    for close_frame in [false, true] {
        let (_client, mut socket, mut events, _) = connected(mock::transcribe_kind()).await;

        if close_frame {
            let _ = socket.close(None).await;
        }
        drop(socket);

        assert_eq!(
            mock::until_end(&mut events).await,
            vec![ProviderEvent::Failed(ProviderError::Transport)],
            "close frame: {close_frame}"
        );
    }
}

#[tokio::test]
async fn handshake_statuses_map_to_session_errors() {
    let cases = [
        ("401 Unauthorized", ProviderError::Authentication),
        ("404 Not Found", ProviderError::Configuration),
        (
            "429 Too Many Requests",
            ProviderError::RateLimited { retry_after: None },
        ),
    ];

    for (status, expected) in cases {
        let port = mock::status_server(status).await;
        let mut client = RealtimeClient::new(mock::config(port, mock::transcribe_kind(), false));
        let (sink, _events) = EventSink::channel();

        let error = client.connect(sink).await.expect_err("handshake rejected");

        assert_eq!(error, expected, "{status}");
    }
}

#[tokio::test]
async fn error_instead_of_session_updated_fails_the_connect() {
    const FRAMES: &[&str] = &[
        r#"{"type":"session.created","session":{}}"#,
        r#"{"type":"error","error":{"code":"model_not_found","message":"x"}}"#,
    ];

    assert_eq!(
        connect_against(FRAMES, false).await,
        ProviderError::Configuration
    );
}

#[tokio::test]
async fn protocol_mismatch_is_invalid_configuration_only_for_custom_endpoints() {
    const FRAMES: &[&str] = &["hello from another protocol"];

    assert_eq!(
        connect_against(FRAMES, true).await,
        ProviderError::Configuration
    );
    assert_eq!(
        connect_against(FRAMES, false).await,
        ProviderError::Provider
    );
}

#[tokio::test]
async fn full_event_queue_sets_the_overflow_flag_and_stops_the_reader() {
    let (port, accepted) = mock::listen().await;
    let mut client = RealtimeClient::new(mock::config(port, mock::transcribe_kind(), false));
    let (sink, mut events) = EventSink::channel();
    let flag = sink.overflow_flag();
    let server = async {
        let mut socket = accepted.await.unwrap();
        mock::handshake(&mut socket).await;
        socket
    };
    let (result, mut socket) = tokio::join!(client.connect(sink), server);
    result.expect("connects");

    mock::send(
        &mut socket,
        r#"{"type":"input_audio_buffer.committed","item_id":"i1"}"#,
    )
    .await;
    let delta = r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"i1","delta":"w"}"#;
    for _ in 0..common::overflow::FLOOD {
        // The client may stop reading mid-flood; a failed send just ends it.
        if socket.send(Message::text(delta)).await.is_err() {
            break;
        }
    }

    common::overflow::assert_reader_stops_on_overflow(&flag, &mut events).await;
    client.disconnect().await;
}
