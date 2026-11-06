//! Display rules: drafts, previews, retraction, late finals, reset and clear (captions.md §2, §8).

mod common;

use common::*;
use serde::Deserialize;
use sheng_core::{CaptionEvent, Source};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmationFixture {
    source_language: String,
    target_language: String,
    events: Vec<CaptionEvent>,
    expected_recent: Vec<(String, String)>,
}

#[test]
fn multilingual_confirmed_pairs_enter_recent_in_order() {
    // Arrange
    let fixture: ConfirmationFixture =
        serde_json::from_str(include_str!("fixtures/multilingual-confirmations.json"))
            .expect("fixture parses");
    let mut reducer = reducer_for(&[Source::System], &fixture.target_language);

    // Act
    feed(&mut reducer, fixture.events);

    // Assert
    let recent: Vec<(String, String)> = reducer
        .recent()
        .into_iter()
        .map(|c| (c.original, c.translation))
        .collect();
    assert_eq!(recent, fixture.expected_recent);
    assert!(reducer
        .recent()
        .iter()
        .all(|c| c.target_language == fixture.target_language && c.source_language == "en"));
    assert_eq!(fixture.source_language, "auto");
}

#[test]
fn draft_shows_immediately_as_unconfirmed_original() {
    let mut reducer = translating();

    feed(&mut reducer, [draft(n(1), "hello wor")]);

    let track = system_track(&reducer);
    assert_eq!(track.original, "hello wor");
    assert_eq!(track.translation, "");
    assert!(!track.is_final);
}

#[test]
fn blank_draft_does_not_overwrite_confirmed_pair() {
    let mut reducer = translating();
    feed(&mut reducer, [pair(n(1), "Hello.", "你好。")]);

    feed(&mut reducer, [draft(n(2), "  ,  "), draft(n(3), "")]);

    let track = system_track(&reducer);
    assert_eq!(track.original, "Hello.");
    assert!(track.is_final);
}

#[test]
fn rewritten_final_replaces_preview_and_keeps_new_draft_pending() {
    let mut reducer = translating();
    feed(&mut reducer, [preview(n(1), "hello wrld", "你好世界")]);

    feed(&mut reducer, [draft(n(2), "next sentence")]);
    let during_preview = system_track(&reducer);
    feed(&mut reducer, [pair(n(1), "Hello, world.", "你好，世界。")]);

    assert_eq!(during_preview.original, "hello wrld");
    assert_eq!(during_preview.pending_original, "next sentence");
    let track = system_track(&reducer);
    assert_eq!(track.original, "Hello, world.");
    assert_eq!(track.translation, "你好，世界。");
    assert_eq!(track.pending_original, "next sentence");
    assert!(track.is_final);
}

#[test]
fn revised_draft_of_previewed_utterance_updates_original_at_once() {
    let mut reducer = translating();
    feed(&mut reducer, [preview(n(1), "hello", "你好")]);

    feed(&mut reducer, [draft(n(1), "hello world how")]);

    let track = system_track(&reducer);
    assert_eq!(track.original, "hello world how");
    assert_eq!(track.translation, "你好");
    assert_eq!(track.pending_original, "");
    assert!(!track.is_final);
}

#[test]
fn anonymous_draft_during_preview_stays_pending() {
    let mut reducer = translating();
    feed(&mut reducer, [preview(None, "hello", "你好")]);

    feed(&mut reducer, [draft(None, "something else")]);

    let track = system_track(&reducer);
    assert_eq!(track.original, "hello");
    assert_eq!(track.pending_original, "something else");
}

#[test]
fn separately_arriving_finals_replace_preview_of_same_utterance() {
    let mut reducer = translating();
    feed(&mut reducer, [preview(n(1), "hello", "你好")]);

    feed(&mut reducer, [original_final(n(1), "Hello there.")]);
    let while_waiting = system_track(&reducer);
    feed(&mut reducer, [translation_final(n(1), "你好呀。")]);

    assert_eq!(while_waiting.original, "hello");
    assert!(!while_waiting.is_final);
    let track = system_track(&reducer);
    assert_eq!(
        (track.original.as_str(), track.translation.as_str()),
        ("Hello there.", "你好呀。")
    );
    assert!(track.is_final);
}

#[test]
fn newer_preview_replaces_older_and_empty_or_oversized_preview_is_ignored() {
    let mut reducer = translating();
    let oversized = "a".repeat(sheng_core::MAX_TEXT_BYTES + 1);

    feed(
        &mut reducer,
        [
            preview(n(1), "first", "第一"),
            preview(n(1), "first try", "第一次"),
            preview(n(1), "", "空"),
            preview(n(1), "first try again", "  "),
            preview(n(1), &oversized, "长"),
        ],
    );

    let track = system_track(&reducer);
    assert_eq!(
        (track.original.as_str(), track.translation.as_str()),
        ("first try", "第一次")
    );
    assert!(!track.is_final);
}

