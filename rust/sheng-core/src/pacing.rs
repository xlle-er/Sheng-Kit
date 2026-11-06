//! Preview throttling and the shared translation request budget (translation-pipeline.md §2).
//!
//! Only lengths and times are kept; no text is stored. Times are durations since any fixed
//! origin chosen by the caller.

use std::time::Duration;

use crate::policy::PREVIEW_COOLDOWN_AFTER_RATE_LIMIT;

pub const PREVIEW_MIN_INTERVAL: Duration = Duration::from_millis(1500);
pub const PREVIEW_MAX_INTERVAL: Duration = Duration::from_millis(8000);
/// Extra preview interval per character of the draft.
pub const PREVIEW_INTERVAL_PER_CHAR: Duration = Duration::from_millis(25);
/// Minimum interval when the same sentence only changed slightly.
pub const PREVIEW_REVISION_INTERVAL: Duration = Duration::from_millis(5000);
/// A same-utterance draft whose length changed by at most this many characters is a revision.
pub const REVISION_MAX_LENGTH_CHANGE: usize = 8;

pub const REQUEST_MIN_SPACING: Duration = Duration::from_millis(1100);
pub const REQUEST_FIXED_COST: u64 = 512;
pub const REQUEST_UNITS_PER_SECOND: u64 = 450;
const ASCII_CHARS_PER_UNIT: u64 = 4;

/// Work units of one request: fixed overhead, one per non-ASCII character, one per 4 ASCII.
pub fn request_cost(text: &str) -> u64 {
    let (ascii, other) = text.chars().fold((0u64, 0u64), |(a, o), c| {
        if c.is_ascii() {
            (a + 1, o)
        } else {
            (a, o + 1)
        }
    });
    REQUEST_FIXED_COST + other + ascii.div_ceil(ASCII_CHARS_PER_UNIT)
}

/// Spacing for a preview of `chars` characters.
pub fn preview_interval(chars: usize, is_revision: bool) -> Duration {
    let per_char =
        PREVIEW_INTERVAL_PER_CHAR.saturating_mul(u32::try_from(chars).unwrap_or(u32::MAX));
    let interval = PREVIEW_MIN_INTERVAL
        .saturating_add(per_char)
        .min(PREVIEW_MAX_INTERVAL);
    if is_revision {
        interval.max(PREVIEW_REVISION_INTERVAL)
    } else {
        interval
    }
}

#[derive(Debug, Clone, Default)]
pub struct PreviewPacer {
    last_sent_at: Option<Duration>,
    last_chars: usize,
    paused_until: Option<Duration>,
}

impl PreviewPacer {
    /// Earliest time a preview of `chars` characters may be sent.
    pub fn earliest(&self, chars: usize, same_utterance: bool) -> Duration {
        let paced = self.last_sent_at.map_or(Duration::ZERO, |last| {
            let is_revision =
                same_utterance && self.last_chars.abs_diff(chars) <= REVISION_MAX_LENGTH_CHANGE;
            last + preview_interval(chars, is_revision)
        });
        self.paused_until.map_or(paced, |pause| paced.max(pause))
    }

    pub fn record_sent(&mut self, now: Duration, chars: usize) {
        self.last_sent_at = Some(now);
        self.last_chars = chars;
    }

    pub fn record_rate_limited(&mut self, now: Duration) {
        self.paused_until = Some(now + PREVIEW_COOLDOWN_AFTER_RATE_LIMIT);
    }
}

/// Spacing and work-rate budget shared by preview and final requests.
#[derive(Debug, Clone, Default)]
pub struct RequestBudget {
    last_start: Option<Duration>,
    free_at: Duration,
}

impl RequestBudget {
    pub fn earliest(&self) -> Duration {
        let spaced = self
            .last_start
            .map_or(Duration::ZERO, |last| last + REQUEST_MIN_SPACING);
        spaced.max(self.free_at)
    }

    pub fn record_start(&mut self, now: Duration, cost: u64) {
        let work = Duration::from_micros(cost.saturating_mul(1_000_000) / REQUEST_UNITS_PER_SECOND);
        self.last_start = Some(now);
        self.free_at = self.free_at.max(now) + work;
    }
}

/// Combines preview pacing with the shared budget; finals always go first.
#[derive(Debug, Clone, Default)]
pub struct TranslationPacer {
    preview: PreviewPacer,
    budget: RequestBudget,
}

impl TranslationPacer {
    pub fn may_send_final(&self, now: Duration) -> bool {
        now >= self.budget.earliest()
    }

    /// A preview waits while any final is waiting, then for both its own pacing and the budget.
    pub fn may_send_preview(
        &self,
        now: Duration,
        chars: usize,
        same_utterance: bool,
        is_final_waiting: bool,
    ) -> bool {
        !is_final_waiting
            && now >= self.budget.earliest()
            && now >= self.preview.earliest(chars, same_utterance)
    }

    pub fn record_final(&mut self, now: Duration, cost: u64) {
        self.budget.record_start(now, cost);
    }

    pub fn record_preview(&mut self, now: Duration, chars: usize, cost: u64) {
        self.budget.record_start(now, cost);
        self.preview.record_sent(now, chars);
    }

    pub fn record_rate_limited(&mut self, now: Duration) {
        self.preview.record_rate_limited(now);
    }
}
