//! Catalog language codes to the codes of the Baidu real-time speech translation API
//! (ai.baidu.com MT/Sl9p2h5k9, “翻译语言列表”, 45 languages).

/// `(catalog code, Baidu code)` for every language in `BAIDU_LANGUAGES`.
pub const BAIDU_CODES: &[(&str, &str)] = &[
    ("zh", "zh"),
    ("en", "en"),
    ("yue", "yue"),
    ("ja", "jp"),
    ("ko", "kor"),
    ("fr", "fra"),
    ("es", "spa"),
    ("th", "th"),
    ("ar", "ara"),
    ("ru", "ru"),
    ("pt", "pt"),
    ("de", "de"),
    ("it", "it"),
    ("el", "el"),
    ("nl", "nl"),
    ("pl", "pl"),
    ("bg", "bul"),
    ("da", "dan"),
    ("fi", "fin"),
    ("cs", "cs"),
    ("ro", "rom"),
    ("sv", "swe"),
    ("hu", "hu"),
    ("vi", "vie"),
    ("id", "id"),
    ("ca", "cat"),
    ("he", "heb"),
    ("hi", "hi"),
    ("ms", "may"),
    ("no", "nor"),
    ("is", "ice"),
    ("tl", "fil"),
    ("km", "hkm"),
    ("hr", "hrv"),
    ("lv", "lav"),
    ("bn", "ben"),
    ("ne", "nep"),
    ("af", "afr"),
    ("sk", "sk"),
    ("si", "sin"),
    ("sr", "srp"),
    ("sw", "swa"),
    ("tr", "tr"),
    ("uk", "ukr"),
    ("hy", "arm"),
];

/// The Baidu code for a catalog code, if the service supports it.
pub fn baidu_code(code: &str) -> Option<&'static str> {
    BAIDU_CODES
        .iter()
        .find(|(catalog, _)| *catalog == code)
        .map(|(_, baidu)| *baidu)
}
