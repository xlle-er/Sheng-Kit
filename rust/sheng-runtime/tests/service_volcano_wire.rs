//! Volcano AST v2: protobuf decoding/encoding properties, 80 ms framing, language codes.

#[path = "fixtures/volcano/proto.rs"]
mod proto;

use proto::{boundaries, encode, events, Field};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use sheng_runtime::services::volcano::frames::{Framer, FRAME_BYTES};
use sheng_runtime::services::volcano::language::wire_pair;
use sheng_runtime::services::volcano::message::{
    decode_response, finish_session, start_session, task_request, Response,
};
use sheng_runtime::services::volcano::wire::{
    Reader, Value, WireError, MAX_FIELD_BYTES, MAX_MESSAGE_BYTES,
};

const CASES: u64 = 300;

fn known_fields(rng: &mut StdRng) -> (Vec<Field>, Response) {
    let event = rng.random_range(0..700);
    let status = if rng.random_bool(0.5) { 20_000_000 } else { 0 };
    let text: String = (0..rng.random_range(0..20))
        .map(|_| ['a', '字', ' ', 'é'][rng.random_range(0..4)])
        .collect();
    let start = rng.random_range(-5..100_000);
    let end = rng.random_range(-5..100_000);
    let fields = vec![
        Field::int32(proto::EVENT, event),
        proto::meta(status),
        Field::text(proto::TEXT, &text),
        Field::int32(proto::START_TIME, start),
        Field::int32(proto::END_TIME, end),
    ];
    let expected = Response {
        event,
        status_code: status,
        text,
        start_time: Some(start),
        end_time: Some(end),
    };
    (fields, expected)
}

fn unknown_field(rng: &mut StdRng) -> Field {
    let number = [3, 7, 8, 9, 10, 11, 99, 5000][rng.random_range(0..8)];
    match rng.random_range(0..4) {
        0 => Field::Varint(number, rng.random()),
        1 => Field::Bytes(
            number,
            (0..rng.random_range(0..40)).map(|_| rng.random()).collect(),
        ),
        2 => Field::Fixed32(number, rng.random()),
        _ => Field::Fixed64(number, rng.random()),
    }
}

fn shuffled_message(rng: &mut StdRng) -> (Vec<Field>, Response) {
    let (mut fields, expected) = known_fields(rng);
    for _ in 0..rng.random_range(0..6) {
        fields.push(unknown_field(rng));
    }
    fields.shuffle(rng);
    (fields, expected)
}

#[test]
fn decodes_known_fields_in_any_order_and_skips_unknown_ones() {
    let mut rng = StdRng::seed_from_u64(0x5eed_0001);
    for _ in 0..CASES {
        let (fields, expected) = shuffled_message(&mut rng);
        assert_eq!(decode_response(&encode(&fields)), Ok(expected));
    }
}

#[test]
fn truncated_messages_fail_unless_cut_between_fields() {
    let mut rng = StdRng::seed_from_u64(0x5eed_0002);
    for _ in 0..60 {
        let (fields, _) = shuffled_message(&mut rng);
        let bytes = encode(&fields);
        let ends = boundaries(&fields);
        for cut in 0..bytes.len() {
            let result = decode_response(&bytes[..cut]);
            if ends.contains(&cut) {
                assert!(result.is_ok(), "cut at a field boundary decodes");
            } else {
                assert_eq!(result, Err(WireError::Truncated), "cut inside a field");
            }
        }
    }
}

#[test]
fn repeated_single_value_fields_are_rejected() {
    let mut rng = StdRng::seed_from_u64(0x5eed_0003);
    for _ in 0..CASES {
        let (mut fields, _) = known_fields(&mut rng);
        let copy = fields[rng.random_range(0..fields.len())].clone();
        fields.push(copy);
        fields.shuffle(&mut rng);
        assert_eq!(decode_response(&encode(&fields)), Err(WireError::Duplicate));
    }
}

#[test]
fn repeated_status_inside_response_meta_is_rejected() {
    let inner = encode(&[
        Field::int32(proto::META_STATUS, 1),
        Field::int32(proto::META_STATUS, 2),
    ]);
    let bytes = encode(&[Field::Bytes(proto::RESPONSE_META, inner)]);
    assert_eq!(decode_response(&bytes), Err(WireError::Duplicate));
}

#[test]
fn oversized_fields_and_messages_are_rejected() {
    let mut declared = Vec::new();
    proto::key(&mut declared, proto::DATA, 2);
    proto::varint(&mut declared, MAX_FIELD_BYTES as u64 + 1);
    assert_eq!(decode_response(&declared), Err(WireError::TooLong));

    let mut huge = Vec::new();
    proto::key(&mut huge, proto::DATA, 2);
    proto::varint(&mut huge, u64::MAX >> 1);
    assert_eq!(decode_response(&huge), Err(WireError::TooLong));

    let whole = vec![0u8; MAX_MESSAGE_BYTES + 1];
    assert_eq!(decode_response(&whole), Err(WireError::TooLong));
}

