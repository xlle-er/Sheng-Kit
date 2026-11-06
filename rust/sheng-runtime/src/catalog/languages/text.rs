//! Language sets: Independent text translators (step 4b).

/// developers.deepl.com supported-languages 中非变体语言
pub const DEEPL_SOURCES: &[&str] = &[
    "auto", "zh", "en", "ja", "ko", "vi", "th", "id", "ms", "tl", "hi", "ar", "fr", "de", "es",
    "pt", "ru", "it", "nl", "sv", "da", "fi", "no", "el", "pl", "cs", "hu", "ro", "bg", "hr", "sk",
    "ace", "af", "an", "as", "ay", "az", "ba", "be", "bho", "bn", "br", "bs", "ca", "ceb", "ckb",
    "cy", "eo", "et", "eu", "fa", "ga", "gl", "gn", "gom", "gu", "ha", "he", "ht", "hy", "ig",
    "is", "jv", "ka", "kk", "kmr", "ky", "la", "lb", "lmo", "ln", "lt", "lv", "mai", "mg", "mi",
    "mk", "ml", "mn", "mr", "mt", "my", "ne", "oc", "om", "pa", "pag", "pam", "prs", "ps", "qu",
    "sa", "scn", "sl", "sq", "sr", "st", "su", "sw", "ta", "te", "tg", "tk", "tn", "tr", "ts",
    "tt", "uk", "ur", "uz", "wo", "xh", "yi", "yue", "zu",
];

/// developers.deepl.com supported-languages 全部语言（含变体）
pub const DEEPL_TARGETS: &[&str] = &[
    "original", "zh", "en", "ja", "zh_tw", "ko", "ru", "es", "fr", "pt", "de", "it", "th", "vi",
    "id", "ms", "ar", "hi", "he", "ur", "bn", "pl", "nl", "tr", "cs", "sv", "hu", "da", "fi", "tl",
    "fa", "ace", "af", "an", "as", "ay", "az", "ba", "be", "bg", "bho", "br", "bs", "ca", "ceb",
    "ckb", "cy", "el", "en-GB", "en-US", "eo", "es-419", "es-ES", "et", "eu", "ga", "gl", "gn",
    "gom", "gu", "ha", "hr", "ht", "hy", "ig", "is", "jv", "ka", "kk", "kmr", "ky", "la", "lb",
    "lmo", "ln", "lt", "lv", "mai", "mg", "mi", "mk", "ml", "mn", "mr", "mt", "my", "ne", "no",
    "oc", "om", "pa", "pag", "pam", "prs", "ps", "pt-BR", "pt-PT", "qu", "ro", "sa", "scn", "sk",
    "sl", "sq", "sr", "st", "su", "sw", "ta", "te", "tg", "tk", "tn", "ts", "tt", "uk", "uz", "wo",
    "xh", "yi", "yue", "zu",
];
