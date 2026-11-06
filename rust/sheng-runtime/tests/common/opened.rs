//! Recorded validation results that changed when Alibaba live translation was opened
//! (step5-alibaba-live brief; questions/step5-alibaba-live.md lists each case).

use serde_json::Value;

use super::str_field;

/// `(provider, speechMode, text, source, target)` of a matrix case.
type CaseKey = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

/// Matrix case → result code after the change.
const OPENED_LIVE_TRANSLATION: &[(CaseKey, &str)] =
    &[(("alibabaCloud", "simultaneous", "-", "auto", "zh"), "ok")];

/// Result code a validation matrix case has now: the recorded one unless listed above.
pub fn expected_matrix_code(case: &Value) -> String {
    let key = (
        str_field(case, "provider"),
        str_field(case, "speechMode"),
        str_field(case, "text"),
        str_field(case, "source"),
        str_field(case, "target"),
    );
    match OPENED_LIVE_TRANSLATION.iter().find(|(k, _)| *k == key) {
        Some((_, code)) => (*code).to_owned(),
        None => recorded_code(&case["result"]),
    }
}

/// Result code as recorded: `ok` or the error code.
pub fn recorded_code(result: &Value) -> String {
    match result["ok"].as_bool() {
        Some(true) => "ok".to_owned(),
        _ => str_field(result, "error").to_owned(),
    }
}
