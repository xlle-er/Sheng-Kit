//! Simultaneous route: input and output transcripts become the two id-less streams that the
//! engine aligns (captions.md §5). Quiet and turn-tail timing live in the aligner.

use sheng_core::align::Stream;

use super::protocol::ServerEvent;
use crate::provider::{ProviderEvent, StreamText};

/// Text of one stream in the current turn. Turns the service's mix of incremental pieces
/// and cumulative snapshots into increments only.
#[derive(Debug, Default, Clone)]
pub struct TurnText {
    text: String,
    /// Pieces the turn text was built from.
    pieces: usize,
}

impl TurnText {
    /// New text since the last call. A strict extension of the turn so far is a snapshot
    /// and only its new tail counts. Text equal to a turn built from several pieces is a
    /// resent snapshot. Anything else is an increment, so repeated words stay.
    pub fn increment(&mut self, incoming: &str) -> Option<String> {
        let is_resent = self.pieces > 1 && incoming == self.text;
        let is_extension = !self.text.is_empty()
            && incoming.len() > self.text.len()
            && incoming.starts_with(self.text.as_str());
        let added = if is_resent {
            ""
        } else if is_extension {
            &incoming[self.text.len()..]
        } else {
            incoming
        };
        if added.is_empty() {
            return None;
        }
        self.text.push_str(added);
        self.pieces += 1;
        Some(added.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.pieces = 0;
    }
}

/// Per-turn state of a translating session.
#[derive(Debug, Default)]
pub struct TranslateState {
    input: TurnText,
    output: TurnText,
    /// Text arrived since the last turn end.
    open: bool,
    /// The current generation was interrupted; its late output is dropped.
    interrupted: bool,
}

fn stream_delta(stream: Stream, text: String) -> ProviderEvent {
    ProviderEvent::StreamText {
        stream,
        text: StreamText::Delta(text),
    }
}

impl TranslateState {
    pub fn apply(&mut self, event: &ServerEvent) -> Vec<ProviderEvent> {
        match event {
            ServerEvent::Input(text) => self.text(Stream::Original, text),
            ServerEvent::Output(text) if !self.interrupted => self.text(Stream::Translation, text),
            ServerEvent::Interrupted => self.interrupt(),
            ServerEvent::GenerationComplete | ServerEvent::TurnComplete => self.end_turn(),
            _ => Vec::new(),
        }
    }

    /// True when nothing has arrived since the last turn end.
    pub fn is_idle(&self) -> bool {
        !self.open
    }

    fn text(&mut self, stream: Stream, text: &str) -> Vec<ProviderEvent> {
        let turn = match stream {
            Stream::Original => &mut self.input,
            Stream::Translation => &mut self.output,
        };
        let Some(added) = turn.increment(text) else {
            return Vec::new();
        };
        self.open = true;
        vec![stream_delta(stream, added)]
    }

    /// Drops the unconfirmed translation of this turn: an empty snapshot replaces whatever
    /// the aligner holds for the turn (questions/step4e.md).
    fn interrupt(&mut self) -> Vec<ProviderEvent> {
        if self.interrupted {
            return Vec::new();
        }
        self.interrupted = true;
        self.open = true;
        self.output.clear();
        vec![ProviderEvent::StreamText {
            stream: Stream::Translation,
            text: StreamText::Snapshot(String::new()),
        }]
    }

    fn end_turn(&mut self) -> Vec<ProviderEvent> {
        if !self.open {
            return Vec::new();
        }
        self.open = false;
        self.interrupted = false;
        self.input.clear();
        self.output.clear();
        vec![ProviderEvent::TurnComplete]
    }
}
