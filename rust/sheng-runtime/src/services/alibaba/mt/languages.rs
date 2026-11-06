//! Qwen-MT `translation_options` language names (Qwen-MT documentation, “支持的语言”).
//! Requests name languages by their English name, not by code.

/// Names of the codes the catalog uses with Qwen-MT. `fa` is the lite model's name; the
/// larger models list it as “Western Persian” (see `name`).
const NAMES: &[(&str, &str)] = &[
    ("en", "English"),
    ("zh", "Chinese"),
    ("zh_tw", "Traditional Chinese"),
    ("ru", "Russian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("pt", "Portuguese"),
    ("de", "German"),
    ("it", "Italian"),
    ("th", "Thai"),
    ("vi", "Vietnamese"),
    ("id", "Indonesian"),
    ("ms", "Malay"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
    ("he", "Hebrew"),
    ("ur", "Urdu"),
    ("bn", "Bengali"),
    ("pl", "Polish"),
    ("nl", "Dutch"),
    ("tr", "Turkish"),
    ("km", "Khmer"),
    ("cs", "Czech"),
    ("sv", "Swedish"),
    ("hu", "Hungarian"),
    ("da", "Danish"),
    ("fi", "Finnish"),
    ("tl", "Tagalog"),
    ("fa", "Persian"),
];

/// Languages only the larger models (not `qwen-mt-lite`) support, among recognition sources.
const LARGER_MODEL_NAMES: &[(&str, &str)] = &[
    ("el", "Greek"),
    ("ro", "Romanian"),
    ("bg", "Bulgarian"),
    ("hr", "Croatian"),
    ("sk", "Slovak"),
];

pub const AUTO: &str = "auto";
const LITE_MODEL: &str = "qwen-mt-lite";

fn lookup(table: &[(&str, &'static str)], code: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| *name)
}

/// English name of `code` for `model`, or `None` when the model does not list it.
pub fn name(code: &str, model: &str) -> Option<&'static str> {
    let lite = model == LITE_MODEL;
    if code == "fa" && !lite {
        return Some("Western Persian");
    }
    lookup(NAMES, code).or_else(|| (!lite).then(|| lookup(LARGER_MODEL_NAMES, code)).flatten())
}

/// Source name; `auto` and languages the model does not list are detected by the model.
pub fn source_name(code: &str, model: &str) -> &'static str {
    name(code, model).unwrap_or(AUTO)
}
