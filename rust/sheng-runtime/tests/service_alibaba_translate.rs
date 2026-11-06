//! Live-translation client and Qwen-MT translator against local servers, plus the
//! factory entry points for validated plans.

mod common;
mod service_alibaba_mock;

use base64::Engine;
use futures_util::SinkExt;
use serde_json::json;
use service_alibaba_mock as mock;
use sheng_core::{CaptionEvent, Source, UtteranceId};
use sheng_runtime::config::check_request;
use sheng_runtime::provider::{
    ProviderError, ProviderEvent, SessionError, SpeechClient, TextTranslator, TranslationRequest,
};
use sheng_runtime::services::alibaba::live::client::{LiveConfig, LiveTranslateClient};
use sheng_runtime::services::alibaba::mt::QwenMtTranslator;
use sheng_runtime::services::alibaba::{speech, translator};
use tokio_tungstenite::tungstenite::Message;

fn caption(event: &ProviderEvent) -> Option<CaptionEvent> {
    match event {
        ProviderEvent::Caption(caption) => Some(caption.clone()),
        _ => None,
    }
}

#[tokio::test]
async fn live_translation_pairs_a_translation_that_arrives_first() {
    let (url, auth) = mock::ws_server(|mut ws| async move {
        mock::send_text(&mut ws, r#"{"type": "session.created", "session": {}}"#).await;
        let update = mock::next_json(&mut ws).await;
        assert_eq!(update["type"], "session.update");
        assert_eq!(update["session"]["translation"]["language"], "en");
        mock::send_text(&mut ws, r#"{"type": "session.updated", "session": {}}"#).await;
        let append = mock::next_json(&mut ws).await;
        assert_eq!(append["type"], "input_audio_buffer.append");
        let audio = base64::engine::general_purpose::STANDARD
            .decode(append["audio"].as_str().unwrap())
            .unwrap();
        assert_eq!(audio, vec![9, 8, 7, 6]);
        for line in include_str!("fixtures/alibaba/live_events.jsonl").lines() {
            mock::send_text(&mut ws, line).await;
        }
        let finish = mock::next_json(&mut ws).await;
        assert_eq!(finish, json!({"type": "session.finish"}));
        mock::send_text(&mut ws, r#"{"type": "session.finished"}"#).await;
        mock::drain(ws).await;
    })
    .await;
    let mut client = LiveTranslateClient::new(LiveConfig {
        endpoint: url,
        api_key: mock::key(),
        proxy: mock::direct(),
        target_language: "en".into(),
        sample_rate: 16_000,
    });
    let (sink, mut events) = mock::sink();
    client.connect(sink).await.expect("connects");
    assert_eq!(auth.await.unwrap().as_deref(), Some("Bearer loopback-key"));
    assert_eq!(mock::next_event(&mut events).await, ProviderEvent::Ready);
    client.send_audio(vec![9, 8, 7, 6]).await.unwrap();
    let mut captions = Vec::new();
    while captions.len() < 6 {
        captions.extend(caption(&mock::next_event(&mut events).await));
    }
    let source = Some(UtteranceId::Text("item_src_1".into()));
    assert_eq!(
        captions[4],
        CaptionEvent::TranslationFinal {
            utterance: source.clone(),
            text: "Good morning".into()
        }
    );
    assert_eq!(
        captions[5],
        CaptionEvent::OriginalFinal {
            utterance: source,
            text: "Guten Morgen.".into()
        }
    );
    client.finish().await.unwrap();
    assert_eq!(mock::next_event(&mut events).await, ProviderEvent::Ended);
    client.disconnect().await;
}

#[tokio::test]
async fn live_translation_rejected_session_update_fails_connect() {
    let (url, _) = mock::ws_server(|mut ws| async move {
        mock::send_text(&mut ws, r#"{"type": "session.created"}"#).await;
        let _ = mock::next_json(&mut ws).await;
        let error =
            r#"{"type": "error", "error": {"code": "UNSUPPORTED_LANGUAGE", "message": "m"}}"#;
        mock::send_text(&mut ws, error).await;
        mock::drain(ws).await;
    })
    .await;
    let mut client = LiveTranslateClient::new(LiveConfig {
        endpoint: url,
        api_key: mock::key(),
        proxy: mock::direct(),
        target_language: "xx".into(),
        sample_rate: 16_000,
    });
    let (sink, _events) = mock::sink();
    assert_eq!(
        client.connect(sink).await,
        Err(ProviderError::Configuration)
    );
}

fn sse_reply(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn sentence() -> TranslationRequest {
    TranslationRequest {
        text: "早上好，各位。".into(),
        source_language: "zh".into(),
        target_language: "en".into(),
    }
}

#[tokio::test]
async fn qwen_mt_streams_and_selects_the_model() {
    for (model, fixture) in [
        (
            "qwen-mt-lite",
            include_str!("fixtures/alibaba/qwen_mt_lite.sse"),
        ),
        (
            "qwen-mt-plus",
            include_str!("fixtures/alibaba/qwen_mt_plus.sse"),
        ),
    ] {
        let (port, mut requests) = mock::raw_http_server(move |_| sse_reply(fixture)).await;
        let endpoint = format!("http://127.0.0.1:{port}/compatible-mode/v1/chat/completions");
        let translator =
            QwenMtTranslator::new(endpoint, model, &mock::key(), &mock::direct()).unwrap();
        let text = translator.translate(sentence()).await.unwrap();
        assert_eq!(text, "Good morning, everyone.");
        let request = requests.recv().await.unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.header("authorization"), Some("Bearer loopback-key"));
        let body = request.json();
        assert_eq!(body["model"], model);
        assert_eq!(body["stream"], true);
        assert_eq!(
            body["translation_options"],
            json!({"source_lang": "Chinese", "target_lang": "English"})
        );
        assert_eq!(
            body["messages"],
            json!([{"role": "user", "content": "早上好，各位。"}])
        );
    }
}

#[tokio::test]
async fn qwen_mt_http_failures_are_classified() {
    let cases = [
        ("401 Unauthorized", ProviderError::Authentication),
        ("400 Bad Request", ProviderError::Configuration),
        (
            "429 Too Many Requests",
            ProviderError::RateLimited {
                retry_after: Some(std::time::Duration::from_secs(7)),
            },
        ),
    ];
    for (status, expected) in cases {
        let port = mock::status_server(status).await;
        let endpoint = format!("http://127.0.0.1:{port}/v1/chat/completions");
        let translator =
            QwenMtTranslator::new(endpoint, "qwen-mt-lite", &mock::key(), &mock::direct()).unwrap();
        assert_eq!(
            translator.translate(sentence()).await,
            Err(expected),
            "{status}"
        );
    }
}

fn plan(provider: &str, text: &str) -> sheng_runtime::config::SessionPlan {
    let request = common::start_request("system", provider, "asrMt", text, "zh", "en");
    check_request(&request.to_string()).expect("valid request")
}

#[test]
fn factory_builds_recognition_and_qwen_mt_for_alibaba_asr_mt() {
    let plan = plan("alibabaCloud", "followService");
    assert!(speech(&plan, Source::System).is_ok());
    assert!(translator(&plan).is_ok());
}

#[test]
fn factory_builds_recognition_for_custom_dashscope() {
    let plan = plan("customDashScopeASR", "deepL");
    assert!(speech(&plan, Source::Microphone).is_ok());
    assert_eq!(
        translator(&plan).err(),
        Some(SessionError::ProviderConfigurationFailed),
        "custom endpoints use the independent translator"
    );
}

#[test]
fn live_translation_rejects_an_explicit_source_language() {
    let request = common::start_request("system", "alibabaCloud", "simultaneous", "-", "zh", "en");
    assert!(check_request(&request.to_string()).is_err());
}

#[tokio::test]
async fn live_translation_full_event_queue_sets_the_overflow_flag() {
    let (url, _) = mock::ws_server(|mut ws| async move {
        mock::send_text(&mut ws, r#"{"type": "session.created", "session": {}}"#).await;
        let _ = mock::next_json(&mut ws).await;
        mock::send_text(&mut ws, r#"{"type": "session.updated", "session": {}}"#).await;
        let delta = r#"{"type": "conversation.item.input_audio_transcription.delta", "item_id": "s1", "content_index": 0, "delta": "w"}"#;
        for _ in 0..common::overflow::FLOOD {
            if ws.send(Message::text(delta)).await.is_err() {
                break;
            }
        }
        mock::drain(ws).await;
    })
    .await;
    let mut client = LiveTranslateClient::new(LiveConfig {
        endpoint: url,
        api_key: mock::key(),
        proxy: mock::direct(),
        target_language: "en".into(),
        sample_rate: 16_000,
    });
    let (sink, mut events) = mock::sink();
    let flag = sink.overflow_flag();
    client.connect(sink).await.expect("connects");

    common::overflow::assert_reader_stops_on_overflow(&flag, &mut events).await;
    client.disconnect().await;
}

/// Answers the first request with a stream cut off mid-way, later ones in full.
fn cut_once_then(fixture: &'static str) -> impl Fn(&common::http::HttpRequest) -> String {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    move |_| {
        if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
            return sse_reply(fixture);
        }
        let partial = &fixture[..fixture.len() / 2];
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{partial}",
            fixture.len()
        )
    }
}

#[tokio::test]
async fn qwen_mt_stream_cut_mid_way_is_a_recoverable_failure_and_the_retry_succeeds() {
    use sheng_runtime::pipeline::TranslationPipeline;
    use sheng_runtime::pipeline::{PipelineConfig, PipelineInput, PipelineOutput};

    let fixture = include_str!("fixtures/alibaba/qwen_mt_lite.sse");
    let (port, _requests) = mock::raw_http_server(cut_once_then(fixture)).await;
    let endpoint = format!("http://127.0.0.1:{port}/compatible-mode/v1/chat/completions");
    let translator =
        QwenMtTranslator::new(endpoint, "qwen-mt-lite", &mock::key(), &mock::direct()).unwrap();
    let config = PipelineConfig {
        source_language: "zh".into(),
        target_language: "en".into(),
    };
    let (pipeline, mut output, _task) =
        TranslationPipeline::spawn(std::sync::Arc::new(translator), config);

    pipeline
        .send(PipelineInput::Final {
            utterance: Some(UtteranceId::Number(1)),
            text: sentence().text,
        })
        .await;
    let mut seen = Vec::new();
    let confirmed = loop {
        let item = tokio::time::timeout(std::time::Duration::from_secs(15), output.recv())
            .await
            .expect("pipeline answers in time")
            .expect("pipeline running");
        if let PipelineOutput::Caption(CaptionEvent::ConfirmedPair { translation, .. }) = &item {
            break translation.clone();
        }
        assert!(!matches!(item, PipelineOutput::Failed(_)), "{item:?}");
        seen.push(item);
    };

    assert_eq!(confirmed, "Good morning, everyone.");
    assert!(
        seen.iter()
            .any(|o| matches!(o, PipelineOutput::Recovery(Some(r)) if r.retry_scheduled)),
        "{seen:?}"
    );
}
