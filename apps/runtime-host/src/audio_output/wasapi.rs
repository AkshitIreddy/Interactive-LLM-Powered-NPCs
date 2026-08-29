use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

use async_trait::async_trait;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, SampleFormat, SampleRate, Stream, StreamConfig, SupportedStreamConfig,
};
use futures_util::StreamExt;
use npc_runtime_core::{
    AudioSink, PlaybackReceipt, RuntimeDependencyError, SpeechStream, SpeechStreamItem,
    TurnIdentity,
};
use tokio_util::sync::CancellationToken;

use super::{decode_chunk, BoundedPcmQueue, INPUT_SAMPLE_RATE_HZ};

const DEFAULT_QUEUE_FRAMES: usize = INPUT_SAMPLE_RATE_HZ as usize / 2;
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(2);
const DEFAULT_DRAIN_GRACE: Duration = Duration::from_millis(50);

type StreamErrorState = Arc<Mutex<Option<String>>>;
type StartedStream = (Stream, StreamErrorState);

#[derive(Clone, Debug)]
pub struct DevWasapiConfig {
    /// At most this many 24 kHz mono source frames may wait for the callback.
    pub queue_capacity_frames: usize,
    /// Optional exact CPAL device name. `None` selects the Windows default output.
    pub device_name: Option<String>,
    /// Cooperative backpressure/cancellation polling cadence.
    pub poll_interval: Duration,
    /// Conservative time for the final callback buffer to leave the device queue.
    /// This is not acoustic verification; only loopback can prove physical output.
    pub drain_grace: Duration,
}

impl Default for DevWasapiConfig {
    fn default() -> Self {
        Self {
            queue_capacity_frames: DEFAULT_QUEUE_FRAMES,
            device_name: None,
            poll_interval: DEFAULT_POLL_INTERVAL,
            drain_grace: DEFAULT_DRAIN_GRACE,
        }
    }
}

#[derive(Debug)]
struct ActivePlayback {
    identity: TurnIdentity,
    queue: Arc<Mutex<BoundedPcmQueue>>,
}

/// A development-only CPAL sink using the Windows WASAPI host.
///
/// The sink accepts only 24 kHz mono S16LE provider chunks. It requests the same
/// sample rate from the device and duplicates mono frames across the selected
/// device channels, avoiding an implicit resampler in receipt accounting.
pub struct DevWasapiAudioSink {
    config: DevWasapiConfig,
    active: Mutex<Option<ActivePlayback>>,
}

impl DevWasapiAudioSink {
    pub fn new(config: DevWasapiConfig) -> Result<Self, RuntimeDependencyError> {
        if config.queue_capacity_frames == 0 {
            return Err(RuntimeDependencyError::Invalid(
                "WASAPI queue capacity must be non-zero".into(),
            ));
        }
        if config.poll_interval.is_zero() {
            return Err(RuntimeDependencyError::Invalid(
                "WASAPI poll interval must be non-zero".into(),
            ));
        }
        Ok(Self {
            config,
            active: Mutex::new(None),
        })
    }

    fn register(
        &self,
        identity: &TurnIdentity,
        queue: Arc<Mutex<BoundedPcmQueue>>,
    ) -> Result<ActiveRegistration<'_>, RuntimeDependencyError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| RuntimeDependencyError::Internal("audio state lock poisoned".into()))?;
        if let Some(existing) = active.as_ref() {
            return Err(RuntimeDependencyError::Unavailable(format!(
                "audio playback already active for {}",
                existing.identity
            )));
        }
        *active = Some(ActivePlayback {
            identity: identity.clone(),
            queue,
        });
        Ok(ActiveRegistration {
            active: &self.active,
            identity: identity.clone(),
        })
    }
}

