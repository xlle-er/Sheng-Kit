//! Request rules of Alibaba live translation (`alibabaRealtimeMode:"liveTranslate"`,
//! start-request.md “路由”). Language sets are checked by the shared language step.

use super::request::Configuration;
use super::validate::ValidationError;

/// The service translates by itself: no text translator, a real target, and a workspace id
/// that can form a host name.
pub fn check(configuration: &Configuration) -> Result<(), ValidationError> {
    if configuration.text_credentials.is_some() {
        return Err(ValidationError::UnexpectedTextCredentials);
    }
    if configuration.target_language == sheng_core::language::ORIGINAL {
        return Err(ValidationError::MissingTarget);
    }
    if !is_workspace_id(&configuration.alibaba_workspace_id) {
        return Err(ValidationError::WorkspaceId);
    }
    Ok(())
}

/// Empty, or only ASCII letters, digits and `-`.
fn is_workspace_id(value: &str) -> bool {
    value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use crate::config::route::{SpeechRoute, TextRoute};
    use crate::config::{check_request, RequestError, TextPlan, ValidationError};

    fn request(source: &str, target: &str, workspace: &str) -> Value {
        json!({
            "input": "system",
            "configuration": {
                "provider": "alibabaCloud",
                "qwenMtModel": "lite",
                "credentials": {"kind": "apiKey", "apiKey": "test-key"},
                "sourceLanguage": source,
                "targetLanguage": target,
                "translationMode": "turbo",
                "networkProxy": {"mode": "system"},
                "textNetworkProxy": {"mode": "system"},
                "speechTranslationMode": "simultaneous",
                "xaiSpeechMode": "voice",
                "alibabaRealtimeMode": "liveTranslate",
                "alibabaWorkspaceId": workspace,
            }
        })
    }

    fn rejection(request: &Value) -> ValidationError {
        match check_request(&request.to_string()) {
            Err(RequestError::Invalid(error)) => error,
            other => panic!("expected configuration_invalid, got {other:?}"),
        }
    }

    #[test]
    fn live_translation_without_text_credentials_is_accepted() {
        let plan = check_request(&request("auto", "en", "").to_string()).unwrap();

        assert_eq!(plan.route, SpeechRoute::AlibabaLiveTranslate);
        assert_eq!(plan.text_route, TextRoute::None);
        assert!(matches!(plan.text, TextPlan::None));
        assert_eq!(plan.sample_rate, 16_000);
        assert_eq!(plan.alibaba_workspace_id, "");
    }

    #[test]
    fn live_translation_accepts_a_workspace_id_of_letters_digits_and_dashes() {
        let plan = check_request(&request("auto", "zh", "ws-Abc-123").to_string()).unwrap();

        assert_eq!(plan.alibaba_workspace_id, "ws-Abc-123");
    }

    #[test]
    fn live_translation_rejects_text_credentials() {
        let mut with_text = request("auto", "en", "");
        with_text["configuration"]["textCredentials"] = json!({"kind": "apple"});

        assert_eq!(
            rejection(&with_text),
            ValidationError::UnexpectedTextCredentials
        );
    }

    #[test]
    fn live_translation_rejects_the_original_target() {
        assert_eq!(
            rejection(&request("auto", "original", "")),
            ValidationError::MissingTarget
        );
    }

    #[test]
    fn live_translation_rejects_languages_outside_its_set() {
        assert_eq!(
            rejection(&request("zh", "en", "")),
            ValidationError::Language
        );
        assert_eq!(
            rejection(&request("auto", "sq", "")),
            ValidationError::Language
        );
    }

    #[test]
    fn live_translation_rejects_workspace_ids_with_other_characters() {
        for workspace in ["ws.example", "ws_1", "ws 1", " ws-1", "ws/1", "工作区"] {
            assert_eq!(
                rejection(&request("auto", "en", workspace)),
                ValidationError::WorkspaceId,
                "workspace {workspace:?}"
            );
        }
    }

    #[test]
    fn recognition_mode_ignores_the_workspace_id() {
        let mut recognition = request("zh", "en", "ws.example");
        recognition["configuration"]["alibabaRealtimeMode"] = json!("disabled");

        let plan = check_request(&recognition.to_string()).unwrap();

        assert_eq!(plan.route, SpeechRoute::AlibabaAsr);
    }
}
