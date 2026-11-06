//! One source's lane: speech client, send loop, heartbeat, caption events and, for
//! recognizing routes, the text translation pipeline (snapshot.md, audio.md §2).

mod events;
mod feed;
mod finish;

use std::future::pending;
use std::time::Duration;

use sheng_core::align::DualStreamAligner;
use sheng_core::language::needs_translation;
use sheng_core::{Source, UtteranceId};
use sheng_runtime::audio::LaneReceiver;
use sheng_runtime::config::route::{SpeechRoute, TextRoute};
use sheng_runtime::config::SessionPlan;
use sheng_runtime::pipeline::{PipelineConfig, PipelineInput, PipelineOutput, TranslationPipeline};
use sheng_runtime::provider::{EventSink, OverflowFlag, ProviderEvent, SessionError, SpeechClient};
use sheng_runtime::transport::CONNECT_TIMEOUT;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::client::{SharedClient, HEARTBEAT_INTERVAL};
use super::context::SessionContext;
use super::state::Phase;

/// Time-dependent display state is checked at least this often.
const TICK: Duration = Duration::from_secs(1);

struct Pipeline {
    input: TranslationPipeline,
    output: mpsc::Receiver<PipelineOutput>,
    task: JoinHandle<()>,
    finished: bool,
    /// Newest draft the full input queue did not take yet; it is never queued twice.
    unsent_draft: Option<PipelineInput>,
    /// Newest draft shown for the current sentence, until its final.
    shown_draft: Option<(Option<UtteranceId>, String)>,
}

pub(super) struct Lane {
    ctx: SessionContext,
    source: Source,
    client: SharedClient,
    events: mpsc::Receiver<ProviderEvent>,
    overflow: OverflowFlag,
    pipeline: Option<Pipeline>,
    aligner: DualStreamAligner,
    origin: Instant,
    phase: watch::Receiver<Phase>,
    ended: bool,
    draining: bool,
}

fn needs_pipeline(plan: &SessionPlan) -> bool {
    matches!(
        plan.text_route,
        TextRoute::Independent(_) | TextRoute::QwenMt(_)
    ) && needs_translation(&plan.target_language)
}

/// Waits until the phase satisfies `wanted`; a closed channel counts as aborted.
async fn until(phase: &mut watch::Receiver<Phase>, wanted: fn(Phase) -> bool) -> Phase {
    loop {
        let current = *phase.borrow_and_update();
        if wanted(current) {
            return current;
        }
        if phase.changed().await.is_err() {
            return Phase::Aborted;
        }
    }
}

fn ends_running(phase: Phase) -> bool {
    matches!(phase, Phase::Draining | Phase::Aborted)
}

fn sender_result(
    joined: Result<Result<(), SessionError>, tokio::task::JoinError>,
) -> Result<(), SessionError> {
    joined.unwrap_or(Err(SessionError::TransportError))
}

async fn next_output(pipeline: &mut Option<Pipeline>) -> Option<PipelineOutput> {
    match pipeline {
        Some(p) if !p.finished => p.output.recv().await,
        _ => pending().await,
    }
}

/// A full provider event queue stops the service reader, which then shows up as a closed
/// channel or a failed send, both reported as `transport_error`; the overflow is the real
/// cause (snapshot.md). Specific codes such as Apple's own backlog code are kept.
fn overflow_or(overflow: &OverflowFlag, error: SessionError) -> SessionError {
    if error == SessionError::TransportError && overflow.is_set() {
        SessionError::ProviderEventBacklogOverflow
    } else {
        error
    }
}

/// Runs the lane to its end; a failure fails the whole session.
pub(super) async fn run(ctx: SessionContext, source: Source, receiver: LaneReceiver) {
    if let Err(error) = run_lane(&ctx, source, receiver).await {
        ctx.fail(error);
    }
}

