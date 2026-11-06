//! Test-side protobuf encoding of AST v2 responses, written from the published `.proto`
//! field numbers independently of the client's encoder.
#![allow(dead_code)]

/// `TranslateResponse` fields.
pub const RESPONSE_META: u32 = 1;
pub const EVENT: u32 = 2;
pub const DATA: u32 = 3;
pub const TEXT: u32 = 4;
pub const START_TIME: u32 = 5;
pub const END_TIME: u32 = 6;
pub const SPK_CHG: u32 = 7;
pub const MUTED_MS: u32 = 8;
pub const SPEAKER_ID: u32 = 9;
pub const LANGUAGE_CONFIDENCE: u32 = 11;
/// `ResponseMeta` fields.
pub const META_SESSION_ID: u32 = 1;
pub const META_STATUS: u32 = 3;
pub const META_MESSAGE: u32 = 4;

/// One encoded field: key plus value.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Varint(u32, u64),
    Bytes(u32, Vec<u8>),
    Fixed32(u32, u32),
    Fixed64(u32, u64),
}

pub fn varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            buf.push(byte);
            return;
        }
        buf.push(byte | 0x80);
    }
}

pub fn key(buf: &mut Vec<u8>, number: u32, wire_type: u8) {
    varint(buf, (u64::from(number) << 3) | u64::from(wire_type));
}

impl Field {
    pub fn int32(number: u32, value: i32) -> Self {
        Self::Varint(number, i64::from(value) as u64)
    }

    pub fn text(number: u32, value: &str) -> Self {
        Self::Bytes(number, value.as_bytes().to_vec())
    }

    pub fn number(&self) -> u32 {
        match self {
            Self::Varint(n, _) | Self::Bytes(n, _) | Self::Fixed32(n, _) | Self::Fixed64(n, _) => {
                *n
            }
        }
    }

    pub fn encode_into(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Varint(n, v) => {
                key(buf, *n, 0);
                varint(buf, *v);
            }
            Self::Fixed64(n, v) => {
                key(buf, *n, 1);
                buf.extend_from_slice(&v.to_le_bytes());
            }
            Self::Bytes(n, v) => {
                key(buf, *n, 2);
                varint(buf, v.len() as u64);
                buf.extend_from_slice(v);
            }
            Self::Fixed32(n, v) => {
                key(buf, *n, 5);
                buf.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
}

pub fn encode(fields: &[Field]) -> Vec<u8> {
    let mut buf = Vec::new();
    for field in fields {
        field.encode_into(&mut buf);
    }
    buf
}

/// Byte offsets at which a field ends (0 included): the only valid truncation points.
pub fn boundaries(fields: &[Field]) -> Vec<usize> {
    let mut buf = Vec::new();
    let mut ends = vec![0];
    for field in fields {
        field.encode_into(&mut buf);
        ends.push(buf.len());
    }
    ends
}

pub fn meta(status: i32) -> Field {
    Field::Bytes(
        RESPONSE_META,
        encode(&[
            Field::text(META_SESSION_ID, "session"),
            Field::int32(META_STATUS, status),
            Field::text(META_MESSAGE, "detail"),
        ]),
    )
}

/// A response with the given event, optional text and optional start/end times.
pub fn response(event: i32, text: Option<&str>, start: Option<i32>, end: Option<i32>) -> Vec<u8> {
    let mut fields = vec![Field::int32(EVENT, event)];
    fields.extend(text.map(|t| Field::text(TEXT, t)));
    fields.extend(start.map(|s| Field::int32(START_TIME, s)));
    fields.extend(end.map(|e| Field::int32(END_TIME, e)));
    encode(&fields)
}

pub fn failure(event: i32, status: i32) -> Vec<u8> {
    encode(&[meta(status), Field::int32(EVENT, event)])
}

pub mod events {
    pub const SESSION_STARTED: i32 = 150;
    pub const SESSION_FINISHED: i32 = 152;
    pub const SESSION_FAILED: i32 = 153;
    pub const USAGE: i32 = 154;
    pub const AUDIO_MUTED: i32 = 250;
    pub const SOURCE_START: i32 = 650;
    pub const SOURCE_RESPONSE: i32 = 651;
    pub const SOURCE_END: i32 = 652;
    pub const TRANSLATION_START: i32 = 653;
    pub const TRANSLATION_RESPONSE: i32 = 654;
    pub const TRANSLATION_END: i32 = 655;
}
