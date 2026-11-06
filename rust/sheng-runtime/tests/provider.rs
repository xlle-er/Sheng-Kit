//! Error normalization and the scripted fake provider used by later ABI tests.

use std::time::Duration;

use sheng_core::policy::TranslationFailure;
use sheng_core::CaptionEvent;
use sheng_runtime::config::request::{Credentials, TextCredentials};
use sheng_runtime::provider::scripted::{ScriptedSpeech, SpeechScript};
use sheng_runtime::provider::{
    classify_vendor_code, EventSink, ProviderError, ProviderEvent, SessionError, SpeechClient,
};

#[test]
fn vendor_codes_map_through_tables_with_prefixes() {
    let table = [
        ("InvalidApiKey", ProviderError::Authentication),
        ("authentication.*", ProviderError::Authentication),
        (
            "Throttling",
            ProviderError::RateLimited { retry_after: None },
        ),
        ("UNSUPPORTED_LANGUAGE", ProviderError::Configuration),
    ];

    let mapped = [
        "invalidapikey",
        "authentication.expired",
        "Throttling",
        "UNSUPPORTED_LANGUAGE",
        "Other",
    ]
    .map(|code| classify_vendor_code(code, &table));

    assert_eq!(
        mapped,
        [
            ProviderError::Authentication,
            ProviderError::Authentication,
            ProviderError::RateLimited { retry_after: None },
            ProviderError::Configuration,
            ProviderError::Provider,
        ]
    );
}

#[test]
fn errors_map_to_session_codes_and_translation_failures() {
    let retry = Some(Duration::from_secs(2));

    assert_eq!(ProviderError::from_status(429, retry).retry_after(), retry);
    assert_eq!(
        ProviderError::Configuration.speech_session_error().code(),
        "invalid_configuration"
    );
    assert_eq!(
        ProviderError::Provider.speech_session_error(),
        SessionError::ProviderError
    );
    assert_eq!(
        ProviderError::Transport.translation_failure(),
        TranslationFailure::TemporarilyUnavailable
    );
    assert_eq!(
        ProviderError::Authentication.translation_failure(),
        TranslationFailure::Authentication
    );
    assert_eq!(
        SessionError::CaptionConsumerBackpressure.code(),
        "caption_consumer_backpressure"
    );
}

#[test]
fn credential_debug_output_shows_only_the_kind() {
    let speech: Credentials =
        serde_json::from_str(r#"{"kind":"apiKey","apiKey":"sk-very-secret"}"#).unwrap();
    let text: TextCredentials = serde_json::from_str(
        r#"{"kind":"openAICompatible","endpoint":"https://x.example/v1?key=hidden","model":"m","apiKey":"sk-very-secret"}"#,
    )
    .unwrap();

    let debug = format!("{speech:?} {text:?}");

    assert!(!debug.contains("very-secret") && !debug.contains("hidden"));
    assert!(debug.contains("apiKey"));
}

#[tokio::test]
async fn scripted_speech_client_replays_its_script_and_records_audio() {
    let caption = ProviderEvent::Caption(CaptionEvent::Reset);
    let script = SpeechScript {
        on_audio: vec![(2, vec![caption.clone()])],
        on_finish: vec![ProviderEvent::TurnComplete],
        ..Default::default()
    };
    let (mut client, log) = ScriptedSpeech::new(script);
    let (sink, mut events) = EventSink::channel();

    client.connect(sink).await.unwrap();
    client.send_audio(vec![1, 2]).await.unwrap();
    client.send_audio(vec![3, 4]).await.unwrap();
    client.heartbeat().await.unwrap();
    client.finish().await.unwrap();
    client.disconnect().await;

    let mut received = Vec::new();
    while let Ok(event) = events.try_recv() {
        received.push(event);
    }
    assert_eq!(
        received,
        [
            ProviderEvent::Ready,
            caption,
            ProviderEvent::TurnComplete,
            ProviderEvent::Ended
        ]
    );
    let log = log.snapshot();
    assert_eq!(
        (log.audio, log.chunks, log.heartbeats),
        (vec![1, 2, 3, 4], 2, 1)
    );
    assert!(log.finished && log.disconnected);
}

#[tokio::test]
async fn scripted_connect_failure_is_returned() {
    let script = SpeechScript {
        connect_error: Some(ProviderError::Authentication),
        ..Default::default()
    };
    let (mut client, _log) = ScriptedSpeech::new(script);
    let (sink, _events) = EventSink::channel();

    let result = client.connect(sink).await;

    assert_eq!(result, Err(ProviderError::Authentication));
}

#[test]
fn full_event_queue_reports_backlog_overflow() {
    let (sink, _events) = EventSink::channel();
    for _ in 0..sheng_runtime::provider::traits::PROVIDER_EVENT_QUEUE {
        sink.emit(ProviderEvent::TurnComplete).unwrap();
    }

    let overflow = sink.emit(ProviderEvent::TurnComplete);

    assert_eq!(overflow, Err(SessionError::ProviderEventBacklogOverflow));
}
