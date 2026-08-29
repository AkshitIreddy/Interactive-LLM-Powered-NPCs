use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, SampleFormat, Stream, StreamConfig,
};
use futures_util::StreamExt;
use npc_runtime_core::{RuntimeDependencyError, SpeechStream, SpeechStreamItem, TurnIdentity};
use tokio_util::sync::CancellationToken;

use super::{
    decode_chunk, spsc_pcm_ring, DevOutputDeviceTelemetry, DevSubmittedPlaybackReceipt,
    LinearResampler, PcmConsumer, SharedPlaybackState, INPUT_SAMPLE_RATE_HZ,
};

const DEFAULT_QUEUE_FRAMES: usize = INPUT_SAMPLE_RATE_HZ as usize / 2;
const DEFAULT_PRODUCER_WAIT: Duration = Duration::from_millis(1);
const DEFAULT_CONTROL_WAIT: Duration = Duration::from_millis(2);
const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_millis(250);
const DEFAULT_STALL_TIMEOUT: Duration = Duration::from_millis(750);
const DEFAULT_ENDPOINT_DRAIN: Duration = Duration::from_millis(100);

#[derive(Clone, Debug)]
pub struct DevWasapiConfig {
    pub queue_capacity_frames: usize,
    /// Exact CPAL device name; `None` selects the Windows default output device.
    pub device_name: Option<String>,
    pub producer_wait: Duration,
    pub control_wait: Duration,
    pub startup_timeout: Duration,
    pub stop_timeout: Duration,
    pub playback_stall_timeout: Duration,
    pub endpoint_drain_duration: Duration,
}

impl Default for DevWasapiConfig {
    fn default() -> Self {
        Self {
            queue_capacity_frames: DEFAULT_QUEUE_FRAMES,
            device_name: None,
            producer_wait: DEFAULT_PRODUCER_WAIT,
            control_wait: DEFAULT_CONTROL_WAIT,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            stop_timeout: DEFAULT_STOP_TIMEOUT,
            playback_stall_timeout: DEFAULT_STALL_TIMEOUT,
            endpoint_drain_duration: DEFAULT_ENDPOINT_DRAIN,
        }
    }
}

