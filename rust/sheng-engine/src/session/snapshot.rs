//! Snapshot JSON (snapshot.md “快照 JSON”).

use std::time::Duration;

use serde_json::{json, Value};

use super::state::{EngineState, LaneDiagnostics, LatencyKind};

const PROTOCOL: u32 = 1;

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn lane_json(lane: &LaneDiagnostics) -> Value {
    let (latency, kind) = match lane.translation_latency {
        Some((latency, kind)) => (
            Some(millis(latency)),
            Some(match kind {
                LatencyKind::Request => "request",
                LatencyKind::Follow => "follow",
            }),
        ),
        None => (None, None),
    };
    json!({
        "source": lane.source,
        "apiLatencyMs": lane.api_latency.map(millis),
        "translationLatencyMs": latency,
        "translationLatencyKind": kind,
        "translationPending": lane.translation_pending,
        "recovery": lane.recovery,
    })
}

pub fn snapshot_json(state: &EngineState) -> Value {
    json!({
        "protocol": PROTOCOL,
        "revision": state.revision,
        "generation": state.generation,
        "runId": state.run_id,
        "status": state.status.name(),
        "error": state.error,
        "sampleRate": state.sample_rate,
        "settled": !state.busy,
        "tracks": state.tracks(),
        "recent": state.recent,
        "diagnostics": state.diagnostics.iter().map(lane_json).collect::<Vec<_>>(),
        "echo": state.echo,
    })
}
