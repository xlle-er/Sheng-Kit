//! Translation pipeline against a scripted local text service, on a paused clock
//! (translation-pipeline.md §5).

use std::sync::Arc;
use std::time::Duration;

use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::pipeline::{
    PipelineConfig, PipelineInput, PipelineOutput, RecoveryReason, TranslationPipeline,
};
use sheng_runtime::provider::scripted::{Reply, ScriptedTranslator};
use sheng_runtime::provider::{ProviderError, SessionError};
use tokio::sync::mpsc::Receiver;
use tokio::time::Instant;

fn start(translator: Arc<ScriptedTranslator>) -> (TranslationPipeline, Receiver<PipelineOutput>) {
    let config = PipelineConfig {
        source_language: "en".into(),
        target_language: "zh".into(),
    };
    let (pipeline, output, _task) = TranslationPipeline::spawn(translator, config);
    (pipeline, output)
}

fn id(n: u64) -> Option<UtteranceId> {
    Some(UtteranceId::Number(n))
}

fn draft(n: u64, text: &str) -> PipelineInput {
    PipelineInput::Draft {
        utterance: id(n),
        text: text.into(),
    }
}

fn final_(n: u64, text: &str) -> PipelineInput {
    PipelineInput::Final {
        utterance: id(n),
        text: text.into(),
    }
}

/// Collects outputs until the pipeline stops or `limit` passes.
async fn collect(output: &mut Receiver<PipelineOutput>, limit: Duration) -> Vec<PipelineOutput> {
    let deadline = Instant::now() + limit;
    let mut seen = Vec::new();
    while let Ok(Some(item)) = tokio::time::timeout_at(deadline, output.recv()).await {
        let done = matches!(item, PipelineOutput::Finished | PipelineOutput::Failed(_));
        seen.push(item);
        if done {
            break;
        }
    }
    seen
}

fn confirmed(outputs: &[PipelineOutput]) -> Vec<(String, String)> {
    outputs
        .iter()
        .filter_map(|o| match o {
            PipelineOutput::Caption(CaptionEvent::ConfirmedPair {
                original,
                translation,
                ..
            }) => Some((original.clone(), translation.clone())),
            _ => None,
        })
        .collect()
}

fn previews(outputs: &[PipelineOutput]) -> usize {
    outputs
        .iter()
        .filter(|o| matches!(o, PipelineOutput::Caption(CaptionEvent::PreviewPair { .. })))
        .count()
}

fn failure(outputs: &[PipelineOutput]) -> Option<SessionError> {
    outputs.iter().find_map(|o| match o {
        PipelineOutput::Failed(error) => Some(*error),
        _ => None,
    })
}

