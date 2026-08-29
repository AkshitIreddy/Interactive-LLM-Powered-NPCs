//! Developer-only audio playback infrastructure.
//!
//! The production architecture sends PCM to the native media broker. This module
//! provides an isolated CPAL/WASAPI qualification path for real 24 kHz mono
//! provider output before that transport is wired. It intentionally reports PCM
//! submitted to device callbacks, never PCM proven audible at the speakers.

use std::{
    cell::UnsafeCell,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use npc_runtime_core::AudioChunk;
use tokio::sync::Notify;

pub const INPUT_SAMPLE_RATE_HZ: u32 = 24_000;
pub const INPUT_CHANNELS: u16 = 1;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PcmFormatError {
    #[error("expected 24 kHz PCM, received {actual_hz} Hz")]
    SampleRate { actual_hz: u32 },
    #[error("expected mono PCM, received {actual_channels} channels")]
    Channels { actual_channels: u16 },
    #[error("PCM S16LE payload contains an incomplete sample byte")]
    IncompleteSample,
}

pub fn decode_chunk(chunk: &AudioChunk) -> Result<Vec<i16>, PcmFormatError> {
    if chunk.sample_rate_hz != INPUT_SAMPLE_RATE_HZ {
        return Err(PcmFormatError::SampleRate {
            actual_hz: chunk.sample_rate_hz,
        });
    }
    if chunk.channels != INPUT_CHANNELS {
        return Err(PcmFormatError::Channels {
            actual_channels: chunk.channels,
        });
    }
    if !chunk.pcm_s16le.len().is_multiple_of(size_of::<i16>()) {
        return Err(PcmFormatError::IncompleteSample);
    }

    Ok(chunk
        .pcm_s16le
        .chunks_exact(size_of::<i16>())
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect())
}

#[derive(Debug)]
struct SpscStorage {
    samples: Box<[UnsafeCell<i16>]>,
    capacity: usize,
    write_cursor: AtomicUsize,
    read_cursor: AtomicUsize,
}

// SAFETY: the SPSC contract guarantees one writer and one reader. The producer
// publishes initialized cells with a Release store and the consumer observes them
// with an Acquire load. A cell is never read and written concurrently.
unsafe impl Sync for SpscStorage {}

#[derive(Debug)]
struct PcmProducer {
    storage: Arc<SpscStorage>,
}

#[derive(Debug)]
struct PcmConsumer {
    storage: Arc<SpscStorage>,
}

/// Create one owned producer and one owned consumer. Neither handle is cloneable,
/// so safe code cannot violate the SPSC contract used by the unsafe storage.
fn spsc_pcm_ring(capacity: usize) -> (PcmProducer, PcmConsumer) {
    assert!(capacity > 0, "PCM ring capacity must be non-zero");
    let samples = (0..capacity)
        .map(|_| UnsafeCell::new(0))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let storage = Arc::new(SpscStorage {
        samples,
        capacity,
        write_cursor: AtomicUsize::new(0),
        read_cursor: AtomicUsize::new(0),
    });
    (
        PcmProducer {
            storage: Arc::clone(&storage),
        },
        PcmConsumer { storage },
    )
}

impl PcmProducer {
    fn push(&mut self, input: &[i16]) -> usize {
        let write = self.storage.write_cursor.load(Ordering::Relaxed);
        let read = self.storage.read_cursor.load(Ordering::Acquire);
        let available = self
            .storage
            .capacity
            .saturating_sub(write.wrapping_sub(read));
        let count = available.min(input.len());
        for (offset, sample) in input[..count].iter().copied().enumerate() {
            let index = write.wrapping_add(offset) % self.storage.capacity;
            // SAFETY: this is the sole producer, and `available` excludes every
            // cell still owned by the consumer.
            unsafe { *self.storage.samples[index].get() = sample };
        }
        self.storage
            .write_cursor
            .store(write.wrapping_add(count), Ordering::Release);
        count
    }
}

impl PcmConsumer {
    fn pop(&mut self) -> Option<i16> {
        let read = self.storage.read_cursor.load(Ordering::Relaxed);
        let write = self.storage.write_cursor.load(Ordering::Acquire);
        if read == write {
            return None;
        }
        let index = read % self.storage.capacity;
        // SAFETY: this is the sole consumer, and the producer published this cell
        // before advancing `write_cursor`.
        let sample = unsafe { *self.storage.samples[index].get() };
        self.storage
            .read_cursor
            .store(read.wrapping_add(1), Ordering::Release);
        Some(sample)
    }

    #[cfg(test)]
    fn available_read(&self) -> usize {
        self.storage
            .write_cursor
            .load(Ordering::Acquire)
            .wrapping_sub(self.storage.read_cursor.load(Ordering::Acquire))
    }
}

#[derive(Debug)]
struct SharedPlaybackState {
    accepted_source_frames: AtomicU64,
    submitted_device_frames: AtomicU64,
    drain_silence_device_frames: AtomicU64,
    underrun_device_frames: AtomicU64,
    end_of_stream: AtomicBool,
    cancelled: AtomicBool,
    source_submission_complete: AtomicBool,
    endpoint_drain_complete: AtomicBool,
    stream_failed: AtomicBool,
    cancellation_notify: Notify,
    terminal_notify: Notify,
}

impl SharedPlaybackState {
    fn new() -> Self {
        Self {
            accepted_source_frames: AtomicU64::new(0),
            submitted_device_frames: AtomicU64::new(0),
            drain_silence_device_frames: AtomicU64::new(0),
            underrun_device_frames: AtomicU64::new(0),
            end_of_stream: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            source_submission_complete: AtomicBool::new(false),
            endpoint_drain_complete: AtomicBool::new(false),
            stream_failed: AtomicBool::new(false),
            cancellation_notify: Notify::new(),
            terminal_notify: Notify::new(),
        }
    }

    fn push(&self, producer: &mut PcmProducer, samples: &[i16]) -> usize {
        if self.cancelled.load(Ordering::Acquire) || self.end_of_stream.load(Ordering::Acquire) {
            return 0;
        }
        let accepted = producer.push(samples);
        self.accepted_source_frames
            .fetch_add(accepted as u64, Ordering::Release);
        accepted
    }

    fn mark_end_of_stream(&self) {
        self.end_of_stream.store(true, Ordering::Release);
    }

    fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            self.cancellation_notify.notify_one();
        }
    }

    async fn cancelled(&self) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let notified = self.cancellation_notify.notified();
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        notified.await;
    }

    fn notify_terminal(&self) {
        self.terminal_notify.notify_one();
    }

    async fn terminal_notified(&self) {
        self.terminal_notify.notified().await;
    }

    fn receipt(&self, device: &DevOutputDeviceTelemetry) -> DevSubmittedPlaybackReceipt {
        let accepted = self.accepted_source_frames.load(Ordering::Acquire);
        let device_frames = self.submitted_device_frames.load(Ordering::Acquire);
        let source_submission_complete = self.source_submission_complete.load(Ordering::Acquire);
        let source_frames = if source_submission_complete {
            accepted
        } else {
            accepted.min(equivalent_source_frames(
                device_frames,
                device.sample_rate_hz,
            ))
        };
        DevSubmittedPlaybackReceipt {
            source_frames_submitted: source_frames,
            device_frames_submitted: device_frames,
            source_duration: duration_for_frames(source_frames, INPUT_SAMPLE_RATE_HZ),
            device_duration: duration_for_frames(device_frames, device.sample_rate_hz),
            drain_silence_device_frames: self.drain_silence_device_frames.load(Ordering::Acquire),
            underrun_device_frames: self.underrun_device_frames.load(Ordering::Acquire),
            source_submission_complete,
            endpoint_drain_complete: self.endpoint_drain_complete.load(Ordering::Acquire),
            cancelled: self.cancelled.load(Ordering::Acquire),
            detached_cleanup_pending: false,
            device: device.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevOutputDeviceTelemetry {
    pub name: String,
    pub channels: u16,
    pub sample_rate_hz: u32,
    pub sample_format: String,
    pub selected_by_exact_name: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevSubmittedPlaybackReceipt {
    pub source_frames_submitted: u64,
    pub device_frames_submitted: u64,
    pub source_duration: Duration,
    pub device_duration: Duration,
    pub drain_silence_device_frames: u64,
    pub underrun_device_frames: u64,
    pub source_submission_complete: bool,
    pub endpoint_drain_complete: bool,
    pub cancelled: bool,
    pub detached_cleanup_pending: bool,
    pub device: DevOutputDeviceTelemetry,
}

#[derive(Debug)]
struct LinearResampler {
    device_sample_rate_hz: u32,
    consumer: PcmConsumer,
    drain_target_device_frames: u64,
    phase: u64,
    current: Option<f32>,
    next: Option<f32>,
}

impl LinearResampler {
    fn new(
        device_sample_rate_hz: u32,
        consumer: PcmConsumer,
        endpoint_drain_duration: Duration,
    ) -> Self {
        assert!(
            device_sample_rate_hz > 0,
            "device sample rate must be non-zero"
        );
        Self {
            device_sample_rate_hz,
            consumer,
            drain_target_device_frames: frames_for_duration(
                endpoint_drain_duration,
                device_sample_rate_hz,
            ),
            phase: 0,
            current: None,
            next: None,
        }
    }

    fn render<T: Copy>(
        &mut self,
        state: &SharedPlaybackState,
        output: &mut [T],
        channels: usize,
        silence: T,
        mut convert: impl FnMut(f32) -> T,
    ) {
        debug_assert!(channels > 0);
        let mut frames = output.chunks_exact_mut(channels);
        let mut submitted = 0_u64;
        let mut drain_silence = 0_u64;
        let mut underruns = 0_u64;

        for frame in &mut frames {
            if state.cancelled.load(Ordering::Acquire) {
                frame.fill(silence);
                continue;
            }
            if self.target_reached(state, submitted) {
                state
                    .source_submission_complete
                    .store(true, Ordering::Release);
                frame.fill(silence);
                if state
                    .drain_silence_device_frames
                    .load(Ordering::Acquire)
                    .saturating_add(drain_silence)
                    < self.drain_target_device_frames
                {
                    drain_silence = drain_silence.saturating_add(1);
                }
                continue;
            }
            match self.next_sample(state) {
                Some(sample) => {
                    frame.fill(convert(sample));
                    submitted = submitted.saturating_add(1);
                }
                None => {
                    frame.fill(silence);
                    if !state.end_of_stream.load(Ordering::Acquire)
                        && state.accepted_source_frames.load(Ordering::Acquire) > 0
                    {
                        underruns = underruns.saturating_add(1);
                    }
                }
            }
        }
        frames.into_remainder().fill(silence);

        if submitted > 0 {
            state
                .submitted_device_frames
                .fetch_add(submitted, Ordering::Release);
        }
        if underruns > 0 {
            state
                .underrun_device_frames
                .fetch_add(underruns, Ordering::Relaxed);
        }
        if drain_silence > 0 {
            state
                .drain_silence_device_frames
                .fetch_add(drain_silence, Ordering::Release);
        }
        if self.target_reached(state, 0) {
            state
                .source_submission_complete
                .store(true, Ordering::Release);
            if state.drain_silence_device_frames.load(Ordering::Acquire)
                >= self.drain_target_device_frames
            {
                state.endpoint_drain_complete.store(true, Ordering::Release);
            }
        }
    }

    fn target_reached(&self, state: &SharedPlaybackState, local_submitted: u64) -> bool {
        if !state.end_of_stream.load(Ordering::Acquire) {
            return false;
        }
        let accepted = state.accepted_source_frames.load(Ordering::Acquire);
        let target = equivalent_device_frames(accepted, self.device_sample_rate_hz);
        state
            .submitted_device_frames
            .load(Ordering::Acquire)
            .saturating_add(local_submitted)
            >= target
    }

    fn next_sample(&mut self, state: &SharedPlaybackState) -> Option<f32> {
        if self.current.is_none() {
            self.current = self.consumer.pop().map(normalize_s16);
        }
        let current = self.current?;
        if self.next.is_none() {
            self.next = self.consumer.pop().map(normalize_s16);
        }
        let next = match self.next {
            Some(next) => next,
            None if state.end_of_stream.load(Ordering::Acquire) => current,
            None => return None,
        };

        let fraction = self.phase as f32 / self.device_sample_rate_hz as f32;
        let sample = current + (next - current) * fraction;
        self.phase = self.phase.saturating_add(u64::from(INPUT_SAMPLE_RATE_HZ));
        while self.phase >= u64::from(self.device_sample_rate_hz) {
            self.phase -= u64::from(self.device_sample_rate_hz);
            self.current = self.next.or(self.current);
            self.next = self.consumer.pop().map(normalize_s16);
            if self.next.is_none() && !state.end_of_stream.load(Ordering::Acquire) {
                break;
            }
        }
        Some(sample.clamp(-1.0, 1.0))
    }
}

fn normalize_s16(sample: i16) -> f32 {
    f32::from(sample) / 32_768.0
}

fn equivalent_device_frames(source_frames: u64, device_sample_rate_hz: u32) -> u64 {
    source_frames
        .saturating_mul(u64::from(device_sample_rate_hz))
        .saturating_add(u64::from(INPUT_SAMPLE_RATE_HZ) - 1)
        / u64::from(INPUT_SAMPLE_RATE_HZ)
}

fn equivalent_source_frames(device_frames: u64, device_sample_rate_hz: u32) -> u64 {
    device_frames.saturating_mul(u64::from(INPUT_SAMPLE_RATE_HZ)) / u64::from(device_sample_rate_hz)
}

fn frames_for_duration(duration: Duration, sample_rate_hz: u32) -> u64 {
    let numerator = duration
        .as_nanos()
        .saturating_mul(u128::from(sample_rate_hz));
    let frames = numerator.saturating_add(999_999_999) / 1_000_000_000;
    frames.min(u128::from(u64::MAX)) as u64
}

fn duration_for_frames(frames: u64, sample_rate_hz: u32) -> Duration {
    Duration::from_secs_f64(frames as f64 / f64::from(sample_rate_hz))
}

#[cfg(all(windows, feature = "dev-wasapi-audio"))]
mod wasapi;

#[cfg(all(windows, feature = "dev-wasapi-audio"))]
pub use wasapi::{DevWasapiAudioSink, DevWasapiConfig};

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread};

    use super::*;

    fn chunk(sample_rate_hz: u32, channels: u16, bytes: Vec<u8>) -> AudioChunk {
        AudioChunk {
            sequence: 1,
            sample_rate_hz,
            channels,
            pcm_s16le: bytes,
            end_of_stream: true,
        }
    }

    #[test]
    fn decodes_only_24_khz_mono_s16le() {
        let decoded = decode_chunk(&chunk(24_000, 1, vec![0x00, 0x80, 0xff, 0x7f]))
            .expect("valid PCM should decode");
        assert_eq!(decoded, vec![i16::MIN, i16::MAX]);
        assert_eq!(
            decode_chunk(&chunk(48_000, 1, vec![])),
            Err(PcmFormatError::SampleRate { actual_hz: 48_000 })
        );
        assert_eq!(
            decode_chunk(&chunk(24_000, 2, vec![])),
            Err(PcmFormatError::Channels { actual_channels: 2 })
        );
        assert_eq!(
            decode_chunk(&chunk(24_000, 1, vec![0])),
            Err(PcmFormatError::IncompleteSample)
        );
    }

    #[test]
    fn spsc_ring_preserves_order_under_concurrent_wraparound() {
        let (mut producer, mut consumer) = spsc_pcm_ring(31);
        let producer = thread::spawn(move || {
            for value in 0_i16..4_000 {
                while producer.push(&[value]) == 0 {
                    thread::yield_now();
                }
            }
        });
        for expected in 0_i16..4_000 {
            let actual = loop {
                if let Some(sample) = consumer.pop() {
                    break sample;
                }
                thread::yield_now();
            };
            assert_eq!(actual, expected);
        }
        producer.join().expect("producer should not panic");
        assert_eq!(consumer.available_read(), 0);
    }

    #[test]
    fn resamples_24khz_to_48khz_with_exact_submitted_accounting() {
        let state = SharedPlaybackState::new();
        let (mut producer, consumer) = spsc_pcm_ring(512);
        let input: Vec<i16> = (0..240).map(|value| value * 100).collect();
        assert_eq!(state.push(&mut producer, &input), input.len());
        state.mark_end_of_stream();
        let mut resampler = LinearResampler::new(48_000, consumer, Duration::ZERO);
        let mut output = vec![0.0_f32; 480 * 2];
        resampler.render(&state, &mut output, 2, 0.0, |sample| sample);

        let device = telemetry(48_000);
        let receipt = state.receipt(&device);
        assert!(receipt.source_submission_complete);
        assert!(receipt.endpoint_drain_complete);
        assert_eq!(receipt.source_frames_submitted, 240);
        assert_eq!(receipt.device_frames_submitted, 480);
        assert_eq!(receipt.source_duration, Duration::from_millis(10));
        assert_eq!(receipt.device_duration, Duration::from_millis(10));
        assert_eq!(receipt.underrun_device_frames, 0);
        assert_eq!(receipt.device, device);
    }

    #[test]
    fn resamples_fractional_44100_rate_to_bounded_exact_duration() {
        let state = SharedPlaybackState::new();
        let (mut producer, consumer) = spsc_pcm_ring(256);
        let input = vec![1_000_i16; 240];
        assert_eq!(state.push(&mut producer, &input), 240);
        state.mark_end_of_stream();
        let mut resampler = LinearResampler::new(44_100, consumer, Duration::ZERO);
        let mut output = vec![0_i16; 441];
        resampler.render(&state, &mut output, 1, 0, |sample| {
            (sample * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16
        });

        let receipt = state.receipt(&telemetry(44_100));
        assert!(receipt.endpoint_drain_complete);
        assert_eq!(receipt.source_frames_submitted, 240);
        assert_eq!(receipt.device_frames_submitted, 441);
        assert_eq!(receipt.device_duration, Duration::from_millis(10));
    }

    #[test]
    fn cancellation_receipt_counts_only_device_duration_already_submitted() {
        let state = SharedPlaybackState::new();
        let (mut producer, consumer) = spsc_pcm_ring(512);
        assert_eq!(state.push(&mut producer, &vec![2_000_i16; 480]), 480);
        let mut resampler = LinearResampler::new(48_000, consumer, Duration::ZERO);
        let mut output = vec![0.0_f32; 480];
        resampler.render(&state, &mut output, 1, 0.0, |sample| sample);
        state.cancel();

        let receipt = state.receipt(&telemetry(48_000));
        assert!(!receipt.endpoint_drain_complete);
        assert!(receipt.cancelled);
        assert_eq!(receipt.device_frames_submitted, 480);
        assert_eq!(receipt.source_frames_submitted, 240);
        assert_eq!(receipt.source_duration, Duration::from_millis(10));
    }

    #[test]
    fn underflow_fills_silence_without_inflating_submission() {
        let state = SharedPlaybackState::new();
        let (mut producer, consumer) = spsc_pcm_ring(8);
        assert_eq!(state.push(&mut producer, &[1_000]), 1);
        let mut resampler = LinearResampler::new(48_000, consumer, Duration::ZERO);
        let mut output = [99_i16; 8];
        resampler.render(&state, &mut output, 2, 0, |_| 1);

        assert_eq!(output, [0; 8]);
        let receipt = state.receipt(&telemetry(48_000));
        assert_eq!(receipt.device_frames_submitted, 0);
        assert_eq!(receipt.source_frames_submitted, 0);
        assert_eq!(receipt.underrun_device_frames, 4);
    }

    #[test]
    fn endpoint_drain_requires_bounded_silence_after_source_submission() {
        let state = SharedPlaybackState::new();
        let (mut producer, consumer) = spsc_pcm_ring(512);
        assert_eq!(state.push(&mut producer, &vec![1_000_i16; 240]), 240);
        state.mark_end_of_stream();
        let mut resampler = LinearResampler::new(48_000, consumer, Duration::from_millis(10));
        let mut source_output = vec![0.0_f32; 480];
        resampler.render(&state, &mut source_output, 1, 0.0, |sample| sample);

        let submitted = state.receipt(&telemetry(48_000));
        assert!(submitted.source_submission_complete);
        assert!(!submitted.endpoint_drain_complete);
        assert_eq!(submitted.drain_silence_device_frames, 0);

        let mut drain_output = vec![1.0_f32; 480];
        resampler.render(&state, &mut drain_output, 1, 0.0, |sample| sample);
        let drained = state.receipt(&telemetry(48_000));
        assert!(drained.endpoint_drain_complete);
        assert_eq!(drained.drain_silence_device_frames, 480);
        assert!(drain_output.iter().all(|sample| *sample == 0.0));
    }

    #[tokio::test]
    async fn internal_cancellation_notification_wakes_a_stalled_waiter() {
        let state = Arc::new(SharedPlaybackState::new());
        let waiter_state = Arc::clone(&state);
        let waiter = tokio::spawn(async move { waiter_state.cancelled().await });
        state.cancel();
        tokio::time::timeout(Duration::from_millis(50), waiter)
            .await
            .expect("cancellation waiter should wake")
            .expect("cancellation waiter should not panic");
    }

    fn telemetry(sample_rate_hz: u32) -> DevOutputDeviceTelemetry {
        DevOutputDeviceTelemetry {
            name: "test-device".into(),
            channels: 2,
            sample_rate_hz,
            sample_format: "f32".into(),
            selected_by_exact_name: false,
        }
    }
}
