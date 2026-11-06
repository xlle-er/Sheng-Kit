//! Stateless queries: version, catalogs, capability queries, validation and retry delays.

use std::ffi::c_char;

use serde_json::{json, Value};
use sheng_core::backoff::retry_delay;
use sheng_runtime::catalog::{language_catalog, profile_capabilities, provider_catalog};
use sheng_runtime::config::{check_request, RequestError, MAX_REQUEST_BYTES};

use crate::ffi::{guard, json_out, read_str, InputError};

const ABI_VERSION: u32 = 1;

fn to_json<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

pub(crate) fn ok_reply() -> Value {
    json!({"ok": true})
}

pub(crate) fn error_reply(code: &str) -> Value {
    json!({"ok": false, "error": code})
}

/// Code for a request string that could not be read.
pub(crate) fn input_error_code(error: InputError) -> &'static str {
    match error {
        InputError::Null => "invalid_request",
        InputError::TooLarge => RequestError::TooLarge.code(),
        InputError::NotUtf8 => RequestError::Parse.code(),
    }
}

/// Reads and checks a session request the way `validate` and `start` do.
///
/// # Safety
/// Same contract as [`read_str`].
pub(crate) unsafe fn checked_request(
    request: *const c_char,
) -> Result<sheng_runtime::config::SessionPlan, &'static str> {
    // SAFETY: forwarded caller contract.
    let text = unsafe { read_str(request, MAX_REQUEST_BYTES) }.map_err(input_error_code)?;
    check_request(text).map_err(RequestError::code)
}

#[no_mangle]
pub extern "C" fn sheng_abi_version() -> u32 {
    ABI_VERSION
}

#[no_mangle]
pub extern "C" fn sheng_language_catalog() -> *mut c_char {
    guard(
        || json_out(&json!({"sources": [], "targets": []})),
        || json_out(&to_json(language_catalog())),
    )
}

#[no_mangle]
pub extern "C" fn sheng_provider_catalog() -> *mut c_char {
    guard(
        || json_out(&json!({"presets": [], "services": []})),
        || json_out(&to_json(provider_catalog())),
    )
}

/// # Safety
/// `request` must be null or a readable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn sheng_profile_capabilities(request: *const c_char) -> *mut c_char {
    let invalid = || json_out(&to_json(profile_capabilities(None)));
    guard(invalid, || {
        // SAFETY: forwarded caller contract.
        let text = unsafe { read_str(request, MAX_REQUEST_BYTES) }.ok();
        json_out(&to_json(profile_capabilities(text)))
    })
}

/// # Safety
/// `request` must be null or a readable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn sheng_validate(request: *const c_char) -> *mut c_char {
    guard(
        || json_out(&error_reply("invalid_request")),
        || {
            // SAFETY: forwarded caller contract.
            let reply = match unsafe { checked_request(request) } {
                Ok(_) => ok_reply(),
                Err(code) => error_reply(code),
            };
            json_out(&reply)
        },
    )
}

#[no_mangle]
pub extern "C" fn sheng_retry_delay(attempt: u32) -> i64 {
    retry_delay(attempt).map_or(-1, |delay| {
        i64::try_from(delay.as_millis()).unwrap_or(i64::MAX)
    })
}
