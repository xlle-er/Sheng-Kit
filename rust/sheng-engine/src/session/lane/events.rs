//! Turning provider events and pipeline results into caption state and diagnostics.

use std::time::Duration;

use sheng_core::align::{AlignedPair, Chunk};
use sheng_core::text::is_meaningful;
use sheng_core::CaptionEvent;
use sheng_runtime::pipeline::PipelineOutput;
use sheng_runtime::provider::{ProviderEvent, SessionError, StreamText};

use super::Lane;
use crate::session::state::LatencyKind;

impl Lane {
    pub(super) async fn on_event(
        &mut self,
        event: Option<ProviderEvent>,
    ) -> Result<(), SessionError> {
        let Some(event) = event else {
            return Err(self.root_cause(SessionError::TransportError));
        };
        match event {
            ProviderEvent::Ready => {
                let source = self.source;
                self.ctx.update(|state| state.connected(source));
            }
            ProviderEvent::Caption(event) => self.on_caption(event).await?,
            ProviderEvent::StreamText { stream, text } => {
                let now = self.now();
                let chunk = match &text {
                    StreamText::Delta(text) => Chunk::Delta(text),
                    StreamText::Snapshot(text) => Chunk::Snapshot(text),
                };
                let pairs = self
                    .aligner
                    .push(stream, chunk, now)
                    .map_err(|_| SessionError::ProviderEventBacklogOverflow)?;
                self.apply_pairs(pairs)?;
                self.show_stream_draft()?;
            }
            ProviderEvent::StreamBoundary(stream) => {
                let pairs = self.aligner.mark_boundary(stream);
                self.apply_pairs(pairs)?;
            }
            ProviderEvent::TurnComplete => self.aligner.complete_turn(self.now()),
            ProviderEvent::ApiLatency(latency) => {
                self.ctx
                    .lane(self.source, |lane| lane.api_latency = Some(latency));
            }
            ProviderEvent::Ended if self.draining => self.ended = true,
            ProviderEvent::Ended => return Err(SessionError::TransportError),
            ProviderEvent::Failed(error) => return Err(error.speech_session_error()),
        }
        Ok(())
    }

    /// Recognizing routes send originals through the pipeline. Drafts are shown first and
    /// offered without waiting, so a slow translator never delays them.
    async fn on_caption(&mut self, event: CaptionEvent) -> Result<(), SessionError> {
        let Some(pipeline) = &mut self.pipeline else {
            return self.ctx.apply(self.source, event);
        };
        match event {
            CaptionEvent::OriginalDraft { utterance, text } => {
                let shown = CaptionEvent::OriginalDraft {
                    utterance: utterance.clone(),
                    text: text.clone(),
                };
                pipeline.offer_draft(utterance, text);
                self.ctx.apply(self.source, shown)
            }
            CaptionEvent::OriginalFinal { utterance, text } => {
                pipeline.send_final(utterance, text).await;
                Ok(())
            }
            other => self.ctx.apply(self.source, other),
        }
    }

    pub(super) fn on_output(&mut self, output: Option<PipelineOutput>) -> Result<(), SessionError> {
        let source = self.source;
        let Some(output) = output else {
            if let Some(pipeline) = &mut self.pipeline {
                pipeline.finished = true;
            }
            return Ok(());
        };
        match output {
            PipelineOutput::Caption(event) => {
                let event = match &self.pipeline {
                    Some(pipeline) => pipeline.with_shown_original(event),
                    None => event,
                };
                self.ctx.apply(source, event)?
            }
            PipelineOutput::Recovery(recovery) => {
                self.ctx.lane(source, |lane| lane.recovery = recovery);
            }
            PipelineOutput::Latency(latency) => {
                self.record_translation_latency(latency, LatencyKind::Request)
            }
            PipelineOutput::Pending(pending) => {
                self.ctx
                    .lane(source, |lane| lane.translation_pending = pending);
            }
            PipelineOutput::Failed(error) => return Err(error),
            PipelineOutput::Finished => {
                if let Some(pipeline) = &mut self.pipeline {
                    pipeline.finished = true;
                }
            }
        }
        Ok(())
    }

    pub(super) fn tick(&mut self) -> Result<(), SessionError> {
        if let Some(pipeline) = &mut self.pipeline {
            pipeline.retry_draft();
        }
        let pairs = self.aligner.tick(self.now());
        self.apply_pairs(pairs)
    }

    pub(super) fn apply_pairs(&mut self, pairs: Vec<AlignedPair>) -> Result<(), SessionError> {
        for pair in pairs {
            if let Some(ms) = pair.follow_latency_ms {
                self.record_translation_latency(Duration::from_millis(ms), LatencyKind::Follow);
            }
            let event = CaptionEvent::ConfirmedPair {
                utterance: None,
                original: pair.original,
                translation: pair.translation,
            };
            self.ctx.apply(self.source, event)?;
        }
        Ok(())
    }

    /// Confirms what the aligner holds: pairs as usual, and one-sided original text as a
    /// draft (with any translation) so the caption state can commit it.
    pub(super) fn flush_streams(&mut self) -> Result<(), SessionError> {
        let (original, translation) = self.aligner.pending();
        let (original, translation) = (original.to_owned(), translation.to_owned());
        let pairs = self.aligner.finish();
        if !pairs.is_empty() {
            return self.apply_pairs(pairs);
        }
        if !is_meaningful(&original) {
            return Ok(());
        }
        self.ctx.apply(
            self.source,
            CaptionEvent::OriginalDraft {
                utterance: None,
                text: original,
            },
        )?;
        if !is_meaningful(&translation) {
            return Ok(());
        }
        self.ctx.apply(
            self.source,
            CaptionEvent::TranslationDraft {
                utterance: None,
                text: translation,
            },
        )
    }

    /// The connection ended unexpectedly or a stop timed out (providers/README.md): what was
    /// recognized but not confirmed is confirmed once, with the translation it has so far.
    pub(super) fn commit_unconfirmed(&mut self) -> Result<(), SessionError> {
        self.flush_streams()?;
        self.ctx.commit_unconfirmed(self.source)
    }

    /// Unconfirmed original stream text is shown as a draft.
    fn show_stream_draft(&mut self) -> Result<(), SessionError> {
        let (original, _) = self.aligner.pending();
        if !is_meaningful(original) {
            return Ok(());
        }
        let event = CaptionEvent::OriginalDraft {
            utterance: None,
            text: original.to_owned(),
        };
        self.ctx.apply(self.source, event)
    }

    fn record_translation_latency(&self, latency: Duration, kind: LatencyKind) {
        self.ctx.lane(self.source, |lane| {
            lane.translation_latency = Some((latency, kind))
        });
    }
}
