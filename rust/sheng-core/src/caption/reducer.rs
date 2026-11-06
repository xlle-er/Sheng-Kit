//! All sources of one session: tracks, `recent[]` and the confirmed-caption queue.

use std::collections::VecDeque;

use super::track::{DisplayKind, Track};
use super::{
    Caption, CaptionError, CaptionEvent, Confirmed, TrackView, CONFIRMED_QUEUE_LIMIT, RECENT_LIMIT,
};
use crate::ids::{CaptionIdFactory, Source};
use crate::language::needs_translation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReducerConfig {
    pub run_id: String,
    pub generation: u64,
    pub sources: Vec<Source>,
    pub source_language: String,
    pub target_language: String,
}

#[derive(Debug, Clone)]
pub struct CaptionReducer {
    source_language: String,
    target_language: String,
    ids: CaptionIdFactory,
    tracks: Vec<(Source, Track)>,
    recent: VecDeque<Caption>,
    queue: VecDeque<Caption>,
}

impl CaptionReducer {
    pub fn new(config: ReducerConfig) -> Self {
        let translate = needs_translation(&config.target_language);
        let mut sources = config.sources;
        sources.dedup();
        Self {
            ids: CaptionIdFactory::new(config.run_id, config.generation),
            tracks: sources
                .into_iter()
                .map(|source| (source, Track::new(translate)))
                .collect(),
            source_language: config.source_language,
            target_language: config.target_language,
            recent: VecDeque::new(),
            queue: VecDeque::new(),
        }
    }

    /// Applies `event` to `source`. Returns whether the visible state changed.
    ///
    /// Events for a source that is not part of the session are ignored. When the confirmed
    /// queue is full the new caption is rejected and the queued ones are kept.
    pub fn apply(
        &mut self,
        source: Source,
        event: CaptionEvent,
        timestamp_ms: u64,
    ) -> Result<bool, CaptionError> {
        let Some(index) = self.tracks.iter().position(|(s, _)| *s == source) else {
            return Ok(false);
        };
        let before = self.tracks();
        let confirmed = self.tracks[index].1.apply(event);
        let changed = before != self.tracks();
        self.enqueue(source, confirmed, timestamp_ms, changed)
    }

    /// Confirms once whatever `source` recognized but has not confirmed yet, with the
    /// translation that arrived for it, if any: the connection ended unexpectedly or a stop
    /// timed out (providers/README.md). Returns whether the visible state changed.
    pub fn commit_unconfirmed(
        &mut self,
        source: Source,
        timestamp_ms: u64,
    ) -> Result<bool, CaptionError> {
        let Some(index) = self.tracks.iter().position(|(s, _)| *s == source) else {
            return Ok(false);
        };
        let before = self.tracks();
        let confirmed = self.tracks[index].1.commit_unconfirmed();
        let changed = before != self.tracks();
        self.enqueue(source, confirmed, timestamp_ms, changed)
    }

    fn enqueue(
        &mut self,
        source: Source,
        confirmed: Vec<Confirmed>,
        timestamp_ms: u64,
        mut changed: bool,
    ) -> Result<bool, CaptionError> {
        for pair in confirmed {
            if self.queue.len() >= CONFIRMED_QUEUE_LIMIT {
                return Err(CaptionError::ConsumerBackpressure);
            }
            let caption = Caption {
                id: self.ids.next_id(),
                source,
                original: pair.original,
                translation: pair.translation,
                timestamp_ms,
                source_language: self.source_language.clone(),
                target_language: self.target_language.clone(),
            };
            self.push_recent(caption.clone());
            self.queue.push_back(caption);
            changed = true;
        }
        Ok(changed)
    }

    /// User clear: empties the display and `recent[]`; events of ids seen before are ignored.
    /// Captions already waiting in the confirmed queue stay there.
    pub fn clear(&mut self) -> bool {
        let changed = !self.recent.is_empty()
            || self.tracks.iter().any(|(_, track)| {
                track.display.kind != DisplayKind::Empty || track.pending.is_some()
            });
        for (_, track) in &mut self.tracks {
            track.clear();
        }
        self.recent.clear();
        changed
    }

    pub fn tracks(&self) -> Vec<TrackView> {
        self.tracks
            .iter()
            .map(|(source, track)| TrackView {
                source: *source,
                original: track.display.original.clone(),
                translation: track.display.translation.clone(),
                pending_original: track
                    .pending
                    .as_ref()
                    .map(|p| p.text.clone())
                    .unwrap_or_default(),
                is_final: track.display.kind == DisplayKind::Confirmed,
            })
            .collect()
    }

    pub fn recent(&self) -> Vec<Caption> {
        self.recent.iter().cloned().collect()
    }

    pub fn confirmed_len(&self) -> usize {
        self.queue.len()
    }

    /// Removes and returns every queued confirmed caption, oldest first.
    pub fn drain_confirmed(&mut self) -> Vec<Caption> {
        self.queue.drain(..).collect()
    }

    fn push_recent(&mut self, caption: Caption) {
        self.recent.push_back(caption);
        while self.recent.len() > RECENT_LIMIT {
            self.recent.pop_front();
        }
    }
}
