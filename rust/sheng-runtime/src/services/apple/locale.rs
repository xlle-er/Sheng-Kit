//! Language code to system locale (apple.md §1): an exact match first, then a regional
//! variant of the same language (`en` → `en-US`, `zh` → `zh-CN`, `yue` → `yue-CN`).

/// Region chosen first when a code names only a language and several variants exist.
const PREFERRED_REGIONS: &[(&str, &str)] = &[("en", "US"), ("zh", "CN"), ("yue", "CN")];

/// Language and region of a code or identifier; scripts and other subtags are ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tag {
    language: String,
    region: Option<String>,
}

fn is_region(subtag: &str) -> bool {
    let alphabetic = subtag.len() == 2 && subtag.bytes().all(|b| b.is_ascii_alphabetic());
    let numeric = subtag.len() == 3 && subtag.bytes().all(|b| b.is_ascii_digit());
    alphabetic || numeric
}

fn parse(identifier: &str) -> Option<Tag> {
    let mut subtags = identifier.split(['-', '_']);
    let language = subtags.next()?;
    let valid =
        (2..=3).contains(&language.len()) && language.bytes().all(|b| b.is_ascii_alphabetic());
    if !valid {
        return None;
    }
    let region = subtags.find(|s| is_region(s)).map(str::to_ascii_uppercase);
    Some(Tag {
        language: language.to_ascii_lowercase(),
        region,
    })
}

fn preferred_region(language: &str) -> String {
    PREFERRED_REGIONS
        .iter()
        .find(|(code, _)| *code == language)
        .map_or_else(
            || language.to_ascii_uppercase(),
            |(_, region)| (*region).to_owned(),
        )
}

/// The identifier in `available` that best matches the catalog `code`, if any.
pub fn map_locale<'a>(code: &str, available: &'a [String]) -> Option<&'a str> {
    let wanted = parse(code)?;
    let mut candidates: Vec<(&'a str, Tag)> = available
        .iter()
        .filter_map(|id| parse(id).map(|tag| (id.as_str(), tag)))
        .filter(|(_, tag)| tag.language == wanted.language)
        .collect();
    candidates.sort_by(|a, b| a.0.cmp(b.0));
    let with_region = |region: Option<&str>| {
        candidates
            .iter()
            .find(|(_, tag)| tag.region.as_deref() == region)
            .map(|(id, _)| *id)
    };
    let preferred = preferred_region(&wanted.language);
    with_region(wanted.region.as_deref())
        .or_else(|| with_region(Some(&preferred)))
        .or_else(|| candidates.first().map(|(id, _)| *id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn maps_spec_examples() {
        let speech = ids(&["en-GB", "en-US", "yue-CN", "zh-CN", "zh-HK", "zh-TW"]);
        assert_eq!(map_locale("en", &speech), Some("en-US"));
        assert_eq!(map_locale("zh", &speech), Some("zh-CN"));
        assert_eq!(map_locale("zh_tw", &speech), Some("zh-TW"));
        assert_eq!(map_locale("yue", &speech), Some("yue-CN"));
        assert_eq!(map_locale("en-GB", &speech), Some("en-GB"));
    }

    #[test]
    fn prefers_exact_then_any_variant() {
        let speech = ids(&["fr-CA", "fr-FR", "de-AT", "es-419", "es-MX"]);
        assert_eq!(map_locale("fr", &speech), Some("fr-FR"));
        assert_eq!(map_locale("de", &speech), Some("de-AT"));
        assert_eq!(map_locale("es-419", &speech), Some("es-419"));
        assert_eq!(map_locale("ja", &speech), None);
    }

    #[test]
    fn ignores_scripts_and_case() {
        let translation = ids(&["zh-Hans-CN", "zh-Hant-TW", "en-Latn-US", "pt-Latn-BR"]);
        assert_eq!(map_locale("zh", &translation), Some("zh-Hans-CN"));
        assert_eq!(map_locale("zh_tw", &translation), Some("zh-Hant-TW"));
        assert_eq!(map_locale("EN", &translation), Some("en-Latn-US"));
        assert_eq!(map_locale("pt", &translation), Some("pt-Latn-BR"));
    }

    #[test]
    fn rejects_malformed_codes() {
        let speech = ids(&["en-US"]);
        assert_eq!(map_locale("", &speech), None);
        assert_eq!(map_locale("original", &speech), None);
        assert_eq!(map_locale("e1", &speech), None);
    }
}
