//! Engine handle functions (abi-v1.md “引擎句柄”, “音频”).

use std::ffi::c_char;

use serde_json::{json, Value};

use super::query::{checked_request, error_reply, ok_reply};
use crate::ffi::{guard, json_out};
use crate::registry;

const REJECTED: i32 = -1;

#[no_mangle]
pub extern "C" fn sheng_engine_create() -> u64 {
    guard(|| 0, registry::create)
}

#[no_mangle]
pub extern "C" fn sheng_engine_destroy(handle: u64) {
    guard(
        || (),
        || {
            if let Some(engine) = registry::remove(handle) {
                engine.destroy();
            }
        },
    );
}

/// # Safety
/// `request` must be null or a readable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn sheng_engine_start(handle: u64, request: *const c_char) -> *mut c_char {
    guard(
        || json_out(&error_reply("invalid_request")),
        || {
            let Some(engine) = registry::get(handle) else {
                return json_out(&error_reply("invalid_handle"));
            };
            // SAFETY: forwarded caller contract.
            let plan = match unsafe { checked_request(request) } {
                Ok(plan) => plan,
                Err(code) => return json_out(&error_reply(code)),
            };
            let reply = match engine.start(plan) {
                Ok(()) => ok_reply(),
                Err(error) => error_reply(error.code()),
            };
            json_out(&reply)
        },
    )
}

/// # Safety
/// `bytes` must be null or point to `length` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn sheng_engine_push_pcm(
    handle: u64,
    source: u32,
    bytes: *const u8,
    length: usize,
) -> i32 {
    guard(
        || REJECTED,
        || {
            if bytes.is_null() || length == 0 {
                return REJECTED;
            }
            let Some(engine) = registry::get(handle) else {
                return REJECTED;
            };
            if length > sheng_runtime::audio::router::MAX_PUSH_BYTES {
                return REJECTED;
            }
            // SAFETY: the caller guarantees `length` readable bytes; they are copied before
            // this function returns.
            let pcm = unsafe { std::slice::from_raw_parts(bytes, length) };
            engine.push(source, pcm).abi_code()
        },
    )
}

#[no_mangle]
pub extern "C" fn sheng_engine_stop(handle: u64) {
    guard(
        || (),
        || {
            if let Some(engine) = registry::get(handle) {
                engine.stop();
            }
        },
    );
}

#[no_mangle]
pub extern "C" fn sheng_engine_clear(handle: u64) {
    guard(
        || (),
        || {
            if let Some(engine) = registry::get(handle) {
                engine.clear();
            }
        },
    );
}

fn invalid_handle() -> Value {
    json!({"error": "invalid_handle"})
}

#[no_mangle]
pub extern "C" fn sheng_engine_snapshot(handle: u64) -> *mut c_char {
    guard(
        || json_out(&invalid_handle()),
        || match registry::get(handle) {
            Some(engine) => json_out(&engine.snapshot()),
            None => json_out(&invalid_handle()),
        },
    )
}

#[no_mangle]
pub extern "C" fn sheng_engine_drain_confirmed(handle: u64) -> *mut c_char {
    guard(
        || json_out(&json!([])),
        || {
            let captions = registry::get(handle)
                .map(|engine| engine.drain())
                .unwrap_or_default();
            json_out(&serde_json::to_value(captions).unwrap_or_else(|_| json!([])))
        },
    )
}
