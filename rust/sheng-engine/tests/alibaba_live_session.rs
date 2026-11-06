//! Alibaba live translation (`alibabaRealtimeMode:"liveTranslate"`) through the C functions,
//! against a loopback realtime server (no network).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::requests::start_request;
use common::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sheng_core::Source;
use sheng_engine::abi::engine::*;
use sheng_engine::factory::{ClientFactory, ServiceFactory};
use sheng_runtime::config::{check_request, SessionPlan};
use sheng_runtime::provider::{SessionError, SpeechClient, TextTranslator};
use sheng_runtime::services::alibaba::live_translate;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use url::Url;

const SYSTEM: u32 = 0;
/// 100 ms of 16 kHz PCM16.
const CHUNK: [u8; 3200] = [0; 3200];
const WAIT: Duration = Duration::from_secs(5);
const EVENTS: &str = include_str!("../../sheng-runtime/tests/fixtures/alibaba/live_events.jsonl");

type Ws = WebSocketStream<tokio::net::TcpStream>;

fn live_request() -> Value {
    start_request("system", "alibabaCloud", "simultaneous", "-", "auto", "en")
}

/// The real Alibaba client from the plan, pointed at the loopback server.
struct LoopbackLive {
    endpoint: Url,
}

impl ClientFactory for LoopbackLive {
    fn speech(
        &self,
        plan: &SessionPlan,
        _source: Source,
    ) -> Result<Box<dyn SpeechClient>, SessionError> {
        live_translate(plan, self.endpoint.clone())
    }

    fn translator(&self, _plan: &SessionPlan) -> Result<Arc<dyn TextTranslator>, SessionError> {
        Err(SessionError::ProviderConfigurationFailed)
    }
}

async fn next_json(ws: &mut Ws) -> Value {
    loop {
        match tokio::time::timeout(WAIT, ws.next()).await.unwrap() {
            Some(Ok(Message::Text(text))) => return serde_json::from_str(text.as_str()).unwrap(),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            other => panic!("expected a text message, got {other:?}"),
        }
    }
}

async fn send(ws: &mut Ws, text: &str) {
    ws.send(Message::Text(text.to_owned().into()))
        .await
        .unwrap();
}

/// Handshake, one pair after the first audio, `session.finished` after `session.finish`.
async fn realtime_script(mut ws: Ws) {
    send(&mut ws, r#"{"type": "session.created", "session": {}}"#).await;
    let update = next_json(&mut ws).await;
    assert_eq!(update["type"], "session.update");
    assert_eq!(update["session"]["translation"]["language"], "en");
    send(&mut ws, r#"{"type": "session.updated", "session": {}}"#).await;
    let mut message = next_json(&mut ws).await;
    assert_eq!(message["type"], "input_audio_buffer.append");
    for line in EVENTS.lines() {
        send(&mut ws, line).await;
    }
    while message["type"] != "session.finish" {
        message = next_json(&mut ws).await;
    }
    send(&mut ws, r#"{"type": "session.finished"}"#).await;
    while let Ok(Some(Ok(message))) = tokio::time::timeout(WAIT, ws.next()).await {
        if message.is_close() {
            break;
        }
    }
}

/// Starts the loopback server on `runtime`; returns its realtime URL.
fn loopback_server(runtime: &tokio::runtime::Runtime) -> Url {
    let listener = runtime.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    runtime.spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        realtime_script(ws).await;
    });
    Url::parse(&format!(
        "ws://127.0.0.1:{port}/api-ws/v1/realtime?model=test"
    ))
    .unwrap()
}

#[test]
fn live_translation_session_listens_and_confirms_an_original_translation_pair() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let endpoint = loopback_server(&runtime);
    let handle = sheng_engine_create();
    assert!(sheng_engine::testing::set_factory(
        handle,
        Arc::new(LoopbackLive { endpoint })
    ));

    assert_eq!(start(handle, &live_request()), json!({"ok": true}));
    wait_for(handle, |s| status(s) == "listening");
    assert_eq!(push(handle, SYSTEM, &CHUNK), 0);
    let shown = wait_for(handle, |s| !s["recent"].as_array().unwrap().is_empty());
    sheng_engine_stop(handle);
    let done = wait_settled(handle);

    assert_eq!(shown["recent"][0]["original"], "Guten Morgen.");
    assert_eq!(shown["recent"][0]["translation"], "Good morning");
    assert_eq!(status(&done), "idle");
    assert_eq!(done["error"], Value::Null);
    let captions = drain(handle);
    assert_eq!(captions.len(), 1);
    assert_eq!(captions[0]["original"], "Guten Morgen.");
    assert_eq!(captions[0]["translation"], "Good morning");
    assert_eq!(captions[0]["source"], "system");
    sheng_engine_destroy(handle);
}

#[test]
fn service_factory_builds_a_live_translation_client_for_the_opened_route() {
    let plan = check_request(&live_request().to_string()).unwrap();

    assert!(ServiceFactory.speech(&plan, Source::System).is_ok());
    assert_eq!(
        ServiceFactory.translator(&plan).err(),
        Some(SessionError::ProviderConfigurationFailed)
    );
}
