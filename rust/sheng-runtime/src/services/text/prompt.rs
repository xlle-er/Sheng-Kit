//! Instruction text for chat-model translation: target language and “only the translation”.
//! The request carries only the sentence to translate, never caption history.

use sheng_core::language::AUTO;

use crate::catalog::language_names::english_name;

/// English name of a catalog code; other codes are named by their code (step 4b question 10).
fn name(code: &str) -> String {
    english_name(code).map_or_else(
        || format!("the language with code \"{code}\""),
        str::to_owned,
    )
}

/// System message telling the model what to translate into and to output nothing else.
pub fn system_instruction(source: &str, target: &str) -> String {
    let from = if source == AUTO {
        String::new()
    } else {
        format!(" from {}", name(source))
    };
    format!(
        "You are a translation engine. Translate the user's text{from} into {}. \
         Output only the translation, without quotes, notes or explanations.",
        name(target)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_source_and_target() {
        let text = system_instruction("en", "zh_tw");
        assert!(text.contains("from English into Traditional Chinese"));
        assert!(text.contains("Output only the translation"));
    }

    #[test]
    fn auto_source_is_not_named_and_unknown_codes_fall_back() {
        let text = system_instruction(AUTO, "kmr");
        assert!(!text.contains(" from "));
        assert!(text.contains("code \"kmr\""));
    }
}
