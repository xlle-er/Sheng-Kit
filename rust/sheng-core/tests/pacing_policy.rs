//! Preview pacing, request budget, retry delays and translation limits
//! (translation-pipeline.md §1–§3).

use std::time::Duration;

use serde::Deserialize;
use sheng_core::backoff::{retry_delay, retry_delay_with_hint};
use sheng_core::pacing::{
    preview_interval, request_cost, PreviewPacer, RequestBudget, TranslationPacer,
};
use sheng_core::policy::{
    admit_final, end_timeout, FinalBudget, PolicyError, ServiceKind, TranslationFailure,
    FINAL_DEADLINE, FINAL_MAX_ATTEMPTS, FINAL_QUEUE_LIMIT, PREVIEW_TIMEOUT, STOP_FINAL_RESULT_WAIT,
};

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetryRecord {
    attempt: u32,
    delay_ms: i64,
}

#[test]
fn retry_delays_match_recorded_schedule_and_then_stop() {
    let records: Vec<RetryRecord> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testdata/golden/retry-delay.json"
    )))
    .expect("golden parses");
    let tolerance_ms = 50;

    for record in &records {
        let delay = retry_delay(record.attempt);

        match u64::try_from(record.delay_ms) {
            Ok(expected) => {
                let actual = delay.expect("attempt is retried").as_millis() as u64;
                assert!(
                    actual.abs_diff(expected) <= tolerance_ms,
                    "attempt {}: {actual} ms vs {expected} ms",
                    record.attempt
                );
            }
            Err(_) => assert_eq!(delay, None, "attempt {} must stop", record.attempt),
        }
    }
}

#[test]
fn retry_delay_values_are_zero_500_1200_2300() {
    let delays: Vec<Option<Duration>> = (0..5).map(retry_delay).collect();

    assert_eq!(
        delays,
        [
            Some(ms(0)),
            Some(ms(500)),
            Some(ms(1200)),
            Some(ms(2300)),
            None
        ]
    );
}

#[test]
fn retry_after_hint_extends_but_never_shortens_the_delay() {
    let longer = retry_delay_with_hint(1, Some(ms(4_000)));
    let shorter = retry_delay_with_hint(2, Some(ms(100)));
    let exhausted = retry_delay_with_hint(9, Some(ms(100)));

    assert_eq!(
        (longer, shorter, exhausted),
        (Some(ms(4_000)), Some(ms(1_200)), None)
    );
}

#[test]
fn request_cost_counts_fixed_overhead_non_ascii_and_quarter_ascii() {
    let costs = [
        request_cost(""),
        request_cost("hello"),
        request_cost("你好"),
        request_cost("ab你"),
    ];

    assert_eq!(costs, [512, 514, 514, 514]);
}

#[test]
fn preview_interval_grows_with_length_between_bounds() {
    let intervals = [
        preview_interval(0, false),
        preview_interval(100, false),
        preview_interval(10_000, false),
        preview_interval(10, true),
        preview_interval(10_000, true),
    ];

    assert_eq!(
        intervals,
        [ms(1_500), ms(4_000), ms(8_000), ms(5_000), ms(8_000)]
    );
}

#[test]
fn preview_pacer_spaces_requests_and_treats_small_changes_as_revisions() {
    let mut pacer = PreviewPacer::default();
    let first = pacer.earliest(20, false);

    pacer.record_sent(ms(10_000), 20);

    assert_eq!(first, Duration::ZERO);
    assert_eq!(pacer.earliest(20, false), ms(12_000));
    assert_eq!(pacer.earliest(22, true), ms(15_000));
    assert_eq!(pacer.earliest(60, true), ms(13_000));
}

#[test]
fn rate_limit_pauses_previews_for_thirty_seconds() {
    let mut pacer = PreviewPacer::default();

    pacer.record_rate_limited(ms(11_000));

    assert_eq!(pacer.earliest(5, false), ms(41_000));
}

#[test]
fn request_budget_enforces_spacing_and_work_rate() {
    let mut small = RequestBudget::default();
    let mut large = RequestBudget::default();

    small.record_start(Duration::ZERO, 495);
    large.record_start(Duration::ZERO, 4_500);

    assert_eq!(small.earliest(), ms(1_100));
    assert_eq!(large.earliest(), ms(10_000));
}

#[test]
fn finals_take_priority_over_previews() {
    let mut pacer = TranslationPacer::default();
    pacer.record_final(Duration::ZERO, 512);
    let later = ms(20_000);

    let blocked_by_final = pacer.may_send_preview(later, 10, false, true);
    let allowed = pacer.may_send_preview(later, 10, false, false);
    let too_soon = pacer.may_send_final(ms(500));

    assert!(!blocked_by_final && allowed && !too_soon);
    assert!(pacer.may_send_final(later));
}

#[test]
fn final_queue_admits_three_waiting_sentences() {
    let results: Vec<Result<(), PolicyError>> = (0..=FINAL_QUEUE_LIMIT).map(admit_final).collect();

    assert_eq!(FINAL_QUEUE_LIMIT, 3);
    assert_eq!(results[..3], [Ok(()), Ok(()), Ok(())]);
    assert_eq!(results[3], Err(PolicyError::TranslationBacklogOverflow));
    assert_eq!(
        PolicyError::TranslationBacklogOverflow.code(),
        "translation_backlog_overflow"
    );
}

#[test]
fn final_budget_allows_three_attempts_within_forty_five_seconds() {
    let mut budget = FinalBudget::new(ms(1_000));
    for _ in 0..FINAL_MAX_ATTEMPTS {
        assert!(budget.may_attempt(ms(2_000), Duration::ZERO));
        budget.record_attempt();
    }

    let after_three = budget.may_attempt(ms(2_000), Duration::ZERO);
    let fresh = FinalBudget::new(ms(1_000));

    assert!(!after_three);
    assert_eq!(FINAL_DEADLINE, Duration::from_secs(45));
    assert!(!fresh.may_attempt(ms(45_000), ms(1_200)));
    assert!(fresh.is_expired(ms(46_000)));
    assert!(!fresh.is_expired(ms(45_999)));
}

#[test]
fn translation_failures_map_to_session_codes_and_retry_rules() {
    let cases = [
        (
            TranslationFailure::RateLimited,
            true,
            "translation_rate_limited",
        ),
        (
            TranslationFailure::TemporarilyUnavailable,
            true,
            "translation_temporarily_unavailable",
        ),
        (
            TranslationFailure::Authentication,
            false,
            "credential_authentication_failed",
        ),
        (
            TranslationFailure::Configuration,
            false,
            "invalid_configuration",
        ),
    ];

    for (failure, retryable, code) in cases {
        assert_eq!(failure.is_retryable(), retryable);
        assert_eq!(failure.session_error_code(), code);
    }
}

#[test]
fn end_timeouts_per_service_stay_within_the_overall_cap() {
    let timeouts = [
        end_timeout(ServiceKind::Recognition),
        end_timeout(ServiceKind::Realtime),
        end_timeout(ServiceKind::Gemini),
        end_timeout(ServiceKind::Other),
    ];

    assert_eq!(timeouts, [ms(1_000), ms(2_000), ms(4_500), ms(6_000)]);
    assert!(timeouts.iter().all(|t| *t <= STOP_FINAL_RESULT_WAIT));
    assert_eq!(PREVIEW_TIMEOUT, ms(12_000));
}