#[test]
fn empty_final_does_not_erase_preview() {
    let mut reducer = translating();
    feed(&mut reducer, [preview(n(1), "hello", "你好")]);

    feed(
        &mut reducer,
        [pair(n(1), " ", "你好"), original_final(n(1), "")],
    );

    let track = system_track(&reducer);
    assert_eq!(track.original, "hello");
    assert!(reducer.recent().is_empty());
}

#[test]
fn completed_preview_is_not_a_confirmation() {
    let mut reducer = translating();

    feed(&mut reducer, [preview(n(1), "Hello world.", "你好世界。")]);

    assert!(reducer.recent().is_empty());
    assert!(reducer.drain_confirmed().is_empty());
    assert!(!system_track(&reducer).is_final);
}

#[test]
fn older_final_does_not_replace_newer_confirmed_pair() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [
            draft(s("a"), "first"),
            draft(s("b"), "second"),
            pair(s("b"), "Second.", "第二。"),
        ],
    );

    feed(&mut reducer, [pair(s("a"), "First.", "第一。")]);

    let track = system_track(&reducer);
    assert_eq!(track.original, "Second.");
    assert!(track.is_final);
    assert_eq!(recent_originals(&reducer), ["Second."]);
}

#[test]
fn late_older_final_enters_recent_without_moving_display_back() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [draft(s("a"), "first"), draft(s("b"), "second draft")],
    );

    feed(&mut reducer, [pair(s("a"), "First.", "第一。")]);

    assert_eq!(recent_originals(&reducer), ["First."]);
    let track = system_track(&reducer);
    assert_eq!(track.original, "second draft");
    assert!(!track.is_final);
}

#[test]
fn retract_removes_only_the_matching_preview() {
    let mut reducer = reducer_for(&[Source::System, Source::Microphone], "zh");
    feed(
        &mut reducer,
        [pair(n(1), "Kept.", "保留。"), preview(n(2), "guess", "猜")],
    );
    reducer
        .apply(
            Source::Microphone,
            preview(n(2), "mic guess", "麦克风"),
            NOW_MS,
        )
        .unwrap();

    feed(&mut reducer, [retract(n(9))]);
    let after_unrelated = system_track(&reducer);
    feed(&mut reducer, [retract(n(2))]);

    assert_eq!(after_unrelated.original, "guess");
    let system = system_track(&reducer);
    assert_eq!((system.original.as_str(), system.is_final), ("Kept.", true));
    assert_eq!(track_of(&reducer, Source::Microphone).original, "mic guess");
    assert_eq!(recent_originals(&reducer), ["Kept."]);
}

#[test]
fn reset_drops_unconfirmed_content_but_keeps_recent_and_queue() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [
            pair(n(1), "Done.", "完成。"),
            original_final(n(2), "Half pair."),
            draft(n(3), "draft"),
        ],
    );

    feed(
        &mut reducer,
        [CaptionEvent::Reset, translation_final(n(2), "半句。")],
    );

    let track = system_track(&reducer);
    assert_eq!(track.original, "");
    assert_eq!(track.pending_original, "");
    assert_eq!(recent_originals(&reducer), ["Done."]);
    assert_eq!(reducer.confirmed_len(), 1);
}

#[test]
fn clear_rejects_old_ids_and_accepts_new_ones() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [pair(n(1), "One.", "一。"), draft(n(2), "two")],
    );

    let changed = reducer.clear();
    feed(
        &mut reducer,
        [
            draft(n(2), "two again"),
            pair(n(2), "Two.", "二。"),
            pair(n(3), "Three.", "三。"),
        ],
    );

    assert!(changed);
    assert_eq!(recent_originals(&reducer), ["Three."]);
    assert_eq!(system_track(&reducer).original, "Three.");
}

#[test]
fn clear_does_not_replay_cleared_content() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [pair(n(1), "One.", "一。"), draft(n(2), "two")],
    );

    reducer.clear();

    let track = system_track(&reducer);
    assert_eq!(
        (track.original.as_str(), track.pending_original.as_str()),
        ("", "")
    );
    assert!(reducer.recent().is_empty());
}

#[test]
fn reset_after_clear_lets_service_reuse_old_ids() {
    let mut reducer = translating();
    feed(&mut reducer, [pair(n(1), "One.", "一。")]);
    reducer.clear();

    feed(
        &mut reducer,
        [CaptionEvent::Reset, pair(n(1), "New one.", "新的一。")],
    );

    assert_eq!(recent_originals(&reducer), ["New one."]);
}
