//! Language sets: OpenAI and Azure OpenAI realtime routes (step 4c).

/// golden（OpenAI 文档未列出逐语言清单）
pub const OPENAI_TRANSCRIBE_SOURCES: &[&str] = &["auto", "zh", "en", "fr", "es", "yue", "zh_tw"];

/// golden（OpenAI 文档未列出逐语言清单）
pub const OPENAI_TRANSLATE_TARGETS: &[&str] = &[
    "zh", "en", "ja", "ko", "ru", "es", "fr", "pt", "de", "it", "vi", "id", "hi",
];