#[derive(Debug)]
struct ActivePlayback {
    identity: TurnIdentity,
    state: Arc<SharedPlaybackState>,
    control: Option<OutputControl>,
}

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
        if [
            config.producer_wait,
            config.control_wait,
            config.startup_timeout,
            config.stop_timeout,
            config.playback_stall_timeout,
            config.endpoint_drain_duration,
        ]
        .iter()
        .any(Duration::is_zero)
        {
            return Err(RuntimeDependencyError::Invalid(
                "WASAPI timing limits must be non-zero".into(),
            ));
        }
        Ok(Self {
            config,
            active: Mutex::new(None),
        })
    }

    /// Submit a provider speech stream to WASAPI callbacks.
    ///
    /// The receipt is intentionally not a runtime `PlaybackReceipt`: CPAL exposes
    /// callback submission, not proof that samples left the physical speakers.
    pub async fn play_submitted(
        &self,
        identity: &TurnIdentity,
        _sentence_id: u64,
        mut speech: SpeechStream,
        cancellation: CancellationToken,
    ) -> Result<DevSubmittedPlaybackReceipt, RuntimeDependencyError> {
        if cancellation.is_cancelled() {
            return Ok(cancelled_before_device_receipt(false));
        }

        let state = Arc::new(SharedPlaybackState::new());
        let (mut producer, consumer) = spsc_pcm_ring(self.config.queue_capacity_frames);
        let _registration = self.reserve(identity, Arc::clone(&state))?;
        let start_config = self.config.clone();
        let start_state = Arc::clone(&state);
        let mut startup = tokio::task::spawn_blocking(move || {
            OutputThread::start(start_config, start_state, consumer)
        });
        let (startup_result, startup_detached) = tokio::select! {
            result = &mut startup => (Some(result), false),
            () = cancellation.cancelled() => {
                state.cancel();
                (
                    tokio::time::timeout(self.config.stop_timeout, &mut startup)
                        .await
                        .ok(),
                    false,
                )
            }
            () = tokio::time::sleep(self.config.startup_timeout) => {
                state.cancel();
                (None, true)
            }
        };
        let Some(startup_result) = startup_result else {
            if startup_detached {
                return Err(RuntimeDependencyError::Unavailable(
                    "WASAPI startup scheduling timed out; cancelled detached cleanup may still be exiting"
                        .into(),
                ));
            }
            return Ok(cancelled_before_device_receipt(true));
        };
        let output = match startup_result {
            Ok(Ok(output)) => output,
            Ok(Err(RuntimeDependencyError::Cancelled))
                if state.cancelled.load(Ordering::Acquire) =>
            {
                return Ok(cancelled_before_device_receipt(false));
            }
            Ok(Err(error)) => return Err(error),
            Err(error) => {
                return Err(RuntimeDependencyError::Internal(format!(
                    "WASAPI startup task failed: {error}"
                )));
            }
        };
        self.attach_control(identity, output.control.clone())?;
        let device = output.device.clone();
        if cancellation.is_cancelled() || state.cancelled.load(Ordering::Acquire) {
            state.cancel();
            output
                .control
                .stop_and_wait(self.config.stop_timeout)
                .await?;
            output.wait_for_exit(self.config.stop_timeout).await?;
            return Ok(state.receipt(&device));
        }

        let mut pending = Vec::new();
        let mut pending_offset = 0_usize;
        let mut pending_eos = false;
        let mut last_sequence = None;
        let mut last_progress_frames = 0_u64;
        let mut last_progress = Instant::now();

        loop {
            if cancellation.is_cancelled() || state.cancelled.load(Ordering::Acquire) {
                state.cancel();
                output
                    .control
                    .stop_and_wait(self.config.stop_timeout)
                    .await?;
                output.wait_for_exit(self.config.stop_timeout).await?;
                return Ok(state.receipt(&device));
            }
            if state.stream_failed.load(Ordering::Acquire) {
                let _ = output.wait_for_exit(self.config.stop_timeout).await;
                return Err(RuntimeDependencyError::Unavailable(
                    "WASAPI output stream failed".into(),
                ));
            }
            if state.endpoint_drain_complete.load(Ordering::Acquire) {
                output.wait_for_exit(self.config.stop_timeout).await?;
                return Ok(state.receipt(&device));
            }

            let progress_frames = state
                .submitted_device_frames
                .load(Ordering::Acquire)
                .saturating_add(state.drain_silence_device_frames.load(Ordering::Acquire));
            if progress_frames != last_progress_frames {
                last_progress_frames = progress_frames;
                last_progress = Instant::now();
            } else if (state.accepted_source_frames.load(Ordering::Acquire) > 0
                || state.end_of_stream.load(Ordering::Acquire))
                && last_progress.elapsed() >= self.config.playback_stall_timeout
            {
                state.cancel();
                let _ = output.control.stop_and_wait(self.config.stop_timeout).await;
                let _ = output.wait_for_exit(self.config.stop_timeout).await;
                return Err(RuntimeDependencyError::Unavailable(
                    "WASAPI callback made no PCM submission progress".into(),
                ));
            }

            if pending_offset < pending.len() {
                let accepted = state.push(&mut producer, &pending[pending_offset..]);
                pending_offset = pending_offset.saturating_add(accepted);
                if pending_offset == pending.len() {
                    pending.clear();
                    pending_offset = 0;
                    if pending_eos {
                        state.mark_end_of_stream();
                    }
                } else {
                    tokio::select! {
                        () = cancellation.cancelled() => {},
                        () = state.cancelled() => {},
                        () = state.terminal_notified() => {},
                        () = tokio::time::sleep(self.config.producer_wait) => {},
                    }
                }
                continue;
            }

            if pending_eos {
                tokio::select! {
                    () = cancellation.cancelled() => {},
                    () = state.cancelled() => {},
                    () = state.terminal_notified() => {},
                    () = tokio::time::sleep(self.config.producer_wait) => {},
                }
                continue;
            }

            let next = tokio::select! {
                () = cancellation.cancelled() => None,
                () = state.cancelled() => None,
                () = state.terminal_notified() => continue,
                item = speech.next() => item,
            };
            let Some(item) = next else {
                if cancellation.is_cancelled() {
                    continue;
                }
                pending_eos = true;
                state.mark_end_of_stream();
                continue;
            };
            let item = match item {
                Ok(item) => item,
                Err(error) => {
                    return stop_with_error(
                        output,
                        &state,
                        self.config.stop_timeout,
                        RuntimeDependencyError::Unavailable(format!(
                            "speech provider stream failed during playback: {error}"
                        )),
                    )
                    .await;
                }
            };
            match item {
                SpeechStreamItem::Alignment(_) => {}
                SpeechStreamItem::Audio(chunk) => {
                    if last_sequence.is_some_and(|last| chunk.sequence <= last) {
                        return stop_with_error(
                            output,
                            &state,
                            self.config.stop_timeout,
                            RuntimeDependencyError::Invalid(format!(
                                "speech audio sequence {} is not greater than the previous sequence",
                                chunk.sequence
                            )),
                        )
                        .await;
                    }
                    last_sequence = Some(chunk.sequence);
                    pending = match decode_chunk(&chunk) {
                        Ok(samples) => samples,
                        Err(error) => {
                            return stop_with_error(
                                output,
                                &state,
                                self.config.stop_timeout,
                                RuntimeDependencyError::Invalid(format!(
                                    "unsupported speech audio format: {error}"
                                )),
                            )
                            .await;
                        }
                    };
                    pending_eos = chunk.end_of_stream;
                    if pending.is_empty() && pending_eos {
                        state.mark_end_of_stream();
                    }
                }
            }
        }
    }

    pub async fn stop(&self, identity: &TurnIdentity) -> Result<(), RuntimeDependencyError> {
        let (state, control) = {
            let active = self.active.lock().map_err(|_| {
                RuntimeDependencyError::Internal("audio state lock poisoned".into())
            })?;
            match active
                .as_ref()
                .filter(|playback| &playback.identity == identity)
            {
                Some(playback) => (Arc::clone(&playback.state), playback.control.clone()),
                None => return Ok(()),
            }
        };
        state.cancel();
        if let Some(control) = control {
            control.stop_and_wait(self.config.stop_timeout).await?;
        }
        Ok(())
    }

    fn reserve(
        &self,
        identity: &TurnIdentity,
        state: Arc<SharedPlaybackState>,
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
            state,
            control: None,
        });
        Ok(ActiveRegistration {
            active: &self.active,
            identity: identity.clone(),
        })
    }

    fn attach_control(
        &self,
        identity: &TurnIdentity,
        control: OutputControl,
    ) -> Result<(), RuntimeDependencyError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| RuntimeDependencyError::Internal("audio state lock poisoned".into()))?;
        let playback = active
            .as_mut()
            .filter(|playback| &playback.identity == identity)
            .ok_or(RuntimeDependencyError::Cancelled)?;
        playback.control = Some(control);
        Ok(())
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

