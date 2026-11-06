//! English names of catalog language codes. One table serves every instruction that names a
//! language: chat-model translation prompts and the xAI voice interpreter instructions.

use sheng_core::language::{ZH_EN, ZH_TW};

/// Catalog code and its English name; codes not listed have no name.
const NAMES: &[(&str, &str)] = &[
    ("zh", "Simplified Chinese"),
    (ZH_TW, "Traditional Chinese"),
    (ZH_EN, "mixed Chinese and English"),
    ("yue", "Cantonese"),
    ("en", "English"),
    ("en-GB", "British English"),
    ("en-US", "American English"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("fr", "French"),
    ("de", "German"),
    ("es", "Spanish"),
    ("es-ES", "European Spanish"),
    ("es-MX", "Mexican Spanish"),
    ("pt", "Portuguese"),
    ("pt-BR", "Brazilian Portuguese"),
    ("pt-PT", "European Portuguese"),
    ("ru", "Russian"),
    ("it", "Italian"),
    ("ar", "Arabic"),
    ("ar-AE", "Emirati Arabic"),
    ("ar-EG", "Egyptian Arabic"),
    ("ar-SA", "Saudi Arabic"),
    ("hi", "Hindi"),
    ("bn", "Bengali"),
    ("th", "Thai"),
    ("vi", "Vietnamese"),
    ("id", "Indonesian"),
    ("ms", "Malay"),
    ("tr", "Turkish"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("uk", "Ukrainian"),
];

/// English name of `code`, if the table has one.
pub fn english_name(code: &str) -> Option<&'static str> {
    NAMES
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, name)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique() {
        for (index, (code, _)) in NAMES.iter().enumerate() {
            assert!(
                NAMES[index + 1..].iter().all(|(other, _)| other != code),
                "{code}"
            );
        }
    }

    #[test]
    fn unknown_codes_have_no_name() {
        assert_eq!(english_name("en"), Some("English"));
        assert_eq!(english_name("kmr"), None);
    }
}
