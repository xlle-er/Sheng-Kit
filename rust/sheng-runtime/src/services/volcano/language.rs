//! Catalog language codes to AST v2 `source_language` / `target_language` values.

/// Chinese–English mixed speech; the service needs `zhen` on both sides.
const MIXED: &str = "zh_en";
const MIXED_WIRE: &str = "zhen";

fn wire_code(code: &str) -> &str {
    match code {
        "yue" => "yue-CN",
        "wuu" => "sh-CN",
        other => other,
    }
}

/// `(source_language, target_language)` for StartSession.
pub fn wire_pair<'a>(source: &'a str, target: &'a str) -> (&'a str, &'a str) {
    if source == MIXED {
        return (MIXED_WIRE, MIXED_WIRE);
    }
    (wire_code(source), wire_code(target))
}
