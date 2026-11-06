//! Audio queues and echo processing, offline (audio.md §4).

use std::time::Duration;

use sheng_runtime::audio::echo::EchoProcessor;
use sheng_runtime::audio::router::{SOURCE_MICROPHONE, SOURCE_SYSTEM};
use sheng_runtime::audio::{audio_lane, drain_within, AudioRouter, PushOutcome};
use sheng_runtime::config::request::Input;
use sheng_runtime::provider::scripted::{ScriptedSpeech, SpeechScript};
use sheng_runtime::provider::SessionError;

const RATE: u32 = 16_000;
const FRAME: usize = 160;

fn pcm(samples: impl IntoIterator<Item = i16>) -> Vec<u8> {
    samples.into_iter().flat_map(i16::to_le_bytes).collect()
}

/// Deterministic white noise in [-amplitude, amplitude].
fn noise(count: usize, amplitude: f64, seed: u64) -> Vec<f64> {
    let mut state = seed;
    (0..count)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as f64 / f64::from(1u32 << 31) * 2.0 - 1.0) * amplitude
        })
        .collect()
}

fn rms(samples: &[i16]) -> f64 {
    let energy: f64 = samples.iter().map(|s| f64::from(*s).powi(2)).sum();
    (energy / samples.len().max(1) as f64).sqrt()
}

#[tokio::test]
async fn dual_source_stop_delivers_system_bytes_and_every_microphone_sample() {
    let (router, receivers) = AudioRouter::new(Input::Both, RATE).expect("echo initializes");
    let (mut system_client, system_log) = ScriptedSpeech::new(SpeechScript::default());
    let (mut mic_client, mic_log) = ScriptedSpeech::new(SpeechScript::default());
    let (mut system_rx, mut mic_rx) = (receivers.system.unwrap(), receivers.microphone.unwrap());
    let system_task = tokio::spawn(async move { system_rx.run(&mut system_client).await });
    let mic_task = tokio::spawn(async move { mic_rx.run(&mut mic_client).await });
    let system_audio = pcm((0..3_200).map(|i| (i % 300) as i16));
    let mic_samples = 2 * FRAME * 5 + 37;

    for chunk in system_audio.chunks(640) {
        assert_eq!(router.push(SOURCE_SYSTEM, chunk), PushOutcome::Accepted);
    }
    let mic_audio = pcm(std::iter::repeat_n(100i16, mic_samples));
    for chunk in mic_audio.chunks(500) {
        assert_eq!(router.push(SOURCE_MICROPHONE, chunk), PushOutcome::Accepted);
    }
    router.stop().expect("stops cleanly");
    let late = router.push(SOURCE_MICROPHONE, &mic_audio[..2]);
    drain_within(async { system_task.await.unwrap() })
        .await
        .unwrap();
    drain_within(async { mic_task.await.unwrap() })
        .await
        .unwrap();

    assert_eq!(system_log.snapshot().audio, system_audio);
    assert_eq!(mic_log.snapshot().audio.len(), mic_samples * 2);
    assert_eq!(late, PushOutcome::Rejected);
    assert!(router.echo_report().unwrap().frames >= (mic_samples / FRAME) as u64);
}

#[tokio::test]
async fn full_send_queue_fails_the_session_with_backpressure() {
    let (router, _receivers) = AudioRouter::new(Input::System, RATE).unwrap();
    let chunk = pcm([1i16; 50]);
    let accepted = (0..20)
        .filter(|_| router.push(SOURCE_SYSTEM, &chunk) == PushOutcome::Accepted)
        .count();

    let overflow = router.push(SOURCE_SYSTEM, &chunk);

    assert_eq!(accepted, 20);
    assert_eq!(overflow, PushOutcome::Backpressure);
    assert_eq!(overflow.abi_code(), -2);
    assert_eq!(router.failure(), Some(SessionError::AudioBackpressure));
}

#[tokio::test]
async fn invalid_pushes_are_rejected() {
    let (router, _receivers) = AudioRouter::new(Input::System, RATE).unwrap();

    let outcomes = [
        router.push(SOURCE_SYSTEM, &[]),
        router.push(SOURCE_SYSTEM, &[1, 2, 3]),
        router.push(SOURCE_SYSTEM, &vec![0; 65_538]),
        router.push(SOURCE_MICROPHONE, &[1, 2]),
        router.push(7, &[1, 2]),
    ];

    assert!(outcomes.iter().all(|o| *o == PushOutcome::Rejected));
    assert_eq!(
        router.push(SOURCE_SYSTEM, &vec![0; 65_536]),
        PushOutcome::Accepted
    );
}