fn open_device(config: &DevWasapiConfig) -> Result<Device, RuntimeDependencyError> {
    let host = cpal::host_from_id(cpal::HostId::Wasapi).map_err(|error| {
        RuntimeDependencyError::Unavailable(format!("WASAPI host unavailable: {error}"))
    })?;
    if let Some(expected_name) = config.device_name.as_deref() {
        let devices = host.output_devices().map_err(|error| {
            RuntimeDependencyError::Unavailable(format!(
                "could not enumerate WASAPI output devices: {error}"
            ))
        })?;
        for device in devices {
            let name = device.name().map_err(|error| {
                RuntimeDependencyError::Unavailable(format!(
                    "could not read WASAPI output device name: {error}"
                ))
            })?;
            if name == expected_name {
                return Ok(device);
            }
        }
        return Err(RuntimeDependencyError::Unavailable(format!(
            "WASAPI output device not found: {expected_name}"
        )));
    }
    host.default_output_device().ok_or_else(|| {
        RuntimeDependencyError::Unavailable("Windows has no default output device".into())
    })
}

struct OutputThread {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<Result<(), RuntimeDependencyError>>>,
}

impl OutputThread {
    fn start(
        config: DevWasapiConfig,
        queue: Arc<Mutex<BoundedPcmQueue>>,
    ) -> Result<Self, RuntimeDependencyError> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let join = thread::Builder::new()
            .name("npc-dev-wasapi".into())
            .spawn(move || {
                let startup: Result<StartedStream, RuntimeDependencyError> = (|| {
                    let device = open_device(&config)?;
                    let supported = select_24khz_config(&device)?;
                    let stream_config = supported.config();
                    let stream_error = Arc::new(Mutex::new(None));
                    let output_stream = build_output_stream(
                        &device,
                        &stream_config,
                        supported.sample_format(),
                        Arc::clone(&queue),
                        Arc::clone(&stream_error),
                    )?;
                    output_stream.play().map_err(|error| {
                        RuntimeDependencyError::Unavailable(format!(
                            "WASAPI playback start failed: {error}"
                        ))
                    })?;
                    Ok((output_stream, stream_error))
                })();

                let (output_stream, stream_error) = match startup {
                    Ok(started) => {
                        let _ = ready_tx.send(Ok(()));
                        started
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.clone()));
                        return Err(error);
                    }
                };

                loop {
                    if thread_stop.load(Ordering::Acquire) {
                        let _ = output_stream.pause();
                        return Ok(());
                    }
                    if let Some(error) = take_stream_error(&stream_error)? {
                        let _ = output_stream.pause();
                        return Err(RuntimeDependencyError::Unavailable(format!(
                            "WASAPI output stream failed: {error}"
                        )));
                    }
                    if queue_is_complete(&queue)? {
                        thread::sleep(config.drain_grace);
                        return Ok(());
                    }
                    thread::sleep(config.poll_interval);
                }
            })
            .map_err(|error| {
                RuntimeDependencyError::Unavailable(format!(
                    "could not start WASAPI output thread: {error}"
                ))
            })?;

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                join: Some(join),
            }),
            Ok(Err(error)) => {
                let _ = join.join();
                Err(error)
            }
            Err(error) => {
                stop.store(true, Ordering::Release);
                let _ = join.join();
                Err(RuntimeDependencyError::Unavailable(format!(
                    "WASAPI output thread did not start: {error}"
                )))
            }
        }
    }

    fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    fn is_finished(&self) -> bool {
        self.join
            .as_ref()
            .is_none_or(thread::JoinHandle::is_finished)
    }

    async fn join(mut self) -> Result<(), RuntimeDependencyError> {
        let Some(join) = self.join.take() else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || join.join())
            .await
            .map_err(|error| {
                RuntimeDependencyError::Internal(format!("WASAPI join task failed: {error}"))
            })?
            .map_err(|_| RuntimeDependencyError::Internal("WASAPI thread panicked".into()))?
    }
}

impl Drop for OutputThread {
    fn drop(&mut self) {
        self.request_stop();
    }
}

