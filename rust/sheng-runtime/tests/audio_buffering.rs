//! One source silent or bursty while the other talks (audio.md §3.1, tests 6–7): the
//! microphone never waits for an absent echo reference, its latency stays bounded and no
//! sample is lost. Audio is pushed at a fixed multiple of real time to keep the run short;
//! nothing here waits on capture time, so the bound is the same as at real time.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sheng_runtime::audio::echo::processor::MAX_REFERENCE_FRAMES;
use sheng_runtime::audio::echo::EchoProcessor;
use sheng_runtime::audio::router::{SOURCE_MICROPHONE, SOURCE_SYSTEM};
use sheng_runtime::audio::{drain_within, AudioRouter, PushOutcome};
use sheng_runtime::config::request::Input;
use sheng_runtime::provider::scripted::{ScriptedSpeech, SpeechScript};
use sheng_runtime::provider::{BoxFuture, EventSink, ProviderError, SpeechClient};

const RATE: u32 = 16_000;
/// Microphone chunk: 20 ms of PCM16.
const MIC_CHUNK: usize = 640;
const CHUNK_AUDIO: Duration = Duration::from_millis(20);
/// Audio is pushed this many times faster than real time.
const SPEEDUP: u32 = 5;
/// audio.md §3.1: alignment delay (about 350 ms) plus one frame, with some margin.
const MAX_LATENCY: Duration = Duration::from_millis(450);

/// Records when each chunk reached the microphone's send side.
#[derive(Default, Clone)]
struct Arrivals(Arc<Mutex<Vec<(Instant, usize)>>>);

