//! Routes pushed PCM to the per-source lanes, through echo processing for the microphone
//! when both sources are captured (abi-v1.md “音频”, audio.md).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::echo::{EchoPushError, EchoReport, EchoStage};
use super::lane::{audio_lane, AudioLane, LaneReceiver, PushError};
use crate::config::request::Input;
use crate::provider::SessionError;

/// Largest chunk accepted by `push_pcm`.
pub const MAX_PUSH_BYTES: usize = 65_536;
pub const SOURCE_SYSTEM: u32 = 0;
pub const SOURCE_MICROPHONE: u32 = 1;

/// `push_pcm` result: 0, -1 and -2 at the ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    Accepted,
    Rejected,
    Backpressure,
}

impl PushOutcome {
    pub fn abi_code(self) -> i32 {
        match self {
            Self::Accepted => 0,
            Self::Rejected => -1,
            Self::Backpressure => -2,
        }
    }
}

/// Consumers of the session's lanes, one per captured source.
#[derive(Debug)]
pub struct LaneReceivers {
    pub system: Option<LaneReceiver>,
    pub microphone: Option<LaneReceiver>,
}

pub struct AudioRouter {
    system: Option<Arc<AudioLane>>,
    microphone: Option<Arc<AudioLane>>,
    echo: Option<EchoStage>,
    started: Instant,
    stopped: AtomicBool,
    failure: Arc<Mutex<Option<SessionError>>>,
}

fn record(failure: &Mutex<Option<SessionError>>, error: SessionError) {
    failure
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get_or_insert(error);
}

fn to_samples(bytes: &[u8]) -> Vec<i16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect()
}

fn to_bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

impl AudioRouter {
    /// Creates lanes for `input`; `None` when echo processing cannot be initialized.
    pub fn new(input: Input, sample_rate: u32) -> Option<(Self, LaneReceivers)> {
        let wants_system = matches!(input, Input::System | Input::Both);
        let wants_mic = matches!(input, Input::Microphone | Input::Both);
        let (system, system_rx) = wants_system.then(|| audio_lane(sample_rate)).unzip();
        let (microphone, mic_rx) = wants_mic.then(|| audio_lane(sample_rate)).unzip();
        let microphone = microphone.map(Arc::new);
        let failure = Arc::new(Mutex::new(None));
        let echo = match (&microphone, input) {
            (Some(lane), Input::Both) => {
                let lane = Arc::clone(lane);
                let output =
                    Box::new(
                        move |samples: Vec<i16>| match lane.push(to_bytes(&samples)) {
                            Ok(()) | Err(PushError::Closed) => Ok(()),
                            Err(PushError::Backpressure) => Err(SessionError::AudioBackpressure),
                        },
                    );
                Some(EchoStage::spawn(sample_rate, output)?)
            }
            _ => None,
        };
        let router = Self {
            system: system.map(Arc::new),
            microphone,
            echo,
            started: Instant::now(),
            stopped: AtomicBool::new(false),
            failure,
        };
        Some((
            router,
            LaneReceivers {
                system: system_rx,
                microphone: mic_rx,
            },
        ))
    }

    fn lane_result(&self, result: Result<(), PushError>) -> PushOutcome {
        match result {
            Ok(()) => PushOutcome::Accepted,
            Err(PushError::Closed) => PushOutcome::Rejected,
            Err(PushError::Backpressure) => {
                record(&self.failure, SessionError::AudioBackpressure);
                PushOutcome::Backpressure
            }
        }
    }

    fn echo_result(&self, result: Result<(), EchoPushError>) -> PushOutcome {
        match result {
            Ok(()) => PushOutcome::Accepted,
            Err(EchoPushError::Closed) => PushOutcome::Rejected,
            Err(EchoPushError::Overflow) => {
                record(&self.failure, SessionError::EchoProcessingFailed);
                PushOutcome::Backpressure
            }
        }
    }

    /// Validates and enqueues one chunk; never waits.
    pub fn push(&self, source: u32, bytes: &[u8]) -> PushOutcome {
        let malformed =
            bytes.is_empty() || bytes.len() > MAX_PUSH_BYTES || !bytes.len().is_multiple_of(2);
        if malformed || self.stopped.load(Ordering::Acquire) {
            return PushOutcome::Rejected;
        }
        let at = self.started.elapsed();
        match source {
            SOURCE_SYSTEM => {
                let Some(lane) = &self.system else {
                    return PushOutcome::Rejected;
                };
                let sent = self.lane_result(lane.push(bytes.to_vec()));
                match (&self.echo, sent) {
                    (Some(echo), PushOutcome::Accepted) => {
                        self.echo_result(echo.push_reference(&to_samples(bytes), at))
                    }
                    _ => sent,
                }
            }
            SOURCE_MICROPHONE => match (&self.echo, &self.microphone) {
                (Some(echo), _) => self.echo_result(echo.push_mic(&to_samples(bytes), at)),
                (None, Some(lane)) => self.lane_result(lane.push(bytes.to_vec())),
                (None, None) => PushOutcome::Rejected,
            },
            _ => PushOutcome::Rejected,
        }
    }

    /// Stops accepting audio, lets echo processing finish, then closes the lanes so their
    /// receivers drain and end.
    pub fn stop(&self) -> Result<(), SessionError> {
        self.stopped.store(true, Ordering::Release);
        let echo = self.echo.as_ref().map_or(Ok(()), EchoStage::stop);
        if let Err(error) = echo {
            record(&self.failure, error);
        }
        for lane in [&self.system, &self.microphone].into_iter().flatten() {
            lane.close();
        }
        self.failure().map_or(Ok(()), Err)
    }

    /// First failure caused by audio handling, reported once by the session.
    pub fn failure(&self) -> Option<SessionError> {
        let own = *self
            .failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        own.or_else(|| self.echo.as_ref().and_then(EchoStage::failure))
    }

    pub fn echo_report(&self) -> Option<EchoReport> {
        self.echo.as_ref().map(EchoStage::report)
    }
}
