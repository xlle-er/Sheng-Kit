//! Audio input: per-source bounded send queues and two-source echo processing (audio.md).

pub mod echo;
pub mod lane;
pub mod router;

pub use lane::{audio_lane, drain_within, AudioLane, LaneReceiver, PushError};
pub use router::{AudioRouter, LaneReceivers, PushOutcome};
