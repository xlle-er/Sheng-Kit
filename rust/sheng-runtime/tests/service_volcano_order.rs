//! Volcano AST v2: response mapping, service order and error codes, checked through the
//! sheng-core caption reducer (captions.md §3, §6).

#[path = "fixtures/volcano/proto.rs"]
mod proto;

use proto::events as ev;
use sheng_core::{CaptionEvent, CaptionReducer, ReducerConfig, Source, UtteranceId};
use sheng_runtime::provider::ProviderError;
use sheng_runtime::services::volcano::mapper::{Mapped, Mapper};
use sheng_runtime::services::volcano::message::decode_response;

/// One scripted service response.
type Step<'a> = (i32, Option<&'a str>, Option<i32>);

fn map_all(mapper: &mut Mapper, steps: &[Step<'_>]) -> Vec<Mapped> {
    steps
        .iter()
        .flat_map(|(event, text, start)| {
            let bytes = proto::response(*event, *text, *start, None);
            mapper.map(decode_response(&bytes).unwrap())
        })
        .collect()
}

fn reducer() -> CaptionReducer {
    CaptionReducer::new(ReducerConfig {
        run_id: "run".into(),
        generation: 1,
        sources: vec![Source::System],
        source_language: "zh".into(),
        target_language: "en".into(),
    })
}

fn play(reducer: &mut CaptionReducer, mapped: Vec<Mapped>) {
    for item in mapped {
        if let Mapped::Caption(event) = item {
            reducer.apply(Source::System, event, 0).unwrap();
        }
    }
}

fn pairs(reducer: &CaptionReducer) -> Vec<(String, String)> {
    reducer
        .recent()
        .into_iter()
        .map(|c| (c.original, c.translation))
        .collect()
}

fn orders(mapped: &[Mapped]) -> Vec<(UtteranceId, i64)> {
    mapped
        .iter()
        .filter_map(|m| match m {
            Mapped::Caption(CaptionEvent::Order {
                utterance,
                sequence,
            }) => Some((utterance.clone(), *sequence)),
            _ => None,
        })
        .collect()
}

#[test]
fn sentence_pairs_confirm_in_order() {
    let mut mapper = Mapper::new();
    let mut state = reducer();
    let mapped = map_all(
        &mut mapper,
        &[
            (ev::SOURCE_START, None, Some(0)),
            (ev::SOURCE_RESPONSE, Some("你好"), None),
            (ev::SOURCE_END, Some("你好。"), Some(0)),
            (ev::TRANSLATION_START, None, Some(0)),
            (ev::TRANSLATION_RESPONSE, Some("Hello"), None),
            (ev::TRANSLATION_END, Some("Hello."), Some(0)),
            (ev::SOURCE_START, None, Some(1500)),
            (ev::SOURCE_END, Some("再见。"), Some(1500)),
            (ev::TRANSLATION_START, None, Some(1500)),
            (ev::TRANSLATION_END, Some("Bye."), Some(1500)),
        ],
    );
    assert_eq!(
        orders(&mapped),
        vec![(UtteranceId::Number(0), 0), (UtteranceId::Number(1), 1500)]
    );
    play(&mut state, mapped);
    let expected = [("你好。", "Hello."), ("再见。", "Bye.")]
        .map(|(o, t)| (o.to_owned(), t.to_owned()))
        .to_vec();
    assert_eq!(pairs(&state), expected);
    let track = &state.tracks()[0];
    assert!(track.is_final);
    assert_eq!(track.original, "再见。");
}

#[test]
fn late_end_of_an_older_sentence_keeps_the_newer_display() {
    let mut mapper = Mapper::new();
    let mut state = reducer();
    let mapped = map_all(
        &mut mapper,
        &[
            (ev::SOURCE_START, None, Some(0)),
            (ev::SOURCE_END, Some("第一句。"), Some(0)),
            (ev::SOURCE_START, None, Some(2000)),
            (ev::SOURCE_RESPONSE, Some("第二"), None),
            (ev::TRANSLATION_START, None, Some(0)),
            (ev::TRANSLATION_END, Some("First."), Some(0)),
        ],
    );
    play(&mut state, mapped);
    assert_eq!(pairs(&state), vec![("第一句。".into(), "First.".into())]);
    let track = &state.tracks()[0];
    assert_eq!(track.original, "第二");
    assert!(!track.is_final);
}

#[test]
fn empty_sentence_boundary_keeps_the_finished_pair() {
    let mut mapper = Mapper::new();
    let mut state = reducer();
    let mapped = map_all(
        &mut mapper,
        &[
            (ev::SOURCE_START, None, Some(0)),
            (ev::SOURCE_END, Some("好的。"), Some(0)),
            (ev::TRANSLATION_START, None, Some(0)),
            (ev::TRANSLATION_END, Some("OK."), Some(0)),
            (ev::SOURCE_START, None, Some(900)),
            (ev::SOURCE_END, Some(""), Some(900)),
            (ev::TRANSLATION_START, None, Some(900)),
            (ev::TRANSLATION_END, Some(""), Some(900)),
        ],
    );
    play(&mut state, mapped);
    assert_eq!(pairs(&state), vec![("好的。".into(), "OK.".into())]);
    let track = &state.tracks()[0];
    assert_eq!(
        (track.original.as_str(), track.translation.as_str()),
        ("好的。", "OK.")
    );
    assert!(track.is_final);
}

#[test]
fn invalid_order_information_is_dropped_but_events_still_count() {
    let mut mapper = Mapper::new();
    let mut state = reducer();
    let mapped = map_all(
        &mut mapper,
        &[
            (ev::SOURCE_START, None, Some(-40)),
            (ev::SOURCE_END, Some("一"), None),
            (ev::SOURCE_START, None, Some(500)),
            (ev::SOURCE_END, Some("二"), Some(500)),
            (ev::SOURCE_START, None, Some(500)),
            (ev::SOURCE_END, Some("三"), Some(500)),
            (ev::SOURCE_START, None, Some(100)),
            (ev::SOURCE_END, Some("四"), Some(100)),
            (ev::SOURCE_START, None, None),
            (ev::SOURCE_END, Some("五"), None),
        ],
    );
    // Negative, duplicate, regressing and missing start times produce no order.
    assert_eq!(orders(&mapped), vec![(UtteranceId::Number(1), 500)]);
    let mut more = mapped;
    more.extend(map_all(
        &mut mapper,
        &[
            (ev::TRANSLATION_END, Some("one"), Some(-40)),
            (ev::TRANSLATION_END, Some("two"), Some(500)),
            (ev::TRANSLATION_END, Some("three"), None),
            (ev::TRANSLATION_END, Some("four"), None),
            (ev::TRANSLATION_END, Some("five"), None),
        ],
    ));
    play(&mut state, more);
    let originals: Vec<String> = pairs(&state).into_iter().map(|(o, _)| o).collect();
    assert_eq!(originals, ["一", "二", "三", "四", "五"]);
}

#[test]
fn translation_pairs_with_the_source_of_the_same_start_time() {
    let mut mapper = Mapper::new();
    let mut state = reducer();
    let mapped = map_all(
        &mut mapper,
        &[
            (ev::SOURCE_START, None, Some(0)),
            (ev::SOURCE_END, Some("甲。"), Some(0)),
            (ev::SOURCE_START, None, Some(700)),
            (ev::SOURCE_END, Some("乙。"), Some(700)),
            (ev::TRANSLATION_START, None, Some(700)),
            (ev::TRANSLATION_END, Some("B."), Some(700)),
            (ev::TRANSLATION_START, None, Some(0)),
            (ev::TRANSLATION_END, Some("A."), Some(0)),
        ],
    );
    play(&mut state, mapped);
    // B. pairs with 乙 (same start time), not with 甲; sheng-core confirms in sentence
    // order once 甲 has its own translation (captions.md §3.3).
    assert_eq!(
        pairs(&state),
        vec![("甲。".into(), "A.".into()), ("乙。".into(), "B.".into())]
    );
}

#[test]
fn drafts_accept_whole_text_or_pieces() {
    let mut mapper = Mapper::new();
    let mapped = map_all(
        &mut mapper,
        &[
            (ev::SOURCE_START, None, Some(0)),
            (ev::SOURCE_RESPONSE, Some("今天"), None),
            (ev::SOURCE_RESPONSE, Some("今天天气"), None),
            (ev::SOURCE_RESPONSE, Some("不错"), None),
        ],
    );
    let drafts: Vec<&str> = mapped
        .iter()
        .filter_map(|m| match m {
            Mapped::Caption(CaptionEvent::OriginalDraft { text, .. }) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(drafts, ["今天", "今天天气", "今天天气不错"]);
}

#[test]
fn lifecycle_events_and_ignored_events() {
    let mut mapper = Mapper::new();
    let mapped = map_all(
        &mut mapper,
        &[
            (ev::SESSION_STARTED, None, None),
            (ev::USAGE, None, None),
            (ev::AUDIO_MUTED, None, None),
            (350, None, None),
            (9999, Some("x"), None),
            (ev::SESSION_FINISHED, None, None),
        ],
    );
    assert_eq!(mapped, vec![Mapped::Started, Mapped::Ended]);
}

#[test]
fn status_codes_map_to_normalized_errors() {
    let cases = [
        (45_000_001, ProviderError::Configuration),
        (45_000_151, ProviderError::Configuration),
        (45_000_002, ProviderError::Provider),
        (45_000_081, ProviderError::Transport),
        (55_000_031, ProviderError::Unavailable { retry_after: None }),
        (55_000_099, ProviderError::Provider),
        (11_200, ProviderError::Authentication),
        (11_301, ProviderError::RateLimited { retry_after: None }),
    ];
    for (status, expected) in cases {
        let mut mapper = Mapper::new();
        let bytes = proto::failure(ev::SESSION_FAILED, status);
        let mapped = mapper.map(decode_response(&bytes).unwrap());
        assert_eq!(mapped, vec![Mapped::Failed(expected)], "status {status}");
    }
    let mut mapper = Mapper::new();
    let ok = proto::failure(ev::SESSION_STARTED, 20_000_000);
    assert_eq!(
        mapper.map(decode_response(&ok).unwrap()),
        vec![Mapped::Started]
    );
    let failed = proto::response(ev::SESSION_FAILED, None, None, None);
    assert_eq!(
        mapper.map(decode_response(&failed).unwrap()),
        vec![Mapped::Failed(ProviderError::Provider)]
    );
}
