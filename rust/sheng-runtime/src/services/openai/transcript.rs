//! Maps decoded server events to provider events (providers/openai.md “会话流程” 3–5).
//! Pure state: no I/O, so it is tested with hand-written event sequences.

use std::collections::{HashMap, VecDeque};

use sheng_core::align::Stream;
use sheng_core::text::MAX_TEXT_BYTES;
use sheng_core::{CaptionEvent, UtteranceId};

use super::errors::{classify, is_commit_empty};
use super::messages::ServerEvent;
use crate::provider::{ProviderError, ProviderEvent, StreamText};

/// Items whose drafts are being accumulated or whose completion was seen; oldest first out.
const MAX_TRACKED_ITEMS: usize = 64;

/// What the connection should do after an event.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Continue,
    /// The service failed; report and stop reading.
    Fail(ProviderError),
    /// Input has ended and everything awaited arrived (or the session closed).
    Done,
}

/// Per-connection mapping state.
#[derive(Debug, Default)]
pub struct Mapper {
    drafts: HashMap<String, String>,
    draft_order: VecDeque<String>,
    completed: VecDeque<String>,
    /// Committed items still waiting for their completion.
    awaiting_items: Vec<String>,
    /// Commits sent but not yet acknowledged by `input_audio_buffer.committed`.
    awaiting_commits: usize,
    /// `session.close` was sent; the translation session ends with `session.closed`.
    awaiting_close: bool,
    finishing: bool,
}

impl Mapper {
    /// A commit was sent by the client.
    pub fn commit_sent(&mut self) {
        self.awaiting_commits += 1;
    }

    /// The client asked a translation session to close.
    pub fn close_sent(&mut self) {
        self.awaiting_close = true;
    }

    /// Input has ended; returns `Done` when nothing is awaited any more.
    pub fn finish(&mut self) -> Step {
        self.finishing = true;
        self.settled()
    }

    fn settled(&self) -> Step {
        let awaiting =
            self.awaiting_close || self.awaiting_commits > 0 || !self.awaiting_items.is_empty();
        if self.finishing && !awaiting {
            Step::Done
        } else {
            Step::Continue
        }
    }

    /// Handles one event, pushing provider events to `out`.
    pub fn on_event(&mut self, event: ServerEvent, out: &mut Vec<ProviderEvent>) -> Step {
        match event {
            ServerEvent::Committed { item_id } => {
                self.awaiting_commits = self.awaiting_commits.saturating_sub(1);
                if !self.is_completed(&item_id) {
                    self.awaiting_items.push(item_id);
                }
            }
            ServerEvent::TranscriptDelta { item_id, delta } => self.on_delta(item_id, delta, out),
            ServerEvent::TranscriptCompleted {
                item_id,
                transcript,
            } => self.on_completed(item_id, transcript, out),
            ServerEvent::TranscriptFailed { item_id } => {
                self.forget(&item_id);
                self.mark_completed(item_id);
            }
            ServerEvent::InputTranscriptDelta(delta) => out.push(stream(Stream::Original, delta)),
            ServerEvent::OutputTranscriptDelta(delta) => {
                out.push(stream(Stream::Translation, delta))
            }
            ServerEvent::SessionClosed => {
                out.push(ProviderEvent::TurnComplete);
                return Step::Done;
            }
            ServerEvent::Error(info) if is_commit_empty(&info) => {
                self.awaiting_commits = self.awaiting_commits.saturating_sub(1);
            }
            ServerEvent::Error(info) => return Step::Fail(classify(&info)),
            ServerEvent::SessionCreated | ServerEvent::SessionUpdated | ServerEvent::Other => {}
        }
        self.settled()
    }

    fn on_delta(&mut self, item_id: String, delta: String, out: &mut Vec<ProviderEvent>) {
        if self.is_completed(&item_id) || delta.is_empty() {
            return;
        }
        if !self.drafts.contains_key(&item_id) {
            self.track_draft(item_id.clone());
        }
        let text = self.drafts.entry(item_id.clone()).or_default();
        // Past the caption limit the draft is rejected downstream anyway; stop growing it.
        if text.len() <= MAX_TEXT_BYTES {
            text.push_str(&delta);
        }
        out.push(ProviderEvent::Caption(CaptionEvent::OriginalDraft {
            utterance: Some(UtteranceId::Text(item_id)),
            text: text.clone(),
        }));
    }

    fn on_completed(&mut self, item_id: String, transcript: String, out: &mut Vec<ProviderEvent>) {
        if self.is_completed(&item_id) {
            return;
        }
        self.forget(&item_id);
        self.mark_completed(item_id.clone());
        out.push(ProviderEvent::Caption(CaptionEvent::OriginalFinal {
            utterance: Some(UtteranceId::Text(item_id)),
            text: transcript,
        }));
    }

    fn track_draft(&mut self, item_id: String) {
        if self.draft_order.len() >= MAX_TRACKED_ITEMS {
            if let Some(oldest) = self.draft_order.pop_front() {
                self.drafts.remove(&oldest);
            }
        }
        self.draft_order.push_back(item_id);
    }

    fn forget(&mut self, item_id: &str) {
        self.drafts.remove(item_id);
        self.draft_order.retain(|id| id != item_id);
        self.awaiting_items.retain(|id| id != item_id);
    }

    fn mark_completed(&mut self, item_id: String) {
        if self.completed.len() >= MAX_TRACKED_ITEMS {
            self.completed.pop_front();
        }
        self.completed.push_back(item_id);
    }

    fn is_completed(&self, item_id: &str) -> bool {
        self.completed.iter().any(|id| id == item_id)
    }
}

/// Translation transcript deltas are append-only fragments (translation server events).
fn stream(stream: Stream, delta: String) -> ProviderEvent {
    ProviderEvent::StreamText {
        stream,
        text: StreamText::Delta(delta),
    }
}
