//! Pairing of separately arriving finals and service order (captions.md §3, §6).

mod common;

use common::*;
use sheng_core::order::UtteranceOrder;
use sheng_core::UtteranceId;

#[test]
fn translation_arriving_first_waits_for_its_own_original() {
    let mut reducer = translating();

    feed(
        &mut reducer,
        [
            translation_final(n(2), "第二。"),
            original_final(n(1), "First."),
        ],
    );
    let before_own_translation = reducer.recent().len();
    feed(
        &mut reducer,
        [
            translation_final(n(1), "第一。"),
            original_final(n(2), "Second."),
        ],
    );

    assert_eq!(before_own_translation, 0);
    let pairs: Vec<(String, String)> = reducer
        .recent()
        .into_iter()
        .map(|c| (c.original, c.translation))
        .collect();
    assert_eq!(
        pairs,
        [
            ("First.".to_owned(), "第一。".to_owned()),
            ("Second.".to_owned(), "第二。".to_owned())
        ]
    );
}

#[test]
fn orphan_translation_never_pairs_with_another_original() {
    let mut reducer = translating();

    feed(
        &mut reducer,
        [
            translation_final(None, "孤立。"),
            translation_final(n(7), "七。"),
            original_final(None, "Anonymous."),
            original_final(n(8), "Eight."),
        ],
    );

    assert!(reducer.recent().is_empty());
}

#[test]
fn orphan_translation_draft_does_not_replace_paired_translation() {
    let mut reducer = translating();
    feed(&mut reducer, [pair(n(1), "One.", "一。")]);

    feed(
        &mut reducer,
        [
            translation_draft(n(9), "别的"),
            translation_draft(None, "无"),
        ],
    );

    assert_eq!(system_track(&reducer).translation, "一。");
}

#[test]
fn translation_draft_updates_its_own_unconfirmed_original() {
    let mut reducer = translating();
    feed(&mut reducer, [draft(n(1), "good morn")]);

    feed(&mut reducer, [translation_draft(n(1), "早上")]);

    let track = system_track(&reducer);
    assert_eq!(
        (track.original.as_str(), track.translation.as_str()),
        ("good morn", "早上")
    );
}

#[test]
fn queued_originals_confirm_in_service_order() {
    let mut reducer = translating();

    feed(
        &mut reducer,
        [
            original_final(n(1), "One."),
            original_final(n(2), "Two."),
            translation_final(n(2), "二。"),
        ],
    );
    let while_first_waits = reducer.recent().len();
    feed(&mut reducer, [translation_final(n(1), "一。")]);

    assert_eq!(while_first_waits, 0);
    assert_eq!(recent_originals(&reducer), ["One.", "Two."]);
}

#[test]
fn final_without_translation_is_not_confirmed() {
    let mut reducer = translating();

    feed(&mut reducer, [original_final(n(1), "Waiting.")]);

    assert!(reducer.recent().is_empty());
    let track = system_track(&reducer);
    assert_eq!(track.original, "Waiting.");
    assert!(!track.is_final);
}

#[test]
fn late_translation_does_not_disturb_next_draft() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [original_final(n(1), "First."), draft(n(2), "second draft")],
    );

    feed(&mut reducer, [translation_final(n(1), "第一。")]);

    assert_eq!(recent_originals(&reducer), ["First."]);
    let track = system_track(&reducer);
    assert_eq!(track.original, "second draft");
    assert!(!track.is_final);
}

#[test]
fn empty_original_final_does_not_move_current_sentence_back() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [pair(n(1), "One.", "一。"), draft(n(2), "two")],
    );

    feed(&mut reducer, [original_final(n(2), "  ")]);

    assert_eq!(system_track(&reducer).original, "two");
}

#[test]
fn late_end_of_older_sentence_keeps_newer_display_under_service_order() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [
            order("b", 2),
            draft(s("b"), "newer"),
            order("a", 1),
            draft(s("a"), "older arrives later"),
        ],
    );
    let after_late_draft = system_track(&reducer);

    feed(
        &mut reducer,
        [
            pair(s("b"), "Newer.", "新。"),
            pair(s("a"), "Older.", "旧。"),
        ],
    );

    assert_eq!(after_late_draft.original, "newer");
    let track = system_track(&reducer);
    assert_eq!((track.original.as_str(), track.is_final), ("Newer.", true));
}

#[test]
fn invalid_order_information_is_ignored_and_state_keeps_working() {
    let mut reducer = translating();

    feed(
        &mut reducer,
        [
            order("a", -1),
            order("a", 1),
            order("b", 1),
            order("a", 0),
            order("c", i64::MAX),
            pair(s("a"), "A.", "甲。"),
            pair(s("b"), "B.", "乙。"),
            pair(s("c"), "C.", "丙。"),
        ],
    );

    assert_eq!(recent_originals(&reducer), ["A.", "B.", "C."]);
}

#[test]
fn service_order_rejects_duplicate_reassigned_and_out_of_range_sequences() {
    let mut order = UtteranceOrder::new();
    let (a, b, c) = (
        UtteranceId::from("a"),
        UtteranceId::from("b"),
        UtteranceId::from("c"),
    );

    let accepted = [
        order.record_service_order(&a, 5),
        order.record_service_order(&a, 5),
        order.record_service_order(&b, 5),
        order.record_service_order(&a, 4),
        order.record_service_order(&c, -3),
        order.record_service_order(&c, i64::MAX),
        order.record_service_order(&b, 2),
    ];

    assert_eq!(accepted, [true, true, false, false, false, false, true]);
    assert!(order.is_older(&b, &a));
}

#[test]
fn arrival_order_applies_when_service_order_is_missing() {
    let mut order = UtteranceOrder::new();
    let (x, y) = (UtteranceId::from("x"), UtteranceId::from("y"));

    order.observe(&x);
    order.observe(&y);
    order.observe(&x);

    assert!(order.is_older(&x, &y));
    assert!(!order.is_older(&y, &x));
    assert!(!order.is_older(&x, &UtteranceId::from("unknown")));
}
