//! Apple speech client (step 4a) against fakes of the system traits. Nothing here touches
//! the real frameworks; native behavior is listed in docs/verification.md.

mod service_apple_support;

use service_apple_support::*;
use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::provider::traits::PROVIDER_EVENT_QUEUE;
use sheng_runtime::provider::{EventSink, ProviderEvent, SessionError, SpeechClient};
use sheng_runtime::services::apple::events::{self, BridgeEvent};
use sheng_runtime::services::apple::system::Push;
use sheng_runtime::services::apple::AppleSpeech;

#[tokio::test]
async fn speech_maps_results_to_numbered_utterances() {
    let (apple, system, recognizer) = fake(
        installed_english(),
        FakeRecognizer::new(Some(BridgeEvent::Ready)),
    );
    let mut speech = AppleSpeech::new(apple, "en");
    let (sink, mut events) = EventSink::channel();

    speech.connect(sink).await.expect("connects");
    assert_eq!(events.recv().await, Some(ProviderEvent::Ready));
    let token = recognizer.token();
    events::deliver(token, BridgeEvent::Draft("hel".into()));
    events::deliver(token, BridgeEvent::Final("hello".into()));
    events::deliver(token, BridgeEvent::Draft("wor".into()));

    let number = |n| Some(UtteranceId::Number(n));
    let caption = |event| Some(ProviderEvent::Caption(event));
    assert_eq!(
        events.recv().await,
        caption(CaptionEvent::OriginalDraft {
            utterance: number(1),
            text: "hel".into()
        })
    );
    assert_eq!(
        events.recv().await,
        caption(CaptionEvent::OriginalFinal {
            utterance: number(1),
            text: "hello".into()
        })
    );
    assert_eq!(
        events.recv().await,
        caption(CaptionEvent::OriginalDraft {
            utterance: number(2),
            text: "wor".into()
        })
    );
    assert_eq!(recognizer.calls()[0], "open:en-US");
    assert!(system.downloads().is_empty(), "sessions never download");
}

#[tokio::test]
async fn speech_without_installed_assets_fails_before_opening() {
    for (source, system) in [
        ("fr", installed_english()),
        ("de", installed_english()),
        ("en", FakeSystem::default()),
    ] {
        let (apple, system, recognizer) =
            fake(system, FakeRecognizer::new(Some(BridgeEvent::Ready)));
        let mut speech = AppleSpeech::new(apple, source);
        let (sink, _events) = EventSink::channel();
        assert_eq!(
            speech.connect(sink).await,
            Err(local(SessionError::AppleSpeechRuntimeServiceUnavailable))
        );
        assert!(recognizer.calls().is_empty());
        assert!(system.downloads().is_empty());
    }
}

#[tokio::test]
async fn speech_start_failures_keep_their_apple_codes() {
    let cases = [
        (
            Some(BridgeEvent::Failed("timeout".into())),
            SessionError::AppleSpeechRuntimeTimeout,
        ),
        (
            Some(BridgeEvent::Failed("other".into())),
            SessionError::AppleSpeechRuntimeServiceUnavailable,
        ),
        (
            Some(BridgeEvent::Ended),
            SessionError::AppleSpeechStreamClosed,
        ),
    ];
    for (first, code) in cases {
        let (apple, _, recognizer) = fake(installed_english(), FakeRecognizer::new(first));
        let mut speech = AppleSpeech::new(apple, "en");
        let (sink, _events) = EventSink::channel();
        assert_eq!(speech.connect(sink).await, Err(local(code)));
        assert_eq!(recognizer.calls().last().map(String::as_str), Some("close"));
        assert!(code.code().starts_with("apple_speech_"));
    }
}

#[tokio::test(start_paused = true)]
async fn speech_start_times_out_without_an_answer() {
    let (apple, _, _) = fake(installed_english(), FakeRecognizer::new(None));
    let mut speech = AppleSpeech::new(apple, "en");
    let (sink, _events) = EventSink::channel();
    assert_eq!(
        speech.connect(sink).await,
        Err(local(SessionError::AppleSpeechRuntimeTimeout))
    );
}

#[tokio::test]
async fn speech_reports_queue_and_stream_state() {
    let mut recognizer = FakeRecognizer::new(Some(BridgeEvent::Ready));
    recognizer.push = Push::Full;
    recognizer.alive = false;
    let (apple, _, recognizer) = fake(installed_english(), recognizer);
    let mut speech = AppleSpeech::new(apple, "en");
    let (sink, mut events) = EventSink::channel();
    speech.connect(sink).await.expect("connects");
    let _ = events.recv().await;

    assert_eq!(
        speech.send_audio(vec![0; 640]).await,
        Err(local(SessionError::AudioBackpressure))
    );
    speech.heartbeat().await.expect("heartbeat itself succeeds");
    assert_eq!(
        events.recv().await,
        Some(ProviderEvent::Failed(local(
            SessionError::AppleSpeechStreamClosed
        )))
    );

    speech.finish().await.expect("finishes");
    speech.disconnect().await;
    assert_eq!(
        speech.send_audio(vec![0; 640]).await,
        Err(local(SessionError::AppleSpeechStreamClosed))
    );
    assert!(recognizer
        .calls()
        .ends_with(&["finish".into(), "close".into()]));
}

#[tokio::test]
async fn late_events_of_a_closed_session_are_dropped() {
    let (apple, _, recognizer) = fake(
        installed_english(),
        FakeRecognizer::new(Some(BridgeEvent::Ready)),
    );
    let mut speech = AppleSpeech::new(apple, "en");
    let (sink, mut events) = EventSink::channel();
    speech.connect(sink).await.expect("connects");
    let _ = events.recv().await;
    let old = recognizer.token();
    speech.disconnect().await;

    events::deliver(old, BridgeEvent::Final("late".into()));
    drop(speech);
    assert_eq!(events.recv().await, None, "no event after close");
}

#[tokio::test]
async fn full_event_queue_sets_the_overflow_flag_and_keeps_the_apple_code() {
    let (apple, _, recognizer) = fake(
        installed_english(),
        FakeRecognizer::new(Some(BridgeEvent::Ready)),
    );
    let mut speech = AppleSpeech::new(apple, "en");
    let (sink, _events) = EventSink::channel();
    let flag = sink.overflow_flag();
    speech.connect(sink).await.expect("connects");

    let token = recognizer.token();
    for n in 0..PROVIDER_EVENT_QUEUE + 64 {
        events::deliver(token, BridgeEvent::Draft(format!("w{n}")));
    }

    assert!(flag.is_set());
    // Apple reports its own backlog code (apple.md), which the session keeps.
    assert_eq!(
        speech.send_audio(vec![0; 640]).await,
        Err(local(SessionError::AppleSpeechResultBacklog))
    );
    speech.disconnect().await;
}
