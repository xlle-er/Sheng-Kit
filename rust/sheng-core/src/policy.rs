//! Limits and timeouts of the translation pipeline and session shutdown
//! (translation-pipeline.md §1, §3; snapshot.md).

use std::time::Duration;

use thiserror::Error;

/// Final sentences that may wait behind the one being translated.
pub const FINAL_QUEUE_LIMIT: usize = 3;
/// Attempts per final sentence, including the first.
pub const FINAL_MAX_ATTEMPTS: u32 = 3;
/// Total time per final sentence from entering the queue, including waits and retries.
pub const FINAL_DEADLINE: Duration = Duration::from_secs(45);
/// A preview request still open after this long is abandoned.
pub const PREVIEW_TIMEOUT: Duration = Duration::from_secs(12);
/// Previews pause this long after a rate-limit response.
pub const PREVIEW_COOLDOWN_AFTER_RATE_LIMIT: Duration = Duration::from_secs(30);
/// On stop, how long recognition may take to hand over its last final.
pub const STOP_RECOGNITION_FINAL_WAIT: Duration = Duration::from_secs(1);
/// On stop, how long the final translation queue may take to drain.
pub const STOP_TRANSLATION_DRAIN: Duration = Duration::from_secs(3);
/// Overall cap for waiting on the last finals after stop.
pub const STOP_FINAL_RESULT_WAIT: Duration = Duration::from_secs(8);

/// Service families with their own end-of-input wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceKind {
    Recognition,
    Realtime,
    Gemini,
    Other,
}

/// How long to wait for a service to finish after input ends; never above the overall cap.
pub fn end_timeout(kind: ServiceKind) -> Duration {
    let timeout = match kind {
        ServiceKind::Recognition => Duration::from_secs(1),
        ServiceKind::Realtime => Duration::from_secs(2),
        ServiceKind::Gemini => Duration::from_millis(4500),
        ServiceKind::Other => Duration::from_secs(6),
    };
    timeout.min(STOP_FINAL_RESULT_WAIT)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PolicyError {
    #[error("too many final sentences are waiting for translation")]
    TranslationBacklogOverflow,
}

impl PolicyError {
    pub fn code(self) -> &'static str {
        match self {
            Self::TranslationBacklogOverflow => "translation_backlog_overflow",
        }
    }
}

/// Admits one more final behind `waiting` queued ones (the one in flight is not counted).
pub fn admit_final(waiting: usize) -> Result<(), PolicyError> {
    if waiting >= FINAL_QUEUE_LIMIT {
        return Err(PolicyError::TranslationBacklogOverflow);
    }
    Ok(())
}

/// Why a translation request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslationFailure {
    RateLimited,
    TemporarilyUnavailable,
    Authentication,
    Configuration,
}

impl TranslationFailure {
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::RateLimited | Self::TemporarilyUnavailable)
    }

    /// Session failure code once retries are exhausted, or immediately when not retryable.
    pub fn session_error_code(self) -> &'static str {
        match self {
            Self::RateLimited => "translation_rate_limited",
            Self::TemporarilyUnavailable => "translation_temporarily_unavailable",
            Self::Authentication => "credential_authentication_failed",
            Self::Configuration => "invalid_configuration",
        }
    }
}

/// Retry budget of one final sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinalBudget {
    enqueued_at: Duration,
    attempts: u32,
}

impl FinalBudget {
    pub fn new(enqueued_at: Duration) -> Self {
        Self {
            enqueued_at,
            attempts: 0,
        }
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn record_attempt(&mut self) {
        self.attempts += 1;
    }

    pub fn deadline(&self) -> Duration {
        self.enqueued_at + FINAL_DEADLINE
    }

    pub fn is_expired(&self, now: Duration) -> bool {
        now >= self.deadline()
    }

    /// True when another attempt may start after waiting `delay` from `now`.
    pub fn may_attempt(&self, now: Duration, delay: Duration) -> bool {
        self.attempts < FINAL_MAX_ATTEMPTS && now + delay < self.deadline()
    }
}
