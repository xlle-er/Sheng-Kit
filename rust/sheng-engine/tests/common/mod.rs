//! Calling the exported C functions from tests, and scripted service clients behind them.
#![allow(dead_code)]

#[path = "../../../sheng-runtime/tests/common/mod.rs"]
pub mod requests;

use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use sheng_core::Source;
use sheng_engine::abi::engine::*;
use sheng_engine::factory::ClientFactory;
use sheng_engine::ffi::sheng_string_free;
use sheng_runtime::config::SessionPlan;
use sheng_runtime::provider::{
    BoxFuture, EventSink, ProviderError, SessionError, SpeechClient, TextTranslator,
};
use tokio::sync::Notify;

/// Reads and frees a JSON string returned by the library.
pub fn take_json(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    // SAFETY: the pointer is a NUL-terminated string returned by the library.
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .unwrap()
        .to_owned();
    // SAFETY: returned by the library and freed exactly once here.
    unsafe { sheng_string_free(pointer) };
    serde_json::from_str(&text).unwrap()
}

pub fn c(text: &str) -> CString {
    CString::new(text).unwrap()
}

pub fn start(handle: u64, request: &Value) -> Value {
    let request = c(&request.to_string());
    // SAFETY: a valid NUL-terminated string.
    take_json(unsafe { sheng_engine_start(handle, request.as_ptr()) })
}

pub fn snapshot(handle: u64) -> Value {
    take_json(sheng_engine_snapshot(handle))
}

pub fn drain(handle: u64) -> Vec<Value> {
    take_json(sheng_engine_drain_confirmed(handle))
        .as_array()
        .unwrap()
        .clone()
}

pub fn push(handle: u64, source: u32, bytes: &[u8]) -> i32 {
    // SAFETY: `bytes` is a readable slice of the given length.
    unsafe { sheng_engine_push_pcm(handle, source, bytes.as_ptr(), bytes.len()) }
}

/// Polls the snapshot until `done` holds; panics after five seconds.
pub fn wait_for(handle: u64, done: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let current = snapshot(handle);
        if done(&current) {
            return current;
        }
        assert!(
            Instant::now() < deadline,
            "timed out; last snapshot {current}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub fn status(snapshot: &Value) -> &str {
    snapshot["status"].as_str().unwrap()
}

pub fn wait_settled(handle: u64) -> Value {
    wait_for(handle, |s| s["settled"] == true)
}

/// Volcano Engine translates by itself, so caption events go straight to the captions.
pub fn translating_request(input: &str) -> Value {
    requests::start_request(input, "volcanoEngine", "simultaneous", "-", "zh", "en")
}

/// Hands out one prepared speech client per source.
#[derive(Default)]
pub struct TestFactory {
    speech: Mutex<HashMap<Source, Box<dyn SpeechClient>>>,
    translator: Option<Arc<dyn TextTranslator>>,
}

impl TestFactory {
    pub fn new(clients: Vec<(Source, Box<dyn SpeechClient>)>) -> Self {
        Self {
            speech: Mutex::new(clients.into_iter().collect()),
            translator: None,
        }
    }

    pub fn with_translator(self, translator: Arc<dyn TextTranslator>) -> Self {
        Self {
            translator: Some(translator),
            ..self
        }
    }
}

impl ClientFactory for TestFactory {
    fn speech(
        &self,
        _plan: &SessionPlan,
        source: Source,
    ) -> Result<Box<dyn SpeechClient>, SessionError> {
        self.speech
            .lock()
            .unwrap()
            .remove(&source)
            .ok_or(SessionError::ProviderConfigurationFailed)
    }

    fn translator(&self, _plan: &SessionPlan) -> Result<Arc<dyn TextTranslator>, SessionError> {
        self.translator
            .clone()
            .ok_or(SessionError::ProviderConfigurationFailed)
    }
}

/// New handle whose sessions use `factory`.
pub fn handle_with(factory: TestFactory) -> u64 {
    let handle = sheng_engine_create();
    assert!(sheng_engine::testing::set_factory(
        handle,
        Arc::new(factory)
    ));
    handle
}

/// Delays `connect` of the wrapped client until the gate is opened.
pub struct GatedSpeech {
    pub inner: Box<dyn SpeechClient>,
    pub gate: Arc<Notify>,
}

impl SpeechClient for GatedSpeech {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            self.gate.notified().await;
            self.inner.connect(events).await
        })
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        self.inner.send_audio(pcm)
    }

    fn heartbeat(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        self.inner.heartbeat()
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        self.inner.finish()
    }

    fn disconnect(&mut self) -> BoxFuture<'_, ()> {
        self.inner.disconnect()
    }
}
