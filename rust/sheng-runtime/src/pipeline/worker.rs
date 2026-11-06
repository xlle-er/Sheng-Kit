//! The pipeline task: one request in flight at a time; finals before previews.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use sheng_core::pacing::{request_cost, TranslationPacer};
use sheng_core::policy::{admit_final, FinalBudget, PREVIEW_TIMEOUT};
use sheng_core::text::{is_meaningful, within_limit};
use sheng_core::UtteranceId;
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::{PipelineConfig, PipelineInput, PipelineOutput, FINISH_DRAIN};
use crate::provider::{BoxFuture, ProviderError, SessionError, TextTranslator, TranslationRequest};

mod results;

/// Wake-up interval while work waits for pacing or a retry.
const TICK: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Sentence {
    utterance: Option<UtteranceId>,
    text: String,
}

struct FinalJob {
    sentence: Sentence,
    budget: FinalBudget,
    ready_at: Instant,
}

enum Kind {
    Preview(Sentence),
    Final,
}

/// `None` result means the request ran out of time.
type Pending = BoxFuture<'static, Option<Result<String, ProviderError>>>;

struct InFlight {
    kind: Kind,
    started: Instant,
    result: Pending,
}

enum Step {
    Continue,
    Stop,
}

pub(super) struct Worker {
    translator: Arc<dyn TextTranslator>,
    config: PipelineConfig,
    output: mpsc::Sender<PipelineOutput>,
    pacer: TranslationPacer,
    origin: Instant,
    latest_draft: Option<Sentence>,
    last_preview: Option<Option<UtteranceId>>,
    supplement: Option<Sentence>,
    finals: VecDeque<FinalJob>,
    in_flight: Option<InFlight>,
    finish_by: Option<Instant>,
    pending: bool,
    recovering: bool,
}

fn usable(text: &str) -> bool {
    within_limit(text) && is_meaningful(text)
}

impl Worker {
    pub(super) fn new(
        translator: Arc<dyn TextTranslator>,
        config: PipelineConfig,
        output: mpsc::Sender<PipelineOutput>,
    ) -> Self {
        Self {
            translator,
            config,
            output,
            pacer: TranslationPacer::default(),
            origin: Instant::now(),
            latest_draft: None,
            last_preview: None,
            supplement: None,
            finals: VecDeque::new(),
            in_flight: None,
            finish_by: None,
            pending: false,
            recovering: false,
        }
    }

    pub(super) async fn run(mut self, mut input: mpsc::Receiver<PipelineInput>) {
        loop {
            self.start_next();
            if self.emit_pending().await.is_err() {
                return;
            }
            let drained = self.finals.is_empty()
                && !matches!(
                    self.in_flight,
                    Some(InFlight {
                        kind: Kind::Final,
                        ..
                    })
                );
            if self.finish_by.is_some() && drained {
                let _ = self.output.send(PipelineOutput::Finished).await;
                return;
            }
            let step = tokio::select! {
                command = input.recv(), if self.finish_by.is_none() => {
                    self.handle_input(command.unwrap_or(PipelineInput::Finish))
                }
                result = async { (&mut self.in_flight.as_mut().expect("guarded").result).await }, if self.in_flight.is_some() => {
                    let done = self.in_flight.take().expect("guarded");
                    self.handle_result(done.kind, done.started, result).await
                }
                _ = tokio::time::sleep(TICK), if self.has_waiting_work() => Step::Continue,
                _ = tokio::time::sleep_until(self.finish_by.unwrap_or_else(Instant::now)), if self.finish_by.is_some() => {
                    let _ = self.output.send(PipelineOutput::Finished).await;
                    Step::Stop
                }
            };
            match step {
                Step::Continue => {}
                Step::Stop => return,
            }
        }
    }

    fn now(&self) -> Duration {
        Instant::now() - self.origin
    }

    fn has_waiting_work(&self) -> bool {
        self.in_flight.is_none() && (!self.finals.is_empty() || self.latest_draft.is_some())
    }