async fn run_lane(
    ctx: &SessionContext,
    source: Source,
    receiver: LaneReceiver,
) -> Result<(), SessionError> {
    let client = SharedClient::new(ctx.factory.speech(&ctx.plan, source)?);
    let pipeline = if needs_pipeline(&ctx.plan) {
        let translator = ctx.factory.translator(&ctx.plan)?;
        let config = PipelineConfig {
            source_language: ctx.plan.source_language.clone(),
            target_language: ctx.plan.target_language.clone(),
        };
        let (input, output, task) = TranslationPipeline::spawn(translator, config);
        Some(Pipeline {
            input,
            output,
            task,
            finished: false,
            unsent_draft: None,
            shown_draft: None,
        })
    } else {
        None
    };
    let (sink, events) = EventSink::channel();
    let mut lane = Lane {
        ctx: ctx.clone(),
        source,
        client,
        overflow: sink.overflow_flag(),
        events,
        pipeline,
        aligner: DualStreamAligner::new(),
        origin: Instant::now(),
        phase: ctx.phase.clone(),
        ended: false,
        draining: false,
    };
    let result = lane
        .connect_and_run(sink, receiver)
        .await
        .map_err(|error| lane.root_cause(error));
    // A retryable failure: the app reconnects, so the sentence in progress is kept.
    let result = match result {
        Err(SessionError::TransportError) => lane
            .commit_unconfirmed()
            .and(Err(SessionError::TransportError)),
        other => other,
    };
    lane.close().await;
    result
}

impl Lane {
    async fn connect_and_run(
        &mut self,
        sink: EventSink,
        receiver: LaneReceiver,
    ) -> Result<(), SessionError> {
        let mut connecting = self.client.clone();
        let connected = tokio::select! {
            result = tokio::time::timeout(CONNECT_TIMEOUT, connecting.connect(sink)) => result,
            _ = until(&mut self.phase, ends_running) => return Ok(()),
        };
        match connected {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error.speech_session_error()),
            Err(_) => return Err(SessionError::TransportError),
        }
        let mut sending = self.client.clone();
        let mut receiver = receiver;
        let sender = tokio::spawn(async move { receiver.run(&mut sending).await });
        let heartbeat = tokio::spawn(heartbeat(
            self.ctx.clone(),
            self.source,
            self.client.clone(),
            self.overflow.clone(),
        ));
        let result = self.run_until_stop(sender).await;
        heartbeat.abort();
        result
    }

    async fn run_until_stop(
        &mut self,
        sender: JoinHandle<Result<(), SessionError>>,
    ) -> Result<(), SessionError> {
        let mut sender = Some(sender);
        let mut tick = tokio::time::interval(TICK);
        let phase = loop {
            tokio::select! {
                event = self.events.recv() => self.on_event(event).await?,
                output = next_output(&mut self.pipeline) => self.on_output(output)?,
                joined = async { sender.as_mut().expect("guarded").await }, if sender.is_some() => {
                    sender = None;
                    sender_result(joined)?;
                }
                _ = tick.tick() => self.tick()?,
                phase = until(&mut self.phase, ends_running) => break phase,
            }
        };
        if phase == Phase::Aborted {
            if let Some(sender) = sender {
                sender.abort();
            }
            return Ok(());
        }
        self.draining = true;
        self.drain_and_finish(sender).await
    }

    fn root_cause(&self, error: SessionError) -> SessionError {
        overflow_or(&self.overflow, error)
    }

    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    /// Releases everything the lane holds; safe to call in any state.
    async fn close(&mut self) {
        if let Some(pipeline) = &self.pipeline {
            pipeline.task.abort();
        }
        self.client.disconnect().await;
        self.ctx
            .lane(self.source, |lane| lane.translation_pending = false);
    }
}

/// Periodic heartbeat; the round trip is the API latency except for local recognition.
async fn heartbeat(
    ctx: SessionContext,
    source: Source,
    client: SharedClient,
    overflow: OverflowFlag,
) {
    let record = ctx.plan.route != SpeechRoute::AppleSpeech;
    loop {
        tokio::time::sleep(HEARTBEAT_INTERVAL).await;
        match client.heartbeat_once().await {
            Ok(round_trip) if record => {
                ctx.lane(source, |lane| lane.api_latency = Some(round_trip))
            }
            Ok(_) => {}
            Err(_) => {
                ctx.fail(overflow_or(&overflow, SessionError::TransportError));
                return;
            }
        }
    }
}
