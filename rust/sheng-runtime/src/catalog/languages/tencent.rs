//! Language sets: Tencent Cloud (step 4g).
//!
//! Source: Tencent Cloud docs, 语音识别 › 实时语音翻译（WebSocket）, `source`/`target` parameters
//! (cloud.tencent.com/document/product/1093/127565, updated 2026-05-27).

/// `source` values the service accepts.
pub const TENCENT_SOURCES: &[&str] = &["zh", "en", "zh_en", "ja", "ko", "yue", "id", "th", "ru"];

/// Targets of `zh` and `en`.
const TENCENT_FROM_ZH_EN: &[&str] = &["zh", "en", "ja", "ko", "yue", "id", "th"];
/// Targets of `zh_en` (mixed Chinese/English may also stay mixed).
const TENCENT_FROM_MIXED: &[&str] = &["zh_en", "zh", "en", "ja", "ko", "yue", "id", "th"];
/// Targets of `ja`, `ko` and `yue`.
const TENCENT_FROM_ASIAN: &[&str] = &["zh", "en", "ja", "ko", "yue"];
/// Union of every source's targets, offered while the source is unsupported.
const TENCENT_ANY_TARGET: &[&str] = &["zh", "en", "ja", "ko", "yue", "id", "th", "ru", "zh_en"];

/// Targets the service lists for `source`.
pub fn tencent_targets(source: &str) -> Vec<&'static str> {
    match source {
        "zh" | "en" => TENCENT_FROM_ZH_EN.to_vec(),
        "zh_en" => TENCENT_FROM_MIXED.to_vec(),
        "ja" | "ko" | "yue" => TENCENT_FROM_ASIAN.to_vec(),
        "id" => vec!["zh", "en", "id"],
        "th" => vec!["zh", "en", "th"],
        "ru" => vec!["zh", "en", "ru"],
        _ => TENCENT_ANY_TARGET.to_vec(),
    }
}
