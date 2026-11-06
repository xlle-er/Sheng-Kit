//! Language codes offered to the UI. The order is the display order.

use serde::Serialize;

/// Source code that asks the service to detect the spoken language.
pub const AUTO: &str = "auto";
/// Target code that shows the recognized text without translation.
pub const ORIGINAL: &str = "original";
/// Traditional Chinese.
pub const ZH_TW: &str = "zh_tw";
/// Mixed Chinese and English speech.
pub const ZH_EN: &str = "zh_en";

/// Selectable source languages, `auto` first.
pub const SOURCE_LANGUAGES: &[&str] = &[
    "auto", "zh", "en", "ja", "ko", "vi", "th", "id", "ms", "tl", "hi", "ar", "fr", "de", "es",
    "pt", "ru", "it", "nl", "sv", "da", "fi", "no", "el", "pl", "cs", "hu", "ro", "bg", "hr", "sk",
    "wuu", "ast", "ace", "af", "ak", "am", "an", "ar-AE", "ar-EG", "ar-SA", "as", "ay", "az", "ba",
    "be", "bho", "bn", "br", "bs", "ca", "ceb", "ckb", "cy", "en-GB", "en-US", "eo", "es-419",
    "es-ES", "es-MX", "et", "eu", "fa", "ga", "gl", "gn", "gom", "gu", "ha", "he", "ht", "hy",
    "ig", "is", "jv", "ka", "kk", "km", "kmr", "kn", "ky", "la", "lb", "lmo", "ln", "lo", "lt",
    "lv", "mai", "mg", "mi", "mk", "ml", "mn", "mr", "mt", "my", "ne", "oc", "om", "pa", "pag",
    "pam", "prs", "ps", "pt-BR", "pt-PT", "qu", "rw", "sa", "scn", "sd", "si", "sl", "sq", "sr",
    "st", "su", "sw", "ta", "te", "tg", "tk", "tn", "tr", "ts", "tt", "uk", "ur", "uz", "wo", "xh",
    "yi", "yue", "zh_en", "zh_tw", "zu",
];

/// Selectable target languages, `original` first.
pub const TARGET_LANGUAGES: &[&str] = &[
    "original", "zh", "en", "ja", "zh_tw", "ko", "ru", "es", "fr", "pt", "de", "it", "th", "vi",
    "id", "ms", "ar", "hi", "he", "ur", "bn", "pl", "nl", "tr", "km", "cs", "sv", "hu", "da", "fi",
    "tl", "fa", "wuu", "ast", "ace", "af", "ak", "am", "an", "ar-AE", "ar-EG", "ar-SA", "as", "ay",
    "az", "ba", "be", "bg", "bho", "br", "bs", "ca", "ceb", "ckb", "cy", "el", "en-GB", "en-US",
    "eo", "es-419", "es-ES", "es-MX", "et", "eu", "ga", "gl", "gn", "gom", "gu", "ha", "hr", "ht",
    "hy", "ig", "is", "jv", "ka", "kk", "kmr", "kn", "ky", "la", "lb", "lmo", "ln", "lo", "lt",
    "lv", "mai", "mg", "mi", "mk", "ml", "mn", "mr", "mt", "my", "ne", "no", "oc", "om", "pa",
    "pag", "pam", "prs", "ps", "pt-BR", "pt-PT", "qu", "ro", "rw", "sa", "scn", "sd", "si", "sk",
    "sl", "sq", "sr", "st", "su", "sw", "ta", "te", "tg", "tk", "tn", "ts", "tt", "uk", "uz", "wo",
    "xh", "yi", "yue", "zh_en", "zu",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LanguageCatalog {
    pub sources: Vec<&'static str>,
    pub targets: Vec<&'static str>,
}

pub fn catalog() -> LanguageCatalog {
    LanguageCatalog {
        sources: SOURCE_LANGUAGES.to_vec(),
        targets: TARGET_LANGUAGES.to_vec(),
    }
}

pub fn is_source_language(code: &str) -> bool {
    SOURCE_LANGUAGES.contains(&code)
}

pub fn is_target_language(code: &str) -> bool {
    TARGET_LANGUAGES.contains(&code)
}

/// A session needs a translation for every confirmed caption unless it only shows the original.
pub fn needs_translation(target: &str) -> bool {
    target != ORIGINAL
}
