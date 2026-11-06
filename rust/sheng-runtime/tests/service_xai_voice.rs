//! xAI Grok Voice client against a scripted local server.

mod common;
#[path = "fixtures/xai/server.rs"]
mod server;

use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::json;
use server::{drain, flood, next, plan, recv_json, send_json, serve, sink, status};
use sheng_core::align::Stream;
use sheng_core::Source;
use sheng_runtime::provider::{ProviderError, ProviderEvent, SessionError, StreamText};
use sheng_runtime::services::xai::{self, Endpoints};

fn voice_client(endpoints: &Endpoints) -> Box<dyn sheng_runtime::provider::SpeechClient> {
    xai::speech_at(&plan("voice", "ja", "en"), Source::System, endpoints).unwrap()
}

fn delta(stream: Stream, text: &str) -> ProviderEvent {
    ProviderEvent::StreamText {
        stream,
        text: StreamText::Delta(text.into()),
    }
}

async fn accept_session(ws: &mut server::Socket) -> serde_json::Value {
    let update = recv_json(ws).await;
    send_json(ws, json!({"type": "session.created", "session": {}})).await;
    send_json(ws, json!({"type": "session.updated", "session": {}})).await;
    update
}

#[tokio::test]
async fn full_session_maps_streams_turns_and_ends_on_the_last_response() {
    let (endpoints, seen) = serve(|mut ws| async move {
        let update = accept_session(&mut ws).await;
        assert_eq!(update["type"], "session.update");
        assert_eq!(update["session"]["audio"]["input"]["format"]["rate"], 24000);
        let append = recv_json(&mut ws).await;
        assert_eq!(append["type"], "input_audio_buffer.append");
        let audio = base64::engine::general_purpose::STANDARD
            .decode(append["audio"].as_str().unwrap())
            .unwrap();
        assert_eq!(audio, [1, 0, 2, 0]);
        let item = "conversation.item.input_audio_transcription.completed";
        send_json(
            &mut ws,
            json!({"type": item, "item_id": "m1", "transcript": "こんにちは。"}),
        )
        .await;
        let spoken = "response.output_audio_transcript.delta";
        send_json(&mut ws, json!({"type": spoken, "delta": "Hello."})).await;
        send_json(
            &mut ws,
            json!({"type": "response.output_audio.delta", "delta": "AAAA"}),
        )
        .await;
        send_json(
            &mut ws,
            json!({"type": "response.done", "response": {"status": "completed"}}),
        )
        .await;
        // After input ends, the next response.done is the last turn.
        send_json(&mut ws, json!({"type": spoken, "delta": "Bye."})).await;
        send_json(
            &mut ws,
            json!({"type": "response.done", "response": {"status": "completed"}}),
        )
        .await;
        drain(ws).await;
    })
    .await;
    let mut client = voice_client(&endpoints);
    let (events, mut rx) = sink();
    client.connect(events).await.unwrap();
    let seen = seen.await.unwrap();
    assert_eq!(seen.uri, "/v1/realtime?model=grok-voice-latest");
    assert_eq!(seen.authorization.as_deref(), Some("Bearer test-key"));
    assert_eq!(next(&mut rx).await, ProviderEvent::Ready);
    client.send_audio(vec![1, 0, 2, 0]).await.unwrap();
    assert_eq!(next(&mut rx).await, delta(Stream::Original, "こんにちは。"));
    assert_eq!(next(&mut rx).await, delta(Stream::Translation, "Hello."));
    assert_eq!(next(&mut rx).await, ProviderEvent::TurnComplete);
    client.finish().await.unwrap();
    assert_eq!(next(&mut rx).await, delta(Stream::Translation, " Bye."));
    assert_eq!(next(&mut rx).await, ProviderEvent::TurnComplete);
    assert_eq!(next(&mut rx).await, ProviderEvent::Ended);
    client.disconnect().await;
}

#[tokio::test]
async fn finish_without_a_last_turn_ends_after_the_wait() {
    let (endpoints, _) = serve(|mut ws| async move {
        accept_session(&mut ws).await;
        drain(ws).await;
    })
    .await;
    let mut client = voice_client(&endpoints);
    let (events, mut rx) = sink();
    client.connect(events).await.unwrap();
    assert_eq!(next(&mut rx).await, ProviderEvent::Ready);
    let started = Instant::now();
    client.finish().await.unwrap();
    assert_eq!(next(&mut rx).await, ProviderEvent::Ended);
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(1900) && waited < Duration::from_secs(4));
    client.disconnect().await;
}

