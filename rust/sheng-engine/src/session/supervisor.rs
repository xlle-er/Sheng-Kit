//! Runs one session: starts the lanes, turns `stop` into closed audio input, and settles
//! the handle once every lane has ended.

use std::sync::Arc;
use std::time::Duration;

use sheng_core::Source;
use sheng_runtime::audio::{AudioRouter, LaneReceiver};
use tokio::sync::watch;
use tokio::task::JoinSet;

use super::context::SessionContext;
use super::lane;
use super::state::Phase;

/// Echo diagnostics are refreshed this often.
const ECHO_REFRESH: Duration = Duration::from_secs(1);
/// Lanes bound their own stop sequence; this only guards against a stuck client.
const SETTLE_LIMIT: Duration = Duration::from_secs(15);

pub(super) struct SessionTasks {
    pub ctx: SessionContext,
    pub router: Arc<AudioRouter>,
    pub lanes: Vec<(Source, LaneReceiver)>,
    pub phase: watch::Receiver<Phase>,
}

pub(super) async fn supervise(tasks: SessionTasks) {
    let SessionTasks {
        ctx,
        router,
        lanes,
        mut phase,
    } = tasks;
    let mut running = JoinSet::new();
    for (source, receiver) in lanes {
        running.spawn(lane::run(ctx.clone(), source, receiver));
    }
    let mut refresh = tokio::time::interval(ECHO_REFRESH);
    let requested = loop {
        let current = *phase.borrow_and_update();
        if matches!(current, Phase::Stopping | Phase::Aborted) {
            break current;
        }
        tokio::select! {
            changed = phase.changed() => if changed.is_err() { break Phase::Aborted },
            _ = refresh.tick() => { ctx.update(|state| state.refresh_echo()); }
            joined = running.join_next() => if joined.is_none() { break Phase::Aborted },
        }
    };
    let closing = Arc::clone(&router);
    let stopped = tokio::task::spawn_blocking(move || closing.stop()).await;
    match stopped {
        Ok(Ok(())) if requested == Phase::Stopping => {
            ctx.update(|state| state.input_closed());
        }
        Ok(Ok(())) => {}
        Ok(Err(error)) => ctx.fail(error),
        Err(_) => ctx.fail(sheng_runtime::provider::SessionError::AudioBackpressure),
    }
    let settled = tokio::time::timeout(SETTLE_LIMIT, async {
        while running.join_next().await.is_some() {}
    })
    .await;
    if settled.is_err() {
        running.abort_all();
    }
    ctx.update(|state| state.settle());
}
