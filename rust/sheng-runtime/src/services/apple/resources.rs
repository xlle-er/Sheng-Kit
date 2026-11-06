//! Resource operations of `sheng_resources_start` (apple.md §1). Downloads happen only in
//! kinds 1 and 2, which the user starts explicitly.

use serde_json::{json, Value};
use sheng_core::language::{AUTO, ORIGINAL, SOURCE_LANGUAGES};

use super::locale::map_locale;
use super::status::{SpeechStatus, TranslationStatus};
use super::system::{Apple, AppleSystem, SpeechInventory};

/// Completed value or failure code of one operation.
pub type Outcome = Result<Value, &'static str>;

pub const REPORT: u32 = 0;
pub const PREPARE_SPEECH: u32 = 1;
pub const PREPARE_TRANSLATION: u32 = 2;
pub const SESSION_READY: u32 = 3;
pub const TRANSLATION_READY: u32 = 4;

const APPLE_UNAVAILABLE: &str = "apple_unavailable";
const LANGUAGE_UNSUPPORTED: &str = "language_unsupported";
const SPEECH_PREPARATION_FAILED: &str = "speech_preparation_failed";
const TRANSLATION_PREPARATION_FAILED: &str = "translation_preparation_failed";
const SPEECH_ASSETS_MISSING: &str = "speech_assets_missing";
const TRANSLATION_UNAVAILABLE: &str = "translation_unavailable";
const TRANSLATION_ASSETS_MISSING: &str = "translation_assets_missing";

/// Runs resource operation `kind`; `apple` is `None` where the bridge does not exist.
pub async fn run(apple: Option<Apple>, kind: u32, source: &str, target: &str) -> Outcome {
    let Some(apple) = apple else {
        return Err(unavailable(kind));
    };
    let system = apple.system.as_ref();
    match kind {
        REPORT => report(system, source, target).await,
        PREPARE_SPEECH => prepare_speech(system, source).await,
        PREPARE_TRANSLATION => prepare_translation(system, source, target).await,
        SESSION_READY => {
            speech_ready(system, source).await?;
            if target != ORIGINAL {
                translation_ready(system, source, target).await?;
            }
            Ok(json!({"ready": true}))
        }
        TRANSLATION_READY => {
            translation_ready(system, source, target).await?;
            Ok(json!({"ready": true}))
        }
        _ => Err("invalid_operation"),
    }
}

/// Failure without the bridge, using only codes apple.md §1 lists for `kind`.
fn unavailable(kind: u32) -> &'static str {
    match kind {
        PREPARE_TRANSLATION => TRANSLATION_PREPARATION_FAILED,
        TRANSLATION_READY => TRANSLATION_UNAVAILABLE,
        REPORT | PREPARE_SPEECH | SESSION_READY => APPLE_UNAVAILABLE,
        _ => "invalid_operation",
    }
}

async fn inventory(system: &dyn AppleSystem) -> Result<SpeechInventory, &'static str> {
    let inventory = system.speech_inventory().await;
    inventory.filter(|i| i.available).ok_or(APPLE_UNAVAILABLE)
}

/// Mapped locale and its status; unsupported languages fail.
fn speech_locale(
    inventory: &SpeechInventory,
    code: &str,
) -> Result<(String, SpeechStatus), &'static str> {
    let identifiers = inventory.identifiers();
    let locale = map_locale(code, &identifiers).ok_or(LANGUAGE_UNSUPPORTED)?;
    match inventory.status(locale) {
        SpeechStatus::Unsupported => Err(LANGUAGE_UNSUPPORTED),
        status => Ok((locale.to_owned(), status)),
    }
}

/// Status of the translation pair; `Unavailable` without the framework.
async fn translation_status(
    system: &dyn AppleSystem,
    source: &str,
    target: &str,
) -> TranslationStatus {
    let Some(languages) = system.translation_languages().await else {
        return TranslationStatus::Unavailable;
    };
    let pair = map_locale(source, &languages).zip(map_locale(target, &languages));
    match pair {
        Some((s, t)) => system.translation_status(s.to_owned(), t.to_owned()).await,
        None => TranslationStatus::Unsupported,
    }
}

async fn report(system: &dyn AppleSystem, source: &str, target: &str) -> Outcome {
    let inventory = system.speech_inventory().await.ok_or(APPLE_UNAVAILABLE)?;
    let identifiers = inventory.identifiers();
    let locales: Vec<Value> = inventory
        .locales
        .iter()
        .map(|(id, status)| json!({"identifier": id, "status": status.as_str()}))
        .collect();
    let languages: Vec<Value> = SOURCE_LANGUAGES
        .iter()
        .filter(|code| **code != AUTO)
        .filter_map(|code| {
            let locale = map_locale(code, &identifiers)?;
            let status = inventory.status(locale).as_str();
            Some(json!({"code": code, "locale": locale, "status": status}))
        })
        .collect();
    let translation = if target == ORIGINAL {
        Value::Null
    } else {
        json!(translation_status(system, source, target).await.as_str())
    };
    Ok(json!({
        "speech": {"available": inventory.available, "locales": locales},
        "translation": translation,
        "languages": languages,
    }))
}

async fn prepare_speech(system: &dyn AppleSystem, source: &str) -> Outcome {
    let inventory = inventory(system).await?;
    let (locale, status) = speech_locale(&inventory, source)?;
    if status != SpeechStatus::Installed && !system.prepare_speech(locale).await {
        return Err(SPEECH_PREPARATION_FAILED);
    }
    Ok(json!({"prepared": true}))
}

async fn prepare_translation(system: &dyn AppleSystem, source: &str, target: &str) -> Outcome {
    let languages = system
        .translation_languages()
        .await
        .ok_or(TRANSLATION_PREPARATION_FAILED)?;
    let pair = map_locale(source, &languages).zip(map_locale(target, &languages));
    let (source, target) = pair.ok_or(TRANSLATION_PREPARATION_FAILED)?;
    let (source, target) = (source.to_owned(), target.to_owned());
    match system
        .translation_status(source.clone(), target.clone())
        .await
    {
        TranslationStatus::Installed => Ok(json!({"prepared": true})),
        TranslationStatus::Supported if system.prepare_translation(source, target).await => {
            Ok(json!({"prepared": true}))
        }
        _ => Err(TRANSLATION_PREPARATION_FAILED),
    }
}

async fn speech_ready(system: &dyn AppleSystem, source: &str) -> Result<(), &'static str> {
    let inventory = inventory(system).await?;
    match speech_locale(&inventory, source)? {
        (_, SpeechStatus::Installed) => Ok(()),
        _ => Err(SPEECH_ASSETS_MISSING),
    }
}

async fn translation_ready(
    system: &dyn AppleSystem,
    source: &str,
    target: &str,
) -> Result<(), &'static str> {
    match translation_status(system, source, target).await {
        TranslationStatus::Installed => Ok(()),
        TranslationStatus::Supported => Err(TRANSLATION_ASSETS_MISSING),
        TranslationStatus::Unsupported | TranslationStatus::Unavailable => {
            Err(TRANSLATION_UNAVAILABLE)
        }
    }
}
