//! Dedupe, text limits, bounded identity sets, queues and caption shape (captions.md §2.8, §4, §7).

mod common;

use common::*;
use sheng_core::{
    CaptionError, CaptionEvent, Source, CONFIRMED_QUEUE_LIMIT, MAX_TEXT_BYTES, RECENT_LIMIT,
};

#[test]
fn same_text_with_different_ids_confirms_each_time() {
    let mut reducer = translating();

    feed(
        &mut reducer,
        [
            pair(n(1), "La la.", "啦啦。"),
            pair(n(2), "La la.", "啦啦。"),
        ],
    );

    assert_eq!(recent_originals(&reducer), ["La la.", "La la."]);
}

#[test]
fn repeated_final_with_same_id_confirms_once() {
    let mut reducer = original_only();

    feed(
        &mut reducer,
        [
            original_final(s("x"), "Hello."),
            original_final(s("x"), "Hello."),
            original_final(s("x"), "Hello again."),
        ],
    );

    assert_eq!(recent_originals(&reducer), ["Hello."]);
}

#[test]
fn ids_older_than_a_confirmed_id_are_ignored() {
    let mut reducer = original_only();

    feed(
        &mut reducer,
        [original_final(n(5), "Five."), original_final(n(4), "Four.")],
    );

    assert_eq!(recent_originals(&reducer), ["Five."]);
}

#[test]
fn empty_final_does_not_claim_identity() {
    let mut reducer = original_only();

    feed(
        &mut reducer,
        [original_final(n(1), " 。 "), original_final(n(1), "Real.")],
    );

    assert_eq!(recent_originals(&reducer), ["Real."]);
}

#[test]
fn oversized_final_is_rejected_without_claiming_identity() {
    let mut reducer = original_only();
    let oversized = "字".repeat(MAX_TEXT_BYTES / 3 + 1);

    feed(
        &mut reducer,
        [
            original_final(n(1), &oversized),
            original_final(n(1), "Normal."),
        ],
    );

    assert_eq!(recent_originals(&reducer), ["Normal."]);
}

#[test]
fn text_of_exactly_64_kib_is_accepted_and_one_byte_more_is_rejected() {
    let mut reducer = original_only();
    let at_limit = "a".repeat(MAX_TEXT_BYTES);
    let over_limit = "b".repeat(MAX_TEXT_BYTES + 1);

    feed(
        &mut reducer,
        [
            original_final(n(1), &at_limit),
            original_final(n(2), &over_limit),
        ],
    );

    assert_eq!(MAX_TEXT_BYTES, 65_536);
    let recent = reducer.recent();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].original.len(), MAX_TEXT_BYTES);
}

#[test]
fn oversized_draft_is_rejected_without_truncation() {
    let mut reducer = translating();
    feed(&mut reducer, [draft(n(1), "short")]);

    feed(&mut reducer, [draft(n(1), &"c".repeat(MAX_TEXT_BYTES + 1))]);

    assert_eq!(system_track(&reducer).original, "short");
}

#[test]
fn consecutive_identical_anonymous_finals_confirm_once() {
    let mut reducer = original_only();

    feed(
        &mut reducer,
        [
            original_final(None, "Same."),
            original_final(None, "Same."),
            original_final(None, "Other."),
            original_final(None, "Same."),
        ],
    );

    assert_eq!(recent_originals(&reducer), ["Same.", "Other.", "Same."]);
}

#[test]
fn reset_does_not_lift_anonymous_dedupe() {
    let mut reducer = original_only();

    feed(
        &mut reducer,
        [
            original_final(None, "Same."),
            CaptionEvent::Reset,
            original_final(None, "Same."),
        ],
    );

    assert_eq!(recent_originals(&reducer), ["Same."]);
}

#[test]
fn new_session_allows_first_final_equal_to_previous_one() {
    let mut first = original_only();
    feed(&mut first, [original_final(None, "Same.")]);

    let mut second = original_only();
    feed(&mut second, [original_final(None, "Same.")]);

    assert_eq!(recent_originals(&second), ["Same."]);
}

#[test]
fn evicted_string_ids_never_confirm_again() {
    let mut reducer = original_only();
    let ids: Vec<String> = (0..200).map(|i| format!("utt-{i}")).collect();
    for id in &ids {
        feed(&mut reducer, [original_final(s(id), "Line.")]);
    }
    reducer.drain_confirmed();

    feed(&mut reducer, [original_final(s("utt-0"), "Line.")]);

    assert!(reducer.drain_confirmed().is_empty());
}

