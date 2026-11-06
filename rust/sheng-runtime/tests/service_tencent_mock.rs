//! Tencent Cloud speech client against a scripted local WebSocket server (127.0.0.1 only).

mod common;
mod service_tencent_support;

use std::time::Duration;

use service_tencent_support::*;
use sheng_core::CaptionEvent;
use sheng_runtime::provider::{EventSink, ProviderError, ProviderEvent, SpeechClient};
use sheng_runtime::services::tencent::client::MAX_FRAME_BYTES;

#[tokio::test]
async fn session_reports_drafts_pair_after_end_and_ends_on_final() {
    let steps = vec![
        Step::Send(HANDSHAKE),
        Step::Send(DRAFT),
        Step::AwaitEnd,
        Step::Send(SENTENCE_END),
        Step::Send(FINAL),
    ];
    let (mut speech, mut events, seen) = connected(steps).await;

    speech
        .send_audio(vec![0; 2 * MAX_FRAME_BYTES + 3200])
        .await
        .unwrap();
    assert!(matches!(
        next(&mut events).await,
        ProviderEvent::Caption(CaptionEvent::OriginalDraft { .. })
    ));
    assert!(matches!(
        next(&mut events).await,
        ProviderEvent::Caption(CaptionEvent::TranslationDraft { .. })
    ));
    speech.finish().await.unwrap();

    assert!(matches!(
        next(&mut events).await,
        ProviderEvent::Caption(CaptionEvent::ConfirmedPair { .. })
    ));
    assert_eq!(next(&mut events).await, ProviderEvent::Ended);
    speech.disconnect().await;
    let seen = tokio::time::timeout(WAIT, seen).await.unwrap().unwrap();
    assert_eq!(seen.frames, vec![MAX_FRAME_BYTES, MAX_FRAME_BYTES, 3200]);
    assert!(seen.got_end);
}

#[tokio::test]
async fn handshake_url_is_signed_and_carries_the_documented_parameters() {
    let (port, seen) = serve(vec![Step::Send(HANDSHAKE), Step::Close]).await;
    let mut speech = client(port);
    let (sink, _events) = EventSink::channel();

    speech.connect(sink).await.unwrap();
    speech.disconnect().await;

    let seen = tokio::time::timeout(WAIT, seen).await.unwrap().unwrap();
    let uri = seen.path_and_query;
    assert!(
        uri.starts_with("/asr/speech_translate/1250000000?expired="),
        "{uri}"
    );
    for part in [
        "secretid=AKIDmock",
        "source=zh",
        "target=en",
        "voice_format=1",
        "trans_model=hunyuan-translation-lite",
    ] {
        assert!(uri.contains(part), "{part}");
    }
    assert!(!uri.contains(SECRET_KEY));
    assert!(signature_is_valid(&uri, port));
}

#[tokio::test]
async fn reconnecting_uses_a_new_voice_id() {
    let mut ids = Vec::new();
    for _ in 0..2 {
        let (port, seen) = serve(vec![Step::Send(HANDSHAKE), Step::Close]).await;
        let mut speech = client(port);
        let (sink, _events) = EventSink::channel();
        speech.connect(sink).await.unwrap();
        speech.disconnect().await;
        let uri = tokio::time::timeout(WAIT, seen)
            .await
            .unwrap()
            .unwrap()
            .path_and_query;
        let voice_id = uri
            .split('&')
            .find(|p| p.starts_with("voice_id="))
            .unwrap()
            .to_owned();
        ids.push(voice_id);
    }

    assert_ne!(ids[0], ids[1]);
}

#[tokio::test]
async fn disconnect_after_end_without_final_ends_with_what_arrived() {
    for ending in [Step::Drop, Step::Close] {
        let steps = vec![
            Step::Send(HANDSHAKE),
            Step::AwaitEnd,
            Step::Send(SENTENCE_END),
            ending,
        ];
        let (mut speech, mut events, _seen) = connected(steps).await;

        speech.finish().await.unwrap();

        assert!(matches!(
            next(&mut events).await,
            ProviderEvent::Caption(CaptionEvent::ConfirmedPair { .. })
        ));
        assert_eq!(next(&mut events).await, ProviderEvent::Ended);
    }
}