#[tokio::test]
async fn failed_response_and_error_events_fail_the_session() {
    let (endpoints, _) = serve(|mut ws| async move {
        accept_session(&mut ws).await;
        send_json(
            &mut ws,
            json!({"type": "response.done", "response": {"status": "failed"}}),
        )
        .await;
        drain(ws).await;
    })
    .await;
    let mut client = voice_client(&endpoints);
    let (events, mut rx) = sink();
    client.connect(events).await.unwrap();
    assert_eq!(next(&mut rx).await, ProviderEvent::Ready);
    let failed = next(&mut rx).await;
    assert_eq!(failed, ProviderEvent::Failed(ProviderError::Provider));
    let ProviderEvent::Failed(error) = failed else {
        unreachable!()
    };
    assert_eq!(error.speech_session_error(), SessionError::ProviderError);
    client.disconnect().await;
}

#[tokio::test]
async fn authentication_errors_during_setup_fail_connect() {
    let (endpoints, _) = serve(|mut ws| async move {
        recv_json(&mut ws).await;
        let error = json!({"type": "error", "error": {"type": "invalid_request_error",
            "code": "invalid_api_key", "message": "bad key"}});
        send_json(&mut ws, error).await;
        drain(ws).await;
    })
    .await;
    let mut client = voice_client(&endpoints);
    let (events, _rx) = sink();
    assert_eq!(
        client.connect(events).await,
        Err(ProviderError::Authentication)
    );
}

#[tokio::test]
async fn handshake_statuses_are_classified() {
    for (line, expected) in [
        (
            "401 Unauthorized",
            SessionError::CredentialAuthenticationFailed,
        ),
        (
            "403 Forbidden",
            SessionError::CredentialAuthenticationFailed,
        ),
        ("404 Not Found", SessionError::InvalidConfiguration),
        ("503 Service Unavailable", SessionError::TransportError),
    ] {
        let endpoints = status(line).await;
        let mut client = voice_client(&endpoints);
        let (events, _rx) = sink();
        let error = client.connect(events).await.unwrap_err();
        assert_eq!(error.speech_session_error(), expected, "{line}");
    }
}

#[tokio::test]
async fn dropped_or_closed_connection_is_a_transport_error() {
    for close_frame in [false, true] {
        let (endpoints, _) = serve(move |mut ws| async move {
            accept_session(&mut ws).await;
            if close_frame {
                let _ = ws.close(None).await;
            }
            // Dropping the socket ends the connection without a close handshake.
        })
        .await;
        let mut client = voice_client(&endpoints);
        let (events, mut rx) = sink();
        client.connect(events).await.unwrap();
        assert_eq!(next(&mut rx).await, ProviderEvent::Ready);
        assert_eq!(
            next(&mut rx).await,
            ProviderEvent::Failed(ProviderError::Transport),
            "close frame: {close_frame}"
        );
        client.disconnect().await;
    }
}

#[tokio::test]
async fn heartbeat_waits_for_the_pong() {
    let (endpoints, _) = serve(|mut ws| async move {
        accept_session(&mut ws).await;
        drain(ws).await;
    })
    .await;
    let mut client = voice_client(&endpoints);
    let (events, mut rx) = sink();
    client.connect(events).await.unwrap();
    assert_eq!(next(&mut rx).await, ProviderEvent::Ready);
    client.heartbeat().await.unwrap();
    client.disconnect().await;
}

#[tokio::test]
async fn nothing_arrives_after_disconnect() {
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let (endpoints, _) = serve(|mut ws| async move {
        accept_session(&mut ws).await;
        let _ = release_rx.await;
        let item = "conversation.item.input_audio_transcription.completed";
        let _ = futures_util::SinkExt::send(
            &mut ws,
            tokio_tungstenite::tungstenite::Message::Text(
                json!({"type": item, "transcript": "late"})
                    .to_string()
                    .into(),
            ),
        )
        .await;
    })
    .await;
    let mut client = voice_client(&endpoints);
    let (events, mut rx) = sink();
    client.connect(events).await.unwrap();
    assert_eq!(next(&mut rx).await, ProviderEvent::Ready);
    client.disconnect().await;
    let _ = release_tx.send(());
    let late = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await;
    assert!(matches!(late, Ok(None) | Err(_)), "{late:?}");
    assert_eq!(
        client.send_audio(vec![0; 4]).await,
        Err(ProviderError::Transport)
    );
}

#[tokio::test]
async fn full_event_queue_sets_the_overflow_flag_and_stops_the_reader() {
    let (endpoints, _seen) = serve(|mut ws| async move {
        accept_session(&mut ws).await;
        let spoken = "response.output_audio_transcript.delta";
        let deltas = (0..common::overflow::FLOOD).map(|_| json!({"type": spoken, "delta": "w"}));
        flood(&mut ws, deltas).await;
        drain(ws).await;
    })
    .await;
    let mut client = voice_client(&endpoints);
    let (events, mut rx) = sink();
    let flag = events.overflow_flag();
    client.connect(events).await.unwrap();

    common::overflow::assert_reader_stops_on_overflow(&flag, &mut rx).await;
    client.disconnect().await;
}
