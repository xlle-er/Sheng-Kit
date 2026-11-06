//! Aligning two independent transcript streams without utterance ids (captions.md §5).
//!
//! Times are durations since any fixed origin chosen by the caller.

use std::time::Duration;

use thiserror::Error;

use crate::text::{first_sentence_end, is_meaningful, MAX_TEXT_BYTES};

/// Both streams silent this long confirms what is already paired.
pub const QUIET_TIMEOUT: Duration = Duration::from_secs(2);
/// Late text after a turn completes still joins that turn within this window.
pub const TURN_TAIL_WINDOW: Duration = Duration::from_millis(500);
/// A sentence cut is refused when one side is more than this many times longer than the other.
pub const MAX_LENGTH_RATIO: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Original,
    Translation,
}

/// How a service delivers stream text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chunk<'a> {
    /// New text to append as is.
    Delta(&'a str),
    /// The whole text of the current turn so far.
    Snapshot(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignedPair {
    pub original: String,
    pub translation: String,
    /// How long the translation trailed the original, rounded once to milliseconds.
    pub follow_latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AlignError {
    #[error("unconfirmed stream text exceeded the size limit")]
    Overflow,
}

#[derive(Debug, Clone, Default)]
struct StreamBuffer {
    pending: String,
    /// Bytes at the start of `pending` carried over from a previous turn.
    carried: usize,
    /// Bytes of the current turn already confirmed; snapshots skip them.
    consumed_in_turn: usize,
    /// `(end offset in pending, arrival time)` per chunk.
    marks: Vec<(usize, Duration)>,
    /// Service cut points as offsets in `pending`.
    boundaries: Vec<usize>,
}

impl StreamBuffer {
    fn push(&mut self, chunk: Chunk<'_>, now: Duration) -> bool {
        match chunk {
            Chunk::Delta(text) => {
                if text.is_empty() {
                    return false;
                }
                self.pending.push_str(text);
            }
            Chunk::Snapshot(text) => {
                let Some(rest) = text.get(self.consumed_in_turn..) else {
                    return false;
                };
                let current = &self.pending[self.carried..];
                if let Some(added) = rest.strip_prefix(current) {
                    if added.is_empty() {
                        return false;
                    }
                    self.pending.push_str(added);
                } else {
                    self.pending.truncate(self.carried);
                    self.pending.push_str(rest);
                    let len = self.pending.len();
                    self.marks.retain(|(end, _)| *end <= len);
                    self.boundaries.retain(|end| *end <= len);
                }
            }
        }
        self.marks.push((self.pending.len(), now));
        true
    }

    fn mark_boundary(&mut self) {
        let end = self.pending.len();
        if end > 0 && self.boundaries.last() != Some(&end) {
            self.boundaries.push(end);
        }
    }

    fn reached_at(&self, end: usize) -> Option<Duration> {
        self.marks.iter().find(|(e, _)| *e >= end).map(|(_, t)| *t)
    }

    /// Removes `pending[..end]` and returns it with the time that offset arrived.
    fn take(&mut self, end: usize) -> (String, Option<Duration>) {
        let reached = self.reached_at(end);
        let text: String = self.pending.drain(..end).collect();
        if end <= self.carried {
            self.carried -= end;
        } else {
            self.consumed_in_turn += end - self.carried;
            self.carried = 0;
        }
        self.marks.retain(|(e, _)| *e > end);
        self.marks.iter_mut().for_each(|(e, _)| *e -= end);
        self.boundaries.retain(|e| *e > end);
        self.boundaries.iter_mut().for_each(|e| *e -= end);
        (text, reached)
    }

    fn end_turn(&mut self) {
        self.carried = self.pending.len();
        self.consumed_in_turn = 0;
    }
}

#[derive(Debug, Clone, Default)]
pub struct DualStreamAligner {
    original: StreamBuffer,
    translation: StreamBuffer,
    last_text_at: Option<Duration>,
    turn_completed_at: Option<Duration>,
}

impl DualStreamAligner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds text to one stream and returns the sentence pairs it completes.
    pub fn push(
        &mut self,
        stream: Stream,
        chunk: Chunk<'_>,
        now: Duration,
    ) -> Result<Vec<AlignedPair>, AlignError> {
        let mut pairs = self.close_turn_if_due(now);
        let in_tail = self.turn_completed_at.is_some();
        if !self.buffer_mut(stream).push(chunk, now) {
            return Ok(pairs);
        }
        self.last_text_at = Some(now);
        if self.original.pending.len() > MAX_TEXT_BYTES
            || self.translation.pending.len() > MAX_TEXT_BYTES
        {
            *self = Self::default();
            return Err(AlignError::Overflow);
        }
        if !in_tail {
            pairs.extend(self.cut_sentences());
        }
        Ok(pairs)
    }

    /// Records a service cut point at the current end of `stream`.
    pub fn mark_boundary(&mut self, stream: Stream) -> Vec<AlignedPair> {
        self.buffer_mut(stream).mark_boundary();
        if self.turn_completed_at.is_some() {
            return Vec::new();
        }
        self.cut_sentences()
    }

    /// The service ended the turn; it is confirmed once the tail window has passed.
    pub fn complete_turn(&mut self, now: Duration) {
        self.turn_completed_at = Some(now);
    }

    /// Time-driven confirmation: closes a finished turn or applies the quiet timeout.
    pub fn tick(&mut self, now: Duration) -> Vec<AlignedPair> {
        if self.turn_completed_at.is_some() {
            return self.close_turn_if_due(now);
        }
        let is_quiet = self
            .last_text_at
            .is_some_and(|last| now >= last + QUIET_TIMEOUT);
        if !is_quiet {
            return Vec::new();
        }
        let (o, t) = (&self.original.pending, &self.translation.pending);
        if is_meaningful(o) && is_meaningful(t) && lengths_comparable(o, t) {
            return vec![self.take_all()];
        }
        Vec::new()
    }

    /// Session end: confirms what is paired with real content on both sides, drops the rest.
    pub fn finish(&mut self) -> Vec<AlignedPair> {
        let paired =
            is_meaningful(&self.original.pending) && is_meaningful(&self.translation.pending);
        let pairs = if paired {
            vec![self.take_all()]
        } else {
            Vec::new()
        };
        *self = Self::default();
        pairs
    }

    /// Unconfirmed `(original, translation)` text for display.
    pub fn pending(&self) -> (&str, &str) {
        (&self.original.pending, &self.translation.pending)
    }

    fn buffer_mut(&mut self, stream: Stream) -> &mut StreamBuffer {
        match stream {
            Stream::Original => &mut self.original,
            Stream::Translation => &mut self.translation,
        }
    }

    fn close_turn_if_due(&mut self, now: Duration) -> Vec<AlignedPair> {
        let Some(completed) = self.turn_completed_at else {
            return Vec::new();
        };
        if now < completed + TURN_TAIL_WINDOW {
            return Vec::new();
        }
        self.turn_completed_at = None;
        let paired =
            is_meaningful(&self.original.pending) && is_meaningful(&self.translation.pending);
        let pairs = if paired {
            vec![self.take_all()]
        } else {
            Vec::new()
        };
        self.original.end_turn();
        self.translation.end_turn();
        pairs
    }

    fn cut_sentences(&mut self) -> Vec<AlignedPair> {
        let mut pairs = Vec::new();
        while let Some((o_end, t_end)) = self.next_cut() {
            pairs.push(self.take(o_end, t_end));
        }
        pairs
    }

    /// Service cut points win when both streams have one; otherwise sentence punctuation.
    fn next_cut(&self) -> Option<(usize, usize)> {
        let (o, t) = (&self.original, &self.translation);
        let (o_end, t_end) = match (o.boundaries.first(), t.boundaries.first()) {
            (Some(a), Some(b)) => (*a, *b),
            (None, None) => (
                first_sentence_end(&o.pending)?,
                first_sentence_end(&t.pending)?,
            ),
            _ => return None,
        };
        let (o_text, t_text) = (&o.pending[..o_end], &t.pending[..t_end]);
        let is_pair =
            is_meaningful(o_text) && is_meaningful(t_text) && lengths_comparable(o_text, t_text);
        is_pair.then_some((o_end, t_end))
    }

    fn take_all(&mut self) -> AlignedPair {
        self.take(self.original.pending.len(), self.translation.pending.len())
    }

    fn take(&mut self, o_end: usize, t_end: usize) -> AlignedPair {
        let (original, o_at) = self.original.take(o_end);
        let (translation, t_at) = self.translation.take(t_end);
        AlignedPair {
            original: original.trim().to_owned(),
            translation: translation.trim().to_owned(),
            follow_latency_ms: o_at.zip(t_at).map(|(o, t)| round_ms(t.saturating_sub(o))),
        }
    }
}

fn lengths_comparable(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim().chars().count(), b.trim().chars().count());
    a.min(b).saturating_mul(MAX_LENGTH_RATIO) >= a.max(b)
}

fn round_ms(duration: Duration) -> u64 {
    let nanos = duration.as_nanos();
    u64::try_from((nanos + 500_000) / 1_000_000).unwrap_or(u64::MAX)
}
