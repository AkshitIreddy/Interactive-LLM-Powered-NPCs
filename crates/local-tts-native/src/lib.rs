//! Native, CPU-only Kokoro TTS process boundary.
//!
//! The executable owns the unsafe sherpa-onnx C ABI. The library side stays
//! safe and exposes the worker only through a supervised, bounded pipe.

use std::{
    collections::BTreeMap,
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};

use async_trait::async_trait;
use futures_util::Stream;
use npc_runtime_core::{
    AudioChunk, DataClass, ProviderDescriptor, ProviderError, ProviderErrorKind, ProviderLocation,
    ProviderModality, SpeechRequest, SpeechStream, SpeechStreamItem, TtsProvider, TtsSession,
    TurnIdentity,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{mpsc, Mutex},
};
use tokio_util::sync::CancellationToken;

pub mod wire;

use wire::{WorkerCommand, WorkerEvent, MAX_CONTROL_BYTES, MAX_PCM_BYTES};

type SpeechItem = Result<SpeechStreamItem, ProviderError>;

struct SupervisedSpeechStream {
    receiver: mpsc::Receiver<SpeechItem>,
    cancellation: CancellationToken,
    cancelled: Pin<Box<dyn Future<Output = ()> + Send>>,
    abandonment: CancellationToken,
    cancellation_emitted: bool,
}

impl SupervisedSpeechStream {
    fn new(
        receiver: mpsc::Receiver<SpeechItem>,
        cancellation: CancellationToken,
        abandonment: CancellationToken,
    ) -> Self {
        let cancelled = Box::pin(cancellation.clone().cancelled_owned());
        Self {
            receiver,
            cancellation,
            cancelled,
            abandonment,
            cancellation_emitted: false,
        }
    }

    fn terminate_as_cancelled(&mut self) -> Poll<Option<SpeechItem>> {
        self.abandonment.cancel();
        self.receiver.close();
        while self.receiver.try_recv().is_ok() {}
        self.cancellation_emitted = true;
        Poll::Ready(Some(Err(ProviderError::cancelled(PROVIDER_ID))))
    }
}

