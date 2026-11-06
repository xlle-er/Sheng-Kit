//! Retry delays for final translations and reconnects.

use std::time::Duration;

/// Delay before attempt `n` (0-based); attempts past the end are not retried.
const RETRY_DELAYS_MS: [u64; 4] = [0, 500, 1200, 2300];

/// Delay before `attempt`, or `None` when no further attempt should be made.
pub fn retry_delay(attempt: u32) -> Option<Duration> {
    let index = usize::try_from(attempt).ok()?;
    RETRY_DELAYS_MS
        .get(index)
        .map(|ms| Duration::from_millis(*ms))
}

/// Like [`retry_delay`], but waits at least as long as a service `Retry-After` hint.
pub fn retry_delay_with_hint(attempt: u32, retry_after: Option<Duration>) -> Option<Duration> {
    let delay = retry_delay(attempt)?;
    Some(retry_after.map_or(delay, |hint| hint.max(delay)))
}
