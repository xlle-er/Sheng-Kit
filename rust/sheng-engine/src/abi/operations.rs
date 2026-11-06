//! Resource and probe operations (abi-v1.md “异步操作”, apple.md §1).

use std::ffi::c_char;

use sheng_runtime::config::{check_request, parse_request, RequestError, MAX_REQUEST_BYTES};
use sheng_runtime::services;

use crate::factory::default_factory;
use crate::ffi::{guard, json_out, read_str};
use crate::{operations, probe};

/// Longest language code accepted by `sheng_resources_start`.
const MAX_LANGUAGE_BYTES: usize = 32;
/// Resource kinds defined by apple.md: report, prepare speech, prepare translation and the
/// two readiness checks.
const APPLE_KINDS: std::ops::RangeInclusive<u32> = 0..=4;
const PROBE_SPEECH: u32 = 0;
const PROBE_TEXT: u32 = 1;

/// # Safety
/// `source` and `target` must be null or readable NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn sheng_resources_start(
    kind: u32,
    source: *const c_char,
    target: *const c_char,
) -> u64 {
    guard(
        || 0,
        || {
            // SAFETY: forwarded caller contract.
            let languages = unsafe {
                read_str(source, MAX_LANGUAGE_BYTES)
                    .and_then(|s| read_str(target, MAX_LANGUAGE_BYTES).map(|t| (s, t)))
            };
            let Ok((source, target)) = languages else {
                return 0;
            };
            if !APPLE_KINDS.contains(&kind) {
                return operations::finished(Err("invalid_operation"));
            }
            let (source, target) = (source.to_owned(), target.to_owned());
            operations::spawn(services::apple::resource(kind, source, target))
        },
    )
}

#[no_mangle]
pub extern "C" fn sheng_resources_poll(operation: u64) -> *mut c_char {
    guard(
        || json_out(&serde_json::json!({"state": "failed", "error": "invalid_operation"})),
        || json_out(&operations::poll(operation)),
    )
}

#[no_mangle]
pub extern "C" fn sheng_resources_cancel(operation: u64) {
    guard(|| (), || operations::cancel(operation));
}

/// Connection check requested by an explicit user action; the only non-session function
/// that contacts a service.
///
/// # Safety
/// `request` must be null or a readable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn sheng_probe(request: *const c_char, kind: u32) -> u64 {
    guard(
        || 0,
        || {
            // SAFETY: forwarded caller contract.
            let Ok(text) = (unsafe { read_str(request, MAX_REQUEST_BYTES) }) else {
                return 0;
            };
            if parse_request(text).is_err() {
                return 0;
            }
            let checked = check_request(text);
            let factory = default_factory();
            match (kind, checked) {
                (PROBE_SPEECH, Ok(plan)) => operations::spawn(probe::speech(factory, plan)),
                (PROBE_SPEECH, Err(_)) => 0,
                (PROBE_TEXT, Ok(plan)) => operations::spawn(probe::text(factory, plan)),
                (PROBE_TEXT, Err(error)) => operations::finished(Err(error.code())),
                _ => operations::finished(Err(RequestError::Parse.code())),
            }
        },
    )
}
