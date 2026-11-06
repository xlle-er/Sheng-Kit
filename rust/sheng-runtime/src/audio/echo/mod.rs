//! Two-source echo removal: the system sound is the reference for the microphone.

pub mod processor;
pub mod speex;
pub mod stage;
pub mod stats;

pub use processor::EchoProcessor;
pub use stage::{EchoPushError, EchoStage};
pub use stats::{EchoReport, EchoStats};