#[tokio::test(start_paused = true)]
async fn previews_translate_only_the_latest_draft_and_are_throttled() {
    let translator = ScriptedTranslator::new([]);
    let (pipeline, mut output) = start(Arc::clone(&translator));
    let words: Vec<String> = (1..=20).map(|n| format!("word{n}")).collect();

    for n in 1..=20 {
        pipeline.send(draft(1, &words[..n].join(" "))).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let outputs = collect(&mut output, Duration::from_secs(10)).await;

    let requests = translator.requests();
    assert!(
        (1..=3).contains(&requests.len()),
        "{} requests",
        requests.len()
    );
    assert_eq!(requests.last().unwrap().text, words.join(" "));
    assert_eq!(previews(&outputs), requests.len());
    assert!(confirmed(&outputs).is_empty());
}

#[tokio::test(start_paused = true)]
async fn finals_confirm_in_order() {
    let translator = ScriptedTranslator::new([]);
    let (pipeline, mut output) = start(Arc::clone(&translator));

    for (n, text) in [(1, "One."), (2, "Two."), (3, "Three.")] {
        pipeline.send(final_(n, text)).await;
    }
    pipeline.send(PipelineInput::Finish).await;
    let outputs = collect(&mut output, Duration::from_secs(10)).await;

    let pairs = confirmed(&outputs);
    let originals: Vec<&str> = pairs.iter().map(|(o, _)| o.as_str()).collect();
    assert_eq!(originals, ["One.", "Two.", "Three."]);
    assert_eq!(pairs[0].1, "tr:One.");
    assert_eq!(outputs.last(), Some(&PipelineOutput::Finished));
}

#[tokio::test(start_paused = true)]
async fn fourth_waiting_final_overflows_the_backlog() {
    let slow = Reply::Delayed(Duration::from_secs(5), "tr:".into());
    let translator = ScriptedTranslator::new([slow]);
    let (pipeline, mut output) = start(translator);
    pipeline.send(final_(1, "In flight.")).await;
    tokio::time::sleep(Duration::from_millis(10)).await;

    for n in 2..=5 {
        pipeline.send(final_(n, "Waiting.")).await;
    }
    let outputs = collect(&mut output, Duration::from_secs(2)).await;

    assert_eq!(
        failure(&outputs),
        Some(SessionError::TranslationBacklogOverflow)
    );
    assert_eq!(
        SessionError::TranslationBacklogOverflow.code(),
        "translation_backlog_overflow"
    );
}

#[tokio::test(start_paused = true)]
async fn rate_limit_reports_recovery_pauses_previews_and_retries_the_final() {
    let limited = Reply::Fail(ProviderError::RateLimited { retry_after: None });
    let translator = ScriptedTranslator::new([limited]);
    let (pipeline, mut output) = start(Arc::clone(&translator));

    pipeline.send(final_(1, "Hello.")).await;
    let first = collect(&mut output, Duration::from_secs(3)).await;
    pipeline
        .send(draft(2, "a new draft that would normally preview"))
        .await;
    let during_cooldown = collect(&mut output, Duration::from_secs(20)).await;

    let recovery = first.iter().find_map(|o| match o {
        PipelineOutput::Recovery(Some(r)) => Some(*r),
        _ => None,
    });
    let recovery = recovery.expect("recovery reported");
    assert_eq!(recovery.reason, RecoveryReason::RateLimited);
    assert!(recovery.retry_scheduled);
    assert_eq!(
        confirmed(&first),
        [("Hello.".to_owned(), "tr:Hello.".to_owned())]
    );
    assert!(first.contains(&PipelineOutput::Recovery(None)));
    assert_eq!(previews(&during_cooldown), 0);
    assert_eq!(translator.requests().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn authentication_failure_ends_the_session_without_retrying() {
    let translator = ScriptedTranslator::new([Reply::Fail(ProviderError::Authentication)]);
    let (pipeline, mut output) = start(Arc::clone(&translator));

    pipeline.send(final_(1, "Hello.")).await;
    let outputs = collect(&mut output, Duration::from_secs(10)).await;

    assert_eq!(
        failure(&outputs),
        Some(SessionError::CredentialAuthenticationFailed)
    );
    assert_eq!(translator.requests().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn stop_without_a_final_supplements_the_last_draft_once() {
    let translator = ScriptedTranslator::new([]);
    let (pipeline, mut output) = start(Arc::clone(&translator));
    pipeline.send(draft(1, "unfinished thought")).await;

    pipeline.send(PipelineInput::Finish).await;
    pipeline.send(PipelineInput::Finish).await;
    let outputs = collect(&mut output, Duration::from_secs(10)).await;

    assert_eq!(
        confirmed(&outputs),
        [(
            "unfinished thought".to_owned(),
            "tr:unfinished thought".to_owned()
        )]
    );
    let finals = translator
        .requests()
        .iter()
        .filter(|r| r.text == "unfinished thought")
        .count();
    assert!(finals <= 2, "preview plus at most one final");
    assert_eq!(outputs.last(), Some(&PipelineOutput::Finished));
}

#[tokio::test(start_paused = true)]
async fn final_translation_gives_up_after_forty_five_seconds() {
    let translator =
        ScriptedTranslator::new([Reply::Delayed(Duration::from_secs(120), "tr:".into())]);
    let (pipeline, mut output) = start(translator);
    let started = Instant::now();

    pipeline.send(final_(1, "Never answered.")).await;
    let outputs = collect(&mut output, Duration::from_secs(120)).await;

    assert_eq!(
        failure(&outputs),
        Some(SessionError::TranslationTemporarilyUnavailable)
    );
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_secs(45) && elapsed < Duration::from_secs(46),
        "{elapsed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn temporary_failures_retry_then_fail_after_three_attempts() {
    let unavailable = || Reply::Fail(ProviderError::Unavailable { retry_after: None });
    let translator = ScriptedTranslator::new([unavailable(), unavailable(), unavailable()]);
    let (pipeline, mut output) = start(Arc::clone(&translator));

    pipeline.send(final_(1, "Flaky.")).await;
    let outputs = collect(&mut output, Duration::from_secs(60)).await;

    assert_eq!(
        failure(&outputs),
        Some(SessionError::TranslationTemporarilyUnavailable)
    );
    assert_eq!(translator.requests().len(), 3);
}
