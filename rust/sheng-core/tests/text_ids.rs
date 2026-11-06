//! Text helpers and bounded identity sets (captions.md §2.8, §7).

use sheng_core::ids::{BoundedMap, BoundedSet, CaptionIdFactory};
use sheng_core::text::{first_sentence_end, is_meaningful, within_limit};
use sheng_core::MAX_TEXT_BYTES;

#[test]
fn whitespace_and_punctuation_alone_are_not_meaningful() {
    let blank = ["", "   ", "。，！", " ... ", "\u{3000}、"];

    assert!(blank.iter().all(|t| !is_meaningful(t)));
    assert!(is_meaningful(" a. "));
    assert!(is_meaningful("好"));
}

#[test]
fn limit_counts_utf8_bytes_not_characters() {
    let three_byte_chars = "字".repeat(MAX_TEXT_BYTES / 3);

    assert!(within_limit(&three_byte_chars));
    assert!(!within_limit(&format!("{three_byte_chars}字")));
}

#[test]
fn sentence_end_needs_whitespace_after_ascii_enders() {
    assert_eq!(first_sentence_end("Pi is 3.14 today"), None);
    assert_eq!(first_sentence_end("Done. Next"), Some(5));
    assert_eq!(first_sentence_end("好。下一句"), Some("好。".len()));
    assert_eq!(
        first_sentence_end("真的？！」然后"),
        Some("真的？！」".len())
    );
}

#[test]
fn bounded_set_evicts_oldest_first() {
    let mut set = BoundedSet::new(2);

    let evictions = [set.insert(1), set.insert(2), set.insert(2), set.insert(3)];

    assert_eq!(evictions, [None, None, None, Some(1)]);
    assert!(!set.contains(&1) && set.contains(&2) && set.contains(&3));
    assert_eq!(set.len(), 2);
}

#[test]
fn bounded_map_evicts_oldest_key_and_keeps_replaced_position() {
    let mut map = BoundedMap::new(2);

    map.insert("a", 1);
    map.insert("b", 2);
    map.insert("a", 3);
    map.insert("c", 4);

    assert_eq!(map.get(&"a"), None);
    assert_eq!((map.get(&"b"), map.get(&"c")), (Some(&2), Some(&4)));
}

#[test]
fn caption_sequence_keeps_increasing_across_generations() {
    let mut ids = CaptionIdFactory::new("r", 1);

    let first = ids.next_id();
    ids.set_generation(2);
    let second = ids.next_id();

    assert_eq!((first.as_str(), second.as_str()), ("r:1:0", "r:2:1"));
}
