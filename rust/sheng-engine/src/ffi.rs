//! Boundary helpers: reading C strings with a size limit, returning JSON strings and keeping
//! panics from crossing the ABI.

use std::ffi::{c_char, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

use serde_json::Value;

/// Why an input string could not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputError {
    Null,
    TooLarge,
    NotUtf8,
}

/// Reads a NUL-terminated UTF-8 string of at most `limit` bytes. Never reads more than
/// `limit + 1` bytes, so an unterminated or huge input is rejected without scanning it all.
///
/// # Safety
/// `pointer` must be null or point to readable memory up to its NUL terminator or at least
/// `limit + 1` bytes.
pub unsafe fn read_str<'a>(pointer: *const c_char, limit: usize) -> Result<&'a str, InputError> {
    if pointer.is_null() {
        return Err(InputError::Null);
    }
    let bytes = pointer.cast::<u8>();
    let mut length = 0;
    // SAFETY: the caller guarantees the bytes up to the terminator (or `limit + 1`) are readable.
    while unsafe { *bytes.add(length) } != 0 {
        if length == limit {
            return Err(InputError::TooLarge);
        }
        length += 1;
    }
    // SAFETY: the `length` bytes before the terminator were just read above.
    let slice = unsafe { std::slice::from_raw_parts(bytes, length) };
    std::str::from_utf8(slice).map_err(|_| InputError::NotUtf8)
}

/// Hands a JSON value to the caller; freed with `sheng_string_free`.
pub fn json_out(value: &Value) -> *mut c_char {
    // Serialized JSON escapes NUL, so the conversion cannot fail in practice.
    CString::new(value.to_string()).map_or(std::ptr::null_mut(), CString::into_raw)
}

/// Runs `body`; a panic yields `fallback()` instead of unwinding into the caller.
pub fn guard<T>(fallback: impl FnOnce() -> T, body: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|_| fallback())
}

/// Frees a string returned by any of the JSON functions. `NULL` is ignored.
///
/// # Safety
/// `value` must be null or a pointer returned by this library that was not freed yet.
#[no_mangle]
pub unsafe extern "C" fn sheng_string_free(value: *mut c_char) {
    guard(
        || (),
        || {
            if !value.is_null() {
                // SAFETY: the pointer came from `CString::into_raw` in `json_out`.
                drop(unsafe { CString::from_raw(value) });
            }
        },
    );
}
