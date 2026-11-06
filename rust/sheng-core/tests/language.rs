//! Language catalog coverage (catalog.md).

use serde::Deserialize;
use sheng_core::language::{
    catalog, is_source_language, is_target_language, needs_translation, AUTO, ORIGINAL, ZH_EN,
    ZH_TW,
};

#[derive(Deserialize)]
struct RecordedCatalog {
    sources: Vec<String>,
    targets: Vec<String>,
}

fn recorded() -> RecordedCatalog {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testdata/golden/language-catalog.json"
    )))
    .expect("golden parses")
}

#[test]
fn catalog_covers_every_recorded_code() {
    let recorded = recorded();

    let current = catalog();

    for code in &recorded.sources {
        assert!(
            current.sources.contains(&code.as_str()),
            "missing source {code}"
        );
    }
    for code in &recorded.targets {
        assert!(
            current.targets.contains(&code.as_str()),
            "missing target {code}"
        );
    }
    assert_eq!(recorded.sources.len(), 137);
    assert_eq!(recorded.targets.len(), 137);
}

#[test]
fn catalog_starts_with_auto_and_original() {
    let current = catalog();

    assert_eq!(current.sources[0], AUTO);
    assert_eq!(current.targets[0], ORIGINAL);
    assert!(!is_target_language(AUTO));
    assert!(!is_source_language(ORIGINAL));
}

#[test]
fn special_chinese_codes_are_offered_both_ways() {
    let codes = [ZH_TW, ZH_EN, "wuu", "yue"];

    assert!(codes
        .iter()
        .all(|c| is_source_language(c) && is_target_language(c)));
}

#[test]
fn catalog_serializes_as_sources_and_targets_arrays() {
    let json = serde_json::to_value(catalog()).unwrap();

    assert!(json["sources"].is_array() && json["targets"].is_array());
    assert_eq!(json.as_object().unwrap().len(), 2);
}

#[test]
fn only_original_target_skips_translation() {
    assert!(!needs_translation(ORIGINAL));
    assert!(needs_translation("zh"));
}
