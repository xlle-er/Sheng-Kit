//! Handling of completed preview and final requests.

use sheng_core::backoff::retry_delay_with_hint;
use sheng_core::policy::{TranslationFailure, PREVIEW_COOLDOWN_AFTER_RATE_LIMIT};
use sheng_core::CaptionEvent;
use tokio::time::Instant;

use super::{usable, Kind, Sentence, Step, Worker};
use crate::pipeline::{PipelineOutput, Recovery, RecoveryReason};
use crate::provider::{translation_session_error, ProviderError, SessionError};

fn recovery_reason(failure: TranslationFailure) -> RecoveryReason {
    match failure {
        TranslationFailure::RateLimited => RecoveryReason::RateLimited,
        _ => RecoveryReason::TemporarilyUnavailable,
    }
}

impl Worker {
    pub(super) async fn handle_result(
        &mut self,
        kind: Kind,
        started: Instant,
        result: Option<Result<String, ProviderError>>,
    ) -> Step {
        match kind {
            Kind::Preview(sentence) => self.preview_done(sentence, started, result).await,
            Kind::Final => self.final_done(started, result).await,
        }
    }

    async fn preview_done(
        &mut self,
        sentence: Sentence,
        started: Instant,
        result: Option<Result<String, ProviderError>>,
    ) -> Step {
        match result {
            Some(Ok(translation)) if usable(&translation) => {
                let event = CaptionEvent::PreviewPair {
                    utterance: sentence.utterance,
                    original: sentence.text,
                    translation,
                };
                let sent = self.emit(PipelineOutput::Caption(event)).await;
                let latency = self.emit(PipelineOutput::Latency(started.elapsed())).await;
                if sent.and(latency).is_err() {
                    return Step::Stop;
                }
            }
            Some(Err(error)) => {
                let failure = error.translation_failure();
                if !failure.is_retryable() {
                    return self.fail(translation_session_error(failure)).await;
                }
                if failure == TranslationFailure::RateLimited {
                    self.pacer.record_rate_limited(self.now());
                    let recovery = Recovery {
                        reason: RecoveryReason::RateLimited,
                        retry_after_ms: PREVIEW_COOLDOWN_AFTER_RATE_LIMIT.as_millis() as u64,
                        retry_scheduled: false,
                    };
                    self.recovering = true;
                    if self
                        .emit(PipelineOutput::Recovery(Some(recovery)))
                        .await
                        .is_err()
                    {
                        return Step::Stop;
                    }
                }
            }
            _ => {}
        }
        Step::Continue
    }

    async fn final_done(
        &mut self,
        started: Instant,
        result: Option<Result<String, ProviderError>>,
    ) -> Step {
        let error = match result {
            Some(Ok(translation)) => return self.confirm(started, translation).await,
            Some(Err(error)) => error,
            None => {
                return self
                    .fail(SessionError::TranslationTemporarilyUnavailable)
                    .await
            }
        };
        let failure = error.translation_failure();
        if !failure.is_retryable() {
            return self.fail(translation_session_error(failure)).await;
        }
        if failure == TranslationFailure::RateLimited {
            self.pacer.record_rate_limited(self.now());
        }
        let now = self.now();
        let Some(head) = self.finals.front_mut() else {
            return Step::Continue;
        };
        let delay = retry_delay_with_hint(head.budget.attempts(), error.retry_after());
        let Some(delay) = delay.filter(|d| head.budget.may_attempt(now, *d)) else {
            return self.fail(translation_session_error(failure)).await;
        };
        head.ready_at = Instant::now() + delay;
        let recovery = Recovery {
            reason: recovery_reason(failure),
            retry_after_ms: delay.as_millis() as u64,
            retry_scheduled: true,
        };
        self.recovering = true;
        match self.emit(PipelineOutput::Recovery(Some(recovery))).await {
            Ok(()) => Step::Continue,
            Err(()) => Step::Stop,
        }
    }

    async fn confirm(&mut self, started: Instant, translation: String) -> Step {
        let Some(job) = self.finals.pop_front() else {
            return Step::Continue;
        };
        let event = CaptionEvent::ConfirmedPair {
            utterance: job.sentence.utterance,
            original: job.sentence.text,
            translation,
        };
        let mut outputs = vec![
            PipelineOutput::Caption(event),
            PipelineOutput::Latency(started.elapsed()),
        ];
        if std::mem::take(&mut self.recovering) {
            outputs.push(PipelineOutput::Recovery(None));
        }
        for output in outputs {
            if self.emit(output).await.is_err() {
                return Step::Stop;
            }
        }
        Step::Continue
    }
}
