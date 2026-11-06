//! One engine handle: starting, feeding, stopping and observing sessions.

mod client;
mod context;
mod lane;
mod snapshot;
pub mod state;
mod supervisor;

use std::sync::{Arc, Mutex, PoisonError, RwLock};

use serde_json::Value;
use sheng_core::{Caption, Source};
use sheng_runtime::audio::{AudioRouter, LaneReceivers, PushOutcome};
use sheng_runtime::config::SessionPlan;
use sheng_runtime::provider::SessionError;
use tokio::sync::watch;

use self::context::{lock, SessionContext, Shared};
use self::state::{EngineState, Phase};
use self::supervisor::{supervise, SessionTasks};
use crate::factory::{default_factory, ClientFactory};

/// Why `start` refused a validated plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
    AlreadyRunning,
    DrainRequired,
    EchoInitializationFailed,
    Unavailable,
}

impl StartError {
    pub fn code(self) -> &'static str {
        match self {
            Self::AlreadyRunning => "already_running",
            Self::DrainRequired => "drain_required",
            Self::EchoInitializationFailed => "echo_initialization_failed",
            // The shared runtime could not be created; nothing can run.
            Self::Unavailable => "provider_configuration_failed",
        }
    }
}

pub struct EngineHandle {
    state: Shared,
    factory: RwLock<Arc<dyn ClientFactory>>,
}

fn session_sources(receivers: LaneReceivers) -> Vec<(Source, sheng_runtime::audio::LaneReceiver)> {
    [
        (Source::System, receivers.system),
        (Source::Microphone, receivers.microphone),
    ]
    .into_iter()
    .filter_map(|(source, receiver)| receiver.map(|r| (source, r)))
    .collect()
}

impl EngineHandle {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(EngineState::new())),
            factory: RwLock::new(default_factory()),
        }
    }

    #[cfg(feature = "test-support")]
    pub fn set_factory(&self, factory: Arc<dyn ClientFactory>) {
        *self.factory.write().unwrap_or_else(PoisonError::into_inner) = factory;
    }

    /// Starts a session for a validated plan; never waits for any service.
    pub fn start(&self, plan: SessionPlan) -> Result<(), StartError> {
        let runtime = crate::runtime::handle().ok_or(StartError::Unavailable)?;
        let mut state = lock(&self.state);
        if state.is_running() {
            return Err(StartError::AlreadyRunning);
        }
        if !state.pending.is_empty() {
            return Err(StartError::DrainRequired);
        }
        let (router, receivers) = AudioRouter::new(plan.input, plan.sample_rate)
            .ok_or(StartError::EchoInitializationFailed)?;
        let router = Arc::new(router);
        let lanes = session_sources(receivers);
        let sources = lanes.iter().map(|(source, _)| *source).collect();
        let (control, phase) = watch::channel(Phase::Running);
        let generation = state.begin(&plan, sources, Arc::clone(&router), control);
        drop(state);
        let factory = self
            .factory
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let ctx = SessionContext {
            shared: Arc::clone(&self.state),
            generation,
            plan: Arc::new(plan),
            factory,
            phase: phase.clone(),
        };
        runtime.spawn(supervise(SessionTasks {
            ctx,
            router,
            lanes,
            phase,
        }));
        Ok(())
    }

    /// Enqueues audio without waiting; backpressure fails the session.
    pub fn push(&self, source: u32, bytes: &[u8]) -> PushOutcome {
        let mut state = lock(&self.state);
        if !state.accepts_audio() {
            return PushOutcome::Rejected;
        }
        let Some(router) = state.router.clone() else {
            return PushOutcome::Rejected;
        };
        let outcome = router.push(source, bytes);
        if outcome == PushOutcome::Backpressure {
            state.fail(router.failure().unwrap_or(SessionError::AudioBackpressure));
        }
        outcome
    }

    pub fn stop(&self) {
        lock(&self.state).request_stop();
    }

    pub fn clear(&self) {
        lock(&self.state).clear();
    }

    pub fn snapshot(&self) -> Value {
        snapshot::snapshot_json(&lock(&self.state))
    }

    pub fn drain(&self) -> Vec<Caption> {
        lock(&self.state).drain()
    }

    /// Stops everything; later service events cannot change any state.
    pub fn destroy(&self) {
        lock(&self.state).destroy();
    }
}

impl Default for EngineHandle {
    fn default() -> Self {
        Self::new()
    }
}
