//! Gemini Live endings: late transcripts after `audioStreamEnd` and unexpected closes
//! (providers/gemini.md “结束与断开”, providers/README.md).

mod common;
mod service_gemini_support;

use serde_json::json;
use service_gemini_support::{client, connected, events_until, mock, plan, Mock, Out};
use sheng_core::align::Stream;
use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::provider::{ProviderError, ProviderEvent, SpeechClient, StreamText};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

fn is_last(event: &ProviderEvent) -> bool {
    matches!(event, ProviderEvent::Ended | ProviderEvent::Failed(_))
}

fn finals(events: &[ProviderEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::Caption(CaptionEvent::OriginalFinal { text, .. }) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn delta(stream: Stream, text: &str) -> ProviderEvent {
    ProviderEvent::StreamText {
        stream,
        text: StreamText::Delta(text.into()),
    }
}

async fn session(mode: &str) -> (Mock, Box<dyn SpeechClient>, mpsc::Receiver<ProviderEvent>) {
    let mut server = mock().await;
    let plan = plan(mode, if mode == "asrMt" { "en" } else { "auto" }, "zh");
    let mut speech = client(&plan, &server);
    let events = connected(&mut speech, &mut server).await;
    (server, speech, events)
}

/// Waits until the reader has shown text, so the stop comes after it.
async fn shown(events: &mut mpsc::Receiver<ProviderEvent>) {
    events_until(events, |e| {
        matches!(
            e,
            ProviderEvent::Caption(CaptionEvent::OriginalDraft { .. })
                | ProviderEvent::StreamText { .. }
        )
    })
    .await;
}

async fn finish(speech: &mut Box<dyn SpeechClient>, server: &mut Mock) {
    speech.finish().await.unwrap();
    assert_eq!(
        server.next().await,
        json!({"realtimeInput": {"audioStreamEnd": true}})
    );
}

#[tokio::test]
async fn asr_mt_keeps_every_transcript_after_audio_stream_end_without_turn_complete() {
    let (mut server, mut speech, mut events) = session("asrMt").await;
    server.content(json!({"inputTranscription": {"text": "Before the stop"}}));
    shown(&mut events).await;
    finish(&mut speech, &mut server).await;
    server.content(json!({"inputTranscription": {"text": " and after"}}));
    server.content(json!({"inputTranscription": {"text": " it, too."}}));

    let got = events_until(&mut events, is_last).await;

    assert_eq!(finals(&got), ["Before the stop and after it, too."]);
    assert_eq!(got.last(), Some(&ProviderEvent::Ended));
    speech.disconnect().await;
}

#[tokio::test]
async fn asr_mt_turn_complete_after_stop_confirms_once() {
    let (mut server, mut speech, mut events) = session("asrMt").await;
    server.content(json!({"inputTranscription": {"text": "Late"}}));
    shown(&mut events).await;
    finish(&mut speech, &mut server).await;
    server.content(json!({"inputTranscription": {"text": " words."}}));
    server.content(json!({"turnComplete": true}));

    let got = events_until(&mut events, is_last).await;

    assert_eq!(finals(&got), ["Late words."], "{got:?}");
    assert_eq!(got.last(), Some(&ProviderEvent::Ended));
}

#[tokio::test]
async fn tail_is_kept_when_the_service_closes_after_stop() {
    for mode in ["asrMt", "simultaneous"] {
        let (mut server, mut speech, mut events) = session(mode).await;
        server.content(json!({"inputTranscription": {"text": "Last"}}));
        shown(&mut events).await;
        finish(&mut speech, &mut server).await;
        server.content(json!({"inputTranscription": {"text": " words"}}));
        server.content(json!({"outputTranscription": {"text": "最后的话"}}));
        server.send(Out::Close(CloseCode::Normal, ""));

        let got = events_until(&mut events, is_last).await;

        assert_eq!(got.last(), Some(&ProviderEvent::Ended), "{mode}");
        if mode == "asrMt" {
            assert_eq!(finals(&got), ["Last words"]);
        } else {
            assert!(got.contains(&delta(Stream::Original, " words")));
            assert!(got.contains(&delta(Stream::Translation, "最后的话")));
        }
    }
}

#[tokio::test]
async fn asr_mt_commits_the_transcript_before_an_unexpected_end() {
    let closes = [
        Out::Binary(json!({"goAway": {"timeLeft": "1s"}})),
        Out::Close(CloseCode::Normal, ""),
        Out::Close(CloseCode::Error, "Internal error occurred."),
        Out::Drop,
    ];
    for out in closes {
        let (server, mut speech, mut events) = session("asrMt").await;
        server.content(json!({"inputTranscription": {"text": "Half a sentence"}}));
        server.send(out);

        let got = events_until(&mut events, is_last).await;

        assert_eq!(finals(&got), ["Half a sentence"]);
        assert_eq!(
            got.last(),
            Some(&ProviderEvent::Failed(ProviderError::Transport))
        );
        assert_eq!(
            got.iter()
                .filter(|e| matches!(
                    e,
                    ProviderEvent::Caption(CaptionEvent::OriginalFinal {
                        utterance: Some(UtteranceId::Number(0)),
                        ..
                    })
                ))
                .count(),
            1
        );
        speech.disconnect().await;
    }
}

#[tokio::test]
async fn simultaneous_unexpected_close_is_a_transport_failure() {
    for out in [
        Out::Close(CloseCode::Normal, ""),
        Out::Close(CloseCode::Error, "Internal error occurred."),
    ] {
        let (server, mut speech, mut events) = session("simultaneous").await;
        server.content(json!({"inputTranscription": {"text": "Half"}}));
        server.send(out);

        let got = events_until(&mut events, is_last).await;

        assert_eq!(
            got.last(),
            Some(&ProviderEvent::Failed(ProviderError::Transport))
        );
        speech.disconnect().await;
    }
}
