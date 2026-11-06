//! A full provider event queue fails the session with `provider_event_backlog_overflow`
//! however the overflow surfaces in the lane (snapshot.md, step 5 fix 1).

mod common;

use common::*;
use sheng_core::Source;
use sheng_engine::abi::engine::sheng_engine_destroy;
use sheng_runtime::provider::scripted::{ScriptedSpeech, SpeechScript};
use sheng_runtime::provider::traits::PROVIDER_EVENT_QUEUE;
use sheng_runtime::provider::{EventSink, ProviderEvent, SessionError};

const SYSTEM: u32 = 0;
/// 100 ms of 16 kHz PCM16.
const CHUNK: [u8; 3200] = [0; 3200];

fn flood() -> Vec<ProviderEvent> {
    vec![ProviderEvent::TurnComplete; PROVIDER_EVENT_QUEUE + 8]
}

fn failed_with(script: SpeechScript, audio: bool) -> String {
    let (client, _) = ScriptedSpeech::new(script);
    let handle = handle_with(TestFactory::new(vec![(Source::System, Box::new(client))]));
    assert_eq!(start(handle, &translating_request("system"))["ok"], true);
    if audio {
        wait_for(handle, |s| status(s) == "listening");
        push(handle, SYSTEM, &CHUNK);
    }
    let failed = wait_for(handle, |s| s["settled"] == true && status(s) == "failed");
    sheng_engine_destroy(handle);
    failed["error"].as_str().unwrap_or_default().to_owned()
}

#[test]
fn overflow_while_connecting_is_reported_as_backlog_overflow() {
    let script = SpeechScript {
        on_connect: flood(),
        ..SpeechScript::default()
    };

    assert_eq!(
        failed_with(script, false),
        "provider_event_backlog_overflow"
    );
}

#[test]
fn overflow_from_a_client_that_keeps_its_sink_still_fails_the_session() {
    // The scripted client holds its sink, so the channel never closes; its failed send is
    // traced back to the overflow.
    let script = SpeechScript {
        on_audio: vec![(1, flood())],
        ..SpeechScript::default()
    };

    assert_eq!(failed_with(script, true), "provider_event_backlog_overflow");
}

#[test]
fn overflow_flag_is_set_once_and_stays_set() {
    let (sink, mut events) = EventSink::channel();
    let flag = sink.overflow_flag();
    assert!(!flag.is_set());
    for _ in 0..PROVIDER_EVENT_QUEUE {
        sink.emit(ProviderEvent::TurnComplete).unwrap();
    }

    let overflow = sink.emit(ProviderEvent::TurnComplete);
    events.try_recv().unwrap();
    sink.emit(ProviderEvent::TurnComplete).unwrap();

    assert_eq!(overflow, Err(SessionError::ProviderEventBacklogOverflow));
    assert!(flag.is_set());
    drop(sink);
    assert!(flag.is_set());
}
