//! Catalog language codes to the BCP-47 codes the Live API documents (providers/gemini.md).

use sheng_core::language::{AUTO, ORIGINAL};

/// Service code for a catalog code; `None` when the service should detect or keep the
/// language itself (`auto`, `original`).
pub fn service_code(code: &str) -> Option<&str> {
    match code {
        AUTO | ORIGINAL | "" => None,
        "zh" => Some("zh-Hans"),
        "zh_tw" => Some("zh-Hant"),
        // The catalog keeps `tl`; the Live Translate table lists Filipino as `fil`.
        "tl" => Some("fil"),
        // Bare Portuguese has no entry in the documented tables (questions/step4e.md).
        "pt" => Some("pt-BR"),
        other => Some(other),
    }
}

#[cfg(test)]
mod tests {
    use super::service_code;

    #[test]
    fn maps_documented_variants() {
        assert_eq!(service_code("auto"), None);
        assert_eq!(service_code("original"), None);
        assert_eq!(service_code("zh"), Some("zh-Hans"));
        assert_eq!(service_code("zh_tw"), Some("zh-Hant"));
        assert_eq!(service_code("tl"), Some("fil"));
        assert_eq!(service_code("pt"), Some("pt-BR"));
        assert_eq!(service_code("pt-PT"), Some("pt-PT"));
        assert_eq!(service_code("ja"), Some("ja"));
    }
}