enum OutputCommand {
    Stop(mpsc::SyncSender<Result<(), RuntimeDependencyError>>),
}

#[derive(Clone, Debug)]
struct OutputControl {
    commands: mpsc::Sender<OutputCommand>,
    stopped: Arc<AtomicBool>,
    state: Arc<SharedPlaybackState>,
}

impl OutputControl {
    async fn stop_and_wait(&self, timeout: Duration) -> Result<(), RuntimeDependencyError> {
        self.state.cancel();
        if self.stopped.load(Ordering::Acquire) {
            return Ok(());
        }
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        if self.commands.send(OutputCommand::Stop(ack_tx)).is_err() {
            return if self.stopped.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err(RuntimeDependencyError::Unavailable(
                    "WASAPI control thread disconnected before stop acknowledgement".into(),
                ))
            };
        }
        let stopped = Arc::clone(&self.stopped);
        let wait = tokio::task::spawn_blocking(move || match ack_rx.recv_timeout(timeout) {
            Ok(result) => Ok(result),
            Err(_) if stopped.load(Ordering::Acquire) => Ok(Ok(())),
            Err(error) => Err(error),
        });
        let acknowledgement = tokio::time::timeout(timeout, wait)
            .await
            .map_err(|_| {
                RuntimeDependencyError::Unavailable(
                    "WASAPI stop wait scheduling timed out; cancelled detached cleanup may still be exiting"
                        .into(),
                )
            })?
            .map_err(|error| {
                RuntimeDependencyError::Internal(format!(
                    "WASAPI stop acknowledgement task failed: {error}"
                ))
            })?;
        acknowledgement.map_err(|error| {
            RuntimeDependencyError::Unavailable(format!(
                "WASAPI pause/drop was not acknowledged in time: {error}"
            ))
        })?
    }
}

