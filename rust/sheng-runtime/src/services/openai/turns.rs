//! Client-side turn ending for transcription sessions. `gpt-live-transcribe` takes no
//! server turn detection, so the client commits the buffer after a pause in speech
//! (realtime transcription guide, “use client-side VAD”). A simple level detector.

use std::time::Duration;

use super::config::SAMPLE_RATE;

/// RMS of 16-bit samples above which a chunk counts as speech (about -36 dBFS).
pub const SPEECH_RMS: f64 = 500.0;
/// Trailing quiet after speech that ends a turn.
pub const TURN_END_SILENCE: Duration = Duration::from_millis(700);
/// Longest turn before a forced commit, so finals keep coming during continuous speech.
pub const MAX_TURN: Duration = Duration::from_secs(15);
/// Shortest buffer worth committing; the service rejects tiny buffers.
pub const MIN_COMMIT: Duration = Duration::from_millis(100);

/// What to do with the input buffer after a chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnAction {
    Keep,
    Commit,
    /// Long stretch without speech: drop it instead of transcribing silence.
    Clear,
}

#[derive(Debug, Default)]
pub struct TurnDetector {
    buffered: Duration,
    trailing_silence: Duration,
    heard_speech: bool,
}

fn duration_of(bytes: usize) -> Duration {
    let samples = (bytes / 2) as u64;
    Duration::from_micros(samples * 1_000_000 / u64::from(SAMPLE_RATE))
}

fn rms(pcm: &[u8]) -> f64 {
    let (samples, _) = pcm.as_chunks::<2>();
    let count = samples.len();
    if count == 0 {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|pair| f64::from(i16::from_le_bytes(*pair)).powi(2))
        .sum();
    (sum / count as f64).sqrt()
}

impl TurnDetector {
    /// Accounts for one appended chunk of PCM16 mono.
    pub fn push(&mut self, pcm: &[u8]) -> TurnAction {
        let length = duration_of(pcm.len());
        self.buffered += length;
        if rms(pcm) >= SPEECH_RMS {
            self.heard_speech = true;
            self.trailing_silence = Duration::ZERO;
        } else {
            self.trailing_silence += length;
        }
        let action = if self.heard_speech {
            let paused = self.trailing_silence >= TURN_END_SILENCE;
            if (paused || self.buffered >= MAX_TURN) && self.buffered >= MIN_COMMIT {
                TurnAction::Commit
            } else {
                TurnAction::Keep
            }
        } else if self.buffered >= MAX_TURN {
            TurnAction::Clear
        } else {
            TurnAction::Keep
        };
        if action != TurnAction::Keep {
            *self = Self::default();
        }
        action
    }

    /// Whether the buffer holds audio worth committing when input ends.
    pub fn take_final(&mut self) -> bool {
        let worth = self.heard_speech && self.buffered >= MIN_COMMIT;
        *self = Self::default();
        worth
    }
}
