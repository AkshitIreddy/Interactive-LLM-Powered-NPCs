//! Developer-only audio playback infrastructure.
//!
//! The production architecture sends PCM to the native media broker. This module
//! provides an isolated CPAL/WASAPI qualification path for real 24 kHz mono
//! provider output before that transport is wired. It intentionally reports PCM
//! submitted to device callbacks, never PCM proven audible at the speakers.

use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    time::Duration,
};

use npc_runtime_core::AudioChunk;

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

/// Preallocated lock-free ring with one producer and one consumer.
#[derive(Debug)]
struct SpscPcmRing {
    samples: Box<[UnsafeCell<i16>]>,
    capacity: usize,
    write_cursor: AtomicUsize,
    read_cursor: AtomicUsize,
}

// SAFETY: the SPSC contract guarantees one writer and one reader. The producer
// publishes initialized cells with a Release store and the consumer observes them
// with an Acquire load. A cell is never read and written concurrently.
unsafe impl Sync for SpscPcmRing {}

impl SpscPcmRing {
    fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "PCM ring capacity must be non-zero");
        let samples = (0..capacity)
            .map(|_| UnsafeCell::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            samples,
            capacity,
            write_cursor: AtomicUsize::new(0),
            read_cursor: AtomicUsize::new(0),
        }
    }

    fn push(&self, input: &[i16]) -> usize {
        let write = self.write_cursor.load(Ordering::Relaxed);
        let read = self.read_cursor.load(Ordering::Acquire);
        let available = self.capacity.saturating_sub(write.wrapping_sub(read));
        let count = available.min(input.len());
        for (offset, sample) in input[..count].iter().copied().enumerate() {
            let index = write.wrapping_add(offset) % self.capacity;
            // SAFETY: this is the sole producer, and `available` excludes every
            // cell still owned by the consumer.
            unsafe { *self.samples[index].get() = sample };
        }
        self.write_cursor
            .store(write.wrapping_add(count), Ordering::Release);
        count
    }

    fn pop(&self) -> Option<i16> {
        let read = self.read_cursor.load(Ordering::Relaxed);
        let write = self.write_cursor.load(Ordering::Acquire);
        if read == write {
            return None;
        }
        let index = read % self.capacity;
        // SAFETY: this is the sole consumer, and the producer published this cell
        // before advancing `write_cursor`.
        let sample = unsafe { *self.samples[index].get() };
        self.read_cursor
            .store(read.wrapping_add(1), Ordering::Release);
        Some(sample)
    }

    #[cfg(test)]
    fn available_read(&self) -> usize {
        self.write_cursor
            .load(Ordering::Acquire)
            .wrapping_sub(self.read_cursor.load(Ordering::Acquire))
    }
}

#[derive(Debug)]
struct SharedPlaybackState {
    ring: SpscPcmRing,
    accepted_source_frames: AtomicU64,
    submitted_device_frames: AtomicU64,
    underrun_device_frames: AtomicU64,
    end_of_stream: AtomicBool,
    cancelled: AtomicBool,
    completed: AtomicBool,
    stream_failed: AtomicBool,
}

impl SharedPlaybackState {
    fn new(capacity_frames: usize) -> Self {
        Self {
            ring: SpscPcmRing::new(capacity_frames),
            accepted_source_frames: AtomicU64::new(0),
            submitted_device_frames: AtomicU64::new(0),
            underrun_device_frames: AtomicU64::new(0),
            end_of_stream: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            completed: AtomicBool::new(false),
            stream_failed: AtomicBool::new(false),
        }
    }

    fn push(&self, samples: &[i16]) -> usize {
        if self.cancelled.load(Ordering::Acquire) || self.end_of_stream.load(Ordering::Acquire) {
            return 0;
        }
        let accepted = self.ring.push(samples);
        self.accepted_source_frames
            .fetch_add(accepted as u64, Ordering::Release);
        accepted
    }

