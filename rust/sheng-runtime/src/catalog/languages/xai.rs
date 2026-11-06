//! Language sets: xAI realtime routes (step 4d).

/// docs.x.ai speech-to-text “Supported Languages”（与 golden 一致）
pub const XAI_STT_SOURCES: &[&str] = &[
    "auto", "zh", "en", "ja", "ko", "vi", "th", "id", "ms", "tl", "hi", "ar", "fr", "de", "es",
    "pt", "ru", "it", "nl", "sv", "da", "fi", "no", "el", "pl", "cs", "hu", "ro", "bg", "hr", "sk",
    "ar-AE", "ar-EG", "ar-SA", "bs", "ca", "es-ES", "es-MX", "fa", "mk", "pt-BR", "pt-PT", "tr",
    "uk", "ur", "yue",
];

/// docs.x.ai speech-to-speech “Supported Languages”（与 golden 一致）
pub const XAI_VOICE_TARGETS: &[&str] = &[
    "zh", "en", "ja", "ko", "vi", "id", "hi", "fr", "de", "ru", "it", "ar-EG", "ar-SA", "ar-AE",
    "bn", "pt-BR", "pt-PT", "es-MX", "es-ES", "tr",
];
