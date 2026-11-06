//! State of one engine handle: status machine, captions and diagnostics (snapshot.md).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sheng_core::{
    Caption, CaptionEvent, CaptionReducer, ReducerConfig, Source, CONFIRMED_QUEUE_LIMIT,
    RECENT_LIMIT,
};
use sheng_runtime::audio::echo::EchoReport;
use sheng_runtime::audio::AudioRouter;
use sheng_runtime::config::SessionPlan;
use sheng_runtime::pipeline::Recovery;
use sheng_runtime::provider::SessionError;
use tokio::sync::watch;

/// Sample rate reported before any session ran.
pub const DEFAULT_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    Connecting,
    Listening,
    Stopping,
    Failed,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Connecting => "connecting",
            Self::Listening => "listening",
            Self::Stopping => "stopping",
            Self::Failed => "failed",
        }
    }

    fn is_active(self) -> bool {
        matches!(self, Self::Connecting | Self::Listening | Self::Stopping)
    }
}

/// What the session tasks should be doing; sent by the ABI calls and failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Running,
    /// `stop` was called; the supervisor stops audio input.
    Stopping,
    /// Audio input is closed; lanes drain, finish and wait for the last results.
    Draining,
    /// Failure or destroy; everything stops now.
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyKind {
    Request,
    Follow,
}

/// One `diagnostics[]` entry; only measured values.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneDiagnostics {
    pub source: Source,
    pub api_latency: Option<Duration>,
    pub translation_latency: Option<(Duration, LatencyKind)>,
    pub translation_pending: bool,
    pub recovery: Option<Recovery>,
}

impl LaneDiagnostics {
    fn new(source: Source) -> Self {
        Self {
            source,
            api_latency: None,
            translation_latency: None,
            translation_pending: false,
            recovery: None,
        }
    }
}

