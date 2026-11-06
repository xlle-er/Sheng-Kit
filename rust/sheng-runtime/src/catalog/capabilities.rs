//! `sheng_profile_capabilities` (catalog.md): metadata only, no network or credentials.

use serde::{Deserialize, Serialize};

use super::routes::{language_issues, language_options, Issue, LanguageOptions};
use crate::config::request::{
    AlibabaRealtimeMode, Provider, QwenMtModel, SpeechTranslationMode, TextKind, XaiSpeechMode,
};
use crate::config::route::{speech_route, text_route, RouteModes};
use crate::config::MAX_REQUEST_BYTES;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilityRequest {
    profile: Profile,
    source: String,
    target: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Profile {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    name: String,
    provider: Provider,
    qwen_mt_model: QwenMtModel,
    speech_translation_mode: SpeechTranslationMode,
    xai_speech_mode: XaiSpeechMode,
    alibaba_realtime_mode: AlibabaRealtimeMode,
    #[serde(default)]
    text_translation: Option<TextChoice>,
    #[serde(default)]
    custom_speech_source_languages: Option<Vec<String>>,
}

/// `textTranslation` in a profile: a translator kind, or follow the speech service.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(untagged)]
enum TextChoice {
    Kind(TextKind),
    Follow(FollowService),
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum FollowService {
    #[serde(rename = "followService")]
    FollowService,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    pub sources: Vec<&'static str>,
    pub targets: Vec<&'static str>,
    pub modes: Vec<&'static str>,
    pub issues: Vec<Issue>,
}

impl Capabilities {
    fn invalid() -> Self {
        Self {
            sources: Vec::new(),
            targets: Vec::new(),
            modes: Vec::new(),
            issues: vec![Issue::invalid_profile()],
        }
    }
}

/// Answers a capability query; malformed input yields the documented invalid-profile answer.
pub fn profile_capabilities(json: Option<&str>) -> Capabilities {
    let parsed = json
        .filter(|j| j.len() <= MAX_REQUEST_BYTES)
        .and_then(|j| serde_json::from_str::<CapabilityRequest>(j).ok());
    let Some(request) = parsed else {
        return Capabilities::invalid();
    };
    let profile = &request.profile;
    let modes = RouteModes {
        speech_translation: profile.speech_translation_mode,
        xai_speech: profile.xai_speech_mode,
        alibaba_realtime: profile.alibaba_realtime_mode,
    };
    let Some(route) = speech_route(profile.provider, modes) else {
        return Capabilities::invalid();
    };
    let configured = match profile.text_translation {
        Some(TextChoice::Kind(kind)) => Some(kind),
        _ => None,
    };
    let text = text_route(route, configured, profile.qwen_mt_model);
    let custom = profile.custom_speech_source_languages.as_deref();
    let options = language_options(route, text, custom, &request.source, &request.target);
    let issues = language_issues(&options, &request.source, &request.target);
    let LanguageOptions { sources, targets } = options;
    Capabilities {
        sources,
        targets,
        modes: vec!["turbo"],
        issues,
    }
}
