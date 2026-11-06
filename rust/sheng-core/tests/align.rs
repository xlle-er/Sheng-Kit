//! Dual-stream alignment without utterance ids (captions.md §5).

use std::time::Duration;

use sheng_core::align::{AlignError, AlignedPair, Chunk, DualStreamAligner, Stream};
use sheng_core::MAX_TEXT_BYTES;

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn texts(pairs: &[AlignedPair]) -> Vec<(&str, &str)> {
    pairs
        .iter()
        .map(|p| (p.original.as_str(), p.translation.as_str()))
        .collect()
}

fn push(
    aligner: &mut DualStreamAligner,
    stream: Stream,
    chunk: Chunk<'_>,
    at: u64,
) -> Vec<AlignedPair> {
    aligner.push(stream, chunk, ms(at)).expect("within limit")
}

#[test]
fn sentence_punctuation_on_both_sides_confirms_a_pair() {
    let mut aligner = DualStreamAligner::new();

    let after_original = push(
        &mut aligner,
        Stream::Original,
        Chunk::Delta("Hello there. How"),
        0,
    );
    let pairs = push(
        &mut aligner,
        Stream::Translation,
        Chunk::Delta("你好。"),
        100,
    );

    assert!(after_original.is_empty());
    assert_eq!(texts(&pairs), [("Hello there.", "你好。")]);
    assert_eq!(aligner.pending(), (" How", ""));
}

#[test]
fn several_sentences_pair_one_by_one() {
    let mut aligner = DualStreamAligner::new();
    push(
        &mut aligner,
        Stream::Original,
        Chunk::Delta("One. Two. "),
        0,
    );

    let pairs = push(
        &mut aligner,
        Stream::Translation,
        Chunk::Delta("一。二。"),
        10,
    );

    assert_eq!(texts(&pairs), [("One.", "一。"), ("Two.", "二。")]);
}

#[test]
fn deltas_keep_repeated_words_and_whitespace() {
    let mut aligner = DualStreamAligner::new();

    for chunk in ["la", " la", " la", "  "] {
        push(&mut aligner, Stream::Original, Chunk::Delta(chunk), 0);
    }

    assert_eq!(aligner.pending().0, "la la la  ");
}

#[test]
fn cumulative_snapshots_append_only_new_text() {
    let mut aligner = DualStreamAligner::new();
    for snapshot in ["Hello", "Hello wor", "Hello world. ", "Hello world. "] {
        push(&mut aligner, Stream::Original, Chunk::Snapshot(snapshot), 0);
    }

    let first = push(
        &mut aligner,
        Stream::Translation,
        Chunk::Snapshot("你好世界。"),
        10,
    );
    push(
        &mut aligner,
        Stream::Original,
        Chunk::Snapshot("Hello world. Again. "),
        20,
    );
    let second = push(
        &mut aligner,
        Stream::Translation,
        Chunk::Snapshot("你好世界。再来。"),
        30,
    );

    assert_eq!(texts(&first), [("Hello world.", "你好世界。")]);
    assert_eq!(texts(&second), [("Again.", "再来。")]);
    assert_eq!(aligner.pending().0.trim(), "");
    assert_eq!(aligner.pending().1, "");
}

#[test]
fn rewritten_snapshot_replaces_unconfirmed_text() {
    let mut aligner = DualStreamAligner::new();
    push(
        &mut aligner,
        Stream::Original,
        Chunk::Snapshot("I scream"),
        0,
    );

    push(
        &mut aligner,
        Stream::Original,
        Chunk::Snapshot("Ice cream"),
        10,
    );

    assert_eq!(aligner.pending().0, "Ice cream");
}

#[test]
fn short_side_cannot_confirm_a_much_longer_side() {
    let mut aligner = DualStreamAligner::new();
    push(
        &mut aligner,
        Stream::Original,
        Chunk::Delta("This is a very long sentence that keeps on going for a while. "),
        0,
    );

    let pairs = push(&mut aligner, Stream::Translation, Chunk::Delta("是。"), 10);

    assert!(pairs.is_empty());
}

#[test]
fn one_sided_text_waits_for_the_other_side() {
    let mut aligner = DualStreamAligner::new();
    push(&mut aligner, Stream::Original, Chunk::Delta("Alone. "), 0);

    let pairs = aligner.tick(ms(5_000));

    assert!(pairs.is_empty());
    assert_eq!(aligner.pending().0, "Alone. ");
}

