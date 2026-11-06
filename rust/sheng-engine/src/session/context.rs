//! What every task of one session shares: the handle state, its generation and the plan.
//! Updates from an older generation or a destroyed handle are dropped.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use sheng_core::{CaptionEvent, Source};
use sheng_runtime::config::SessionPlan;
use sheng_runtime::provider::SessionError;
use tokio::sync::watch;

use super::state::{EngineState, LaneDiagnostics, Phase};
use crate::factory::ClientFactory;

pub type Shared = Arc<Mutex<EngineState>>;

pub fn lock(shared: &Shared) -> MutexGuard<'_, EngineState> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Clone)]
pub struct SessionContext {
    pub shared: Shared,
    pub generation: u64,
    pub plan: Arc<SessionPlan>,
    pub factory: Arc<dyn ClientFactory>,
    pub phase: watch::Receiver<Phase>,
}

impl SessionContext {
    /// Runs `change` only while this session's generation is current.
    pub fn update<R>(&self, change: impl FnOnce(&mut EngineState) -> R) -> Option<R> {
        let mut state = lock(&self.shared);
        if state.destroyed || state.generation != self.generation {
            return None;
        }
        Some(change(&mut state))
    }

    pub fn fail(&self, error: SessionError) {
        self.update(|state| state.fail(error));
    }

    pub fn apply(&self, source: Source, event: CaptionEvent) -> Result<(), SessionError> {
        self.update(|state| state.apply_caption(source, event))
            .unwrap_or(Ok(()))
    }

    pub fn commit_unconfirmed(&self, source: Source) -> Result<(), SessionError> {
        self.update(|state| state.commit_unconfirmed(source))
            .unwrap_or(Ok(()))
    }

    pub fn lane(&self, source: Source, change: impl FnOnce(&mut LaneDiagnostics)) {
        self.update(|state| state.update_lane(source, change));
    }
}
