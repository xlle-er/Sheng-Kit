//! Shared builders for caption tests.
#![allow(dead_code)]

use sheng_core::{CaptionEvent, CaptionReducer, ReducerConfig, Source, TrackView, UtteranceId};

pub const RUN_ID: &str = "run-test";
pub const NOW_MS: u64 = 1_700_000_000_000;

pub fn reducer_for(sources: &[Source], target: &str) -> CaptionReducer {
    CaptionReducer::new(ReducerConfig {
        run_id: RUN_ID.to_owned(),
        generation: 0,
        sources: sources.to_vec(),
        source_language: "en".to_owned(),
        target_language: target.to_owned(),
    })
}

/// System-only session that translates to Chinese.
pub fn translating() -> CaptionReducer {
    reducer_for(&[Source::System], "zh")
}

/// System-only session that shows the original text only.
pub fn original_only() -> CaptionReducer {
    reducer_for(&[Source::System], "original")
}

pub fn feed(reducer: &mut CaptionReducer, events: impl IntoIterator<Item = CaptionEvent>) {
    for event in events {
        reducer
            .apply(Source::System, event, NOW_MS)
            .expect("event applies");
    }
}

pub fn system_track(reducer: &CaptionReducer) -> TrackView {
    track_of(reducer, Source::System)
}

pub fn track_of(reducer: &CaptionReducer, source: Source) -> TrackView {
    reducer
        .tracks()
        .into_iter()
        .find(|track| track.source == source)
        .expect("track exists")
}

pub fn recent_originals(reducer: &CaptionReducer) -> Vec<String> {
    reducer.recent().into_iter().map(|c| c.original).collect()
}

pub fn n(id: u64) -> Option<UtteranceId> {
    Some(UtteranceId::Number(id))
}

pub fn s(id: &str) -> Option<UtteranceId> {
    Some(UtteranceId::Text(id.to_owned()))
}

pub fn draft(utterance: Option<UtteranceId>, text: &str) -> CaptionEvent {
    CaptionEvent::OriginalDraft {
        utterance,
        text: text.to_owned(),
    }
}

pub fn original_final(utterance: Option<UtteranceId>, text: &str) -> CaptionEvent {
    CaptionEvent::OriginalFinal {
        utterance,
        text: text.to_owned(),
    }
}

pub fn translation_draft(utterance: Option<UtteranceId>, text: &str) -> CaptionEvent {
    CaptionEvent::TranslationDraft {
        utterance,
        text: text.to_owned(),
    }
}

pub fn translation_final(utterance: Option<UtteranceId>, text: &str) -> CaptionEvent {
    CaptionEvent::TranslationFinal {
        utterance,
        text: text.to_owned(),
    }
}

pub fn pair(utterance: Option<UtteranceId>, original: &str, translation: &str) -> CaptionEvent {
    CaptionEvent::ConfirmedPair {
        utterance,
        original: original.to_owned(),
        translation: translation.to_owned(),
    }
}

pub fn preview(utterance: Option<UtteranceId>, original: &str, translation: &str) -> CaptionEvent {
    CaptionEvent::PreviewPair {
        utterance,
        original: original.to_owned(),
        translation: translation.to_owned(),
    }
}

pub fn retract(utterance: Option<UtteranceId>) -> CaptionEvent {
    CaptionEvent::Retract { utterance }
}

pub fn order(id: &str, sequence: i64) -> CaptionEvent {
    CaptionEvent::Order {
        utterance: UtteranceId::Text(id.to_owned()),
        sequence,
    }
}
