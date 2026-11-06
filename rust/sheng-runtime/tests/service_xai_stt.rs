//! xAI streaming STT client against a scripted local server.

mod common;
#[path = "fixtures/xai/server.rs"]
mod server;

use std::time::{Duration, Instant};

use serde_json::json;
use server::{drain, flood, next, plan, recv_binary, recv_json, send_json, serve, sink};
use sheng_core::{CaptionEvent, Source, UtteranceId};
use sheng_runtime::provider::{ProviderError, ProviderEvent, SessionError, SpeechClient};
use sheng_runtime::services::xai::{self, Endpoints};

fn stt_client(endpoints: &Endpoints, source: &str) -> Box<dyn SpeechClient> {
    xai::speech_at(&plan("asrMt", source, "en"), Source::Microphone, endpoints).unwrap()
}

fn partial(text: &str, is_final: bool, speech_final: bool) -> serde_json::Value {
    json!({"type": "transcript.partial", "text": text, "words": [], "is_final": is_final,
        "speech_final": speech_final, "start": 0.0, "duration": 0.5})
}

fn draft(n: u64, text: &str) -> ProviderEvent {
    ProviderEvent::Caption(CaptionEvent::OriginalDraft {
        utterance: Some(UtteranceId::Number(n)),
        text: text.into(),
    })
}

fn last(n: u64, text: &str) -> ProviderEvent {
    ProviderEvent::Caption(CaptionEvent::OriginalFinal {
        utterance: Some(UtteranceId::Number(n)),
        text: text.into(),
    })
}

async fn created(ws: &mut server::Socket) {
    send_json(ws, json!({"type": "transcript.created", "id": "session-1"})).await;
}

#[tokio::test]
async fn session_streams_binary_audio_and_ends_on_transcript_done() {
    let (endpoints, seen) = serve(|mut ws| async move {
        created(&mut ws).await;
        assert_eq!(recv_binary(&mut ws).await, [9, 0, 8, 0]);
        send_json(&mut ws, partial("hello", false, false)).await;
        send_json(&mut ws, partial("hello world.", true, true)).await;
        send_json(&mut ws, partial("and", false, false)).await;
        assert_eq!(recv_json(&mut ws).await, json!({"type": "audio.done"}));
        send_json(&mut ws, partial("and more.", true, true)).await;
        let done = json!({"type": "transcript.done", "text": "hello world. and more.",
            "words": [], "duration": 3.0});
        send_json(&mut ws, done).await;
        drain(ws).await;
    })
    .await;
    let mut client = stt_client(&endpoints, "en");
    let (events, mut rx) = sink();
    client.connect(events).await.unwrap();
    let seen = seen.await.unwrap();
    assert_eq!(
        seen.uri,
        "/v1/stt?model=grok-voice-transcribe-2.0&encoding=pcm&sample_rate=24000\
         &interim_results=true&language=en"
    );
    assert_eq!(seen.authorization.as_deref(), Some("Bearer test-key"));
    assert_eq!(next(&mut rx).await, ProviderEvent::Ready);
    client.send_audio(vec![9, 0, 8, 0]).await.unwrap();
    assert_eq!(next(&mut rx).await, draft(1, "hello"));
    assert_eq!(next(&mut rx).await, last(1, "hello world."));
    assert_eq!(next(&mut rx).await, draft(2, "and"));
    client.finish().await.unwrap();
    assert_eq!(next(&mut rx).await, last(2, "and more."));
    assert_eq!(next(&mut rx).await, ProviderEvent::Ended);
    client.disconnect().await;
}

#[tokio::test]
async fn finish_timeout_confirms_the_remainder_within_one_second() {
    let (endpoints, _) = serve(|mut ws| async move {
        created(&mut ws).await;
        send_json(&mut ws, partial("unfinished", true, false)).await;
        drain(ws).await;
    })
    .await;
    let mut client = stt_client(&endpoints, "auto");
    let (events, mut rx) = sink();
    client.connect(events).await.unwrap();
    assert_eq!(next(&mut rx).await, ProviderEvent::Ready);
    assert_eq!(next(&mut rx).await, draft(1, "unfinished"));
    let started = Instant::now();
    client.finish().await.unwrap();
    assert_eq!(next(&mut rx).await, last(1, "unfinished"));
    assert_eq!(next(&mut rx).await, ProviderEvent::Ended);
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(900) && waited < Duration::from_secs(2));
    client.disconnect().await;
}

#[tokio::test]
async fn service_closing_after_finish_ends_the_session() {
    let (endpoints, _) = serve(|mut ws| async move {
        created(&mut ws).await;
        recv_json(&mut ws).await;
        let _ = ws.close(None).await;
    })
    .await;
    let mut client = stt_client(&endpoints, "auto");
    let (events, mut rx) = sink();
    client.connect(events).await.unwrap();
    assert_eq!(next(&mut rx).await, ProviderEvent::Ready);
    client.finish().await.unwrap();
    assert_eq!(next(&mut rx).await, ProviderEvent::Ended);
    client.disconnect().await;
}

#[tokio::test]
async fn error_message_fails_and_drop_is_transport() {
    let (endpoints, _) = serve(|mut ws| async move {
        created(&mut ws).await;
        send_json(
            &mut ws,
            json!({"type": "error", "message": "pipeline failure"}),
        )
        .await;
        drain(ws).await;
    })
    .await;
    let mut client = stt_client(&endpoints, "auto");
    let (events, mut rx) = sink();
    client.connect(events).await.unwrap();
    assert_eq!(next(&mut rx).await, ProviderEvent::Ready);
    assert_eq!(
        next(&mut rx).await,
        ProviderEvent::Failed(ProviderError::Provider)
    );
    client.disconnect().await;

    let (endpoints, _) = serve(|mut ws| async move { created(&mut ws).await }).await;
    let mut client = stt_client(&endpoints, "auto");
    let (events, mut rx) = sink();
    client.connect(events).await.unwrap();
    assert_eq!(next(&mut rx).await, ProviderEvent::Ready);
    let dropped = next(&mut rx).await;
    let ProviderEvent::Failed(error) = dropped else {
        panic!("{dropped:?}");
    };
    assert_eq!(error.speech_session_error(), SessionError::TransportError);
}

#[tokio::test]
async fn connection_closed_before_ready_fails_connect() {
    let (endpoints, _) = serve(|ws| async move { drop(ws) }).await;
    let mut client = stt_client(&endpoints, "auto");
    let (events, _rx) = sink();
    assert_eq!(client.connect(events).await, Err(ProviderError::Transport));
}

#[test]
fn production_factory_builds_both_routes_without_connecting() {
    assert!(xai::speech(&plan("voice", "auto", "zh"), Source::System).is_ok());
    assert!(xai::speech(&plan("asrMt", "auto", "zh"), Source::Microphone).is_ok());
}

#[tokio::test]
async fn full_event_queue_sets_the_overflow_flag_and_stops_the_reader() {
    let (endpoints, _seen) = serve(|mut ws| async move {
        created(&mut ws).await;
        let drafts = (0..common::overflow::FLOOD).map(|n| partial(&format!("w{n}"), false, false));
        flood(&mut ws, drafts).await;
        drain(ws).await;
    })
    .await;
    let mut client = stt_client(&endpoints, "en");
    let (events, mut rx) = sink();
    let flag = events.overflow_flag();
    client.connect(events).await.unwrap();

    common::overflow::assert_reader_stops_on_overflow(&flag, &mut rx).await;
    client.disconnect().await;
}
