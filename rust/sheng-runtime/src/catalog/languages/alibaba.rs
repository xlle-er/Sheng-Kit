//! Language sets: Alibaba Cloud routes and Qwen-MT (step 4f).

/// golden（阿里云页面为动态渲染，未能离线核对）
pub const ALIBABA_ASR_SOURCES: &[&str] = &[
    "auto", "zh", "en", "ja", "ko", "vi", "th", "id", "ms", "tl", "hi", "ar", "fr", "de", "es",
    "pt", "ru", "it", "nl", "sv", "da", "fi", "no", "el", "pl", "cs", "hu", "ro", "bg", "hr", "sk",
];

/// golden（Qwen-MT 文档页语言表未能离线提取）
pub const QWEN_MT_TARGETS: &[&str] = &[
    "original", "en", "zh", "zh_tw", "ru", "ja", "ko", "es", "fr", "pt", "de", "it", "th", "vi",
    "id", "ms", "ar", "hi", "he", "ur", "bn", "pl", "nl", "tr", "km", "cs", "sv", "hu", "da", "fi",
    "tl", "fa",
];

/// golden（阿里云页面为动态渲染）
pub const ALIBABA_LIVE_TARGETS: &[&str] = &[
    "original", "zh", "en", "ja", "ko", "ru", "es", "fr", "pt", "de", "it", "th", "vi", "id", "ms",
    "ar", "hi", "he", "ur", "bn", "pl", "nl", "tr", "cs", "sv", "hu", "da", "fi", "tl", "fa",
    "ast", "af", "az", "be", "bg", "bs", "ca", "ceb", "el", "et", "gl", "gu", "hr", "is", "jv",
    "kk", "kn", "ky", "lv", "mk", "ml", "mr", "no", "pa", "ro", "sk", "sl", "sw", "tg", "uk",
    "yue",
];