struct ActiveRegistration<'a> {
    active: &'a Mutex<Option<ActivePlayback>>,
    identity: TurnIdentity,
}

impl Drop for ActiveRegistration<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            if active
                .as_ref()
                .is_some_and(|playback| playback.identity == self.identity)
            {
                *active = None;
            }
        }
    }
}

#[async_trait]
impl AudioSink for DevWasapiAudioSink {
    async fn play(
        &self,
        identity: &TurnIdentity,
        _sentence_id: u64,
        mut speech: SpeechStream,
        cancellation: CancellationToken,
    ) -> Result<PlaybackReceipt, RuntimeDependencyError> {
        if cancellation.is_cancelled() {
            return Ok(PlaybackReceipt {
                audible_frames: 0,
                duration: Duration::ZERO,
                completed: false,
            });
        }

        let queue = Arc::new(Mutex::new(BoundedPcmQueue::new(
            self.config.queue_capacity_frames,
        )));
        let _registration = self.register(identity, Arc::clone(&queue))?;
        let output = OutputThread::start(self.config.clone(), Arc::clone(&queue))?;

        let mut pending = Vec::new();
        let mut pending_offset = 0_usize;
        let mut pending_eos = false;
        let mut last_sequence = None;

        loop {
            if cancellation.is_cancelled() || queue_is_cancelled(&queue)? {
                cancel_queue(&queue)?;
                output.request_stop();
                let _ = output.join().await;
                return queue_receipt(&queue);
            }
            if output.is_finished() {
                let thread_result = output.join().await;
                if let Err(error) = thread_result {
                    cancel_queue(&queue)?;
                    return Err(error);
                }
                if queue_is_complete(&queue)? {
                    return queue_receipt(&queue);
                }
                cancel_queue(&queue)?;
                return Err(RuntimeDependencyError::Unavailable(
                    "WASAPI output thread ended before playback drained".into(),
                ));
            }

            if pending_offset < pending.len() {
                let accepted = push_pending(&queue, &pending[pending_offset..])?;
                pending_offset = pending_offset.saturating_add(accepted);
                if pending_offset == pending.len() {
                    pending.clear();
                    pending_offset = 0;
                    if pending_eos {
                        mark_end_of_stream(&queue)?;
                    }
                } else {
                    tokio::select! {
                        () = cancellation.cancelled() => {},
                        () = tokio::time::sleep(self.config.poll_interval) => {},
                    }
                }
                continue;
            }

            if pending_eos {
                if queue_is_complete(&queue)? {
                    output.join().await?;
                    return queue_receipt(&queue);
                }
                tokio::select! {
                    () = cancellation.cancelled() => {},
                    () = tokio::time::sleep(self.config.poll_interval) => {},
                }
                continue;
            }

            let next = tokio::select! {
                () = cancellation.cancelled() => None,
                next = speech.next() => next,
            };
            let Some(item) = next else {
                if cancellation.is_cancelled() {
                    continue;
                }
                pending_eos = true;
                mark_end_of_stream(&queue)?;
                continue;
            };
            let item = match item {
                Ok(item) => item,
                Err(error) => {
                    cancel_queue(&queue)?;
                    output.request_stop();
                    let _ = output.join().await;
                    return Err(RuntimeDependencyError::Unavailable(format!(
                        "speech provider stream failed during playback: {error}"
                    )));
                }
            };
            match item {
                SpeechStreamItem::Alignment(_) => {}
                SpeechStreamItem::Audio(chunk) => {
                    if last_sequence.is_some_and(|last| chunk.sequence <= last) {
                        cancel_queue(&queue)?;
                        output.request_stop();
                        let _ = output.join().await;
                        return Err(RuntimeDependencyError::Invalid(format!(
                            "speech audio sequence {} is not greater than the previous sequence",
                            chunk.sequence
                        )));
                    }
                    last_sequence = Some(chunk.sequence);
                    pending = match decode_chunk(&chunk) {
                        Ok(samples) => samples,
                        Err(error) => {
                            cancel_queue(&queue)?;
                            output.request_stop();
                            let _ = output.join().await;
                            return Err(RuntimeDependencyError::Invalid(format!(
                                "unsupported speech audio format: {error}"
                            )));
                        }
                    };
                    pending_eos = chunk.end_of_stream;
                    if pending.is_empty() && pending_eos {
                        mark_end_of_stream(&queue)?;
                    }
                }
            }
        }
    }

