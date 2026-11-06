//! AST v2 `TranslateRequest` / `TranslateResponse`, only the fields this client uses.
//!
//! Field numbers follow the vendor's published `.proto` definitions
//! (`ast_service.proto`, `au_base.proto`, `rpcmeta.proto`, `events.proto`).

use super::wire::{set_once, Reader, WireError, Writer};

/// Event numbers (`events.proto`).
pub mod event {
    pub const START_SESSION: i32 = 100;
    pub const FINISH_SESSION: i32 = 102;
    pub const SESSION_STARTED: i32 = 150;
    pub const SESSION_FINISHED: i32 = 152;
    pub const SESSION_FAILED: i32 = 153;
    pub const TASK_REQUEST: i32 = 200;
    pub const SOURCE_START: i32 = 650;
    pub const SOURCE_RESPONSE: i32 = 651;
    pub const SOURCE_END: i32 = 652;
    pub const TRANSLATION_START: i32 = 653;
    pub const TRANSLATION_RESPONSE: i32 = 654;
    pub const TRANSLATION_END: i32 = 655;
}

mod field {
    // TranslateRequest
    pub const REQUEST_META: u32 = 1;
    pub const EVENT: u32 = 2;
    pub const SOURCE_AUDIO: u32 = 4;
    pub const REQUEST: u32 = 6;
    // RequestMeta
    pub const SESSION_ID: u32 = 6;
    // Audio
    pub const FORMAT: u32 = 4;
    pub const CODEC: u32 = 5;
    pub const RATE: u32 = 7;
    pub const BITS: u32 = 8;
    pub const CHANNEL: u32 = 9;
    pub const BINARY_DATA: u32 = 14;
    // ReqParams
    pub const MODE: u32 = 1;
    pub const SOURCE_LANGUAGE: u32 = 2;
    pub const TARGET_LANGUAGE: u32 = 3;
    // TranslateResponse
    pub const RESPONSE_META: u32 = 1;
    pub const TEXT: u32 = 4;
    pub const START_TIME: u32 = 5;
    pub const END_TIME: u32 = 6;
    // ResponseMeta
    pub const STATUS_CODE: u32 = 3;
}

/// Declared source audio: `wav` container with `raw` codec, sent as headerless PCM16 mono.
pub const AUDIO_FORMAT: &str = "wav";
pub const AUDIO_CODEC: &str = "raw";
pub const AUDIO_RATE: i32 = 16_000;
pub const AUDIO_BITS: i32 = 16;
pub const AUDIO_CHANNELS: i32 = 1;
/// Speech-to-text mode.
pub const MODE_S2T: &str = "s2t";

fn meta(session_id: &str) -> Writer {
    Writer::new().string(field::SESSION_ID, session_id)
}

/// StartSession: speech-to-text with the declared source audio format.
pub fn start_session(session_id: &str, source_language: &str, target_language: &str) -> Vec<u8> {
    let audio = Writer::new()
        .string(field::FORMAT, AUDIO_FORMAT)
        .string(field::CODEC, AUDIO_CODEC)
        .int32(field::RATE, AUDIO_RATE)
        .int32(field::BITS, AUDIO_BITS)
        .int32(field::CHANNEL, AUDIO_CHANNELS);
    let params = Writer::new()
        .string(field::MODE, MODE_S2T)
        .string(field::SOURCE_LANGUAGE, source_language)
        .string(field::TARGET_LANGUAGE, target_language);
    Writer::new()
        .message(field::REQUEST_META, meta(session_id))
        .int32(field::EVENT, event::START_SESSION)
        .message(field::SOURCE_AUDIO, audio)
        .message(field::REQUEST, params)
        .finish()
}

/// TaskRequest carrying one audio frame.
pub fn task_request(session_id: &str, pcm: &[u8]) -> Vec<u8> {
    Writer::new()
        .message(field::REQUEST_META, meta(session_id))
        .int32(field::EVENT, event::TASK_REQUEST)
        .message(
            field::SOURCE_AUDIO,
            Writer::new().bytes(field::BINARY_DATA, pcm),
        )
        .finish()
}

pub fn finish_session(session_id: &str) -> Vec<u8> {
    Writer::new()
        .message(field::REQUEST_META, meta(session_id))
        .int32(field::EVENT, event::FINISH_SESSION)
        .finish()
}

/// Decoded `TranslateResponse`. Absent fields take proto3 defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Response {
    pub event: i32,
    pub status_code: i32,
    pub text: String,
    pub start_time: Option<i32>,
    pub end_time: Option<i32>,
}

/// Decodes one binary frame. Unknown fields are skipped; a known single-value field that
/// repeats, a wrong wire type, truncation or an oversized field is an error.
pub fn decode_response(buf: &[u8]) -> Result<Response, WireError> {
    let mut reader = Reader::new(buf)?;
    let mut status = None;
    let mut event = None;
    let mut text = None;
    let mut start = None;
    let mut end = None;
    while let Some((number, value)) = reader.next_field()? {
        match number {
            field::RESPONSE_META => set_once(&mut status, decode_status(value.bytes()?)?)?,
            field::EVENT => set_once(&mut event, value.int32()?)?,
            field::TEXT => set_once(&mut text, value.string()?.to_owned())?,
            field::START_TIME => set_once(&mut start, value.int32()?)?,
            field::END_TIME => set_once(&mut end, value.int32()?)?,
            _ => {}
        }
    }
    Ok(Response {
        event: event.unwrap_or_default(),
        status_code: status.unwrap_or_default(),
        text: text.unwrap_or_default(),
        start_time: start,
        end_time: end,
    })
}

/// `ResponseMeta.StatusCode`; other meta fields (session id, message, billing) are skipped.
fn decode_status(buf: &[u8]) -> Result<i32, WireError> {
    let mut reader = Reader::new(buf)?;
    let mut status = None;
    while let Some((number, value)) = reader.next_field()? {
        if number == field::STATUS_CODE {
            set_once(&mut status, value.int32()?)?;
        }
    }
    Ok(status.unwrap_or_default())
}
