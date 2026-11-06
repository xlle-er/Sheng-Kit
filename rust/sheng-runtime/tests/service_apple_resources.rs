//! Apple translator and resource checks (step 4a) against fakes of the system traits.

mod service_apple_support;

use std::sync::Arc;

use serde_json::json;
use service_apple_support::*;
use sheng_runtime::provider::{ProviderError, TextTranslator, TranslationRequest};
use sheng_runtime::services::apple::resources::{self, run};
use sheng_runtime::services::apple::status::TranslationStatus;
use sheng_runtime::services::apple::system::TranslateFailure;
use sheng_runtime::services::apple::AppleTranslator;

#[tokio::test]
async fn translator_maps_languages_and_failures() {
    let mut system = installed_english();
    system.translation = Some(Ok("你好".into()));
    let translator = AppleTranslator::new(Arc::new(system));
    let request = |source: &str, target: &str| TranslationRequest {
        text: "hello".into(),
        source_language: source.into(),
        target_language: target.into(),
    };
    assert_eq!(
        translator.translate(request("en", "zh")).await,
        Ok("你好".into())
    );
    assert_eq!(
        translator.translate(request("en", "ja")).await,
        Err(ProviderError::Configuration)
    );

    for (failure, error) in [
        (
            TranslateFailure::AssetsMissing,
            ProviderError::Configuration,
        ),
        (
            TranslateFailure::Failed,
            ProviderError::Unavailable { retry_after: None },
        ),
    ] {
        let mut system = installed_english();
        system.translation = Some(Err(failure));
        let translator = AppleTranslator::new(Arc::new(system));
        assert_eq!(translator.translate(request("en", "zh")).await, Err(error));
    }
}

#[tokio::test]
async fn resources_without_the_bridge_are_unavailable() {
    let expected = [
        "apple_unavailable",
        "apple_unavailable",
        "translation_preparation_failed",
        "apple_unavailable",
        "translation_unavailable",
    ];
    for (kind, code) in (0..).zip(expected) {
        assert_eq!(run(None, kind, "en", "zh").await, Err(code));
    }
}

#[tokio::test]
async fn report_lists_locales_without_downloading() {
    let (apple, system, _) = fake(installed_english(), FakeRecognizer::new(None));
    let value = run(Some(apple), resources::REPORT, "en", "zh")
        .await
        .expect("report");
    assert_eq!(value["translation"], "installed");
    assert_eq!(value["speech"]["available"], true);
    let languages = value["languages"].as_array().expect("languages");
    assert!(languages.contains(&json!({"code": "en", "locale": "en-US", "status": "installed"})));
    assert!(languages.contains(&json!({"code": "fr", "locale": "fr-FR", "status": "supported"})));
    assert!(languages.iter().all(|l| l["code"] != "auto"));
    assert!(system.downloads().is_empty());

    let (apple, _, _) = fake(installed_english(), FakeRecognizer::new(None));
    let value = run(Some(apple), resources::REPORT, "en", "original")
        .await
        .expect("report");
    assert_eq!(value["translation"], json!(null));
}

#[tokio::test]
async fn readiness_checks_never_download() {
    let mut missing = installed_english();
    missing.pair = Some(TranslationStatus::Supported);
    let cases = [
        (
            installed_english(),
            resources::SESSION_READY,
            "en",
            "zh",
            Ok(()),
        ),
        (
            installed_english(),
            resources::SESSION_READY,
            "fr",
            "original",
            Err("speech_assets_missing"),
        ),
        (
            installed_english(),
            resources::SESSION_READY,
            "de",
            "original",
            Err("language_unsupported"),
        ),
        (
            missing,
            resources::TRANSLATION_READY,
            "en",
            "zh",
            Err("translation_assets_missing"),
        ),
        (
            installed_english(),
            resources::TRANSLATION_READY,
            "en",
            "ja",
            Err("translation_unavailable"),
        ),
    ];
    for (system, kind, source, target, expected) in cases {
        let (apple, system, _) = fake(system, FakeRecognizer::new(None));
        let outcome = run(Some(apple), kind, source, target).await;
        assert_eq!(outcome.map(|_| ()), expected, "{kind} {source}>{target}");
        assert!(system.downloads().is_empty());
    }
}

#[tokio::test]
async fn prepare_downloads_only_what_is_missing() {
    let (apple_a, system, _) = fake(installed_english(), FakeRecognizer::new(None));
    run(Some(apple_a.clone()), resources::PREPARE_SPEECH, "en", "zh")
        .await
        .expect("installed");
    run(Some(apple_a), resources::PREPARE_SPEECH, "fr", "zh")
        .await
        .expect("downloads");
    assert_eq!(system.downloads(), ["speech:fr-FR"]);

    let mut supported = installed_english();
    supported.pair = Some(TranslationStatus::Supported);
    let (apple_b, system, _) = fake(supported, FakeRecognizer::new(None));
    run(Some(apple_b), resources::PREPARE_TRANSLATION, "en", "zh")
        .await
        .expect("downloads");
    assert_eq!(system.downloads(), ["translation:en-Latn-US>zh-Hans-CN"]);

    let mut failing = installed_english();
    failing.prepare_ok = false;
    let (apple_c, _, _) = fake(failing, FakeRecognizer::new(None));
    assert_eq!(
        run(Some(apple_c.clone()), resources::PREPARE_SPEECH, "fr", "zh").await,
        Err("speech_preparation_failed")
    );
    assert_eq!(
        run(Some(apple_c), resources::PREPARE_TRANSLATION, "en", "ja").await,
        Err("translation_preparation_failed")
    );
}

#[tokio::test]
async fn unknown_resource_kind_is_invalid() {
    let (apple, _, _) = fake(installed_english(), FakeRecognizer::new(None));
    assert_eq!(
        run(Some(apple), 9, "en", "zh").await,
        Err("invalid_operation")
    );
}

/// Asks the real system; run with `cargo test -p sheng-runtime --test service_apple_resources -- --ignored`.
/// Target `original` skips Translation queries: `LanguageAvailability` only answers on a running
/// main run loop, which the app has and a test process does not (docs/verification.md, step 4a).
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "queries this Mac's Speech assets"]
async fn report_on_this_mac_lists_locales() {
    let value =
        sheng_runtime::services::apple::resource(resources::REPORT, "en".into(), "original".into())
            .await
            .expect("report");
    assert!(!value["speech"]["locales"]
        .as_array()
        .expect("locales")
        .is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn bridge_selftest_passes() {
    assert_eq!(sheng_runtime::services::apple::bridge::selftest(), 0);
}