#[test]
fn quiet_timeout_confirms_paired_text_after_two_seconds() {
    let mut aligner = DualStreamAligner::new();
    push(
        &mut aligner,
        Stream::Original,
        Chunk::Delta("How are you"),
        0,
    );
    push(
        &mut aligner,
        Stream::Translation,
        Chunk::Delta("你好吗"),
        100,
    );

    let early = aligner.tick(ms(2_099));
    let due = aligner.tick(ms(2_100));

    assert!(early.is_empty());
    assert_eq!(texts(&due), [("How are you", "你好吗")]);
}

#[test]
fn turn_complete_merges_late_tail_into_the_finished_pair() {
    let mut aligner = DualStreamAligner::new();
    push(
        &mut aligner,
        Stream::Original,
        Chunk::Delta("Good morning"),
        0,
    );
    push(&mut aligner, Stream::Translation, Chunk::Delta("早上"), 100);
    aligner.complete_turn(ms(1_000));

    let during_tail = push(
        &mut aligner,
        Stream::Translation,
        Chunk::Delta("好。"),
        1_300,
    );
    let before_window = aligner.tick(ms(1_499));
    let closed = aligner.tick(ms(1_500));

    assert!(during_tail.is_empty() && before_window.is_empty());
    assert_eq!(texts(&closed), [("Good morning", "早上好。")]);
}

#[test]
fn text_after_tail_window_starts_a_new_turn() {
    let mut aligner = DualStreamAligner::new();
    push(
        &mut aligner,
        Stream::Original,
        Chunk::Snapshot("First turn"),
        0,
    );
    push(
        &mut aligner,
        Stream::Translation,
        Chunk::Snapshot("第一轮"),
        0,
    );
    aligner.complete_turn(ms(1_000));

    let closed = push(
        &mut aligner,
        Stream::Original,
        Chunk::Snapshot("Second"),
        2_000,
    );

    assert_eq!(texts(&closed), [("First turn", "第一轮")]);
    assert_eq!(aligner.pending(), ("Second", ""));
}

#[test]
fn service_cut_points_pair_text_without_punctuation() {
    let mut aligner = DualStreamAligner::new();
    push(
        &mut aligner,
        Stream::Original,
        Chunk::Delta("first part. second part. "),
        0,
    );
    aligner.mark_boundary(Stream::Original);
    push(
        &mut aligner,
        Stream::Translation,
        Chunk::Delta("erster und zweiter Teil"),
        10,
    );

    let pairs = aligner.mark_boundary(Stream::Translation);

    assert_eq!(
        texts(&pairs),
        [("first part. second part.", "erster und zweiter Teil")]
    );
}

#[test]
fn follow_latency_is_rounded_once_to_milliseconds() {
    let mut aligner = DualStreamAligner::new();
    aligner
        .push(
            Stream::Original,
            Chunk::Delta("Hi. "),
            Duration::from_micros(1_000_100),
        )
        .unwrap();

    let pairs = aligner
        .push(
            Stream::Translation,
            Chunk::Delta("嗨。"),
            Duration::from_micros(1_250_700),
        )
        .unwrap();

    assert_eq!(pairs[0].follow_latency_ms, Some(251));
}

#[test]
fn one_sided_growth_past_the_limit_fails_instead_of_dropping() {
    let mut aligner = DualStreamAligner::new();
    push(
        &mut aligner,
        Stream::Original,
        Chunk::Delta(&"a".repeat(MAX_TEXT_BYTES)),
        0,
    );

    let result = aligner.push(Stream::Original, Chunk::Delta("b"), ms(1));

    assert_eq!(result, Err(AlignError::Overflow));
}

#[test]
fn finish_confirms_only_pairs_with_content_on_both_sides() {
    let mut paired = DualStreamAligner::new();
    push(&mut paired, Stream::Original, Chunk::Delta("Bye"), 0);
    push(&mut paired, Stream::Translation, Chunk::Delta("再见"), 0);
    let mut one_sided = DualStreamAligner::new();
    push(
        &mut one_sided,
        Stream::Original,
        Chunk::Delta("Unanswered"),
        0,
    );
    push(&mut one_sided, Stream::Translation, Chunk::Delta(" 。"), 0);

    let paired_result = paired.finish();
    let one_sided_result = one_sided.finish();

    assert_eq!(texts(&paired_result), [("Bye", "再见")]);
    assert!(one_sided_result.is_empty());
    assert_eq!(one_sided.pending(), ("", ""));
}