    fn mark_end_of_stream(&self) {
        self.end_of_stream.store(true, Ordering::Release);
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    fn receipt(&self, device_sample_rate_hz: u32) -> DevSubmittedPlaybackReceipt {
        let accepted = self.accepted_source_frames.load(Ordering::Acquire);
        let device_frames = self.submitted_device_frames.load(Ordering::Acquire);
        let completed = self.completed.load(Ordering::Acquire)
            && !self.cancelled.load(Ordering::Acquire)
            && !self.stream_failed.load(Ordering::Acquire);
        let source_frames = if completed {
            accepted
        } else {
            accepted.min(equivalent_source_frames(
                device_frames,
                device_sample_rate_hz,
            ))
        };
        DevSubmittedPlaybackReceipt {
            source_frames_submitted: source_frames,
            device_frames_submitted: device_frames,
            source_duration: duration_for_frames(source_frames, INPUT_SAMPLE_RATE_HZ),
            device_duration: duration_for_frames(device_frames, device_sample_rate_hz),
            device_sample_rate_hz,
            underrun_device_frames: self.underrun_device_frames.load(Ordering::Acquire),
            completed,
            cancelled: self.cancelled.load(Ordering::Acquire),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevSubmittedPlaybackReceipt {
    pub source_frames_submitted: u64,
    pub device_frames_submitted: u64,
    pub source_duration: Duration,
    pub device_duration: Duration,
    pub device_sample_rate_hz: u32,
    pub underrun_device_frames: u64,
    pub completed: bool,
    pub cancelled: bool,
}

#[derive(Debug)]
struct LinearResampler {
    device_sample_rate_hz: u32,
    phase: u64,
    current: Option<f32>,
    next: Option<f32>,
}

impl LinearResampler {
    fn new(device_sample_rate_hz: u32) -> Self {
        assert!(
            device_sample_rate_hz > 0,
            "device sample rate must be non-zero"
        );
        Self {
            device_sample_rate_hz,
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
        let mut underruns = 0_u64;

        for frame in &mut frames {
            if state.cancelled.load(Ordering::Acquire) {
                frame.fill(silence);
                continue;
            }
            if self.target_reached(state, submitted) {
                frame.fill(silence);
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
        if self.target_reached(state, 0) {
            state.completed.store(true, Ordering::Release);
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
            self.current = state.ring.pop().map(normalize_s16);
        }
        let current = self.current?;
        if self.next.is_none() {
            self.next = state.ring.pop().map(normalize_s16);
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
            self.next = state.ring.pop().map(normalize_s16);
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
        let ring = Arc::new(SpscPcmRing::new(31));
        let producer_ring = Arc::clone(&ring);
        let producer = thread::spawn(move || {
            for value in 0_i16..4_000 {
                while producer_ring.push(&[value]) == 0 {
                    thread::yield_now();
                }
            }
        });
        for expected in 0_i16..4_000 {
            let actual = loop {
                if let Some(sample) = ring.pop() {
                    break sample;
                }
                thread::yield_now();
            };
            assert_eq!(actual, expected);
        }
        producer.join().expect("producer should not panic");
        assert_eq!(ring.available_read(), 0);
    }

    #[test]
    fn resamples_24khz_to_48khz_with_exact_submitted_accounting() {
        let state = SharedPlaybackState::new(512);
        let input: Vec<i16> = (0..240).map(|value| value * 100).collect();
        assert_eq!(state.push(&input), input.len());
        state.mark_end_of_stream();
        let mut resampler = LinearResampler::new(48_000);
        let mut output = vec![0.0_f32; 480 * 2];
        resampler.render(&state, &mut output, 2, 0.0, |sample| sample);

        let receipt = state.receipt(48_000);
        assert!(receipt.completed);
        assert_eq!(receipt.source_frames_submitted, 240);
        assert_eq!(receipt.device_frames_submitted, 480);
        assert_eq!(receipt.source_duration, Duration::from_millis(10));
        assert_eq!(receipt.device_duration, Duration::from_millis(10));
        assert_eq!(receipt.underrun_device_frames, 0);
    }

    #[test]
    fn resamples_fractional_44100_rate_to_bounded_exact_duration() {
        let state = SharedPlaybackState::new(256);
        let input = vec![1_000_i16; 240];
        assert_eq!(state.push(&input), 240);
        state.mark_end_of_stream();
        let mut resampler = LinearResampler::new(44_100);
        let mut output = vec![0_i16; 441];
        resampler.render(&state, &mut output, 1, 0, |sample| {
            (sample * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16
        });

        let receipt = state.receipt(44_100);
        assert!(receipt.completed);
        assert_eq!(receipt.source_frames_submitted, 240);
        assert_eq!(receipt.device_frames_submitted, 441);
        assert_eq!(receipt.device_duration, Duration::from_millis(10));
    }

    #[test]
    fn resamples_to_lower_device_rate_without_losing_duration_accounting() {
        let state = SharedPlaybackState::new(256);
        let input: Vec<i16> = (0..240).map(|value| value * 50).collect();
        assert_eq!(state.push(&input), 240);
        state.mark_end_of_stream();
        let mut resampler = LinearResampler::new(16_000);
        let mut output = vec![0.0_f32; 160];
        resampler.render(&state, &mut output, 1, 0.0, |sample| sample);

        let receipt = state.receipt(16_000);
        assert!(receipt.completed);
        assert_eq!(receipt.source_frames_submitted, 240);
        assert_eq!(receipt.device_frames_submitted, 160);
        assert_eq!(receipt.device_duration, Duration::from_millis(10));
    }

    #[test]
    fn cancellation_receipt_counts_only_device_duration_already_submitted() {
        let state = SharedPlaybackState::new(512);
        assert_eq!(state.push(&vec![2_000_i16; 480]), 480);
        let mut resampler = LinearResampler::new(48_000);
        let mut output = vec![0.0_f32; 480];
        resampler.render(&state, &mut output, 1, 0.0, |sample| sample);
        state.cancel();

        let receipt = state.receipt(48_000);
        assert!(!receipt.completed);
        assert!(receipt.cancelled);
        assert_eq!(receipt.device_frames_submitted, 480);
        assert_eq!(receipt.source_frames_submitted, 240);
        assert_eq!(receipt.source_duration, Duration::from_millis(10));
    }

    #[test]
    fn underflow_fills_silence_without_inflating_submission() {
        let state = SharedPlaybackState::new(8);
        assert_eq!(state.push(&[1_000]), 1);
        let mut resampler = LinearResampler::new(48_000);
        let mut output = [99_i16; 8];
        resampler.render(&state, &mut output, 2, 0, |_| 1);

        assert_eq!(output, [0; 8]);
        let receipt = state.receipt(48_000);
        assert_eq!(receipt.device_frames_submitted, 0);
        assert_eq!(receipt.source_frames_submitted, 0);
        assert_eq!(receipt.underrun_device_frames, 4);
    }
}
