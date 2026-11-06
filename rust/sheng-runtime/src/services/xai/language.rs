//! English names of the voice route's target languages, for the interpreter instructions.

use crate::catalog::language_names::english_name;

/// Name used in the instructions; codes without a catalog name keep their code.
pub fn language_name(code: &str) -> &str {
    english_name(code).unwrap_or(code)
}

#[cfg(test)]
mod tests {
    use super::language_name;
    use crate::catalog::languages::XAI_VOICE_TARGETS;

    #[test]
    fn every_voice_target_has_a_name() {
        for code in XAI_VOICE_TARGETS {
            assert_ne!(language_name(code), *code, "{code}");
        }
    }
}
