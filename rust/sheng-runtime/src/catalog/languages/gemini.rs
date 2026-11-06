//! Language sets: Google Gemini Live routes (step 4e).

/// ai.google.dev Live API “Supported languages”（99 种）与目录的交集，另保留 `pt`（gemini.md 映射到 pt-BR/pt-PT）
pub const GEMINI_TRANSCRIBE_SOURCES: &[&str] = &[
    "auto", "zh", "en", "ja", "ko", "vi", "th", "id", "ms", "tl", "hi", "ar", "fr", "de", "es",
    "pt", "ru", "it", "nl", "sv", "da", "fi", "no", "el", "pl", "cs", "hu", "ro", "bg", "hr", "sk",
    "af", "ak", "am", "as", "az", "be", "bn", "bs", "ca", "ceb", "cy", "et", "eu", "fa", "ga",
    "gl", "gu", "ha", "he", "hy", "is", "ka", "kk", "km", "kn", "ky", "lo", "lt", "lv", "mi", "mk",
    "ml", "mn", "mr", "mt", "my", "ne", "om", "pa", "ps", "pt-BR", "pt-PT", "qu", "rw", "sd", "si",
    "sl", "sq", "sr", "st", "sw", "ta", "te", "tg", "tk", "tn", "tr", "uk", "ur", "uz", "wo",
    "zh_tw", "zu",
];

/// golden（同传模型目标语言未在公开文档列出）
pub const GEMINI_TRANSLATE_TARGETS: &[&str] = &[
    "zh", "en", "ja", "zh_tw", "ko", "ru", "es", "fr", "de", "it", "th", "vi", "id", "ms", "ar",
    "hi", "he", "ur", "bn", "pl", "nl", "tr", "km", "cs", "sv", "hu", "da", "fi", "tl", "fa", "af",
    "ak", "am", "az", "be", "bg", "ca", "el", "et", "eu", "gl", "gu", "ha", "hr", "hy", "is", "jv",
    "ka", "kk", "kn", "lo", "lt", "lv", "mk", "ml", "mn", "mr", "my", "ne", "no", "pa", "pt-BR",
    "pt-PT", "ro", "rw", "sd", "si", "sk", "sl", "sq", "sr", "su", "sw", "ta", "te", "uk", "uz",
    "zu",
];