    async fn stop(&self, identity: &TurnIdentity) -> Result<(), RuntimeDependencyError> {
        let queue = {
            let active = self.active.lock().map_err(|_| {
                RuntimeDependencyError::Internal("audio state lock poisoned".into())
            })?;
            active
                .as_ref()
                .filter(|playback| &playback.identity == identity)
                .map(|playback| Arc::clone(&playback.queue))
        };
        if let Some(queue) = queue {
            cancel_queue(&queue)?;
        }
        Ok(())
    }
}

fn select_24khz_config(device: &Device) -> Result<SupportedStreamConfig, RuntimeDependencyError> {
    let ranges = device.supported_output_configs().map_err(|error| {
        RuntimeDependencyError::Unavailable(format!(
            "could not query WASAPI output formats: {error}"
        ))
    })?;
    let requested_rate = SampleRate(INPUT_SAMPLE_RATE_HZ);
    ranges
        .filter(|range| {
            range.min_sample_rate() <= requested_rate && range.max_sample_rate() >= requested_rate
        })
        .min_by_key(|range| {
            let channel_rank = match range.channels() {
                1 => 0,
                2 => 1,
                channels => 2_u16.saturating_add(channels),
            };
            let format_rank = match range.sample_format() {
                SampleFormat::F32 => 0,
                SampleFormat::I16 => 1,
                SampleFormat::U16 => 2,
                _ => u16::MAX,
            };
            (format_rank, channel_rank)
        })
        .filter(|range| {
            matches!(
                range.sample_format(),
                SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
            )
        })
        .map(|range| range.with_sample_rate(requested_rate))
        .ok_or_else(|| {
            RuntimeDependencyError::Unavailable(
                "default WASAPI device exposes no 24 kHz F32/I16/U16 output format".into(),
            )
        })
}

fn build_output_stream(
    device: &Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    queue: Arc<Mutex<BoundedPcmQueue>>,
    stream_error: Arc<Mutex<Option<String>>>,
) -> Result<Stream, RuntimeDependencyError> {
    let channels = usize::from(config.channels);
    let result = match sample_format {
        SampleFormat::F32 => device.build_output_stream(
            config,
            move |output: &mut [f32], _| {
                render_callback(&queue, output, channels, 0.0, |sample| {
                    f32::from(sample) / 32_768.0
                });
            },
            move |error| record_stream_error(&stream_error, error.to_string()),
            None,
        ),
        SampleFormat::I16 => device.build_output_stream(
            config,
            move |output: &mut [i16], _| {
                render_callback(&queue, output, channels, 0, |sample| sample);
            },
            move |error| record_stream_error(&stream_error, error.to_string()),
            None,
        ),
        SampleFormat::U16 => device.build_output_stream(
            config,
            move |output: &mut [u16], _| {
                render_callback(&queue, output, channels, 32_768, |sample| {
                    (i32::from(sample) + 32_768) as u16
                });
            },
            move |error| record_stream_error(&stream_error, error.to_string()),
            None,
        ),
        _ => {
            return Err(RuntimeDependencyError::Unavailable(format!(
                "unsupported WASAPI device sample format: {sample_format}"
            )))
        }
    };
    result.map_err(|error| {
        RuntimeDependencyError::Unavailable(format!("could not build WASAPI stream: {error}"))
    })
}

