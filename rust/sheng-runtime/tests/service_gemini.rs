//! Gemini Live client against a local scripted WebSocket server (no network).

mod common;
mod service_gemini_support;

use serde_json::json;
use service_gemini_support::{client, connected, events_until, mock, next_event, plan, Out};
use sheng_core::align::Stream;
use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::provider::{EventSink, ProviderError, ProviderEvent, StreamText};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

fn is_ended(event: &ProviderEvent) -> bool {
    matches!(event, ProviderEvent::Ended | ProviderEvent::Failed(_))
}

fn delta(stream: Stream, text: &str) -> ProviderEvent {
    ProviderEvent::StreamText {
        stream,
        text: StreamText::Delta(text.into()),
    }
}

#[tokio::test]
async fn key_goes_in_a_header_never_the_url() {
    let mut server = mock().await;
    let plan = plan("simultaneous", "auto", "zh");
    let mut speech = client(&plan, &server);
    let mut events = connected(&mut speech, &mut server).await;
    assert_eq!(next_event(&mut events).await, ProviderEvent::Ready);
    let seen = server.seen.try_recv().unwrap();
    assert_eq!(seen.api_key.as_deref(), Some("placeholder-key"));
    assert!(seen.query.is_none_or(|q| !q.contains("key")));
    speech.disconnect().await;
}

#[tokio::test]
async fn simultaneous_streams_both_transcripts_and_ends_on_finish() {
    let mut server = mock().await;
    let plan = plan("simultaneous", "auto", "zh");
    let mut speech = client(&plan, &server);
    let (sink, mut events) = EventSink::channel();
    let (connect, setup) = tokio::join!(speech.connect(sink), server.complete_setup());
    connect.unwrap();
    assert_eq!(
        setup["setup"]["generationConfig"]["translationConfig"]["targetLanguageCode"],
        json!("zh-Hans")
    );
    assert_eq!(next_event(&mut events).await, ProviderEvent::Ready);

    speech.send_audio(vec![0, 1, 2, 3]).await.unwrap();
    let audio = server.next().await;
    assert_eq!(
        audio["realtimeInput"]["audio"]["mimeType"],
        json!("audio/pcm;rate=16000")
    );

    server.content(json!({"inputTranscription": {"text": "Hello."}}));
    server.content(json!({"outputTranscription": {"text": "你好。"}}));
    server.content(json!({"turnComplete": true}));
    let got = events_until(&mut events, |e| matches!(e, ProviderEvent::TurnComplete)).await;
    assert_eq!(
        got,
        vec![
            delta(Stream::Original, "Hello."),
            delta(Stream::Translation, "你好。"),
            ProviderEvent::TurnComplete
        ]
    );

    speech.finish().await.unwrap();
    assert_eq!(
        server.next().await,
        json!({"realtimeInput": {"audioStreamEnd": true}})
    );
    assert_eq!(next_event(&mut events).await, ProviderEvent::Ended);
    speech.disconnect().await;
}

#[tokio::test]
async fn asr_mt_reports_finals_with_utterance_ids() {
    let mut server = mock().await;
    let plan = plan("asrMt", "en", "zh");
    let mut speech = client(&plan, &server);
    let mut events = connected(&mut speech, &mut server).await;
    assert_eq!(next_event(&mut events).await, ProviderEvent::Ready);
    server.content(json!({"inputTranscription": {"text": "Good morning."}}));
    server.content(json!({"turnComplete": true}));
    let got = events_until(&mut events, |e| {
        matches!(
            e,
            ProviderEvent::Caption(CaptionEvent::OriginalFinal { .. })
        )
    })
    .await;
    assert_eq!(
        got.last().unwrap(),
        &ProviderEvent::Caption(CaptionEvent::OriginalFinal {
            utterance: Some(UtteranceId::Number(0)),
            text: "Good morning.".into()
        })
    );
    speech.disconnect().await;
}

#[tokio::test]
async fn setup_error_fails_connect_with_its_class() {
    let mut server = mock().await;
    let plan = plan("simultaneous", "auto", "zh");
    let mut speech = client(&plan, &server);
    let (sink, _events) = EventSink::channel();
    let answer = async {
        server.next().await;
        server.send(Out::Close(CloseCode::Policy, "PERMISSION_DENIED"));
    };
    let (connect, ()) = tokio::join!(speech.connect(sink), answer);
    assert_eq!(connect, Err(ProviderError::Authentication));
}

#[tokio::test]
async fn error_message_after_setup_fails_the_session() {
    let mut server = mock().await;
    let plan = plan("simultaneous", "auto", "zh");
    let mut speech = client(&plan, &server);
    let mut events = connected(&mut speech, &mut server).await;
    server.json(json!({"error": {"code": 400, "status": "INVALID_ARGUMENT"}}));
    let got = events_until(&mut events, is_ended).await;
    assert_eq!(
        got.last().unwrap(),
        &ProviderEvent::Failed(ProviderError::Configuration)
    );
}

#[tokio::test]
async fn go_away_and_dropped_connections_are_transport_failures() {
    for out in [
        Out::Binary(json!({"goAway": {"timeLeft": "1s"}})),
        Out::Drop,
    ] {
        let mut server = mock().await;
        let plan = plan("simultaneous", "auto", "zh");
        let mut speech = client(&plan, &server);
        let mut events = connected(&mut speech, &mut server).await;
        server.send(out);
        let got = events_until(&mut events, is_ended).await;
        assert_eq!(
            got.last().unwrap(),
            &ProviderEvent::Failed(ProviderError::Transport)
        );
    }
}

#[tokio::test]
async fn normal_close_after_finish_ends_cleanly() {
    let mut server = mock().await;
    let plan = plan("simultaneous", "auto", "zh");
    let mut speech = client(&plan, &server);
    let mut events = connected(&mut speech, &mut server).await;
    server.content(json!({"inputTranscription": {"text": "Partial"}}));
    assert_eq!(next_event(&mut events).await, ProviderEvent::Ready);
    assert_eq!(
        next_event(&mut events).await,
        delta(Stream::Original, "Partial")
    );
    speech.finish().await.unwrap();
    server.next().await;
    server.send(Out::Close(CloseCode::Normal, ""));
    let got = events_until(&mut events, is_ended).await;
    assert_eq!(got.last().unwrap(), &ProviderEvent::Ended);
}

#[tokio::test]
async fn heartbeat_waits_for_pong() {
    let mut server = mock().await;
    let plan = plan("simultaneous", "auto", "zh");
    let mut speech = client(&plan, &server);
    let _events = connected(&mut speech, &mut server).await;
    tokio::time::timeout(service_gemini_support::WAIT, speech.heartbeat())
        .await
        .expect("pong in time")
        .unwrap();
    speech.disconnect().await;
}

#[tokio::test]
async fn full_event_queue_sets_the_overflow_flag_and_stops_the_reader() {
    let mut server = mock().await;
    let plan = plan("asrMt", "en", "zh");
    let mut speech = client(&plan, &server);
    let (sink, mut events) = EventSink::channel();
    let flag = sink.overflow_flag();
    let (connect, _) = tokio::join!(speech.connect(sink), server.complete_setup());
    connect.expect("connects");

    for _ in 0..common::overflow::FLOOD {
        server.content(json!({"inputTranscription": {"text": "word "}}));
    }

    common::overflow::assert_reader_stops_on_overflow(&flag, &mut events).await;
    speech.disconnect().await;
}
