//! Capability issues agree with the recorded catalog; every language-set difference is
//! documented in docs/spec/catalog-diff.md or docs/spec/catalog-diff/*.md; catalog shapes match.

mod common;

use std::collections::BTreeSet;

use common::*;
use serde_json::Value;
use sheng_runtime::catalog::{language_catalog, profile_capabilities, provider_catalog};

/// Files that document differences: `catalog-diff.md` plus every `catalog-diff/*.md`.
fn diff_files() -> Vec<std::path::PathBuf> {
    let spec = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata");
    let mut files = vec![spec.join("catalog-diff.md")];
    let mut extra: Vec<_> = std::fs::read_dir(spec.join("catalog-diff"))
        .expect("catalog-diff/ exists")
        .map(|entry| entry.expect("catalog-diff/ entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    extra.sort();
    files.extend(extra);
    files
}

/// `(scope, list, code, change)` rows of the documented differences in one file.
fn rows(text: &str) -> Vec<(String, String, String, String)> {
    text.lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line
                .trim()
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect();
            let [scope, list, code, change, _basis] = cells.as_slice() else {
                return None;
            };
            let code = code.trim_matches('`');
            matches!(*change, "新增" | "删除").then(|| {
                (
                    scope.trim_matches('`').into(),
                    list.to_string(),
                    code.to_owned(),
                    change.to_string(),
                )
            })
        })
        .collect()
}

/// `(scope, list, code, change)` rows of all documented differences.
fn documented() -> BTreeSet<(String, String, String, String)> {
    diff_files()
        .iter()
        .flat_map(|path| {
            let text = std::fs::read_to_string(path).expect("catalog difference file is readable");
            rows(&text)
        })
        .collect()
}

fn codes(value: &Value) -> BTreeSet<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_owned())
        .collect()
}

/// Scopes a difference may be attributed to for one case.
fn scopes(provider: &str, text: &str) -> Vec<String> {
    let mut scopes = vec![provider.to_owned(), text.to_owned()];
    if provider == "alibabaCloud" && text == "followService" {
        scopes.push("qwenMt".into());
    }
    scopes
}

#[test]
fn capability_issues_match_and_language_differences_are_documented() {
    let recorded = golden("profile-capabilities.json");
    let documented = documented();
    let mut issue_mismatches = Vec::new();
    let mut undocumented = BTreeSet::new();

    for case in recorded["cases"].as_array().unwrap() {
        let (provider, mode, text) = (
            str_field(case, "provider"),
            str_field(case, "speechMode"),
            str_field(case, "text"),
        );
        let (source, target) = (str_field(case, "source"), str_field(case, "target"));
        let expected = &recorded["results"][str_field(case, "result")];
        let request = capability_request(provider, mode, text, source, target).to_string();

        let actual = serde_json::to_value(profile_capabilities(Some(&request))).unwrap();

        if actual["issues"] != expected["issues"] {
            issue_mismatches.push(format!(
                "{case}: {} vs {}",
                actual["issues"], expected["issues"]
            ));
        }
        for list in ["sources", "targets"] {
            let (new, old) = (codes(&actual[list]), codes(&expected[list]));
            let changes = new
                .difference(&old)
                .map(|c| (c, "新增"))
                .chain(old.difference(&new).map(|c| (c, "删除")));
            for (code, change) in changes {
                let covered = scopes(provider, text).into_iter().any(|scope| {
                    documented.contains(&(scope, list.to_owned(), code.clone(), change.to_owned()))
                });
                if !covered {
                    undocumented.insert(format!(
                        "| {provider}/{text} | {list} | `{code}` | {change} |"
                    ));
                }
            }
        }
    }

    assert!(
        issue_mismatches.is_empty(),
        "{}",
        issue_mismatches.join("\n")
    );
    assert!(
        undocumented.is_empty(),
        "undocumented:\n{}",
        undocumented.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn asr_mt_profile_acceptance_matches_recorded_results() {
    let recorded = golden("profile-speech-mode-acceptance.json");

    for case in recorded.as_array().unwrap() {
        let (provider, mode) = (
            str_field(case, "provider"),
            str_field(case, "speechTranslationMode"),
        );
        let request =
            raw_capability_request(provider, mode, "simultaneous", "-", "auto", "original")
                .to_string();

        let answer = profile_capabilities(Some(&request));

        let accepted = !answer.issues.iter().any(|issue| issue.field == "profile");
        assert_eq!(
            accepted,
            case["profileAccepted"].as_bool().unwrap(),
            "{case}"
        );
    }
}

#[test]
fn malformed_capability_queries_report_an_invalid_profile() {
    let recorded = golden("abi-edge-cases.json");
    let unknown_field = r#"{"profile":{},"source":"en","target":"zh","extra":1}"#;

    for input in [None, Some("{"), Some(unknown_field)] {
        let answer = serde_json::to_value(profile_capabilities(input)).unwrap();

        assert_eq!(answer, recorded["capabilitiesInvalid"]);
    }
}

#[test]
fn language_catalog_covers_every_recorded_code() {
    let recorded = golden("language-catalog.json");

    let current = serde_json::to_value(language_catalog()).unwrap();

    for list in ["sources", "targets"] {
        let missing: Vec<String> = codes(&recorded[list])
            .difference(&codes(&current[list]))
            .cloned()
            .collect();
        assert!(missing.is_empty(), "{list} missing {missing:?}");
    }
}

#[test]
fn provider_catalog_has_the_recorded_shape() {
    let recorded = golden("provider-catalog-shape.json");
    let catalog = serde_json::to_value(provider_catalog()).unwrap();
    let allowed_preset_keys = codes(&recorded["presets"]["keys"]);
    let service_keys = codes(&recorded["services"]["keys"]);

    let top: BTreeSet<String> = catalog.as_object().unwrap().keys().cloned().collect();

    assert_eq!(top, codes(&recorded["topLevelKeys"]));
    for preset in catalog["presets"].as_array().unwrap() {
        let keys: BTreeSet<String> = preset.as_object().unwrap().keys().cloned().collect();
        assert!(keys.is_subset(&allowed_preset_keys), "{keys:?}");
        for required in ["id", "label", "apiKeyUrl", "guideUrl", "endpoint", "model"] {
            assert!(keys.contains(required), "preset missing {required}");
        }
    }
    for service in catalog["services"].as_array().unwrap() {
        let keys: BTreeSet<String> = service.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, service_keys);
        assert!(service["apiKeyUrl"]
            .as_str()
            .unwrap()
            .starts_with("https://"));
    }
    assert!(!catalog["presets"].as_array().unwrap().is_empty());
}
