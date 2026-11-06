//! Fakes of the Apple system traits shared by the Apple service tests (step 4a).
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use sheng_runtime::provider::{BoxFuture, ProviderError, SessionError};
use sheng_runtime::services::apple::events::{self, BridgeEvent};
use sheng_runtime::services::apple::status::{SpeechStatus, TranslationStatus};
use sheng_runtime::services::apple::system::{
    Apple, AppleSystem, Push, Recognizer, SpeechInventory, TranslateFailure,
};

#[derive(Default)]
pub struct FakeSystem {
    pub inventory: Option<SpeechInventory>,
    pub languages: Option<Vec<String>>,
    pub pair: Option<TranslationStatus>,
    pub prepare_ok: bool,
    pub translation: Option<Result<String, TranslateFailure>>,
    /// Every download that was started, in order.
    pub downloads: Mutex<Vec<String>>,
}

impl FakeSystem {
    pub fn downloads(&self) -> Vec<String> {
        self.downloads.lock().unwrap().clone()
    }
}

pub fn ready<T: Send + 'static>(value: T) -> BoxFuture<'static, T> {
    Box::pin(async move { value })
}

impl AppleSystem for FakeSystem {
    fn speech_inventory(&self) -> BoxFuture<'_, Option<SpeechInventory>> {
        ready(self.inventory.clone())
    }
    fn prepare_speech(&self, locale: String) -> BoxFuture<'_, bool> {
        self.downloads
            .lock()
            .unwrap()
            .push(format!("speech:{locale}"));
        ready(self.prepare_ok)
    }
    fn translation_languages(&self) -> BoxFuture<'_, Option<Vec<String>>> {
        ready(self.languages.clone())
    }
    fn translation_status(&self, _: String, _: String) -> BoxFuture<'_, TranslationStatus> {
        ready(self.pair.unwrap_or(TranslationStatus::Unavailable))
    }
    fn prepare_translation(&self, source: String, target: String) -> BoxFuture<'_, bool> {
        let entry = format!("translation:{source}>{target}");
        self.downloads.lock().unwrap().push(entry);
        ready(self.prepare_ok)
    }
    fn translate(
        &self,
        _: String,
        _: String,
        _: String,
    ) -> BoxFuture<'_, Result<String, TranslateFailure>> {
        ready(
            self.translation
                .clone()
                .unwrap_or(Err(TranslateFailure::Failed)),
        )
    }
}

/// Recognizer that answers `open` with a scripted first event.
pub struct FakeRecognizer {
    pub open_ok: bool,
    pub first: Option<BridgeEvent>,
    pub push: Push,
    pub alive: bool,
    pub calls: Mutex<Vec<String>>,
    pub token: Mutex<Option<u64>>,
}

impl FakeRecognizer {
    pub fn new(first: Option<BridgeEvent>) -> Self {
        Self {
            open_ok: true,
            first,
            push: Push::Accepted,
            alive: true,
            calls: Mutex::default(),
            token: Mutex::default(),
        }
    }
    pub fn token(&self) -> u64 {
        self.token.lock().unwrap().expect("opened")
    }
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Recognizer for FakeRecognizer {
    fn open(&self, locale: &str, token: u64) -> bool {
        self.calls.lock().unwrap().push(format!("open:{locale}"));
        *self.token.lock().unwrap() = Some(token);
        if self.open_ok {
            if let Some(first) = self.first.clone() {
                events::deliver(token, first);
            }
        }
        self.open_ok
    }
    fn push(&self, _: u64, pcm: &[u8]) -> Push {
        self.calls
            .lock()
            .unwrap()
            .push(format!("push:{}", pcm.len()));
        self.push
    }
    fn finish(&self, _: u64) {
        self.calls.lock().unwrap().push("finish".into());
    }
    fn alive(&self, _: u64) -> bool {
        self.alive
    }
    fn close(&self, _: u64) {
        self.calls.lock().unwrap().push("close".into());
    }
}

pub fn inventory(locales: &[(&str, SpeechStatus)]) -> SpeechInventory {
    SpeechInventory {
        available: true,
        locales: locales
            .iter()
            .map(|(id, s)| ((*id).to_owned(), *s))
            .collect(),
    }
}

pub fn installed_english() -> FakeSystem {
    FakeSystem {
        inventory: Some(inventory(&[
            ("en-US", SpeechStatus::Installed),
            ("fr-FR", SpeechStatus::Supported),
        ])),
        languages: Some(vec!["en-Latn-US".into(), "zh-Hans-CN".into()]),
        pair: Some(TranslationStatus::Installed),
        prepare_ok: true,
        ..FakeSystem::default()
    }
}

pub fn fake(
    system: FakeSystem,
    recognizer: FakeRecognizer,
) -> (Apple, Arc<FakeSystem>, Arc<FakeRecognizer>) {
    let (system, recognizer) = (Arc::new(system), Arc::new(recognizer));
    let apple = Apple {
        system: system.clone(),
        recognizer: recognizer.clone(),
    };
    (apple, system, recognizer)
}

pub fn local(code: SessionError) -> ProviderError {
    ProviderError::Local(code)
}