struct OutputThread {
    control: OutputControl,
    join: Option<thread::JoinHandle<Result<(), RuntimeDependencyError>>>,
    device: DevOutputDeviceTelemetry,
}

impl OutputThread {
    fn start(
        config: DevWasapiConfig,
        state: Arc<SharedPlaybackState>,
        consumer: PcmConsumer,
    ) -> Result<Self, RuntimeDependencyError> {
        let startup_timeout = config.startup_timeout;
        let startup_cancelled = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (command_tx, command_rx) = mpsc::channel();
        let thread_cancelled = Arc::clone(&startup_cancelled);
        let thread_stopped = Arc::clone(&stopped);
        let thread_state = Arc::clone(&state);
        let join = thread::Builder::new()
            .name("npc-dev-wasapi".into())
            .spawn(move || {
                run_output_thread(
                    &config,
                    thread_state,
                    thread_cancelled,
                    thread_stopped,
                    ready_tx,
                    command_rx,
                    consumer,
                )
            })
            .map_err(|error| {
                RuntimeDependencyError::Unavailable(format!(
                    "could not start WASAPI output thread: {error}"
                ))
            })?;

        let info = match ready_rx.recv_timeout(startup_timeout) {
            Ok(Ok(info)) => info,
            Ok(Err(error)) => return Err(error),
            Err(error) => {
                startup_cancelled.store(true, Ordering::Release);
                state.cancel();
                return Err(RuntimeDependencyError::Unavailable(format!(
                    "WASAPI output thread did not start in time; cancelled detached cleanup may still be exiting: {error}"
                )));
            }
        };
        Ok(Self {
            control: OutputControl {
                commands: command_tx,
                stopped,
                state,
            },
            join: Some(join),
            device: info,
        })
    }

