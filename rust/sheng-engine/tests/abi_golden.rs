//! The exported functions against the recorded golden answers (abi-v1.md).

mod common;

use std::ffi::c_char;
use std::ptr;

use common::requests::negative::negative_case;
use common::requests::{capability_request, golden, start_request, str_field};
use common::*;
use serde_json::{json, Value};
use sheng_engine::abi::engine::*;
use sheng_engine::abi::operations::*;
use sheng_engine::abi::query::*;
use sheng_runtime::catalog::profile_capabilities;

fn validate(request: &str) -> Value {
    let request = c(request);
    // SAFETY: a valid NUL-terminated string.
    take_json(unsafe { sheng_validate(request.as_ptr()) })
}

fn capabilities(request: *const c_char) -> Value {
    // SAFETY: null or a valid NUL-terminated string.
    take_json(unsafe { sheng_profile_capabilities(request) })
}

fn without_run_id(mut snapshot: Value) -> Value {
    snapshot["runId"] = json!("<uuid>");
    snapshot
}

#[test]
fn abi_version_matches_the_recording() {
    assert_eq!(json!(sheng_abi_version()), golden("abi-version.json"));
}

#[test]
fn edge_cases_match_the_recording() {
    let recorded = golden("abi-edge-cases.json");
    let handle = sheng_engine_create();
    let unknown = u64::MAX;
    let not_json = c("{not json");
    let pcm = [0u8; 4];

    assert_eq!(without_run_id(snapshot(handle)), recorded["freshSnapshot"]);
    assert_eq!(json!(drain(handle)), recorded["freshDrain"]);
    assert_eq!(json!(drain(unknown)), recorded["drainInvalidHandle"]);
    assert_eq!(snapshot(unknown), recorded["snapshotInvalidHandle"]);
    assert_eq!(start(unknown, &json!({})), recorded["startInvalidHandle"]);
    // SAFETY: null and valid strings only.
    unsafe {
        assert_eq!(
            take_json(sheng_engine_start(handle, ptr::null())),
            recorded["startNullRequest"]
        );
        assert_eq!(
            take_json(sheng_engine_start(handle, not_json.as_ptr())),
            recorded["startInvalidJson"]
        );
        assert_eq!(
            json!(sheng_engine_push_pcm(handle, 0, ptr::null(), 4)),
            recorded["pushNull"]
        );
        assert_eq!(json!(sheng_probe(ptr::null(), 0)), recorded["probeNull"]);
        assert_eq!(
            json!(sheng_probe(not_json.as_ptr(), 0)),
            recorded["probeInvalidJson"]
        );
        assert_eq!(
            json!(sheng_resources_start(0, ptr::null(), ptr::null())),
            recorded["resourcesNullLanguage"]
        );
    }
    assert_eq!(json!(push(handle, 0, &pcm[..3])), recorded["pushOddLength"]);
    assert_eq!(json!(push(handle, 2, &pcm)), recorded["pushInvalidSource"]);
    assert_eq!(json!(push(handle, 0, &pcm)), recorded["pushNotRunning"]);
    assert_eq!(
        json!(push(handle, 0, &vec![0u8; 65_538])),
        recorded["pushOversize"]
    );
    assert_eq!(capabilities(ptr::null()), recorded["capabilitiesNull"]);
    assert_eq!(
        capabilities(not_json.as_ptr()),
        recorded["capabilitiesInvalid"]
    );
    assert_eq!(
        take_json(sheng_resources_poll(u64::MAX)),
        recorded["pollInvalidOperation"]
    );
    let language = c("en");
    // SAFETY: valid strings.
    let unknown_kind = unsafe { sheng_resources_start(99, language.as_ptr(), language.as_ptr()) };
    assert_eq!(
        take_json(sheng_resources_poll(unknown_kind)),
        recorded["resourcesUnknownKind"]
    );
    sheng_resources_cancel(unknown_kind);
    assert_eq!(
        take_json(sheng_resources_poll(unknown_kind)),
        recorded["pollAfterCancel"]
    );
    sheng_engine_destroy(handle);
}

