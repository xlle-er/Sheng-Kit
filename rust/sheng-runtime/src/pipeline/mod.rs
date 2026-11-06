//! Recognition + independent text translation (translation-pipeline.md §1–§3): a preview
//! channel for the latest draft and a serial final channel, sharing one request budget.

mod worker;

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use sheng_core::{CaptionEvent, UtteranceId};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::provider::{SessionError, TextTranslator};

/// Commands waiting for the pipeline.
pub const PIPELINE_INPUT_QUEUE: usize = 64;
/// Results waiting for the session.
pub const PIPELINE_OUTPUT_QUEUE: usize = 256;
/// After stop, final translations must drain within this time.
pub const FINISH_DRAIN: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineInput {
    Draft {
        utterance: Option<UtteranceId>,
        text: String,
    },
    Final {
        utterance: Option<UtteranceId>,
        text: String,
    },
    /// Input ended; the recognizer has had its chance to deliver the last final.
    Finish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryReason {
    RateLimited,
    TemporarilyUnavailable,
}

/// Diagnostics `recovery` while translation is being retried (snapshot.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    pub reason: RecoveryReason,
    #[serde(rename = "retryAfterMs")]
    pub retry_after_ms: u64,
    pub retry_scheduled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineOutput {
    /// `PreviewPair` or `ConfirmedPair` for the caption reducer.
    Caption(CaptionEvent),
    /// `None` clears a previously reported recovery.
    Recovery(Option<Recovery>),
    /// Request time of a completed translation (`translationLatencyKind:"request"`).
    Latency(Duration),
    /// `translationPending` changed.
    Pending(bool),
    /// The session must fail with this code; the pipeline has stopped.
    Failed(SessionError),
    /// Finish completed; the pipeline has stopped.
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineConfig {
    pub source_language: String,
    pub target_language: String,
}

/// Outcome of `TranslationPipeline::offer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    Accepted,
    /// The input queue is full; the command is handed back.
    Full(PipelineInput),
    /// The pipeline has stopped.
    Stopped,
}

/// Handle used by the session to feed the pipeline.
#[derive(Debug, Clone)]
pub struct TranslationPipeline {
    input: mpsc::Sender<PipelineInput>,
}

impl TranslationPipeline {
    /// Starts the pipeline task; results arrive on the returned receiver.
    pub fn spawn(
        translator: Arc<dyn TextTranslator>,
        config: PipelineConfig,
    ) -> (Self, mpsc::Receiver<PipelineOutput>, JoinHandle<()>) {
        let (input, input_rx) = mpsc::channel(PIPELINE_INPUT_QUEUE);
        let (output, output_rx) = mpsc::channel(PIPELINE_OUTPUT_QUEUE);
        let task = tokio::spawn(worker::Worker::new(translator, config, output).run(input_rx));
        (Self { input }, output_rx, task)
    }

    /// Sends a command; returns false once the pipeline has stopped.
    pub async fn send(&self, command: PipelineInput) -> bool {
        self.input.send(command).await.is_ok()
    }

    /// Hands over a command without waiting; a full queue gives it back.
    pub fn offer(&self, command: PipelineInput) -> Offer {
        match self.input.try_send(command) {
            Ok(()) => Offer::Accepted,
            Err(mpsc::error::TrySendError::Full(command)) => Offer::Full(command),
            Err(mpsc::error::TrySendError::Closed(_)) => Offer::Stopped,
        }
    }
}
