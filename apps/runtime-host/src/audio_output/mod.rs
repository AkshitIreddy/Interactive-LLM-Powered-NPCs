//! Developer-only audio playback infrastructure.
//!
//! The production architecture sends PCM to the native media broker. This module
//! provides a deliberately isolated CPAL/WASAPI sink for qualifying real 24 kHz
//! mono provider output before that transport is wired. It is not compiled into a
//! normal runtime-host build.

use std::{collections::VecDeque, time::Duration};

use npc_runtime_core::{AudioChunk, PlaybackReceipt};

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

/// Decode one provider chunk after enforcing the dev sink's only accepted format.
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

/// A bounded, single-producer/single-consumer queue shared with the audio callback.
///
/// The callback never waits for data: an underrun is filled with silence. The queue
/// counts source frames only after they are copied into a device callback buffer,
/// which keeps partial cancellation receipts deterministic.
#[derive(Debug)]
pub struct BoundedPcmQueue {
    samples: VecDeque<i16>,
    capacity_frames: usize,
    accepted_frames: u64,
    rendered_frames: u64,
    underrun_frames: u64,
    end_of_stream: bool,
    cancelled: bool,
}

impl BoundedPcmQueue {
    pub fn new(capacity_frames: usize) -> Self {
        assert!(capacity_frames > 0, "PCM queue capacity must be non-zero");
        Self {
            samples: VecDeque::with_capacity(capacity_frames),
            capacity_frames,
            accepted_frames: 0,
            rendered_frames: 0,
            underrun_frames: 0,
            end_of_stream: false,
            cancelled: false,
        }
    }

    /// Push as many source frames as fit, returning the number accepted.
    pub fn push(&mut self, samples: &[i16]) -> usize {
        if self.cancelled || self.end_of_stream {
            return 0;
        }
        let count = samples
            .len()
            .min(self.capacity_frames.saturating_sub(self.samples.len()));
        self.samples.extend(samples[..count].iter().copied());
        self.accepted_frames = self.accepted_frames.saturating_add(count as u64);
        count
    }

    pub fn mark_end_of_stream(&mut self) {
        self.end_of_stream = true;
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.samples.clear();
    }

    #[cfg(test)]
    pub fn available_capacity(&self) -> usize {
        self.capacity_frames.saturating_sub(self.samples.len())
    }

    pub fn is_complete(&self) -> bool {
        self.end_of_stream
            && !self.cancelled
            && self.samples.is_empty()
            && self.rendered_frames == self.accepted_frames
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    #[cfg(test)]
    pub fn rendered_frames(&self) -> u64 {
        self.rendered_frames
    }

    #[cfg(test)]
    pub fn underrun_frames(&self) -> u64 {
        self.underrun_frames
    }

    /// Fill an interleaved output buffer, duplicating mono input to each channel.
    ///
    /// `write_sample` converts S16 input to the device sample representation. A
    /// source frame is counted once regardless of the device channel count.
    pub fn render_interleaved<T>(
        &mut self,
        output: &mut [T],
        channels: usize,
        silence: T,
        mut write_sample: impl FnMut(i16) -> T,
    ) where
        T: Copy,
    {
        assert!(channels > 0, "device channel count must be non-zero");
        let mut frame_chunks = output.chunks_exact_mut(channels);
        for frame in &mut frame_chunks {
            if self.cancelled {
                frame.fill(silence);
                continue;
            }
            if let Some(sample) = self.samples.pop_front() {
                let converted = write_sample(sample);
                frame.fill(converted);
                self.rendered_frames = self.rendered_frames.saturating_add(1);
            } else {
                frame.fill(silence);
                if !self.end_of_stream {
                    self.underrun_frames = self.underrun_frames.saturating_add(1);
                }
            }
        }
        frame_chunks.into_remainder().fill(silence);
    }

    /// Build the exact supervisor receipt for the frames submitted to callbacks.
    pub fn receipt(&self) -> PlaybackReceipt {
        PlaybackReceipt {
            audible_frames: self.rendered_frames,
            duration: duration_for_frames(self.rendered_frames),
            completed: self.is_complete(),
        }
    }
}

pub fn duration_for_frames(frames: u64) -> Duration {
    Duration::from_secs_f64(frames as f64 / f64::from(INPUT_SAMPLE_RATE_HZ))
}

#[cfg(all(windows, feature = "dev-wasapi-audio"))]
mod wasapi;

#[cfg(all(windows, feature = "dev-wasapi-audio"))]
pub use wasapi::{DevWasapiAudioSink, DevWasapiConfig};

#[cfg(test)]
mod tests {
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
    fn bounded_queue_applies_backpressure_without_losing_order() {
        let mut queue = BoundedPcmQueue::new(3);
        assert_eq!(queue.push(&[1, 2, 3, 4]), 3);
        assert_eq!(queue.available_capacity(), 0);

        let mut first = [0_i16; 4];
        queue.render_interleaved(&mut first, 2, 0, |sample| sample);
        assert_eq!(first, [1, 1, 2, 2]);
        assert_eq!(queue.push(&[4]), 1);

        let mut second = [0_i16; 4];
        queue.mark_end_of_stream();
        queue.render_interleaved(&mut second, 2, 0, |sample| sample);
        assert_eq!(second, [3, 3, 4, 4]);
        assert!(queue.is_complete());
        assert_eq!(queue.receipt().audible_frames, 4);
    }

    #[test]
    fn cancellation_discards_queued_frames_and_returns_exact_partial_receipt() {
        let mut queue = BoundedPcmQueue::new(8);
        assert_eq!(queue.push(&[10, 20, 30, 40]), 4);

        let mut output = [0_i16; 2];
        queue.render_interleaved(&mut output, 1, 0, |sample| sample);
        queue.cancel();
        queue.render_interleaved(&mut output, 1, 0, |sample| sample);

        assert_eq!(output, [0, 0]);
        assert_eq!(queue.rendered_frames(), 2);
        assert_eq!(
            queue.receipt(),
            PlaybackReceipt {
                audible_frames: 2,
                duration: Duration::from_secs_f64(2.0 / 24_000.0),
                completed: false,
            }
        );
    }

    #[test]
    fn completion_requires_eos_and_every_accepted_frame_to_render() {
        let mut queue = BoundedPcmQueue::new(4);
        queue.push(&[1, 2]);
        queue.mark_end_of_stream();
        assert!(!queue.receipt().completed);

        let mut output = [0_i16; 1];
        queue.render_interleaved(&mut output, 1, 0, |sample| sample);
        assert!(!queue.receipt().completed);
        queue.render_interleaved(&mut output, 1, 0, |sample| sample);

        let receipt = queue.receipt();
        assert!(receipt.completed);
        assert_eq!(receipt.audible_frames, 2);
        assert_eq!(receipt.duration, duration_for_frames(2));
    }

    #[test]
    fn underrun_silence_is_not_counted_as_audible() {
        let mut queue = BoundedPcmQueue::new(2);
        let mut output = [99_i16; 4];
        queue.render_interleaved(&mut output, 2, 0, |sample| sample);

        assert_eq!(output, [0, 0, 0, 0]);
        assert_eq!(queue.rendered_frames(), 0);
        assert_eq!(queue.underrun_frames(), 2);
    }
}