    /// The returned future owns a sender clone, so it does not borrow the worker across
    /// an await (the in-flight request future is not `Sync`).
    fn emit(
        &self,
        output: PipelineOutput,
    ) -> impl Future<Output = Result<(), ()>> + Send + 'static {
        let sender = self.output.clone();
        async move { sender.send(output).await.map_err(|_| ()) }
    }

    async fn emit_pending(&mut self) -> Result<(), ()> {
        let pending = self.in_flight.is_some() || !self.finals.is_empty();
        if pending != self.pending {
            self.pending = pending;
            self.emit(PipelineOutput::Pending(pending)).await?;
        }
        Ok(())
    }

    fn handle_input(&mut self, command: PipelineInput) -> Step {
        match command {
            PipelineInput::Draft { utterance, text } if usable(&text) => {
                let draft = Sentence { utterance, text };
                self.supplement = Some(draft.clone());
                self.latest_draft = Some(draft);
            }
            PipelineInput::Draft { .. } => {}
            PipelineInput::Final { utterance, text } if usable(&text) => {
                self.supplement = None;
                if self
                    .latest_draft
                    .as_ref()
                    .is_some_and(|d| d.utterance == utterance)
                {
                    self.latest_draft = None;
                }
                return self.enqueue_final(Sentence { utterance, text });
            }
            PipelineInput::Final { .. } => {}
            PipelineInput::Finish => {
                self.finish_by = Some(Instant::now() + FINISH_DRAIN);
                self.latest_draft = None;
                if let Some(last) = self.supplement.take() {
                    return self.enqueue_final(last);
                }
            }
        }
        Step::Continue
    }

    fn enqueue_final(&mut self, sentence: Sentence) -> Step {
        let in_flight = usize::from(matches!(
            self.in_flight,
            Some(InFlight {
                kind: Kind::Final,
                ..
            })
        ));
        if admit_final(self.finals.len() - in_flight).is_err() {
            let _ = self.output.try_send(PipelineOutput::Failed(
                SessionError::TranslationBacklogOverflow,
            ));
            return Step::Stop;
        }
        self.finals.push_back(FinalJob {
            sentence,
            budget: FinalBudget::new(self.now()),
            ready_at: Instant::now(),
        });
        Step::Continue
    }

    fn request(&self, text: &str, limit: Duration) -> Pending {
        let translator = Arc::clone(&self.translator);
        let request = TranslationRequest {
            text: text.to_owned(),
            source_language: self.config.source_language.clone(),
            target_language: self.config.target_language.clone(),
        };
        Box::pin(async move {
            tokio::time::timeout(limit, translator.translate(request))
                .await
                .ok()
        })
    }

    fn start_next(&mut self) {
        if self.in_flight.is_some() {
            return;
        }
        let now = self.now();
        if let Some(head) = self.finals.front_mut() {
            if Instant::now() >= head.ready_at && self.pacer.may_send_final(now) {
                head.budget.record_attempt();
                let limit = head.budget.deadline().saturating_sub(now);
                let text = head.sentence.text.clone();
                self.pacer.record_final(now, request_cost(&text));
                let result = self.request(&text, limit);
                self.in_flight = Some(InFlight {
                    kind: Kind::Final,
                    started: Instant::now(),
                    result,
                });
            }
            return;
        }
        let Some(draft) = &self.latest_draft else {
            return;
        };
        let chars = draft.text.chars().count();
        let same = self.last_preview.as_ref() == Some(&draft.utterance);
        if !self.pacer.may_send_preview(now, chars, same, false) {
            return;
        }
        let draft = self.latest_draft.take().expect("checked above");
        self.pacer
            .record_preview(now, chars, request_cost(&draft.text));
        self.last_preview = Some(draft.utterance.clone());
        let result = self.request(&draft.text, PREVIEW_TIMEOUT);
        self.in_flight = Some(InFlight {
            kind: Kind::Preview(draft),
            started: Instant::now(),
            result,
        });
    }

    fn fail(&self, error: SessionError) -> impl Future<Output = Step> + Send + 'static {
        let sent = self.emit(PipelineOutput::Failed(error));
        async move {
            let _ = sent.await;
            Step::Stop
        }
    }
}
