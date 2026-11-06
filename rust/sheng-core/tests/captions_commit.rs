//! Committing unconfirmed text when a connection ends unexpectedly or a stop times out
//! (providers/README.md “意外断开”, “结束时不丢尾句”).

mod common;

use common::*;
use sheng_core::Source;

fn commit(reducer: &mut sheng_core::CaptionReducer) -> bool {
    reducer
        .commit_unconfirmed(Source::System, NOW_MS)
        .expect("commit applies")
}

fn recent_pairs(reducer: &sheng_core::CaptionReducer) -> Vec<(String, String)> {
    reducer
        .recent()
        .into_iter()
        .map(|c| (c.original, c.translation))
        .collect()
}

#[test]
fn original_draft_is_committed_once() {
    let mut reducer = original_only();
    feed(&mut reducer, [draft(n(1), "half a sen")]);

    let first = commit(&mut reducer);
    let second = commit(&mut reducer);
    feed(&mut reducer, [original_final(n(1), "half a sentence")]);

    assert!(first);
    assert!(!second);
    assert_eq!(recent_originals(&reducer), ["half a sen"]);
    assert_eq!(reducer.confirmed_len(), 1);
    assert!(system_track(&reducer).is_final);
}

#[test]
fn translation_draft_is_confirmed_with_its_original() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [draft(n(1), "Hello wor"), translation_draft(n(1), "你好世")],
    );

    commit(&mut reducer);

    assert_eq!(
        recent_pairs(&reducer),
        [("Hello wor".to_owned(), "你好世".to_owned())]
    );
}

#[test]
fn early_translation_final_is_confirmed_with_the_draft() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [
            draft(s("a"), "Good morn"),
            translation_final(s("a"), "早上好。"),
        ],
    );

    commit(&mut reducer);

    assert_eq!(
        recent_pairs(&reducer),
        [("Good morn".to_owned(), "早上好。".to_owned())]
    );
}

#[test]
fn original_final_waiting_for_translation_is_committed_once() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [
            original_final(n(1), "First."),
            translation_draft(n(1), "第"),
            draft(n(2), "Seco"),
        ],
    );

    commit(&mut reducer);
    commit(&mut reducer);

    assert_eq!(
        recent_pairs(&reducer),
        [
            ("First.".to_owned(), String::new()),
            ("Seco".to_owned(), String::new())
        ]
    );
}

#[test]
fn draft_without_translation_keeps_the_original() {
    let mut reducer = translating();
    feed(&mut reducer, [draft(None, "no translation yet")]);

    commit(&mut reducer);

    assert_eq!(
        recent_pairs(&reducer),
        [("no translation yet".to_owned(), String::new())]
    );
}

#[test]
fn nothing_meaningful_commits_nothing() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [pair(n(1), "Done.", "完成。"), draft(n(2), " ，")],
    );

    let changed = commit(&mut reducer);

    assert!(!changed);
    assert_eq!(recent_originals(&reducer), ["Done."]);
}

#[test]
fn preview_and_pending_draft_are_both_committed() {
    let mut reducer = translating();
    feed(
        &mut reducer,
        [preview(s("a"), "One two", "一二"), draft(s("b"), "Three")],
    );

    commit(&mut reducer);

    assert_eq!(
        recent_pairs(&reducer),
        [
            ("One two".to_owned(), "一二".to_owned()),
            ("Three".to_owned(), String::new())
        ]
    );
}