impl Stream for SupervisedSpeechStream {
    type Item = SpeechItem;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.as_mut().get_mut();
        if this.cancellation_emitted {
            return Poll::Ready(None);
        }
        // Check both the atomic state and the registered cancellation future.
        // The future supplies the waker when the worker-side receiver is idle.
        if this.cancellation.is_cancelled() || this.cancelled.as_mut().poll(context).is_ready() {
            return this.terminate_as_cancelled();
        }
        match this.receiver.poll_recv(context) {
            Poll::Ready(Some(item)) => {
                // Cancellation can race a queued item between the first check
                // and `poll_recv`. Recheck before exposing any buffered PCM.
                if this.cancellation.is_cancelled() {
                    this.terminate_as_cancelled()
                } else {
                    Poll::Ready(Some(item))
                }
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending if this.cancellation.is_cancelled() => this.terminate_as_cancelled(),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for SupervisedSpeechStream {
    fn drop(&mut self) {
        self.abandonment.cancel();
    }
}

pub const PROVIDER_ID: &str = "kokoro-local";
pub const MODEL_ID: &str = "kokoro-82m-v1.0-int8-multilang";
pub const PACK_ID: &str = "local.tts.kokoro-v1.0-int8.sherpa-onnx-cpu.windows-x64";
pub const PACK_REVISION: &str = "2026.08.30-r1";
/// Canonical digest used by the isolated Python lifecycle verifier. Runtime
/// admission must use Model Manager's separately normalized signed digest.
pub const WORKER_LIFECYCLE_MANIFEST_SHA256: &str =
    "a9100c432183f810a712f539a23baef4100d8810e6c383fdaa5d80204043a1e2";
pub const OUTPUT_SAMPLE_RATE_HZ: u32 = 24_000;

pub const STOCK_VOICES: [&str; 28] = [
    "af_alloy",
    "af_aoede",
    "af_bella",
    "af_heart",
    "af_jessica",
    "af_kore",
    "af_nicole",
    "af_nova",
    "af_river",
    "af_sarah",
    "af_sky",
    "am_adam",
    "am_echo",
    "am_eric",
    "am_fenrir",
    "am_liam",
    "am_michael",
    "am_onyx",
    "am_puck",
    "am_santa",
    "bf_alice",
    "bf_emma",
    "bf_isabella",
    "bf_lily",
    "bm_daniel",
    "bm_fable",
    "bm_george",
    "bm_lewis",
];

#[derive(Clone, Debug)]
pub struct KokoroLocalConfig {
    pub worker_executable: PathBuf,
    pub pack_root: PathBuf,
    pub voice_id: String,
    pub locale: String,
    pub cpu_threads: u8,
    pub load_timeout: Duration,
    /// Maximum inactivity while writing to or reading from the worker after
    /// startup. A timeout retires the child so the next request gets a fresh
    /// process instead of inheriting an indeterminate protocol stream.
    pub response_timeout: Duration,
}

impl KokoroLocalConfig {
    pub fn validate(&self) -> Result<(), KokoroLocalConfigError> {
        if !self.worker_executable.is_absolute() || !self.worker_executable.is_file() {
            return Err(KokoroLocalConfigError::WorkerUnavailable);
        }
        if !self.pack_root.is_absolute()
            || self.pack_root.file_name().and_then(|part| part.to_str()) != Some(PACK_REVISION)
            || self
                .pack_root
                .parent()
                .and_then(|path| path.file_name())
                .and_then(|part| part.to_str())
                != Some(PACK_ID)
        {
            return Err(KokoroLocalConfigError::PackIdentityMismatch);
        }
        if !STOCK_VOICES.contains(&self.voice_id.as_str()) {
            return Err(KokoroLocalConfigError::UnknownStockVoice);
        }
        let expected_locale = if self.voice_id.starts_with('b') {
            "en-GB"
        } else {
            "en-US"
        };
        if self.locale != expected_locale {
            return Err(KokoroLocalConfigError::VoiceLocaleMismatch);
        }
        if !(1..=8).contains(&self.cpu_threads) {
            return Err(KokoroLocalConfigError::InvalidCpuThreads);
        }
        if self.load_timeout.is_zero() || self.load_timeout > Duration::from_secs(300) {
            return Err(KokoroLocalConfigError::InvalidLoadTimeout);
        }
        if self.response_timeout.is_zero() || self.response_timeout > Duration::from_secs(300) {
            return Err(KokoroLocalConfigError::InvalidResponseTimeout);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum KokoroLocalConfigError {
    #[error("native Kokoro worker executable is unavailable")]
    WorkerUnavailable,
    #[error("installed Kokoro pack identity does not match the pinned revision")]
    PackIdentityMismatch,
    #[error("voice is not in the pinned Kokoro stock catalog")]
    UnknownStockVoice,
    #[error("selected stock voice and locale do not match")]
    VoiceLocaleMismatch,
    #[error("CPU thread count must be between 1 and 8")]
    InvalidCpuThreads,
    #[error("worker load timeout is outside the bounded range")]
    InvalidLoadTimeout,
    #[error("worker response timeout is outside the bounded range")]
    InvalidResponseTimeout,
}

pub struct KokoroLocalProvider {
    config: KokoroLocalConfig,
    descriptor: ProviderDescriptor,
    workers: Arc<WorkerSlot>,
}

impl KokoroLocalProvider {
    pub fn new(config: KokoroLocalConfig) -> Result<Self, KokoroLocalConfigError> {
        config.validate()?;
        let descriptor = ProviderDescriptor {
            id: PROVIDER_ID.into(),
            display_name: "Kokoro local stock voices".into(),
            modality: ProviderModality::Speech,
            location: ProviderLocation::Local,
            may_retain_data: false,
            transmitted_data: Vec::<DataClass>::new(),
            capabilities: BTreeMap::from([
                ("model_id".into(), MODEL_ID.into()),
                ("pack_id".into(), PACK_ID.into()),
                ("pack_revision".into(), PACK_REVISION.into()),
                ("streaming_pcm".into(), "true".into()),
                ("alignment".into(), "false".into()),
                ("visemes_or_phonemes".into(), "false".into()),
                ("cancellation".into(), "true".into()),
            ]),
        };
        Ok(Self {
            config,
            descriptor,
            workers: Arc::new(WorkerSlot::default()),
        })
    }

    /// Stops the persistent worker during host shutdown. Dropping the provider
    /// also closes its pipes and `kill_on_drop` contains an unresponsive child.
    pub async fn shutdown(&self) -> Result<(), ProviderError> {
        let Some(worker) = self.workers.take().await else {
            return Ok(());
        };
        let request_id = worker.request_id("shutdown");
        let _ = worker.write(&WorkerCommand::Shutdown { request_id }).await;
        let mut child = worker.child.lock().await;
        match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            _ => {
                child
                    .kill()
                    .await
                    .map_err(|_| unavailable("worker_shutdown_failed"))?;
                Ok(())
            }
        }
    }

    /// Returns the supervised worker PID for native telemetry correlation.
    /// The PID is diagnostic evidence only and never authorizes admission.
    pub async fn worker_process_id(&self) -> Option<u32> {
        let worker = self.workers.current().await?;
        let process_id = worker.child.lock().await.id();
        process_id
    }
}

#[derive(Default)]
struct WorkerSlot {
    current: Mutex<Option<Arc<WorkerClient>>>,
}

impl WorkerSlot {
    async fn acquire(
        &self,
        config: &KokoroLocalConfig,
        cancellation: &CancellationToken,
    ) -> Result<Arc<WorkerClient>, ProviderError> {
        let mut current = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(ProviderError::cancelled(PROVIDER_ID)),
            current = self.current.lock() => current,
        };
        if cancellation.is_cancelled() {
            return Err(ProviderError::cancelled(PROVIDER_ID));
        }
        if let Some(worker) = current.as_ref().filter(|worker| worker.is_healthy()) {
            return Ok(Arc::clone(worker));
        }
        let worker = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(ProviderError::cancelled(PROVIDER_ID)),
            worker = launch(config) => Arc::new(worker?),
        };
        *current = Some(Arc::clone(&worker));
        Ok(worker)
    }

    async fn current(&self) -> Option<Arc<WorkerClient>> {
        self.current.lock().await.as_ref().cloned()
    }

    async fn take(&self) -> Option<Arc<WorkerClient>> {
        self.current.lock().await.take()
    }

    async fn retire(&self, worker: &Arc<WorkerClient>) {
        worker.healthy.store(false, Ordering::Release);
        {
            let mut current = self.current.lock().await;
            if current
                .as_ref()
                .is_some_and(|candidate| Arc::ptr_eq(candidate, worker))
            {
                current.take();
            }
        }
        worker.terminate().await;
    }
}

struct WorkerClient {
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: Arc<Mutex<ChildStdout>>,
    child: Arc<Mutex<Child>>,
    next_request: AtomicU64,
    next_generation: AtomicU64,
    synthesis_gate: Arc<Mutex<()>>,
    response_timeout: Duration,
    healthy: AtomicBool,
}

impl WorkerClient {
    async fn write(&self, command: &WorkerCommand) -> Result<(), ProviderError> {
        if !self.is_healthy() {
            return Err(unavailable("worker_retired"));
        }
        let encoded = serde_json::to_vec(command).map_err(|_| protocol_error("encode_failed"))?;
        if encoded.is_empty() || encoded.len() > MAX_CONTROL_BYTES {
            return Err(protocol_error("control_frame_too_large"));
        }
        tokio::time::timeout(self.response_timeout, async {
            let mut stdin = self.stdin.lock().await;
            stdin
                .write_all(&(encoded.len() as u32).to_be_bytes())
                .await
                .map_err(|_| unavailable("worker_input_closed"))?;
            stdin
                .write_all(&encoded)
                .await
                .map_err(|_| unavailable("worker_input_closed"))?;
            stdin
                .flush()
                .await
                .map_err(|_| unavailable("worker_input_closed"))
        })
        .await
        .map_err(|_| unavailable("worker_input_timeout"))?
    }

    fn request_id(&self, prefix: &str) -> String {
        format!(
            "{prefix}-{}",
            self.next_request.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }

    async fn terminate(&self) {
        let mut child = self.child.lock().await;
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill().await;
        }
    }
}

async fn read_event(stdout: &mut ChildStdout) -> Result<(WorkerEvent, Vec<u8>), ProviderError> {
    let header_len = stdout
        .read_u32()
        .await
        .map_err(|_| unavailable("worker_output_closed"))? as usize;
    let pcm_len = stdout
        .read_u32()
        .await
        .map_err(|_| unavailable("worker_output_closed"))? as usize;
    if header_len == 0 || header_len > MAX_CONTROL_BYTES || pcm_len > MAX_PCM_BYTES {
        return Err(protocol_error("invalid_worker_frame_length"));
    }
    let mut header = vec![0_u8; header_len];
    stdout
        .read_exact(&mut header)
        .await
        .map_err(|_| unavailable("worker_output_closed"))?;
    let mut pcm = vec![0_u8; pcm_len];
    stdout
        .read_exact(&mut pcm)
        .await
        .map_err(|_| unavailable("worker_output_closed"))?;
    let event: WorkerEvent =
        serde_json::from_slice(&header).map_err(|_| protocol_error("invalid_worker_event"))?;
    Ok((event, pcm))
}

async fn send_audio_unless_cancelled(
    sender: &mpsc::Sender<SpeechItem>,
    pcm_s16le: Vec<u8>,
    sequence: u64,
    end_of_stream: bool,
    cancellation: &CancellationToken,
    abandonment: &CancellationToken,
) -> bool {
    let item = Ok(SpeechStreamItem::Audio(AudioChunk {
        sequence,
        sample_rate_hz: OUTPUT_SAMPLE_RATE_HZ,
        channels: 1,
        pcm_s16le,
        end_of_stream,
    }));
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => false,
        _ = abandonment.cancelled() => false,
        result = sender.send(item) => result.is_ok(),
    }
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

async fn launch(config: &KokoroLocalConfig) -> Result<WorkerClient, ProviderError> {
    let mut command = Command::new(&config.worker_executable);
    command
        .arg("--pack-root")
        .arg(&config.pack_root)
        .arg("--cpu-threads")
        .arg(config.cpu_threads.to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| unavailable("worker_launch_failed"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| unavailable("worker_input_unavailable"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| unavailable("worker_output_unavailable"))?;
    let (ready, pcm) = tokio::time::timeout(config.load_timeout, read_event(&mut stdout))
        .await
        .map_err(|_| unavailable("worker_load_timeout"))??;
    if ready.event != "ready"
        || ready.request_id != "startup"
        || !pcm.is_empty()
        || ready.sample_rate_hz != OUTPUT_SAMPLE_RATE_HZ
        || ready.channels != 1
    {
        return Err(protocol_error("invalid_worker_handshake"));
    }
    Ok(WorkerClient {
        stdin: Arc::new(Mutex::new(stdin)),
        stdout: Arc::new(Mutex::new(stdout)),
        child: Arc::new(Mutex::new(child)),
        next_request: AtomicU64::new(1),
        next_generation: AtomicU64::new(1),
        synthesis_gate: Arc::new(Mutex::new(())),
        response_timeout: config.response_timeout,
        healthy: AtomicBool::new(true),
    })
}

struct KokoroLocalSession {
    descriptor: ProviderDescriptor,
    config: KokoroLocalConfig,
    workers: Arc<WorkerSlot>,
    identity: TurnIdentity,
}

#[async_trait]
impl TtsProvider for KokoroLocalProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    async fn start_session(
        &self,
        identity: &TurnIdentity,
        locale: &str,
        cancellation: CancellationToken,
    ) -> Result<Box<dyn TtsSession>, ProviderError> {
        if cancellation.is_cancelled() {
            return Err(ProviderError::cancelled(PROVIDER_ID));
        }
        if locale != self.config.locale {
            return Err(invalid("session_locale_does_not_match_selected_voice"));
        }
        // Load before returning the session so a selected route cannot report
        // itself ready while its native model has not passed the handshake.
        self.workers.acquire(&self.config, &cancellation).await?;
        Ok(Box::new(KokoroLocalSession {
            descriptor: self.descriptor.clone(),
            config: self.config.clone(),
            workers: Arc::clone(&self.workers),
            identity: identity.clone(),
        }))
    }
}

#[async_trait]
impl TtsSession for KokoroLocalSession {
    fn provider(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    async fn synthesize(
        &mut self,
        request: SpeechRequest,
        cancellation: CancellationToken,
    ) -> Result<SpeechStream, ProviderError> {
        if request.identity != self.identity || request.locale != self.config.locale {
            return Err(invalid("speech_request_identity_or_locale_mismatch"));
        }
        if request.text.trim().is_empty() || request.text.chars().count() > 16_384 {
            return Err(invalid("speech_text_outside_bounds"));
        }
        if request
            .voice_hint
            .as_deref()
            .is_some_and(|hint| hint != self.config.voice_id)
        {
            return Err(invalid("voice_hint_does_not_match_selected_stock_voice"));
        }
        let (worker, synthesis_guard) = loop {
            let worker = self.workers.acquire(&self.config, &cancellation).await?;
            let synthesis_guard = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(ProviderError::cancelled(PROVIDER_ID)),
                guard = Arc::clone(&worker.synthesis_gate).lock_owned() => guard,
            };
            if cancellation.is_cancelled() {
                return Err(ProviderError::cancelled(PROVIDER_ID));
            }
            // A prior reader can retire the process while this request waits
            // for its gate. Never write a new command into that stale child.
            if worker.is_healthy() {
                break (worker, synthesis_guard);
            }
            drop(synthesis_guard);
        };
        let request_id = worker.request_id("synth");
        // The native worker is persistent across sessions whose public turn
        // generations may be independent. Give its cancellation domain a
        // provider-local monotonic generation instead of comparing unrelated
        // session counters.
        let generation = worker.next_generation.fetch_add(2, Ordering::AcqRel);
        if let Err(error) = worker
            .write(&WorkerCommand::Synthesize {
                request_id: request_id.clone(),
                generation,
                text: request.text,
                voice_id: self.config.voice_id.clone(),
            })
            .await
        {
            self.workers.retire(&worker).await;
            return Err(error);
        }

        let workers = Arc::clone(&self.workers);
        let cancellation_for_stream = cancellation.clone();
        let cancellation_for_writer = cancellation.clone();
        let abandonment = CancellationToken::new();
        let abandonment_for_writer = abandonment.clone();
        let abandonment_for_reader = abandonment.clone();
        let cancel_worker = Arc::clone(&worker);
        let cancel_id = worker.request_id("cancel");
        let cancel_task = tokio::spawn(async move {
            tokio::select! {
                _ = cancellation_for_writer.cancelled() => {}
                _ = abandonment_for_writer.cancelled() => {}
            }
            let _ = cancel_worker
                .write(&WorkerCommand::Cancel {
                    request_id: cancel_id,
                    generation: generation.saturating_add(1),
                })
                .await;
        });
        let (sender, receiver) = mpsc::channel(8);
        tokio::spawn(async move {
            let _synthesis_guard = synthesis_guard;
            let mut stdout = worker.stdout.lock().await;
            let mut pending_pcm: Option<Vec<u8>> = None;
            let mut expected_worker_sequence = 0_u64;
            let mut next_core_sequence = 1_u64;
            let mut abandoned = false;
            let mut protocol_failed = false;
            let mut ignored_control_events = 0_u8;
            loop {
                let (event, pcm) =
                    match tokio::time::timeout(worker.response_timeout, read_event(&mut stdout))
                        .await
                    {
                        Ok(Ok(frame)) => frame,
                        Err(_) => {
                            protocol_failed = true;
                            let error = if cancellation.is_cancelled() {
                                ProviderError::cancelled(PROVIDER_ID)
                            } else {
                                unavailable("worker_response_timeout")
                            };
                            let _ = sender.send(Err(error)).await;
                            break;
                        }
                        Ok(Err(error)) => {
                            protocol_failed = true;
                            let _ = sender.send(Err(error)).await;
                            break;
                        }
                    };
                if event.event == "cancel_ack"
                    && pcm.is_empty()
                    && event.end_of_stream
                    && event.sequence == 0
                    && event.sample_rate_hz == OUTPUT_SAMPLE_RATE_HZ
                    && event.channels == 1
                    && event.code.is_none()
                {
                    // A cancellation can race the synthesis terminal event.
                    // Accept the bounded late acknowledgement here or at the
                    // start of the next gated request; no audio or synthesis
                    // terminal from another request is ever ignored.
                    ignored_control_events = ignored_control_events.saturating_add(1);
                    if ignored_control_events > 8 {
                        protocol_failed = true;
                        let _ = sender
                            .send(Err(protocol_error("excess_worker_control_events")))
                            .await;
                        break;
                    }
                    continue;
                }
                if event.request_id != request_id {
                    protocol_failed = true;
                    let _ = sender
                        .send(Err(protocol_error("unexpected_worker_request_id")))
                        .await;
                    break;
                }
                match event.event.as_str() {
                    "audio" => {
                        if pcm.is_empty()
                            || pcm.len() % 2 != 0
                            || event.sequence != expected_worker_sequence
                            || event.sample_rate_hz != OUTPUT_SAMPLE_RATE_HZ
                            || event.channels != 1
                            || event.end_of_stream
                            || event.code.is_some()
                        {
                            protocol_failed = true;
                            let _ = sender.send(Err(protocol_error("invalid_pcm_event"))).await;
                            break;
                        }
                        expected_worker_sequence = expected_worker_sequence.saturating_add(1);

                        // Cancellation is authoritative even when PCM was
                        // already waiting in the OS pipe. Never enqueue audio
                        // after the token has fired.
                        if cancellation.is_cancelled()
                            || abandonment_for_reader.is_cancelled()
                            || abandoned
                        {
                            pending_pcm.take();
                            continue;
                        }
                        let mut deliver = Vec::with_capacity(2);
                        if let Some(previous) = pending_pcm.take() {
                            deliver.push(previous);
                        }
                        let mut current = pcm;
                        // Retain exactly one complete PCM frame for EOS. This
                        // satisfies the core contract without delaying a whole
                        // clause callback before first audio delivery.
                        if current.len() > 2 {
                            let tail = current.split_off(current.len() - 2);
                            deliver.push(current);
                            pending_pcm = Some(tail);
                        } else {
                            pending_pcm = Some(current);
                        }
                        let mut delivery_failed = false;
                        for payload in deliver {
                            if !send_audio_unless_cancelled(
                                &sender,
                                payload,
                                next_core_sequence,
                                false,
                                &cancellation,
                                &abandonment_for_reader,
                            )
                            .await
                            {
                                delivery_failed = true;
                                break;
                            }
                            next_core_sequence = next_core_sequence.saturating_add(1);
                        }
                        if delivery_failed {
                            pending_pcm.take();
                            if !abandoned {
                                abandoned = true;
                                let _ = worker
                                    .write(&WorkerCommand::Cancel {
                                        request_id: worker.request_id("abandon"),
                                        generation: generation.saturating_add(1),
                                    })
                                    .await;
                            }
                        }
                    }
                    "completed" => {
                        if !pcm.is_empty()
                            || !event.end_of_stream
                            || event.sequence != expected_worker_sequence
                            || event.sample_rate_hz != OUTPUT_SAMPLE_RATE_HZ
                            || event.channels != 1
                            || event.code.is_some()
                        {
                            protocol_failed = true;
                            let _ = sender
                                .send(Err(protocol_error("invalid_completed_event")))
                                .await;
                            break;
                        }
                        if cancellation.is_cancelled()
                            || abandonment_for_reader.is_cancelled()
                            || abandoned
                        {
                            let _ = sender
                                .send(Err(ProviderError::cancelled(PROVIDER_ID)))
                                .await;
                            break;
                        }
                        let Some(final_pcm) = pending_pcm.take() else {
                            protocol_failed = true;
                            let _ = sender.send(Err(protocol_error("empty_audio_stream"))).await;
                            break;
                        };
                        let sent = send_audio_unless_cancelled(
                            &sender,
                            final_pcm,
                            next_core_sequence,
                            true,
                            &cancellation,
                            &abandonment_for_reader,
                        )
                        .await;
                        if !sent && !sender.is_closed() {
                            let _ = sender
                                .send(Err(ProviderError::cancelled(PROVIDER_ID)))
                                .await;
                        }
                        break;
                    }
                    "cancelled" => {
                        if !pcm.is_empty()
                            || !event.end_of_stream
                            || event.sequence != expected_worker_sequence
                        {
                            protocol_failed = true;
                            let _ = sender
                                .send(Err(protocol_error("invalid_cancelled_event")))
                                .await;
                        } else {
                            let _ = sender
                                .send(Err(ProviderError::cancelled(PROVIDER_ID)))
                                .await;
                        }
                        break;
                    }
                    "error" => {
                        // The native worker should never reject a request that
                        // passed the provider-side bounds. Retire it before a
                        // later synthesis can reuse potentially bad engine
                        // state.
                        protocol_failed = true;
                        if !pcm.is_empty() || !event.end_of_stream {
                            let _ = sender
                                .send(Err(protocol_error("invalid_error_event")))
                                .await;
                        } else {
                            let _ = sender
                                .send(Err(protocol_error(
                                    event.code.as_deref().unwrap_or("native_synthesis_failed"),
                                )))
                                .await;
                        }
                        break;
                    }
                    _ => {
                        protocol_failed = true;
                        let _ = sender
                            .send(Err(protocol_error("unexpected_worker_event")))
                            .await;
                        break;
                    }
                }
            }
            drop(stdout);
            cancel_task.abort();
            if protocol_failed {
                workers.retire(&worker).await;
            }
        });
        Ok(Box::pin(SupervisedSpeechStream::new(
            receiver,
            cancellation_for_stream,
            abandonment,
        )))
    }

    async fn close(&mut self) -> Result<(), ProviderError> {
        // The verified model remains warm across turn sessions. Host shutdown
        // calls `KokoroLocalProvider::shutdown`; process drop remains a
        // kill-on-close containment fallback.
        Ok(())
    }
}

fn error(kind: ProviderErrorKind, message: impl Into<String>, retryable: bool) -> ProviderError {
    ProviderError {
        provider_id: PROVIDER_ID.into(),
        kind,
        message: message.into(),
        retryable,
        retry_after: None,
    }
}

fn invalid(message: &str) -> ProviderError {
    error(ProviderErrorKind::InvalidRequest, message, false)
}

fn protocol_error(message: &str) -> ProviderError {
    error(ProviderErrorKind::Protocol, message, false)
}

fn unavailable(message: &str) -> ProviderError {
    error(ProviderErrorKind::Unavailable, message, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[test]
    fn stock_voice_and_locale_are_exact() {
        assert_eq!(STOCK_VOICES.len(), 28);
        assert!(STOCK_VOICES.contains(&"af_heart"));
        assert!(STOCK_VOICES.contains(&"bm_lewis"));
        assert!(!STOCK_VOICES.contains(&"unknown"));
    }

    #[test]
    fn descriptor_is_local_and_discloses_no_egress() {
        let descriptor = ProviderDescriptor {
            id: PROVIDER_ID.into(),
            display_name: "Kokoro local stock voices".into(),
            modality: ProviderModality::Speech,
            location: ProviderLocation::Local,
            may_retain_data: false,
            transmitted_data: Vec::new(),
            capabilities: BTreeMap::new(),
        };
        assert!(descriptor.location.is_local());
        assert!(descriptor.transmitted_data.is_empty());
        assert!(!descriptor.may_retain_data);
    }

    #[tokio::test]
    async fn consumer_boundary_discards_pcm_queued_before_cancel() {
        let (sender, receiver) = mpsc::channel(1);
        sender
            .try_send(Ok(SpeechStreamItem::Audio(AudioChunk {
                sequence: 1,
                sample_rate_hz: OUTPUT_SAMPLE_RATE_HZ,
                channels: 1,
                pcm_s16le: vec![1, 0],
                end_of_stream: true,
            })))
            .expect("queue PCM before first poll");
        let cancellation = CancellationToken::new();
        let abandonment = CancellationToken::new();
        let mut stream =
            SupervisedSpeechStream::new(receiver, cancellation.clone(), abandonment.clone());

        cancellation.cancel();
        let error = stream
            .next()
            .await
            .expect("one cancellation item")
            .expect_err("queued audio is discarded");
        assert_eq!(error.kind, ProviderErrorKind::Cancelled);
        assert!(stream.next().await.is_none());
        assert!(abandonment.is_cancelled());
    }

    #[tokio::test]
    async fn consumer_boundary_wakes_on_cancel_with_pending_receiver() {
        let (_sender, receiver) = mpsc::channel(1);
        let cancellation = CancellationToken::new();
        let abandonment = CancellationToken::new();
        let stream =
            SupervisedSpeechStream::new(receiver, cancellation.clone(), abandonment.clone());
        let waiting = tokio::spawn(async move {
            let mut stream = stream;
            stream.next().await
        });
        tokio::task::yield_now().await;

        cancellation.cancel();
        let error = tokio::time::timeout(Duration::from_millis(100), waiting)
            .await
            .expect("cancellation wakes a pending stream")
            .expect("poll task completes")
            .expect("one cancellation item")
            .expect_err("pending stream terminates as cancelled");
        assert_eq!(error.kind, ProviderErrorKind::Cancelled);
        assert!(abandonment.is_cancelled());
    }
}