    async fn wait_for_exit(mut self, timeout: Duration) -> Result<(), RuntimeDependencyError> {
        let Some(join) = self.join.take() else {
            return Ok(());
        };
        let deadline = tokio::time::Instant::now() + timeout;
        while !join.is_finished() {
            if tokio::time::Instant::now() >= deadline {
                return Err(RuntimeDependencyError::Unavailable(
                    "WASAPI output thread did not exit in time; cancelled detached cleanup may still be exiting"
                        .into(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        join.join()
            .map_err(|_| RuntimeDependencyError::Internal("WASAPI thread panicked".into()))?
    }
}

impl Drop for OutputThread {
    fn drop(&mut self) {
        self.control.state.cancel();
        if !self.control.stopped.load(Ordering::Acquire) {
            let (ack, _receiver) = mpsc::sync_channel(1);
            let _ = self.control.commands.send(OutputCommand::Stop(ack));
        }
    }
}

fn run_output_thread(
    config: &DevWasapiConfig,
    state: Arc<SharedPlaybackState>,
    startup_cancelled: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<DevOutputDeviceTelemetry, RuntimeDependencyError>>,
    commands: mpsc::Receiver<OutputCommand>,
    consumer: PcmConsumer,
) -> Result<(), RuntimeDependencyError> {
    let startup: Result<(Stream, DevOutputDeviceTelemetry), RuntimeDependencyError> = (|| {
        let (device, device_name, selected_by_exact_name) = open_device(config)?;
        let supported = device.default_output_config().map_err(|error| {
            RuntimeDependencyError::Unavailable(format!(
                "could not read default WASAPI mix format: {error}"
            ))
        })?;
        let sample_rate_hz = supported.sample_rate().0;
        let channels = supported.channels();
        validate_device_mix(channels, sample_rate_hz)?;
        let telemetry = DevOutputDeviceTelemetry {
            name: device_name,
            channels,
            sample_rate_hz,
            sample_format: supported.sample_format().to_string(),
            selected_by_exact_name,
        };
        if startup_cancelled.load(Ordering::Acquire) || state.cancelled.load(Ordering::Acquire) {
            return Err(RuntimeDependencyError::Cancelled);
        }
        let stream = build_output_stream(
            &device,
            &supported.config(),
            supported.sample_format(),
            Arc::clone(&state),
            consumer,
            config.endpoint_drain_duration,
        )?;
        if startup_cancelled.load(Ordering::Acquire) || state.cancelled.load(Ordering::Acquire) {
            return Err(RuntimeDependencyError::Cancelled);
        }
        stream.play().map_err(|error| {
            RuntimeDependencyError::Unavailable(format!("WASAPI playback start failed: {error}"))
        })?;
        Ok((stream, telemetry))
    })();
    let (stream, telemetry) = match startup {
        Ok(started) => started,
        Err(error) => {
            stopped.store(true, Ordering::Release);
            state.notify_terminal();
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    if ready.send(Ok(telemetry)).is_err() {
        let _ = stream.pause();
        stopped.store(true, Ordering::Release);
        state.notify_terminal();
        return Ok(());
    }

    loop {
        if state.stream_failed.load(Ordering::Acquire) {
            let _ = stream.pause();
            stopped.store(true, Ordering::Release);
            state.notify_terminal();
            return Err(RuntimeDependencyError::Unavailable(
                "WASAPI output callback reported a stream error".into(),
            ));
        }
        if state.endpoint_drain_complete.load(Ordering::Acquire) {
            drop(stream);
            stopped.store(true, Ordering::Release);
            state.notify_terminal();
            return Ok(());
        }
        match commands.recv_timeout(config.control_wait) {
            Ok(OutputCommand::Stop(ack)) => {
                state.cancel();
                let result = stream.pause().map_err(|error| {
                    RuntimeDependencyError::Unavailable(format!(
                        "WASAPI pause failed during stop: {error}"
                    ))
                });
                drop(stream);
                stopped.store(true, Ordering::Release);
                state.notify_terminal();
                let _ = ack.send(result.clone());
                return result;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                state.cancel();
                let _ = stream.pause();
                stopped.store(true, Ordering::Release);
                state.notify_terminal();
                return Ok(());
            }
        }
    }
}

fn open_device(config: &DevWasapiConfig) -> Result<(Device, String, bool), RuntimeDependencyError> {
    let host = cpal::host_from_id(cpal::HostId::Wasapi).map_err(|error| {
        RuntimeDependencyError::Unavailable(format!("WASAPI host unavailable: {error}"))
    })?;
    if let Some(expected_name) = config.device_name.as_deref() {
        let devices = host.output_devices().map_err(|error| {
            RuntimeDependencyError::Unavailable(format!(
                "could not enumerate WASAPI output devices: {error}"
            ))
        })?;
        let mut matched = None;
        for device in devices {
            let name = device.name().map_err(|error| {
                RuntimeDependencyError::Unavailable(format!(
                    "could not read WASAPI output device name: {error}"
                ))
            })?;
            if name == expected_name {
                reject_duplicate_device_name(matched.is_some(), expected_name)?;
                matched = Some(device);
            }
        }
        return matched
            .map(|device| (device, expected_name.to_owned(), true))
            .ok_or_else(|| {
                RuntimeDependencyError::Unavailable(format!(
                    "WASAPI output device not found: {expected_name}"
                ))
            });
    }
    let device = host.default_output_device().ok_or_else(|| {
        RuntimeDependencyError::Unavailable("Windows has no default output device".into())
    })?;
    let name = device.name().map_err(|error| {
        RuntimeDependencyError::Unavailable(format!(
            "could not read default WASAPI output device name: {error}"
        ))
    })?;
    Ok((device, name, false))
}

fn reject_duplicate_device_name(
    already_matched: bool,
    expected_name: &str,
) -> Result<(), RuntimeDependencyError> {
    if already_matched {
        return Err(RuntimeDependencyError::Unavailable(format!(
            "WASAPI output device name is ambiguous: {expected_name}"
        )));
    }
    Ok(())
}

fn build_output_stream(
    device: &Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    state: Arc<SharedPlaybackState>,
    consumer: PcmConsumer,
    endpoint_drain_duration: Duration,
) -> Result<Stream, RuntimeDependencyError> {
    let channels = usize::from(config.channels);
    let rate = config.sample_rate.0;
    validate_device_mix(config.channels, rate)?;
    let error_state = Arc::clone(&state);
    let result = match sample_format {
        SampleFormat::F32 => {
            let mut resampler = LinearResampler::new(rate, consumer, endpoint_drain_duration);
            device.build_output_stream(
                config,
                move |output: &mut [f32], _| {
                    resampler.render(&state, output, channels, 0.0, |sample| sample);
                },
                move |_| error_state.stream_failed.store(true, Ordering::Release),
                None,
            )
        }
        SampleFormat::I16 => {
            let mut resampler = LinearResampler::new(rate, consumer, endpoint_drain_duration);
            device.build_output_stream(
                config,
                move |output: &mut [i16], _| {
                    resampler.render(&state, output, channels, 0, f32_to_i16);
                },
                move |_| error_state.stream_failed.store(true, Ordering::Release),
                None,
            )
        }
        SampleFormat::U16 => {
            let mut resampler = LinearResampler::new(rate, consumer, endpoint_drain_duration);
            device.build_output_stream(
                config,
                move |output: &mut [u16], _| {
                    resampler.render(&state, output, channels, 32_768, |sample| {
                        u16::try_from(i32::from(f32_to_i16(sample)) + 32_768).unwrap_or(32_768)
                    });
                },
                move |_| error_state.stream_failed.store(true, Ordering::Release),
                None,
            )
        }
        _ => {
            return Err(RuntimeDependencyError::Unavailable(format!(
                "unsupported default WASAPI sample format: {sample_format}"
            )));
        }
    };
    result.map_err(|error| {
        RuntimeDependencyError::Unavailable(format!("could not build WASAPI stream: {error}"))
    })
}

fn validate_device_mix(channels: u16, sample_rate_hz: u32) -> Result<(), RuntimeDependencyError> {
    if !(1..=2).contains(&channels) {
        return Err(RuntimeDependencyError::Unavailable(format!(
            "default WASAPI mix format has unsupported channel count {channels}; dev playback accepts mono or stereo only"
        )));
    }
    if sample_rate_hz < INPUT_SAMPLE_RATE_HZ {
        return Err(RuntimeDependencyError::Unavailable(format!(
            "default WASAPI mix rate {sample_rate_hz} Hz is below 24 kHz; anti-aliasing downsampling is not qualified"
        )));
    }
    Ok(())
}

fn f32_to_i16(sample: f32) -> i16 {
    (sample * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16
}

async fn stop_with_error(
    output: OutputThread,
    state: &SharedPlaybackState,
    timeout: Duration,
    error: RuntimeDependencyError,
) -> Result<DevSubmittedPlaybackReceipt, RuntimeDependencyError> {
    state.cancel();
    let _ = output.control.stop_and_wait(timeout).await;
    let _ = output.wait_for_exit(timeout).await;
    Err(error)
}

fn cancelled_before_device_receipt(detached_cleanup_pending: bool) -> DevSubmittedPlaybackReceipt {
    DevSubmittedPlaybackReceipt {
        source_frames_submitted: 0,
        device_frames_submitted: 0,
        source_duration: Duration::ZERO,
        device_duration: Duration::ZERO,
        drain_silence_device_frames: 0,
        underrun_device_frames: 0,
        source_submission_complete: false,
        endpoint_drain_complete: false,
        cancelled: true,
        detached_cleanup_pending,
        device: DevOutputDeviceTelemetry {
            name: "not-selected".into(),
            channels: 0,
            sample_rate_hz: 0,
            sample_format: "not-selected".into(),
            selected_by_exact_name: false,
        },
    }
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

    #[test]
    fn rejects_unqualified_mix_formats_without_opening_a_device() {
        assert!(validate_device_mix(1, 24_000).is_ok());
        assert!(validate_device_mix(2, 48_000).is_ok());
        assert!(matches!(
            validate_device_mix(3, 48_000),
            Err(RuntimeDependencyError::Unavailable(_))
        ));
        assert!(matches!(
            validate_device_mix(2, 16_000),
            Err(RuntimeDependencyError::Unavailable(_))
        ));
    }

    #[test]
    fn rejects_duplicate_exact_friendly_names() {
        assert!(reject_duplicate_device_name(false, "Speakers").is_ok());
        assert!(matches!(
            reject_duplicate_device_name(true, "Speakers"),
            Err(RuntimeDependencyError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn pre_cancelled_submission_returns_truthful_empty_receipt() {
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
            .play_submitted(&identity, 1, speech, cancellation)
            .await
            .expect("pre-cancellation should not open a device");

        assert_eq!(receipt, cancelled_before_device_receipt(false));
    }
}