#[tokio::test(start_paused = true)]
async fn stuck_transport_is_abandoned_after_one_second_on_stop() {
    let (router, receivers) = AudioRouter::new(Input::System, RATE).unwrap();
    let script = SpeechScript {
        send_delay: Some(Duration::from_secs(30)),
        ..Default::default()
    };
    let (mut client, _log) = ScriptedSpeech::new(script);
    let mut receiver = receivers.system.unwrap();
    for _ in 0..3 {
        router.push(SOURCE_SYSTEM, &pcm([0i16; 160]));
    }
    let started = tokio::time::Instant::now();

    router.stop().unwrap();
    let result = drain_within(receiver.run(&mut client)).await;

    assert_eq!(result, Err(SessionError::AudioBackpressure));
    assert_eq!(started.elapsed(), Duration::from_secs(1));
}

#[tokio::test]
async fn lane_releases_byte_counts_when_sent_or_dropped() {
    let (lane, mut receiver) = audio_lane(RATE);
    let (mut client, _log) = ScriptedSpeech::new(SpeechScript::default());
    lane.push(vec![0; 100]).unwrap();
    lane.push(vec![0; 60]).unwrap();
    let queued = lane.pending_bytes();

    lane.close();
    receiver.run(&mut client).await.unwrap();
    let after_send = lane.pending_bytes();
    let (other, other_receiver) = audio_lane(RATE);
    other.push(vec![0; 40]).unwrap();
    drop(other_receiver);

    assert_eq!((queued, after_send, other.pending_bytes()), (160, 0, 0));
}

#[test]
fn echo_is_reduced_while_near_end_speech_is_kept() {
    let mut processor = EchoProcessor::new(RATE).unwrap();
    let seconds = 10;
    let total = RATE as usize * seconds;
    let delay = 480;
    let far = noise(total + delay, 6_000.0, 7);
    let near_start = RATE as usize * 8;
    let near: Vec<f64> = (0..total)
        .map(|i| {
            if i >= near_start {
                5_000.0 * (i as f64 * 0.11).sin()
            } else {
                0.0
            }
        })
        .collect();
    let mut output = Vec::with_capacity(total);
    let mut input = Vec::with_capacity(total);

    for (index, block) in (0..total).step_by(FRAME).enumerate() {
        let at = Duration::from_millis(index as u64 * 10);
        let reference: Vec<i16> = far[delay + block..delay + block + FRAME]
            .iter()
            .map(|x| *x as i16)
            .collect();
        let mic: Vec<i16> = (block..block + FRAME)
            .map(|i| (0.5 * far[i] + near[i]).clamp(-32_000.0, 32_000.0) as i16)
            .collect();
        processor.push_reference(&reference, at);
        output.extend(processor.push_mic(&mic, at));
        input.extend(mic);
    }

    let echo_only = RATE as usize * 6..near_start;
    let reduction = rms(&input[echo_only.clone()]) / rms(&output[echo_only]);
    let near_only: Vec<i16> = near[near_start..].iter().map(|x| *x as i16).collect();
    let kept = rms(&output[near_start..]) / rms(&near_only);
    assert!(reduction > 3.0, "echo reduced only {reduction:.2}x");
    assert!(kept > 0.6, "near-end kept only {kept:.2}");
    let report = processor.stats().report();
    assert!(report.reduction_db.unwrap() > 3.0);
    assert_eq!(report.missing_references, 0);
}

#[test]
fn capture_gap_rebuilds_the_canceller_and_missing_references_are_counted() {
    let mut processor = EchoProcessor::new(RATE).unwrap();
    let frame = vec![1_000i16; FRAME];
    processor.push_reference(&frame, Duration::ZERO);
    processor.push_mic(&frame, Duration::ZERO);

    let passthrough = processor.push_mic(&frame, Duration::from_millis(10));
    processor.push_reference(&frame, Duration::from_millis(500));
    processor.push_mic(&frame, Duration::from_millis(500));

    let stats = processor.stats().clone();
    assert_eq!(passthrough, frame);
    assert_eq!(stats.frames, 3);
    assert_eq!(stats.matched_references, 2);
    assert_eq!(stats.missing_references, 1);
    assert_eq!(stats.resets, 1);
}

#[test]
fn final_partial_frame_is_zero_padded_and_trimmed() {
    let mut processor = EchoProcessor::new(RATE).unwrap();

    let whole = processor.push_mic(&[5i16; FRAME + 37], Duration::ZERO);
    let rest = processor.finish();

    assert_eq!((whole.len(), rest.len()), (FRAME, 37));
    assert!(processor.finish().is_empty());
}