#[test]
fn illegal_wire_types_and_malformed_keys_are_rejected() {
    for wire_type in [3u8, 4, 6, 7] {
        let mut bytes = Vec::new();
        proto::key(&mut bytes, 99, wire_type);
        bytes.extend_from_slice(&[0; 8]);
        assert_eq!(decode_response(&bytes), Err(WireError::WireType));
    }
    // Known fields with the wrong wire type.
    let wrong_event = encode(&[Field::text(proto::EVENT, "650")]);
    assert_eq!(decode_response(&wrong_event), Err(WireError::WireType));
    let wrong_text = encode(&[Field::Varint(proto::TEXT, 1)]);
    assert_eq!(decode_response(&wrong_text), Err(WireError::WireType));
    // Field number zero, varint longer than ten bytes, invalid UTF-8 text.
    assert_eq!(decode_response(&[0x00, 0x01]), Err(WireError::FieldNumber));
    let long = [
        0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f,
    ];
    assert_eq!(decode_response(&long), Err(WireError::VarintOverflow));
    let bad_utf8 = encode(&[Field::Bytes(proto::TEXT, vec![0xff, 0xfe])]);
    assert_eq!(decode_response(&bad_utf8), Err(WireError::Utf8));
}

#[test]
fn hand_written_frame_decodes() {
    // event = 652 (SourceSubtitleEnd), text = "hi", start_time = 120, end_time = 980.
    let bytes = [
        0x10, 0x8c, 0x05, 0x22, 0x02, b'h', b'i', 0x28, 0x78, 0x30, 0xd4, 0x07,
    ];
    let response = decode_response(&bytes).unwrap();
    assert_eq!(response.event, events::SOURCE_END);
    assert_eq!(response.text, "hi");
    assert_eq!(
        (response.start_time, response.end_time),
        (Some(120), Some(980))
    );
    // Negative int32 is a ten-byte varint.
    let negative = encode(&[Field::int32(proto::START_TIME, -1)]);
    assert_eq!(negative.len(), 11);
    assert_eq!(decode_response(&negative).unwrap().start_time, Some(-1));
}

/// Fields of one encoded message as `(number, value)` pairs.
fn fields_of(bytes: &[u8]) -> Vec<(u32, Value<'_>)> {
    let mut reader = Reader::new(bytes).unwrap();
    std::iter::from_fn(|| reader.next_field().unwrap()).collect()
}

fn find<'a>(fields: &[(u32, Value<'a>)], number: u32) -> Value<'a> {
    fields
        .iter()
        .find(|(n, _)| *n == number)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("field {number} present"))
}

#[test]
fn start_session_declares_speech_to_text_and_wav_raw_audio() {
    let bytes = start_session("sid-1", "zh", "en");
    let top = fields_of(&bytes);
    assert_eq!(find(&top, 2).varint(), Ok(100));
    let meta = fields_of(find(&top, 1).bytes().unwrap());
    assert_eq!(find(&meta, 6).string(), Ok("sid-1"));
    let audio = fields_of(find(&top, 4).bytes().unwrap());
    assert_eq!(find(&audio, 4).string(), Ok("wav"));
    assert_eq!(find(&audio, 5).string(), Ok("raw"));
    assert_eq!(find(&audio, 7).varint(), Ok(16_000));
    assert_eq!(find(&audio, 8).varint(), Ok(16));
    assert_eq!(find(&audio, 9).varint(), Ok(1));
    let params = fields_of(find(&top, 6).bytes().unwrap());
    assert_eq!(find(&params, 1).string(), Ok("s2t"));
    assert_eq!(find(&params, 2).string(), Ok("zh"));
    assert_eq!(find(&params, 3).string(), Ok("en"));
}

#[test]
fn task_and_finish_requests_round_trip() {
    let mut rng = StdRng::seed_from_u64(0x5eed_0004);
    let pcm: Vec<u8> = (0..FRAME_BYTES).map(|_| rng.random()).collect();
    let task = task_request("sid-2", &pcm);
    let top = fields_of(&task);
    assert_eq!(find(&top, 2).varint(), Ok(200));
    let audio = fields_of(find(&top, 4).bytes().unwrap());
    assert_eq!(find(&audio, 14).bytes(), Ok(pcm.as_slice()));

    let finish = finish_session("sid-2");
    let top = fields_of(&finish);
    assert_eq!(find(&top, 2).varint(), Ok(102));
    assert!(
        top.iter().all(|(n, _)| *n != 4),
        "no audio in FinishSession"
    );
}

#[test]
fn framer_cuts_any_input_into_exact_80ms_frames() {
    assert_eq!(FRAME_BYTES, 2560);
    let mut rng = StdRng::seed_from_u64(0x5eed_0005);
    for _ in 0..100 {
        let total = rng.random_range(0..20_000);
        let input: Vec<u8> = (0..total).map(|_| rng.random()).collect();
        let mut framer = Framer::new();
        let mut frames = Vec::new();
        let mut offset = 0;
        while offset < total {
            let step = rng.random_range(1..=6000).min(total - offset);
            frames.extend(framer.push(&input[offset..offset + step]));
            assert!(framer.pending() < FRAME_BYTES);
            offset += step;
        }
        assert_eq!(frames.len(), total / FRAME_BYTES);
        let last = framer.flush();
        assert_eq!(last.is_some(), total % FRAME_BYTES != 0);
        frames.extend(last);
        assert!(frames.iter().all(|f| f.len() == FRAME_BYTES));
        let joined: Vec<u8> = frames.concat();
        assert_eq!(&joined[..total], input.as_slice());
        assert!(
            joined[total..].iter().all(|b| *b == 0),
            "tail padded with silence"
        );
        assert_eq!(framer.flush(), None);
    }
}

#[test]
fn language_codes_follow_the_document() {
    assert_eq!(wire_pair("yue", "zh"), ("yue-CN", "zh"));
    assert_eq!(wire_pair("wuu", "en"), ("sh-CN", "en"));
    assert_eq!(wire_pair("zh_en", "en"), ("zhen", "zhen"));
    assert_eq!(wire_pair("ja", "zh"), ("ja", "zh"));
    assert_eq!(wire_pair("en", "pt"), ("en", "pt"));
}
