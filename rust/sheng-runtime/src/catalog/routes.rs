//! Selectable languages per route (catalog.md): recognizing routes combine their speech
//! languages with the translator's; translating routes use their own pair tables.

use sheng_core::language::{AUTO, ORIGINAL, SOURCE_LANGUAGES, TARGET_LANGUAGES};

use super::languages::*;
use crate::config::request::TextKind;
use crate::config::route::{SpeechRoute, TextRoute};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguageOptions {
    pub sources: Vec<&'static str>,
    pub targets: Vec<&'static str>,
}

fn without_auto() -> Vec<&'static str> {
    SOURCE_LANGUAGES
        .iter()
        .copied()
        .filter(|c| *c != AUTO)
        .collect()
}

fn intersect(base: &[&'static str], allowed: &[&'static str]) -> Vec<&'static str> {
    base.iter()
        .copied()
        .filter(|c| allowed.contains(c))
        .collect()
}

/// Languages a recognizing route can transcribe.
fn speech_sources(route: SpeechRoute, custom: Option<&[String]>) -> Vec<&'static str> {
    let listed: Vec<&'static str> = match route {
        SpeechRoute::AppleSpeech => without_auto(),
        SpeechRoute::AlibabaAsr => ALIBABA_ASR_SOURCES.to_vec(),
        SpeechRoute::OpenAiTranscribe | SpeechRoute::AzureTranscribe => {
            OPENAI_TRANSCRIBE_SOURCES.to_vec()
        }
        SpeechRoute::GeminiTranscribe => GEMINI_TRANSCRIBE_SOURCES.to_vec(),
        SpeechRoute::XaiStt => XAI_STT_SOURCES.to_vec(),
        _ => SOURCE_LANGUAGES.to_vec(),
    };
    let is_custom = matches!(
        route,
        SpeechRoute::CustomDashScope | SpeechRoute::CustomOpenAi
    );
    match custom {
        Some(chosen) if is_custom => listed
            .into_iter()
            .filter(|code| chosen.iter().any(|c| c == code))
            .collect(),
        _ => listed,
    }
}

/// `(sources, targets)` a translator accepts; targets include `original` first.
fn translator_languages(text: TextRoute) -> (Vec<&'static str>, Vec<&'static str>) {
    match text {
        TextRoute::None => (SOURCE_LANGUAGES.to_vec(), vec![ORIGINAL]),
        TextRoute::Independent(TextKind::Apple) => (without_auto(), TARGET_LANGUAGES.to_vec()),
        TextRoute::Independent(TextKind::DeepL | TextKind::DeepLx) => {
            (DEEPL_SOURCES.to_vec(), DEEPL_TARGETS.to_vec())
        }
        TextRoute::Independent(_) => (SOURCE_LANGUAGES.to_vec(), TARGET_LANGUAGES.to_vec()),
        TextRoute::QwenMt(_) => {
            let sources = std::iter::once(AUTO)
                .chain(QWEN_MT_TARGETS.iter().copied().filter(|c| *c != ORIGINAL))
                .collect();
            (sources, QWEN_MT_TARGETS.to_vec())
        }
    }
}

fn recognizing_options(
    route: SpeechRoute,
    text: TextRoute,
    custom: Option<&[String]>,
    target: &str,
) -> LanguageOptions {
    let speech = speech_sources(route, custom);
    let (text_sources, targets) = translator_languages(text);
    let sources = if target == ORIGINAL || text == TextRoute::None {
        speech
    } else {
        intersect(&speech, &text_sources)
    };
    LanguageOptions { sources, targets }
}

const CHINESE_OR_ENGLISH: &[&str] = &["zh", "en"];

/// Volcano S2T: one side must be Chinese or English; dialects are source-only.
fn volcano_targets(source: &str) -> Vec<&'static str> {
    match source {
        "zh" | "en" => VOLCANO_FOREIGN_TARGETS.to_vec(),
        code if VOLCANO_SOURCES.contains(&code) => CHINESE_OR_ENGLISH.to_vec(),
        _ => VOLCANO_FOREIGN_TARGETS.to_vec(),
    }
}

fn translating_options(route: SpeechRoute, source: &str) -> LanguageOptions {
    let auto_only = vec![AUTO];
    let (sources, targets) = match route {
        SpeechRoute::Volcano => (VOLCANO_SOURCES.to_vec(), volcano_targets(source)),
        SpeechRoute::Tencent => (TENCENT_SOURCES.to_vec(), tencent_targets(source)),
        SpeechRoute::Baidu => (BAIDU_LANGUAGES.to_vec(), BAIDU_LANGUAGES.to_vec()),
        SpeechRoute::XaiVoice => {
            let sources = std::iter::once(AUTO)
                .chain(XAI_VOICE_TARGETS.iter().copied())
                .collect();
            (sources, XAI_VOICE_TARGETS.to_vec())
        }
        SpeechRoute::OpenAiTranslate | SpeechRoute::AzureTranslate => {
            (auto_only, OPENAI_TRANSLATE_TARGETS.to_vec())
        }
        SpeechRoute::GeminiTranslate => (auto_only, GEMINI_TRANSLATE_TARGETS.to_vec()),
        _ => (auto_only, ALIBABA_LIVE_TARGETS.to_vec()),
    };
    LanguageOptions { sources, targets }
}

/// Languages selectable for `route` with the current `source`/`target` choice.
pub fn language_options(
    route: SpeechRoute,
    text: TextRoute,
    custom: Option<&[String]>,
    source: &str,
    target: &str,
) -> LanguageOptions {
    if route.translates() {
        translating_options(route, source)
    } else {
        recognizing_options(route, text, custom, target)
    }
}

/// Field-level problems of a language choice, `source` before `target`.
pub fn language_issues(options: &LanguageOptions, source: &str, target: &str) -> Vec<Issue> {
    let mut issues = Vec::new();
    if !options.sources.contains(&source) {
        issues.push(Issue::language("source"));
    }
    if !options.targets.contains(&target) {
        issues.push(Issue::language("target"));
    }
    issues
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Issue {
    pub field: &'static str,
    pub code: &'static str,
}

impl Issue {
    pub fn language(field: &'static str) -> Self {
        Self {
            field,
            code: "language_unsupported",
        }
    }

    pub fn invalid_profile() -> Self {
        Self {
            field: "profile",
            code: "invalid_configuration",
        }
    }
}
