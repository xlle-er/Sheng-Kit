//! Feeding recognized originals to the translation pipeline (translation-pipeline.md §1).
//! Drafts are shown before they are offered and never wait for the pipeline: a full input
//! queue keeps only the newest unsent draft. Finals and finish wait, are never dropped and
//! stay behind any unsent draft so the pipeline sees the recognizer's order.

use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::pipeline::{Offer, PipelineInput};

use super::Pipeline;

impl Pipeline {
    /// Offers a draft that is already shown; replaces an older unsent one.
    pub(super) fn offer_draft(&mut self, utterance: Option<UtteranceId>, text: String) {
        self.shown_draft = Some((utterance.clone(), text.clone()));
        self.unsent_draft = Some(PipelineInput::Draft { utterance, text });
        self.retry_draft();
    }

    /// Hands the unsent draft over if the queue has room now.
    pub(super) fn retry_draft(&mut self) {
        let Some(draft) = self.unsent_draft.take() else {
            return;
        };
        if let Offer::Full(draft) = self.input.offer(draft) {
            self.unsent_draft = Some(draft);
        }
    }

    /// Sends a final; false once the pipeline has stopped.
    pub(super) async fn send_final(
        &mut self,
        utterance: Option<UtteranceId>,
        text: String,
    ) -> bool {
        self.shown_draft = None;
        self.send_in_order(PipelineInput::Final { utterance, text })
            .await
    }

    /// Sends a command that must not be lost, after any unsent draft.
    pub(super) async fn send_in_order(&mut self, command: PipelineInput) -> bool {
        if let Some(draft) = self.unsent_draft.take() {
            if !self.input.send(draft).await {
                return false;
            }
        }
        self.input.send(command).await
    }

    /// A preview requested for an older draft of the shown sentence keeps the newer
    /// original on screen; only its translation is new (pipeline §1 preview rule). Without
    /// utterance ids a late preview cannot be told apart from the next sentence, so it is
    /// left as it is.
    pub(super) fn with_shown_original(&self, event: CaptionEvent) -> CaptionEvent {
        let CaptionEvent::PreviewPair {
            utterance,
            original,
            translation,
        } = event
        else {
            return event;
        };
        let original = match &self.shown_draft {
            Some((shown @ Some(_), text)) if *shown == utterance => text.clone(),
            _ => original,
        };
        CaptionEvent::PreviewPair {
            utterance,
            original,
            translation,
        }
    }
}
