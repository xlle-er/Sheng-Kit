//! Validation result codes agree with the recorded matrix and negative cases.

mod common;

use common::negative::negative_case;
use common::opened::{expected_matrix_code, recorded_code};
use common::*;
use sheng_runtime::config::check_request;

fn code_of(request: &str) -> String {
    match check_request(request) {
        Ok(_) => "ok".to_owned(),
        Err(error) => error.code().to_owned(),
    }
}

#[test]
fn validation_matrix_matches_recorded_results() {
    let matrix = golden("validate-matrix.json");
    let mut mismatches = Vec::new();

    for case in matrix.as_array().unwrap() {
        let request = start_request(
            str_field(case, "input"),
            str_field(case, "provider"),
            str_field(case, "speechMode"),
            str_field(case, "text"),
            str_field(case, "source"),
            str_field(case, "target"),
        );
        let actual = code_of(&request.to_string());
        if actual != expected_matrix_code(case) {
            mismatches.push(format!("{case} -> {actual}"));
        }
    }

    assert_eq!(matrix.as_array().unwrap().len(), 528);
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn negative_cases_match_recorded_results() {
    let cases = golden("validate-negative.json");

    for case in cases.as_array().unwrap() {
        let name = str_field(case, "case");

        let actual = code_of(&negative_case(name));

        assert_eq!(actual, recorded_code(&case["result"]), "case {name}");
    }
}