#[test]
fn evicted_numeric_ids_never_confirm_again() {
    let mut reducer = original_only();
    for id in 0..200 {
        feed(&mut reducer, [original_final(n(id), "Line.")]);
    }
    reducer.drain_confirmed();

    feed(&mut reducer, [original_final(n(3), "Line.")]);

    assert!(reducer.drain_confirmed().is_empty());
}

#[test]
fn quiet_source_is_pushed_out_of_recent_but_stays_queued() {
    let mut reducer = reducer_for(&[Source::System, Source::Microphone], "zh");
    reducer
        .apply(Source::Microphone, pair(n(1), "Mic.", "麦。"), NOW_MS)
        .unwrap();

    for id in 1..=20 {
        reducer
            .apply(
                Source::System,
                pair(n(id), &format!("System {id}."), "系统。"),
                NOW_MS,
            )
            .unwrap();
    }

    let recent = reducer.recent();
    assert_eq!(recent.len(), RECENT_LIMIT);
    assert!(recent.iter().all(|c| c.source == Source::System));
    assert_eq!(recent[0].original, "System 11.");
    let queued = reducer.drain_confirmed();
    assert_eq!(queued.len(), 21);
    assert_eq!(queued[0].source, Source::Microphone);
    assert!(queued[1..].iter().all(|c| c.source == Source::System));
}

#[test]
fn drain_returns_each_caption_once() {
    let mut reducer = translating();
    feed(&mut reducer, [pair(n(1), "One.", "一。")]);

    let first = reducer.drain_confirmed();
    let second = reducer.drain_confirmed();

    assert_eq!(first.len(), 1);
    assert!(second.is_empty());
}

#[test]
fn full_queue_fails_with_backpressure_and_keeps_queued_captions() {
    let mut reducer = original_only();
    for id in 0..CONFIRMED_QUEUE_LIMIT as u64 {
        feed(&mut reducer, [original_final(n(id), "Line.")]);
    }

    let result = reducer.apply(Source::System, original_final(n(999), "Overflow."), NOW_MS);

    assert_eq!(result, Err(CaptionError::ConsumerBackpressure));
    assert_eq!(
        CaptionError::ConsumerBackpressure.code(),
        "caption_consumer_backpressure"
    );
    assert_eq!(reducer.confirmed_len(), CONFIRMED_QUEUE_LIMIT);
}

#[test]
fn caption_ids_are_run_generation_sequence_and_increase() {
    let mut reducer = translating();

    feed(
        &mut reducer,
        [pair(n(1), "One.", "一。"), pair(n(2), "Two.", "二。")],
    );

    let ids: Vec<String> = reducer
        .drain_confirmed()
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(ids, [format!("{RUN_ID}:0:0"), format!("{RUN_ID}:0:1")]);
}

#[test]
fn original_target_confirms_original_final_with_empty_translation() {
    let mut reducer = original_only();

    feed(&mut reducer, [original_final(n(1), "Only original.")]);

    let caption = &reducer.recent()[0];
    assert_eq!(caption.translation, "");
    assert_eq!(caption.target_language, "original");
    assert!(system_track(&reducer).is_final);
}

#[test]
fn caption_and_track_serialize_with_snapshot_field_names() {
    let mut reducer = translating();
    feed(&mut reducer, [pair(n(1), "One.", "一。")]);

    let caption = serde_json::to_value(&reducer.recent()[0]).unwrap();
    let track = serde_json::to_value(system_track(&reducer)).unwrap();

    assert_eq!(caption["source"], "system");
    assert_eq!(caption["timestampMs"], NOW_MS);
    for key in [
        "id",
        "original",
        "translation",
        "sourceLanguage",
        "targetLanguage",
    ] {
        assert!(caption.get(key).is_some(), "missing {key}");
    }
    assert_eq!(track["pendingOriginal"], "");
    assert_eq!(track["isFinal"], true);
}

#[test]
fn events_for_a_source_outside_the_session_are_ignored() {
    let mut reducer = translating();

    let changed = reducer
        .apply(Source::Microphone, pair(n(1), "Mic.", "麦。"), NOW_MS)
        .unwrap();

    assert!(!changed);
    assert!(reducer.recent().is_empty());
    assert_eq!(reducer.tracks().len(), 1);
}
