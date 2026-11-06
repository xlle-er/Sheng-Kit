//! Language sets: Volcano Engine (step 4i).

/// volcengine 6561/1756902 S2T：20 种外语 + 粤语、上海话；`zh_en` 沿用 golden
pub const VOLCANO_SOURCES: &[&str] = &[
    "zh", "en", "pt", "es", "ja", "id", "de", "fr", "ru", "it", "ko", "ar", "tr", "ms", "vi", "th",
    "nl", "ro", "pl", "cs", "yue", "wuu", "zh_en",
];

/// volcengine 6561/1756902 S2T 外语列表
pub const VOLCANO_FOREIGN_TARGETS: &[&str] = &[
    "zh", "en", "pt", "es", "ja", "id", "de", "fr", "ru", "it", "ko", "ar", "tr", "ms", "vi", "th",
    "nl", "ro", "pl", "cs",
];