fn render_callback<T: Copy>(
    queue: &Arc<Mutex<BoundedPcmQueue>>,
    output: &mut [T],
    channels: usize,
    silence: T,
    convert: impl FnMut(i16) -> T,
) {
    let Ok(mut queue) = queue.try_lock() else {
        output.fill(silence);
        return;
    };
    queue.render_interleaved(output, channels, silence, convert);
}

fn record_stream_error(state: &Arc<Mutex<Option<String>>>, error: String) {
    if let Ok(mut current) = state.lock() {
        if current.is_none() {
            *current = Some(error);
        }
    }
}

fn take_stream_error(
    state: &Arc<Mutex<Option<String>>>,
) -> Result<Option<String>, RuntimeDependencyError> {
    state
        .lock()
        .map(|mut error| error.take())
        .map_err(|_| RuntimeDependencyError::Internal("audio error lock poisoned".into()))
}

fn push_pending(
    queue: &Arc<Mutex<BoundedPcmQueue>>,
    samples: &[i16],
) -> Result<usize, RuntimeDependencyError> {
    queue
        .lock()
        .map(|mut queue| queue.push(samples))
        .map_err(|_| RuntimeDependencyError::Internal("PCM queue lock poisoned".into()))
}

fn mark_end_of_stream(queue: &Arc<Mutex<BoundedPcmQueue>>) -> Result<(), RuntimeDependencyError> {
    queue
        .lock()
        .map(|mut queue| queue.mark_end_of_stream())
        .map_err(|_| RuntimeDependencyError::Internal("PCM queue lock poisoned".into()))
}

fn cancel_queue(queue: &Arc<Mutex<BoundedPcmQueue>>) -> Result<(), RuntimeDependencyError> {
    queue
        .lock()
        .map(|mut queue| queue.cancel())
        .map_err(|_| RuntimeDependencyError::Internal("PCM queue lock poisoned".into()))
}

fn queue_is_cancelled(queue: &Arc<Mutex<BoundedPcmQueue>>) -> Result<bool, RuntimeDependencyError> {
    queue
        .lock()
        .map(|queue| queue.is_cancelled())
        .map_err(|_| RuntimeDependencyError::Internal("PCM queue lock poisoned".into()))
}

fn queue_is_complete(queue: &Arc<Mutex<BoundedPcmQueue>>) -> Result<bool, RuntimeDependencyError> {
    queue
        .lock()
        .map(|queue| queue.is_complete())
        .map_err(|_| RuntimeDependencyError::Internal("PCM queue lock poisoned".into()))
}

fn queue_receipt(
    queue: &Arc<Mutex<BoundedPcmQueue>>,
) -> Result<PlaybackReceipt, RuntimeDependencyError> {
    queue
        .lock()
        .map(|queue| queue.receipt())
        .map_err(|_| RuntimeDependencyError::Internal("PCM queue lock poisoned".into()))
}

#[cfg(test)]
mod tests {
    use futures_util::stream;

    use super::*;

    #[test]
    fn rejects_zero_capacity_without_opening_a_device() {
        let config = DevWasapiConfig {
            queue_capacity_frames: 0,
            ..DevWasapiConfig::default()
        };
        assert!(matches!(
            DevWasapiAudioSink::new(config),
            Err(RuntimeDependencyError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn pre_cancelled_playback_returns_an_exact_empty_receipt() {
        let sink = DevWasapiAudioSink::new(DevWasapiConfig::default())
            .expect("default dev sink config should validate");
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let speech: SpeechStream = Box::pin(stream::empty());
        let identity = TurnIdentity {
            session_id: "test-session".into(),
            turn_id: "test-turn".into(),
            cancellation_generation: 1,
        };

        let receipt = sink
            .play(&identity, 1, speech, cancellation)
            .await
            .expect("pre-cancellation should return a partial receipt");

        assert_eq!(
            receipt,
            PlaybackReceipt {
                audible_frames: 0,
                duration: Duration::ZERO,
                completed: false,
            }
        );
    }
}
