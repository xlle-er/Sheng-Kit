//! Explicit connection checks (`sheng_probe`): one speech connection or one text request.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use sheng_core::Source;
use sheng_runtime::config::{SessionPlan, TextPlan};
use sheng_runtime::provider::{EventSink, ProviderError, TranslationRequest};
use tokio::time::Instant;

use crate::factory::ClientFactory;
use crate::operations::Outcome;

/// Stays below the 20 second limit of the Swift caller.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const PROBE_TEXT: &str = "Hello";

fn reply(started: Instant, result: Result<(), &'static str>) -> Value {
    let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match result {
        Ok(()) => json!({"service": "available", "elapsedMs": elapsed}),
        Err(reason) => json!({"service": "unavailable", "elapsedMs": elapsed, "reason": reason}),
    }
}

fn reason(error: ProviderError) -> &'static str {
    error.speech_session_error().code()
}

pub async fn speech(factory: Arc<dyn ClientFactory>, plan: SessionPlan) -> Outcome {
    let mut client = factory
        .speech(&plan, Source::System)
        .map_err(|error| error.code())?;
    let started = Instant::now();
    let (sink, _events) = EventSink::channel();
    let result = match tokio::time::timeout(PROBE_TIMEOUT, client.connect(sink)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(reason(error)),
        Err(_) => Err("transport_error"),
    };
    client.disconnect().await;
    Ok(reply(started, result))
}

pub async fn text(factory: Arc<dyn ClientFactory>, plan: SessionPlan) -> Outcome {
    if matches!(plan.text, TextPlan::None) {
        return Err("text_translation_not_configured");
    }
    let translator = factory.translator(&plan).map_err(|error| error.code())?;
    let request = TranslationRequest {
        text: PROBE_TEXT.to_owned(),
        source_language: plan.source_language.clone(),
        target_language: plan.target_language.clone(),
    };
    let started = Instant::now();
    let result = match tokio::time::timeout(PROBE_TIMEOUT, translator.translate(request)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(reason(error)),
        Err(_) => Err("transport_error"),
    };
    Ok(reply(started, result))
}