impl SpeechClient for Arrivals {
    fn connect(&mut self, _events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async { Ok(()) })
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        self.0.lock().unwrap().push((Instant::now(), pcm.len()));
        Box::pin(async { Ok(()) })
    }

    fn heartbeat(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async { Ok(()) })
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async { Ok(()) })
    }

    fn disconnect(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// What the system source does at each microphone chunk `index`.
type SystemPlan = fn(usize) -> Option<Vec<u8>>;

fn tone(samples: usize, seed: usize) -> Vec<u8> {
    (0..samples)
        .flat_map(|i| (((i + seed) % 200) as i16 * 40 - 4_000).to_le_bytes())
        .collect()
}

struct Run {
    pushed: Vec<(Instant, usize)>,
    arrived: Vec<(Instant, usize)>,
    system_bytes: usize,
    system_received: usize,
}

/// Pushes `duration` of microphone audio at `SPEEDUP`× real time with `system` alongside,
/// then stops and drains both lanes.
async fn run(duration: Duration, system: SystemPlan) -> Run {
    let (router, receivers) = AudioRouter::new(Input::Both, RATE).expect("echo initializes");
    let router = Arc::new(router);
    let arrivals = Arrivals::default();
    let (mut system_rx, mut mic_rx) = (receivers.system.unwrap(), receivers.microphone.unwrap());
    let (mut system_client, system_log) = ScriptedSpeech::new(SpeechScript::default());
    let mut mic_client = arrivals.clone();
    let system_task = tokio::spawn(async move { system_rx.run(&mut system_client).await });
    let mic_task = tokio::spawn(async move { mic_rx.run(&mut mic_client).await });

    let pusher = Arc::clone(&router);
    let chunks = (duration.as_millis() / CHUNK_AUDIO.as_millis()) as usize;
    let (pushed, system_bytes) = tokio::task::spawn_blocking(move || {
        let start = Instant::now();
        let mut pushed = Vec::with_capacity(chunks);
        let mut system_bytes = 0;
        for index in 0..chunks {
            let due = start + CHUNK_AUDIO * index as u32 / SPEEDUP;
            std::thread::sleep(due.saturating_duration_since(Instant::now()));
            if let Some(bytes) = system(index) {
                system_bytes += bytes.len();
                assert_eq!(pusher.push(SOURCE_SYSTEM, &bytes), PushOutcome::Accepted);
            }
            let mic = tone(MIC_CHUNK / 2, index);
            pushed.push((Instant::now(), mic.len()));
            assert_eq!(pusher.push(SOURCE_MICROPHONE, &mic), PushOutcome::Accepted);
        }
        (pushed, system_bytes)
    })
    .await
    .unwrap();

    assert_eq!(router.failure(), None);
    router.stop().expect("stops cleanly");
    drain_within(async { system_task.await.unwrap() })
        .await
        .unwrap();
    drain_within(async { mic_task.await.unwrap() })
        .await
        .unwrap();
    let arrived = arrivals.0.lock().unwrap().clone();
    Run {
        pushed,
        arrived,
        system_bytes,
        system_received: system_log.snapshot().audio.len(),
    }
}

/// Latency of each pushed chunk: until the send side has received all bytes up to its end.
fn latencies(run: &Run) -> Vec<Duration> {
    let mut arrived = run.arrived.iter().scan(0, |total, (at, len)| {
        *total += len;
        Some((*at, *total))
    });
    let mut current = arrived.next();
    let mut pushed_total = 0;
    run.pushed
        .iter()
        .map(|(at, len)| {
            pushed_total += len;
            while current.is_some_and(|(_, total)| total < pushed_total) {
                current = arrived.next();
            }
            let (reached, _) = current.expect("every pushed sample arrives");
            reached.saturating_duration_since(*at)
        })
        .collect()
}

fn mean(values: &[Duration]) -> Duration {
    values.iter().sum::<Duration>() / values.len().max(1) as u32
}

fn assert_bounded_and_complete(run: &Run) {
    let pushed: usize = run.pushed.iter().map(|(_, len)| len).sum();
    let arrived: usize = run.arrived.iter().map(|(_, len)| len).sum();
    assert_eq!(arrived, pushed, "no microphone sample is lost");
    assert_eq!(run.system_received, run.system_bytes);
    let latencies = latencies(run);
    let worst = latencies.iter().max().copied().unwrap_or_default();
    assert!(worst <= MAX_LATENCY, "worst microphone latency {worst:?}");
    let third = latencies.len() / 3;
    let (early, late) = (mean(&latencies[..third]), mean(&latencies[2 * third..]));
    assert!(
        late <= early + Duration::from_millis(50),
        "latency grows: {early:?} → {late:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn microphone_alone_never_waits_for_an_absent_reference() {
    let run = run(Duration::from_secs(30), |_| None).await;

    assert_bounded_and_complete(&run);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn microphone_with_a_silent_system_source_keeps_bounded_latency() {
    let run = run(Duration::from_secs(30), |_| Some(vec![0; MIC_CHUNK])).await;

    assert_bounded_and_complete(&run);
}

/// Every 2 s of audio, 2 s of system sound arrive at once.
fn bursts(index: usize) -> Option<Vec<u8>> {
    const CHUNKS_PER_BURST: usize = 100;
    index
        .is_multiple_of(CHUNKS_PER_BURST)
        .then(|| tone(RATE as usize * 2, index))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bursty_system_sound_does_not_fail_or_slow_the_microphone() {
    let run = run(Duration::from_secs(60), bursts).await;

    assert_bounded_and_complete(&run);
}

#[test]
fn bursty_reference_stays_within_its_one_second_buffer() {
    let mut processor = EchoProcessor::new(RATE).expect("canceller");
    let frame = RATE as usize / 100;
    let mut max_buffered = 0;
    for step in 0..3_000u32 {
        let at = Duration::from_millis(u64::from(step) * 20);
        if step.is_multiple_of(100) {
            let burst: Vec<i16> = (0..RATE as usize * 2).map(|i| (i % 300) as i16).collect();
            processor.push_reference(&burst, at);
        }
        let mic = vec![100i16; 2 * frame];
        let out = processor.push_mic(&mic, at);
        assert_eq!(out.len(), mic.len(), "the microphone never waits");
        max_buffered = max_buffered.max(processor.buffered_reference_frames());
    }

    assert!(max_buffered <= MAX_REFERENCE_FRAMES, "{max_buffered}");
    assert!(processor.stats().matched_references > 0);
}
