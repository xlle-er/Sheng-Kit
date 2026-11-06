//! Frame-by-frame echo removal for the microphone, with the system sound as reference
//! (audio.md §3). Pure and synchronous; callers provide capture times.

use std::collections::VecDeque;
use std::time::Duration;

use super::speex::SpeexEcho;
use super::stats::EchoStats;

pub const FRAME_DURATION: Duration = Duration::from_millis(10);
pub const FILTER_DURATION: Duration = Duration::from_millis(500);
/// At most this many reference frames are kept waiting for microphone frames.
pub const MAX_REFERENCE_FRAMES: usize = 100;
/// A capture gap longer than this rebuilds the canceller.
pub const MAX_CAPTURE_GAP: Duration = Duration::from_millis(100);

fn samples_for(rate: u32, duration: Duration) -> usize {
    usize::try_from(u128::from(rate) * duration.as_millis() / 1000).unwrap_or(usize::MAX)
}

fn duration_of(rate: u32, samples: usize) -> Duration {
    Duration::from_micros(samples as u64 * 1_000_000 / u64::from(rate.max(1)))
}

/// Tracks where the next capture should start, to detect gaps.
#[derive(Debug, Default)]
struct Continuity {
    next_at: Option<Duration>,
}

impl Continuity {
    /// Records a capture of `length` at `at`; true when it follows a gap.
    fn advance(&mut self, at: Duration, length: Duration) -> bool {
        let gap = self
            .next_at
            .is_some_and(|expected| at > expected + MAX_CAPTURE_GAP);
        self.next_at = Some(at + length);
        gap
    }
}

pub struct EchoProcessor {
    rate: u32,
    frame: usize,
    canceller: Option<SpeexEcho>,
    reference: VecDeque<i16>,
    mic: Vec<i16>,
    mic_clock: Continuity,
    reference_clock: Continuity,
    stats: EchoStats,
}

impl EchoProcessor {
    /// Fails when the canceller cannot be created (`echo_initialization_failed`).
    pub fn new(rate: u32) -> Option<Self> {
        let frame = samples_for(rate, FRAME_DURATION);
        let mut processor = Self {
            rate,
            frame,
            canceller: None,
            reference: VecDeque::new(),
            mic: Vec::new(),
            mic_clock: Continuity::default(),
            reference_clock: Continuity::default(),
            stats: EchoStats::default(),
        };
        processor.canceller = Some(processor.new_canceller()?);
        Some(processor)
    }

    fn new_canceller(&self) -> Option<SpeexEcho> {
        SpeexEcho::new(
            self.rate,
            self.frame,
            samples_for(self.rate, FILTER_DURATION),
        )
    }

    pub fn stats(&self) -> &EchoStats {
        &self.stats
    }

    /// Whole reference frames waiting for microphone frames (at most `MAX_REFERENCE_FRAMES`).
    pub fn buffered_reference_frames(&self) -> usize {
        self.reference.len() / self.frame
    }

    /// Drops the adaptive filter; a fresh one is created for the next matched frame.
    fn reset(&mut self) {
        if self.canceller.take().is_some() {
            self.stats.resets += 1;
        }
    }

    pub fn push_reference(&mut self, samples: &[i16], at: Duration) {
        if self
            .reference_clock
            .advance(at, duration_of(self.rate, samples.len()))
        {
            self.reference.clear();
            self.reset();
        }
        self.reference.extend(samples.iter().copied());
        let limit = MAX_REFERENCE_FRAMES * self.frame;
        let excess = self.reference.len().saturating_sub(limit);
        self.reference.drain(..excess);
    }

    /// Processes whole frames of microphone audio; a partial frame waits for more input.
    pub fn push_mic(&mut self, samples: &[i16], at: Duration) -> Vec<i16> {
        if self
            .mic_clock
            .advance(at, duration_of(self.rate, samples.len()))
        {
            self.reset();
        }
        self.mic.extend_from_slice(samples);
        let whole = self.mic.len() / self.frame * self.frame;
        let ready: Vec<i16> = self.mic.drain(..whole).collect();
        ready
            .chunks(self.frame)
            .flat_map(|frame| self.process_frame(frame))
            .collect()
    }

    /// Processes the remaining partial frame zero-padded, trimmed back to its real length.
    pub fn finish(&mut self) -> Vec<i16> {
        let length = self.mic.len();
        if length == 0 {
            return Vec::new();
        }
        let mut frame = std::mem::take(&mut self.mic);
        frame.resize(self.frame, 0);
        let mut out = self.process_frame(&frame);
        out.truncate(length);
        out
    }

    fn process_frame(&mut self, mic: &[i16]) -> Vec<i16> {
        self.stats.frames += 1;
        if self.reference.len() < self.frame {
            self.stats.missing_references += 1;
            self.reset();
            self.stats.record(mic, mic, None);
            return mic.to_vec();
        }
        let reference: Vec<i16> = self.reference.drain(..self.frame).collect();
        if self.canceller.is_none() {
            self.canceller = self.new_canceller();
        }
        let Some(canceller) = self.canceller.as_mut() else {
            self.stats.record(mic, mic, None);
            return mic.to_vec();
        };
        let mut out = vec![0i16; self.frame];
        canceller.cancel(mic, &reference, &mut out);
        self.stats.matched_references += 1;
        self.stats.record(mic, &out, Some(&reference));
        out
    }
}