#[tokio::test]
async fn handshake_error_codes_fail_the_connection() {
    let cases = [
        (
            r#"{"code":6002,"message":"x","voice_id":"v"}"#,
            ProviderError::Authentication,
        ),
        (
            r#"{"code":6001,"message":"x","voice_id":"v"}"#,
            ProviderError::Configuration,
        ),
        (
            r#"{"code":6006,"message":"x","voice_id":"v"}"#,
            ProviderError::Provider,
        ),
    ];
    for (reply, expected) in cases {
        let (port, _seen) = serve(vec![Step::Send(reply), Step::Close]).await;
        let mut speech = client(port);
        let (sink, mut events) = EventSink::channel();

        let error = tokio::time::timeout(WAIT, speech.connect(sink))
            .await
            .unwrap()
            .unwrap_err();

        assert_eq!(error, expected);
        assert!(
            events.try_recv().is_err(),
            "no Ready before the handshake reply"
        );
    }
}

#[tokio::test]
async fn error_code_during_the_session_fails_it() {
    let steps = vec![
        Step::Send(HANDSHAKE),
        Step::Send(r#"{"code":6002,"message":"x"}"#),
    ];
    let (_speech, mut events, _seen) = connected(steps).await;

    assert_eq!(
        next(&mut events).await,
        ProviderEvent::Failed(ProviderError::Authentication)
    );
}

#[tokio::test]
async fn connection_closed_before_the_handshake_reply_is_a_transport_error() {
    let (port, _seen) = serve(vec![Step::Close]).await;
    let mut speech = client(port);
    let (sink, _events) = EventSink::channel();

    let error = tokio::time::timeout(WAIT, speech.connect(sink))
        .await
        .unwrap()
        .unwrap_err();

    assert_eq!(error, ProviderError::Transport);
}

#[tokio::test]
async fn silent_server_never_reports_ready() {
    let (port, _seen) = serve(vec![Step::Pause(Duration::from_secs(30))]).await;
    let mut speech = client(port);
    let (sink, mut events) = EventSink::channel();

    let attempt = tokio::time::timeout(Duration::from_millis(300), speech.connect(sink)).await;

    assert!(attempt.is_err(), "connect waits for the handshake reply");
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn heartbeat_round_trips_a_ping_and_fails_once_the_server_is_gone() {
    let steps = vec![Step::Send(HANDSHAKE), Step::AwaitEnd, Step::Drop];
    let (mut speech, mut events, _seen) = connected(steps).await;

    tokio::time::timeout(WAIT, speech.heartbeat())
        .await
        .unwrap()
        .unwrap();
    speech.finish().await.unwrap();
    // Closed during the finish wait: the session ends with what arrived (README).
    assert_eq!(next(&mut events).await, ProviderEvent::Ended);

    let after = tokio::time::timeout(WAIT, speech.heartbeat())
        .await
        .unwrap();
    assert_eq!(after, Err(ProviderError::Transport));
}

#[tokio::test]
async fn results_after_disconnect_are_dropped() {
    let steps = vec![
        Step::Send(HANDSHAKE),
        Step::Pause(Duration::from_millis(100)),
        Step::Send(SENTENCE_END),
    ];
    let (mut speech, mut events, _seen) = connected(steps).await;

    speech.disconnect().await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(events.try_recv().is_err());
    assert_eq!(
        speech.send_audio(vec![0; 2]).await,
        Err(ProviderError::Transport)
    );
}

#[tokio::test]
async fn full_event_queue_sets_the_overflow_flag_and_stops_the_reader() {
    let steps = vec![
        Step::Send(HANDSHAKE),
        Step::Repeat(DRAFT, common::overflow::FLOOD),
    ];
    let (port, _seen) = serve(steps).await;
    let mut speech = client(port);
    let (sink, mut events) = EventSink::channel();
    let flag = sink.overflow_flag();
    tokio::time::timeout(WAIT, speech.connect(sink))
        .await
        .unwrap()
        .unwrap();

    common::overflow::assert_reader_stops_on_overflow(&flag, &mut events).await;
    speech.disconnect().await;
}

async fn until_last(events: &mut tokio::sync::mpsc::Receiver<ProviderEvent>) -> Vec<ProviderEvent> {
    let mut seen = Vec::new();
    loop {
        let event = next(events).await;
        let last = matches!(event, ProviderEvent::Ended | ProviderEvent::Failed(_));
        seen.push(event);
        if last {
            return seen;
        }
    }
}

fn is_draft(event: &ProviderEvent) -> bool {
    matches!(
        event,
        ProviderEvent::Caption(CaptionEvent::OriginalDraft { .. })
    )
}

#[tokio::test]
async fn close_before_end_reports_the_draft_then_a_transport_failure() {
    for ending in [Step::Close, Step::Drop] {
        let (_speech, mut events, _seen) =
            connected(vec![Step::Send(HANDSHAKE), Step::Send(DRAFT), ending]).await;

        let got = until_last(&mut events).await;

        assert!(got.iter().any(is_draft));
        assert_eq!(
            got.last(),
            Some(&ProviderEvent::Failed(ProviderError::Transport))
        );
    }
}