pub struct EngineState {
    pub run_id: String,
    pub generation: u64,
    pub revision: u64,
    pub status: Status,
    pub error: Option<&'static str>,
    pub sample_rate: u32,
    /// A session's tasks are still running (snapshot `settled` is the negation).
    pub busy: bool,
    pub destroyed: bool,
    reducer: Option<CaptionReducer>,
    pub recent: VecDeque<Caption>,
    pub pending: VecDeque<Caption>,
    next_seq: u64,
    pub sources: Vec<Source>,
    connected: Vec<Source>,
    pub diagnostics: Vec<LaneDiagnostics>,
    pub router: Option<Arc<AudioRouter>>,
    pub echo: Option<EchoReport>,
    pub control: Option<watch::Sender<Phase>>,
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl EngineState {
    pub fn new() -> Self {
        Self {
            run_id: uuid::Uuid::new_v4().to_string(),
            generation: 0,
            revision: 0,
            status: Status::Idle,
            error: None,
            sample_rate: DEFAULT_SAMPLE_RATE,
            busy: false,
            destroyed: false,
            reducer: None,
            recent: VecDeque::new(),
            pending: VecDeque::new(),
            next_seq: 0,
            sources: Vec::new(),
            connected: Vec::new(),
            diagnostics: Vec::new(),
            router: None,
            echo: None,
            control: None,
        }
    }

    /// Running, stopping or a previous session still finishing.
    pub fn is_running(&self) -> bool {
        self.busy || self.status.is_active()
    }

    pub fn accepts_audio(&self) -> bool {
        matches!(self.status, Status::Connecting | Status::Listening)
    }

    fn touch(&mut self) {
        self.revision += 1;
    }

    fn signal(&self, phase: Phase) {
        if let Some(control) = &self.control {
            control.send_replace(phase);
        }
    }

    /// Starts a new generation for a validated plan.
    pub fn begin(
        &mut self,
        plan: &SessionPlan,
        sources: Vec<Source>,
        router: Arc<AudioRouter>,
        control: watch::Sender<Phase>,
    ) -> u64 {
        self.generation += 1;
        self.status = Status::Connecting;
        self.error = None;
        self.sample_rate = plan.sample_rate;
        self.busy = true;
        self.reducer = Some(CaptionReducer::new(ReducerConfig {
            run_id: self.run_id.clone(),
            generation: self.generation,
            sources: sources.clone(),
            source_language: plan.source_language.clone(),
            target_language: plan.target_language.clone(),
        }));
        self.diagnostics = sources.iter().copied().map(LaneDiagnostics::new).collect();
        self.sources = sources;
        self.connected.clear();
        self.echo = router.echo_report();
        self.router = Some(router);
        self.control = Some(control);
        self.touch();
        self.generation
    }

    /// Fails the running session once; later failures keep the first code.
    pub fn fail(&mut self, error: SessionError) {
        if !self.status.is_active() {
            return;
        }
        self.status = Status::Failed;
        self.error = Some(error.code());
        self.signal(Phase::Aborted);
        self.touch();
    }

    pub fn request_stop(&mut self) {
        if !matches!(self.status, Status::Connecting | Status::Listening) {
            return;
        }
        self.status = Status::Stopping;
        self.signal(Phase::Stopping);
        self.touch();
    }

    /// Audio input of a stopping session is closed; lanes may drain now.
    pub fn input_closed(&mut self) {
        if self.status == Status::Stopping {
            self.signal(Phase::Draining);
        }
    }

    pub fn connected(&mut self, source: Source) {
        if !self.connected.contains(&source) {
            self.connected.push(source);
        }
        let all = self.sources.iter().all(|s| self.connected.contains(s));
        if all && self.status == Status::Connecting {
            self.status = Status::Listening;
            self.touch();
        }
    }

    /// All session tasks ended; a failed session stays failed until the next start.
    pub fn settle(&mut self) {
        self.busy = false;
        self.control = None;
        if self.status != Status::Failed {
            self.status = Status::Idle;
        }
        for lane in &mut self.diagnostics {
            lane.translation_pending = false;
        }
        self.refresh_echo();
        self.touch();
    }

    /// Applies a caption event while the session is active. Newly confirmed captions get
    /// handle-wide ids and enter `recent` and the pending queue.
    pub fn apply_caption(
        &mut self,
        source: Source,
        event: CaptionEvent,
    ) -> Result<(), SessionError> {
        if !self.status.is_active() {
            return Ok(());
        }
        let Some(reducer) = self.reducer.as_mut() else {
            return Ok(());
        };
        let changed = reducer
            .apply(source, event, epoch_ms())
            .map_err(|_| SessionError::CaptionConsumerBackpressure)?;
        self.take_confirmed(changed)
    }

    /// Confirms once what `source` recognized but has not confirmed (providers/README.md
    /// “意外断开”, “结束时不丢尾句”).
    pub fn commit_unconfirmed(&mut self, source: Source) -> Result<(), SessionError> {
        if !self.status.is_active() {
            return Ok(());
        }
        let Some(reducer) = self.reducer.as_mut() else {
            return Ok(());
        };
        let changed = reducer
            .commit_unconfirmed(source, epoch_ms())
            .map_err(|_| SessionError::CaptionConsumerBackpressure)?;
        self.take_confirmed(changed)
    }

    /// Moves newly confirmed captions into `recent` and the pending queue with handle-wide ids.
    fn take_confirmed(&mut self, mut changed: bool) -> Result<(), SessionError> {
        let Some(reducer) = self.reducer.as_mut() else {
            return Ok(());
        };
        let confirmed = reducer.drain_confirmed();
        for caption in confirmed {
            if self.pending.len() >= CONFIRMED_QUEUE_LIMIT {
                self.touch();
                return Err(SessionError::CaptionConsumerBackpressure);
            }
            let caption = Caption {
                id: format!("{}:{}:{}", self.run_id, self.generation, self.next_seq),
                ..caption
            };
            self.next_seq += 1;
            self.recent.push_back(caption.clone());
            while self.recent.len() > RECENT_LIMIT {
                self.recent.pop_front();
            }
            self.pending.push_back(caption);
            changed = true;
        }
        if changed {
            self.touch();
        }
        Ok(())
    }

    pub fn update_lane(&mut self, source: Source, change: impl FnOnce(&mut LaneDiagnostics)) {
        let Some(lane) = self.diagnostics.iter_mut().find(|l| l.source == source) else {
            return;
        };
        let before = lane.clone();
        change(lane);
        if *lane != before {
            self.touch();
        }
    }

    pub fn refresh_echo(&mut self) {
        let report = self.router.as_ref().and_then(|r| r.echo_report());
        if report != self.echo {
            self.echo = report;
            self.touch();
        }
    }

    /// User clear, only while nothing runs: empties `tracks` and `recent`.
    pub fn clear(&mut self) {
        if self.is_running() {
            return;
        }
        if self.reducer.is_some() || !self.recent.is_empty() {
            self.reducer = None;
            self.recent.clear();
            self.touch();
        }
    }

    pub fn drain(&mut self) -> Vec<Caption> {
        self.pending.drain(..).collect()
    }

    pub fn destroy(&mut self) {
        self.destroyed = true;
        self.generation += 1;
        self.signal(Phase::Aborted);
    }

    pub fn tracks(&self) -> Vec<sheng_core::TrackView> {
        self.reducer
            .as_ref()
            .map(CaptionReducer::tracks)
            .unwrap_or_default()
    }
}

impl Default for EngineState {
    fn default() -> Self {
        Self::new()
    }
}