#[test]
fn validation_matrix_and_negative_cases_match_through_the_abi() {
    use common::requests::opened::{expected_matrix_code, recorded_code};
    let actual = |reply: Value| recorded_code(&reply);
    let matrix = golden("validate-matrix.json");
    for case in matrix.as_array().unwrap() {
        let request = start_request(
            str_field(case, "input"),
            str_field(case, "provider"),
            str_field(case, "speechMode"),
            str_field(case, "text"),
            str_field(case, "source"),
            str_field(case, "target"),
        );
        assert_eq!(
            actual(validate(&request.to_string())),
            expected_matrix_code(case),
            "{case}"
        );
    }
    for case in golden("validate-negative.json").as_array().unwrap() {
        let name = str_field(case, "case");
        assert_eq!(
            actual(validate(&negative_case(name))),
            recorded_code(&case["result"]),
            "{name}"
        );
    }
}

#[test]
fn capability_queries_match_the_recording_through_the_abi() {
    let recorded = golden("profile-capabilities.json");
    for case in recorded["cases"].as_array().unwrap() {
        let request = capability_request(
            str_field(case, "provider"),
            str_field(case, "speechMode"),
            str_field(case, "text"),
            str_field(case, "source"),
            str_field(case, "target"),
        )
        .to_string();
        let expected = &recorded["results"][str_field(case, "result")];
        let text = c(&request);

        let answer = capabilities(text.as_ptr());

        assert_eq!(answer["issues"], expected["issues"], "{case}");
        // Language differences are checked against catalog-diff.md by sheng-runtime.
        let direct = serde_json::to_value(profile_capabilities(Some(&request))).unwrap();
        assert_eq!(answer, direct);
    }
}

#[test]
fn retry_delay_increases_is_bounded_and_stops_at_the_fourth_attempt() {
    let delays: Vec<i64> = (0..12).map(|attempt| sheng_retry_delay(attempt)).collect();

    assert_eq!(delays[0], 0);
    assert!(delays[..4].windows(2).all(|pair| pair[0] < pair[1]));
    assert!(delays[..4].iter().all(|delay| *delay <= 5_000));
    assert!(delays[4..].iter().all(|delay| *delay == -1));
    assert_eq!(sheng_retry_delay(u32::MAX), -1);
}

#[test]
fn catalogs_have_the_documented_shape() {
    let languages = take_json(sheng_language_catalog());
    let providers = take_json(sheng_provider_catalog());

    assert_eq!(languages["sources"][0], "auto");
    assert_eq!(languages["targets"][0], "original");
    assert!(!providers["presets"].as_array().unwrap().is_empty());
    assert!(!providers.to_string().contains("apiKey\":\""));
}

#[test]
fn apple_resource_operations_validate_their_input() {
    // Kinds 0–4 reach the real Apple bridge (and 1–2 would download), so only input
    // validation is checked here; the kinds are covered by sheng-runtime's service_apple tests.
    let (source, target) = (c("en"), c("zh"));
    // SAFETY: valid strings.
    let unknown = unsafe { sheng_resources_start(99, source.as_ptr(), target.as_ptr()) };
    assert_ne!(unknown, 0);
    assert_eq!(
        take_json(sheng_resources_poll(unknown)),
        json!({"state": "failed", "error": "invalid_operation"})
    );
    sheng_resources_cancel(unknown);
    let long = c(&"x".repeat(33));
    // SAFETY: valid strings or null.
    unsafe {
        assert_eq!(sheng_resources_start(1, long.as_ptr(), target.as_ptr()), 0);
        assert_eq!(sheng_resources_start(2, source.as_ptr(), ptr::null()), 0);
    }
}

#[test]
fn oversized_and_non_utf8_requests_are_rejected() {
    let large = c(&"x".repeat(70_000));
    let invalid = [0xffu8, 0xfe, 0];
    let handle = sheng_engine_create();
    // SAFETY: valid NUL-terminated byte strings.
    unsafe {
        assert_eq!(
            take_json(sheng_engine_start(handle, large.as_ptr()))["error"],
            "request_too_large"
        );
        assert_eq!(
            take_json(sheng_validate(invalid.as_ptr().cast()))["error"],
            "invalid_configuration"
        );
    }
    let invalid_request = json!({"input": "system", "configuration": {}});
    assert_eq!(
        start(handle, &invalid_request)["error"],
        "invalid_configuration"
    );
    sheng_engine_destroy(handle);
}
