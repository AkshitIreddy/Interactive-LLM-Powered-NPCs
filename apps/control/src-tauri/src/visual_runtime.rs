//! Authenticated product caller for the procedural current-frame mouth worker.
//!
//! This module is intentionally WebView-inaccessible. Actor identity, process
//! identity, shared handles, launch nonces, and broker session credentials stay
//! in the native control plane. A worker failure produces a visual-only bypass;
//! it never cancels broker PCM playback or blocks the dialogue turn.

use crate::identity_runtime::{
    NativeActorLockBusV1, NativeActorLockProvenanceV1, NativeActorSelectionAuthorityV1,
    NativeSelectedActorLockV1,
};
use crate::local_resources::{
    LocalResourceManager, NativeAdmittedVisualPackLaunchV1, NativeScreenSpaceLipSyncWorkV1,
    NativeVisualScheduleDispositionV1,
};
use crate::media_broker::{
    AudioPlaybackLease, BrokerOcclusionEvidence, BrokerPresentationReceipt, BrokerResidualProposal,
    CapturePixelScope, CapturePixelSource, MediaBrokerSupervisor, VisualAudioEnvelope,
    VisualAudioEnvelopeQuery, VisualSourceLease, VisualTrackBinding, VisualWorkerIdentity,
    MAX_PLAYBACK_LEASES_PER_TURN,
};
use crate::sidecar_supervisor::RuntimeSupervisor;
use async_trait::async_trait;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const WORKER_FILE_NAME: &str = if cfg!(windows) {
    "npc-mouth-worker.exe"
} else {
    "npc-mouth-worker"
};
const PROTOCOL_MAGIC: u32 = 0x3152_574d;
const PROTOCOL_VERSION: u16 = 1;
const MAX_MESSAGE_BYTES: usize = 256 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const OPENSEEFACE_PACK_ID: &str = "openseeface-mnv3-lm1-mouth-signal";
const OPENSEEFACE_PACK_REVISION: &str = "85aa70fc67582d046e771ea73625182a0d8f7475";
const OPENSEEFACE_RUNTIME_REVISION: &str = "1.22.1";
const OPENSEEFACE_BACKEND: &str = "cpu-execution-provider-one-thread";
const VISUAL_COORDINATOR_INTERVAL: Duration = Duration::from_millis(67);
const VISUAL_FRAME_DEADLINE_NS: i64 = 150_000_000;
#[cfg(debug_assertions)]
const REVIEW_VISUAL_CAPTURE_MAX_AGE_NS: i64 = 120_000_000;
#[cfg(debug_assertions)]
const REVIEW_VISUAL_INFERENCE_BUDGET_NS: i64 = 220_000_000;
const MAX_VISUAL_RECEIPT_HISTORY: usize = 128;
#[cfg(debug_assertions)]
const REVIEW_DETECTOR_SHA256: &str =
    "0e8e4806766d85ab067a52c7af0dcb59eb7f9dfe580b44f20a8e6ab712d89809";
#[cfg(debug_assertions)]
const REVIEW_LANDMARK_SHA256: &str =
    "5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f";
#[cfg(debug_assertions)]
const REVIEW_ORT_SHA256: &str = "ea37f63d94a0f37405bf47eaf9c2287cd84b8084bfc37f682d07c4bc305105ed";
#[cfg(debug_assertions)]
const REVIEW_ORT_SHARED_SHA256: &str =
    "6da7afec6c88cf51572c1d0ce60cd97c865b5f348c2431b0733ce48cd03c201c";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WorkerFailurePolicy {
    release_visual_source_metadata: bool,
    drop_visual_worker: bool,
    cancel_playback: bool,
    advance_broker_generation: bool,
}

const WORKER_FAILURE_POLICY: WorkerFailurePolicy = WorkerFailurePolicy {
    release_visual_source_metadata: true,
    drop_visual_worker: true,
    cancel_playback: false,
    advance_broker_generation: false,
};

#[derive(Clone, Debug)]
pub struct MouthWorkerLaunchConfig {
    pub executable: PathBuf,
    pub development_fixture_allowed: bool,
    #[cfg(debug_assertions)]
    pub review_openseeface_root: Option<PathBuf>,
}

impl MouthWorkerLaunchConfig {
    pub fn from_application(
        development_fixture_allowed: bool,
        config_directory: &Path,
    ) -> Result<Self, VisualRuntimeError> {
        let executable = std::env::current_exe()
            .map_err(|_| VisualRuntimeError::InvalidBundle)?
            .parent()
            .ok_or(VisualRuntimeError::InvalidBundle)?
            .join(WORKER_FILE_NAME);
        #[cfg(debug_assertions)]
        let review_openseeface_root = {
            let portable_relative = Path::new("review-model-packs")
                .join(OPENSEEFACE_PACK_ID)
                .join(OPENSEEFACE_PACK_REVISION);
            let portable = executable
                .parent()
                .ok_or(VisualRuntimeError::InvalidBundle)?
                .join(portable_relative);
            if portable.is_dir() {
                Some(portable)
            } else {
                Some(
                    config_directory
                        .join("model-packs")
                        .join(OPENSEEFACE_PACK_ID)
                        .join(OPENSEEFACE_PACK_REVISION),
                )
            }
        };
        Ok(Self {
            executable,
            development_fixture_allowed,
            #[cfg(debug_assertions)]
            review_openseeface_root,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SessionBinding {
    nonce: [u8; 32],
    high: u64,
    low: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedLandmark {
    pub x: f64,
    pub y: f64,
    pub confidence: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenSeeFacePacket {
    pub provider_instance_id: u64,
    pub expected_frame_sequence: u64,
    pub face_x: f64,
    pub face_y: f64,
    pub face_width: f64,
    pub face_height: f64,
    pub landmarks: [NormalizedLandmark; 66],
    pub yaw_degrees: f64,
    pub pitch_degrees: f64,
    pub roll_degrees: f64,
    pub detector_confidence: f64,
    pub landmark_confidence: f64,
    pub visibility_ratio: f64,
    pub mouth_occluded: bool,
    pub measured_at_ns: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceGateEvidence {
    pub descriptor_revision: u64,
    pub expected_digest_high: u64,
    pub expected_digest_low: u64,
    pub observed_digest_high: u64,
    pub observed_digest_low: u64,
    pub similarity: f64,
    pub temporal_iou: f64,
    pub blocker_coverage: f64,
    pub identity_locked: bool,
    pub target_visible: bool,
    pub scene_transition: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VisualPressure {
    Nominal = 0,
    ElevatedCpu = 1,
    ElevatedMemory = 2,
    Critical = 3,
    Suspended = 4,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MouthDrive {
    CausalEnvelopeCoefficients {
        stream_generation: u64,
        segment_id: u64,
        first_sample_index: u64,
        sample_rate: u32,
        channels: u16,
        playback_at_ns: i64,
        coefficients: [f64; 8],
    },
    TimedViseme {
        stream_generation: u64,
        segment_id: u64,
        first_sample_index: u64,
        sample_rate: u32,
        channels: u16,
        playback_at_ns: i64,
        viseme: u8,
        strength: f64,
    },
    PcmWindow {
        stream_generation: u64,
        segment_id: u64,
        first_sample_index: u64,
        sample_rate: u32,
        channels: u16,
        playback_at_ns: i64,
        interleaved_pcm: Vec<f32>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisualFrameRequest {
    pub request_id: u64,
    pub session_id_high: u64,
    pub session_id_low: u64,
    pub turn_id_high: u64,
    pub turn_id_low: u64,
    pub sentence_id: u64,
    pub track: VisualTrackBinding,
    pub landmarks: OpenSeeFacePacket,
    pub appearance: AppearanceGateEvidence,
    pub pressure: VisualPressure,
    pub admitted_signal_rate_hz: u32,
    pub local_visuals_admitted: bool,
    pub drive: MouthDrive,
    pub deadline_ns: i64,
}

/// Native-only request for the ordinary product path. It contains the
/// identity engine's exact selected track and seed face ROI, never caller-
/// supplied landmarks. The authenticated worker produces and binds the 66
/// points to the exact leased WGC frame using the admitted optional pack.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AdmittedVisualFrameRequest {
    pub request_id: u64,
    pub session_id_high: u64,
    pub session_id_low: u64,
    pub turn_id_high: u64,
    pub turn_id_low: u64,
    pub sentence_id: u64,
    pub selected_process_id: u32,
    pub selected_window_handle: u64,
    pub selected_executable_name: String,
    pub track: VisualTrackBinding,
    pub cancellation_generation: u64,
    pub expected_frame_sequence: u64,
    pub expected_frame_qpc: u64,
    pub qpc_frequency: u64,
    pub expected_device_generation: u64,
    pub expected_geometry_epoch: u64,
    pub expected_source_width: u32,
    pub expected_source_height: u32,
    pub seed_face_x: f64,
    pub seed_face_y: f64,
    pub seed_face_width: f64,
    pub seed_face_height: f64,
    pub appearance: AppearanceGateEvidence,
    pub pressure: VisualPressure,
    pub admitted_signal_rate_hz: u32,
    pub audio: VisualAudioBinding,
    pub deadline_ns: i64,
}

/// Exact causal address of the active one-sentence playback lease. The native
/// visual coordinator resolves it through command 29 after WASAPI accepts a
/// buffer; neither PCM nor caller-authored coefficients are accepted here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VisualAudioBinding {
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub stream_id: String,
}

/// Native-only selected-turn coordinator. Audio lease identities originate in
/// the broker allocation returned to RuntimeRouter; actor/frame/ROI/appearance
/// authority originates only in the immutable identity bus. The coordinator
/// accepts neither WebView DTOs nor caller-authored visual requests.
#[derive(Clone)]
pub(crate) struct VisualCoordinator {
    actor_locks: Arc<dyn ActorLockSource>,
    presenter: Arc<dyn AdmittedVisualPresenter>,
    active: Arc<tokio::sync::Mutex<Option<ActiveVisualTurn>>>,
    next_request_id: Arc<AtomicU64>,
    latest_receipt: Arc<Mutex<Option<VisualPresentationReceipt>>>,
    best_turn_receipts: Arc<Mutex<VecDeque<VisualPresentationReceipt>>>,
    receipt_history: Arc<Mutex<VecDeque<VisualPresentationReceipt>>>,
}

impl std::fmt::Debug for VisualCoordinator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VisualCoordinator")
            .field("native_actor_lock_only", &true)
            .finish_non_exhaustive()
    }
}

struct ActiveVisualTurn {
    generation: u64,
    cancellation: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

#[async_trait]
trait AdmittedVisualPresenter: Send + Sync {
    async fn present(
        &self,
        request: AdmittedVisualFrameRequest,
        now_monotonic_millis: u64,
    ) -> VisualPresentationReceipt;

    #[cfg(debug_assertions)]
    async fn prepare_project_owned_review(&self) {}

    #[cfg(debug_assertions)]
    async fn present_project_owned_review(
        &self,
        request_id: u64,
        audio: &VisualAudioBinding,
        sentence_id: u64,
        now_ns: i64,
    ) -> Option<VisualPresentationReceipt>;
}

trait ActorLockSource: Send + Sync {
    fn selected_current(&self, now_unix_ms: u64) -> Option<Arc<NativeSelectedActorLockV1>>;
}

impl ActorLockSource for NativeActorLockBusV1 {
    fn selected_current(&self, now_unix_ms: u64) -> Option<Arc<NativeSelectedActorLockV1>> {
        NativeActorLockBusV1::selected_current(self, now_unix_ms)
    }
}

struct ProductionVisualPresenter {
    worker: MouthWorkerSupervisor,
    resources: Arc<LocalResourceManager>,
}

#[async_trait]
impl AdmittedVisualPresenter for ProductionVisualPresenter {
    async fn present(
        &self,
        request: AdmittedVisualFrameRequest,
        now_monotonic_millis: u64,
    ) -> VisualPresentationReceipt {
        let turn_generation = request.audio.generation;
        let mut receipt = self
            .worker
            .present_admitted_current_frame(&self.resources, request, now_monotonic_millis)
            .await;
        // Receipt indexing follows the dialogue/playback turn. Broker capture
        // cancellation remains independently validated inside the worker path.
        receipt.cancellation_generation = turn_generation;
        receipt
    }

    #[cfg(debug_assertions)]
    async fn prepare_project_owned_review(&self) {
        let _ = self.worker.prepare_project_owned_review_provider().await;
    }

    #[cfg(debug_assertions)]
    async fn present_project_owned_review(
        &self,
        request_id: u64,
        audio: &VisualAudioBinding,
        sentence_id: u64,
        now_ns: i64,
    ) -> Option<VisualPresentationReceipt> {
        let mut receipt = self
            .worker
            .present_project_owned_review_frame(request_id, audio, sentence_id, now_ns)
            .await?;
        receipt.cancellation_generation = audio.generation;
        Some(receipt)
    }
}

impl VisualCoordinator {
    pub(crate) fn new(
        actor_locks: Arc<NativeActorLockBusV1>,
        worker: MouthWorkerSupervisor,
        resources: Arc<LocalResourceManager>,
    ) -> Self {
        Self::from_parts(
            actor_locks,
            Arc::new(ProductionVisualPresenter { worker, resources }),
        )
    }

    fn from_parts(
        actor_locks: Arc<dyn ActorLockSource>,
        presenter: Arc<dyn AdmittedVisualPresenter>,
    ) -> Self {
        Self {
            actor_locks,
            presenter,
            active: Arc::new(tokio::sync::Mutex::new(None)),
            next_request_id: Arc::new(AtomicU64::new(1)),
            latest_receipt: Arc::new(Mutex::new(None)),
            best_turn_receipts: Arc::new(Mutex::new(VecDeque::new())),
            receipt_history: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    #[cfg(test)]
    pub(crate) fn unavailable_for_tests() -> Self {
        Self::from_parts(
            NativeActorLockBusV1::new_unqualified(),
            Arc::new(NoopVisualPresenter),
        )
    }

    /// Starts one bounded visual-only loop from broker-issued playback leases.
    /// Replacing a turn cancels only the previous visual task; it never sends a
    /// broker playback/global cancellation.
    pub(crate) async fn start_turn(&self, leases: &[AudioPlaybackLease]) {
        let (generation, audio) = match visual_audio_bindings(leases) {
            Ok(binding) => binding,
            Err(detail) => {
                self.stop_turn(None).await;
                let request_id = self
                    .next_request_id
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                        value.checked_add(1)
                    })
                    .unwrap_or(1);
                let mut receipt = VisualPresentationReceipt::fail_open(request_id, detail);
                receipt.cancellation_generation =
                    leases.first().map_or(0, |lease| lease.generation);
                self.record_receipt(receipt);
                return;
            }
        };
        self.start_audio_bindings(generation, audio).await;
    }

    async fn start_audio_bindings(&self, generation: u64, audio: Vec<VisualAudioBinding>) {
        self.stop_turn(None).await;
        if generation == 0 || audio.is_empty() {
            return;
        }
        if let Ok(mut receipts) = self.best_turn_receipts.lock() {
            receipts.retain(|receipt| receipt.cancellation_generation != generation);
        }
        #[cfg(debug_assertions)]
        self.presenter.prepare_project_owned_review().await;
        let pending_request_id = self
            .next_request_id
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .unwrap_or(1);
        let mut pending = VisualPresentationReceipt::fail_open(
            pending_request_id,
            "visual_audio_coordinator_waiting_for_presentable_frame",
        );
        pending.cancellation_generation = generation;
        self.record_receipt(pending);
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let actor_locks = Arc::clone(&self.actor_locks);
        let presenter = Arc::clone(&self.presenter);
        let next_request_id = Arc::clone(&self.next_request_id);
        let latest_receipt = Arc::clone(&self.latest_receipt);
        let best_turn_receipts = Arc::clone(&self.best_turn_receipts);
        let receipt_history = Arc::clone(&self.receipt_history);
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(VISUAL_COORDINATOR_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut preferred_stream = 0_usize;
            loop {
                tokio::select! {
                    _ = task_cancellation.cancelled() => break,
                    _ = interval.tick() => {}
                }
                let Ok(now_ns) = monotonic_ns() else { continue };
                let now_unix_ms = current_unix_millis();
                let Some(lock) = actor_locks.selected_current(now_unix_ms) else {
                    #[cfg(debug_assertions)]
                    {
                        let mut handled = false;
                        for offset in 0..audio.len() {
                            let index = (preferred_stream + offset) % audio.len();
                            let request_id = next_request_id
                                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                                    value.checked_add(1)
                                })
                                .unwrap_or(1);
                            if let Some(receipt) = presenter
                                .present_project_owned_review(
                                    request_id,
                                    &audio[index],
                                    index as u64 + 1,
                                    now_ns,
                                )
                                .await
                            {
                                handled = true;
                                record_visual_receipt(
                                    &latest_receipt,
                                    &best_turn_receipts,
                                    &receipt_history,
                                    receipt,
                                );
                                preferred_stream = index;
                                break;
                            }
                        }
                        if handled {
                            continue;
                        }
                    }
                    let request_id = next_request_id
                        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                            value.checked_add(1)
                        })
                        .unwrap_or(1);
                    let mut receipt = VisualPresentationReceipt::fail_open(
                        request_id,
                        "visual_actor_lock_unavailable",
                    );
                    receipt.cancellation_generation = generation;
                    record_visual_receipt(
                        &latest_receipt,
                        &best_turn_receipts,
                        &receipt_history,
                        receipt,
                    );
                    continue;
                };
                for offset in 0..audio.len() {
                    let index = (preferred_stream + offset) % audio.len();
                    let request_id = next_request_id
                        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                            value.checked_add(1)
                        })
                        .unwrap_or(1);
                    let Some(request) = admitted_request_from_actor_lock(
                        request_id,
                        &lock,
                        &audio[index],
                        index as u64 + 1,
                        now_ns,
                    ) else {
                        break;
                    };
                    let receipt = presenter
                        .present(request, (now_ns as u64) / 1_000_000)
                        .await;
                    let reached_audio_or_worker = receipt.product_disposition != 4;
                    record_visual_receipt(
                        &latest_receipt,
                        &best_turn_receipts,
                        &receipt_history,
                        receipt,
                    );
                    if reached_audio_or_worker {
                        preferred_stream = index;
                        break;
                    }
                }
            }
        });
        *self.active.lock().await = Some(ActiveVisualTurn {
            generation,
            cancellation,
            task,
        });
    }

    pub(crate) async fn stop_turn(&self, generation: Option<u64>) {
        let active = {
            let mut state = self.active.lock().await;
            if generation.is_some_and(|expected| {
                state
                    .as_ref()
                    .is_some_and(|active| active.generation != expected)
            }) {
                return;
            }
            state.take()
        };
        if let Some(active) = active {
            active.cancellation.cancel();
            let mut task = active.task;
            if tokio::time::timeout(Duration::from_millis(250), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
    }

    pub(crate) fn latest_receipt(&self) -> Option<VisualPresentationReceipt> {
        self.latest_receipt
            .lock()
            .ok()
            .and_then(|receipt| receipt.clone())
    }

    pub(crate) fn receipt_history(&self) -> Vec<VisualPresentationReceipt> {
        self.receipt_history
            .lock()
            .map(|receipts| receipts.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) fn best_receipt_for_generation(
        &self,
        generation: u64,
    ) -> Option<VisualPresentationReceipt> {
        self.best_turn_receipts.lock().ok().and_then(|receipts| {
            receipts
                .iter()
                .find(|receipt| receipt.cancellation_generation == generation)
                .cloned()
        })
    }

    fn record_receipt(&self, receipt: VisualPresentationReceipt) {
        record_visual_receipt(
            &self.latest_receipt,
            &self.best_turn_receipts,
            &self.receipt_history,
            receipt,
        );
    }
}

fn record_visual_receipt(
    latest: &Arc<Mutex<Option<VisualPresentationReceipt>>>,
    best_turn_receipts: &Arc<Mutex<VecDeque<VisualPresentationReceipt>>>,
    history: &Arc<Mutex<VecDeque<VisualPresentationReceipt>>>,
    receipt: VisualPresentationReceipt,
) {
    if let Ok(mut latest) = latest.lock() {
        *latest = Some(receipt.clone());
    }
    if receipt.cancellation_generation != 0 {
        if let Ok(mut receipts) = best_turn_receipts.lock() {
            if let Some(current) = receipts
                .iter_mut()
                .find(|current| current.cancellation_generation == receipt.cancellation_generation)
            {
                if better_visual_receipt(&receipt, current) {
                    *current = receipt.clone();
                }
            } else {
                receipts.push_back(receipt.clone());
                while receipts.len() > MAX_VISUAL_RECEIPT_HISTORY {
                    receipts.pop_front();
                }
            }
        }
    }
    if let Ok(mut history) = history.lock() {
        history.push_back(receipt);
        while history.len() > MAX_VISUAL_RECEIPT_HISTORY {
            history.pop_front();
        }
    }
}

fn better_visual_receipt(
    candidate: &VisualPresentationReceipt,
    current: &VisualPresentationReceipt,
) -> bool {
    let quality = |receipt: &VisualPresentationReceipt| {
        (
            receipt.presented,
            receipt.residual_proposed,
            receipt.source_frame_sequence != 0,
            receipt.product_disposition != 4,
            receipt.request_id,
        )
    };
    quality(candidate) > quality(current)
}

#[cfg(test)]
struct NoopVisualPresenter;

#[cfg(test)]
#[async_trait]
impl AdmittedVisualPresenter for NoopVisualPresenter {
    async fn present(
        &self,
        request: AdmittedVisualFrameRequest,
        _now_monotonic_millis: u64,
    ) -> VisualPresentationReceipt {
        VisualPresentationReceipt::fail_open(request.request_id, "test_visual_unavailable")
    }

    #[cfg(debug_assertions)]
    async fn present_project_owned_review(
        &self,
        _request_id: u64,
        _audio: &VisualAudioBinding,
        _sentence_id: u64,
        _now_ns: i64,
    ) -> Option<VisualPresentationReceipt> {
        None
    }
}

fn visual_audio_bindings(
    leases: &[AudioPlaybackLease],
) -> Result<(u64, Vec<VisualAudioBinding>), &'static str> {
    let first = leases.first().ok_or("visual_audio_lease_pool_empty")?;
    if leases.len() > MAX_PLAYBACK_LEASES_PER_TURN {
        return Err("visual_audio_lease_pool_exceeds_runtime_bound");
    }
    if first.generation == 0 {
        return Err("visual_audio_lease_generation_missing");
    }
    if first.session_id.is_empty() || first.turn_id.is_empty() {
        return Err("visual_audio_lease_correlation_missing");
    }
    let mut streams = std::collections::BTreeSet::new();
    let mut bindings = Vec::with_capacity(leases.len());
    for lease in leases {
        if lease.schema_version != 2 {
            return Err("visual_audio_lease_schema_unsupported");
        }
        if lease.generation != first.generation
            || lease.session_id != first.session_id
            || lease.turn_id != first.turn_id
        {
            return Err("visual_audio_lease_pool_correlation_mismatch");
        }
        if lease.stream_id.is_empty() || !streams.insert(lease.stream_id.as_str()) {
            return Err("visual_audio_lease_stream_identity_invalid");
        }
        bindings.push(VisualAudioBinding {
            session_id: lease.session_id.clone(),
            turn_id: lease.turn_id.clone(),
            generation: lease.generation,
            stream_id: lease.stream_id.clone(),
        });
    }
    Ok((first.generation, bindings))
}

fn admitted_request_from_actor_lock(
    request_id: u64,
    lock: &NativeSelectedActorLockV1,
    audio: &VisualAudioBinding,
    sentence_id: u64,
    now_ns: i64,
) -> Option<AdmittedVisualFrameRequest> {
    let captured_ns = qpc_value_to_ns(lock.source_frame_qpc, lock.qpc_frequency).ok()?;
    let deadline_ns = captured_ns.checked_add(VISUAL_FRAME_DEADLINE_NS)?;
    let roi = lock.full_source_roi;
    let lower_sha256 = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    let authority_admitted = match (&lock.selection_authority, &lock.provenance) {
        (
            NativeActorSelectionAuthorityV1::Consensus,
            NativeActorLockProvenanceV1::QualifiedIdentity {
                qualification_id,
                catalog_admission_sha256,
                admission_receipt_sha256,
            },
        ) => {
            !qualification_id.is_empty()
                && lower_sha256(catalog_admission_sha256)
                && lower_sha256(admission_receipt_sha256)
        }
        (
            NativeActorSelectionAuthorityV1::SealedNativeClick,
            NativeActorLockProvenanceV1::SealedNativeClick {
                visual_pack_id,
                visual_pack_admission_sha256,
                native_click_receipt_sha256,
                candidate_set_sha256,
                receipt_nonce_high,
                receipt_nonce_low,
            },
        ) => {
            !visual_pack_id.is_empty()
                && lower_sha256(visual_pack_admission_sha256)
                && lower_sha256(native_click_receipt_sha256)
                && lower_sha256(candidate_set_sha256)
                && (*receipt_nonce_high != 0 || *receipt_nonce_low != 0)
        }
        _ => false,
    };
    if request_id == 0
        || lock.runtime_actor_id == 0
        || lock.track_id == 0
        || lock.track_epoch == 0
        || lock.source_frame_sequence == 0
        || lock.source_frame_qpc == 0
        || lock.device_generation == 0
        || lock.geometry_epoch == 0
        || lock.selected_process_id == 0
        || lock.selected_window_handle == 0
        || lock.selected_executable_name.is_empty()
        || !authority_admitted
        || lock.scene_transition_detected
        || now_ns < captured_ns
        || now_ns > deadline_ns
        || roi.source_width == 0
        || roi.source_height == 0
    {
        return None;
    }
    let probability = |value: f32| value.is_finite() && (0.0..=1.0).contains(&value);
    if !probability(lock.appearance_similarity)
        || !probability(lock.temporal_iou)
        || !probability(lock.blocker_coverage)
    {
        return None;
    }
    let (session_id_high, session_id_low) = correlation_words(&audio.session_id);
    let (turn_id_high, turn_id_low) = correlation_words(&audio.turn_id);
    Some(AdmittedVisualFrameRequest {
        request_id,
        session_id_high,
        session_id_low,
        turn_id_high,
        turn_id_low,
        sentence_id,
        selected_process_id: lock.selected_process_id,
        selected_window_handle: lock.selected_window_handle,
        selected_executable_name: lock.selected_executable_name.clone(),
        track: VisualTrackBinding {
            actor_id: lock.runtime_actor_id,
            track_id: lock.track_id,
            track_epoch: lock.track_epoch,
        },
        cancellation_generation: lock.cancellation_generation,
        expected_frame_sequence: lock.source_frame_sequence,
        expected_frame_qpc: lock.source_frame_qpc,
        qpc_frequency: lock.qpc_frequency,
        expected_device_generation: lock.device_generation,
        expected_geometry_epoch: lock.geometry_epoch,
        expected_source_width: roi.source_width,
        expected_source_height: roi.source_height,
        seed_face_x: f64::from(roi.x),
        seed_face_y: f64::from(roi.y),
        seed_face_width: f64::from(roi.width),
        seed_face_height: f64::from(roi.height),
        appearance: AppearanceGateEvidence {
            descriptor_revision: lock.appearance_descriptor_revision,
            expected_digest_high: lock.expected_appearance_digest_high,
            expected_digest_low: lock.expected_appearance_digest_low,
            observed_digest_high: lock.observed_appearance_digest_high,
            observed_digest_low: lock.observed_appearance_digest_low,
            similarity: f64::from(lock.appearance_similarity),
            temporal_iou: f64::from(lock.temporal_iou),
            blocker_coverage: f64::from(lock.blocker_coverage),
            identity_locked: true,
            target_visible: true,
            scene_transition: lock.scene_transition_detected,
        },
        pressure: VisualPressure::Nominal,
        admitted_signal_rate_hz: 15,
        audio: audio.clone(),
        deadline_ns,
    })
}

fn correlation_words(value: &str) -> (u64, u64) {
    let digest = Sha256::digest(value.as_bytes());
    let mut high = u64::from_le_bytes(digest[0..8].try_into().unwrap_or([0; 8]));
    let low = u64::from_le_bytes(digest[8..16].try_into().unwrap_or([0; 8]));
    if high == 0 && low == 0 {
        high = 1;
    }
    (high, low)
}

fn current_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VisualPresentationReceipt {
    pub schema_version: u32,
    pub request_id: u64,
    pub source_frame_sequence: u64,
    pub cancellation_generation: u64,
    pub product_disposition: u8,
    pub worker_disposition: u8,
    pub signal_disposition: u8,
    pub queue_replacements: u64,
    pub residual_proposed: bool,
    pub presented: bool,
    pub degraded: bool,
    pub pixel_source: Option<CapturePixelSource>,
    pub pixel_scope: Option<CapturePixelScope>,
    pub external_display_overlay_pixels_excluded: bool,
    pub desktop_luminance_excluded_from_pixel_evidence: bool,
    pub external_display_overlays_may_change_perceived_brightness: bool,
    pub detail: String,
    pub broker: Option<BrokerPresentationReceipt>,
}

impl VisualPresentationReceipt {
    fn fail_open(request_id: u64, detail: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            request_id,
            source_frame_sequence: 0,
            cancellation_generation: 0,
            product_disposition: 4,
            worker_disposition: 1,
            signal_disposition: 4,
            queue_replacements: 0,
            residual_proposed: false,
            presented: false,
            degraded: true,
            pixel_source: None,
            pixel_scope: None,
            external_display_overlay_pixels_excluded: false,
            desktop_luminance_excluded_from_pixel_evidence: false,
            external_display_overlays_may_change_perceived_brightness: false,
            detail: detail.into(),
            broker: None,
        }
    }
}

#[derive(Clone)]
pub struct MouthWorkerSupervisor {
    config: Arc<MouthWorkerLaunchConfig>,
    parent_job: RuntimeSupervisor,
    broker: MediaBrokerSupervisor,
    managed: Arc<tokio::sync::Mutex<Option<ManagedWorker>>>,
    startup_gate: Arc<tokio::sync::Mutex<()>>,
    frame_gate: Arc<tokio::sync::Mutex<()>>,
}

impl std::fmt::Debug for MouthWorkerSupervisor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MouthWorkerSupervisor")
            .field("configured", &self.config.executable)
            .finish_non_exhaustive()
    }
}

impl MouthWorkerSupervisor {
    pub fn new(
        config: MouthWorkerLaunchConfig,
        parent_job: RuntimeSupervisor,
        broker: MediaBrokerSupervisor,
    ) -> Self {
        Self {
            config: Arc::new(config),
            parent_job,
            broker,
            managed: Arc::new(tokio::sync::Mutex::new(None)),
            startup_gate: Arc::new(tokio::sync::Mutex::new(())),
            frame_gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Runs one exact-frame visual attempt. Contention, stale tracker output,
    /// worker failure, and broker pressure all return a visual-only bypass.
    pub async fn present_current_frame(
        &self,
        request: VisualFrameRequest,
    ) -> VisualPresentationReceipt {
        let Ok(_frame) = self.frame_gate.try_lock() else {
            return VisualPresentationReceipt::fail_open(
                request.request_id,
                "visual_queue_pressure_bypass",
            );
        };
        match self.present_current_frame_inner(&request).await {
            Ok(receipt) => receipt,
            Err(error) => VisualPresentationReceipt::fail_open(
                request.request_id,
                format!("visual_only_bypass:{error}"),
            ),
        }
    }

    /// Review-build bridge for the project-owned realistic capture target.
    /// The target metadata proves that the embedded source mouth is static;
    /// this method then runs the pinned OpenSeeFace detector/66-point model on
    /// the broker's exact current WGC texture and drives the procedural mouth
    /// residual from the post-WASAPI causal envelope. It is unavailable in a
    /// release build and for every target except the native-bound fixture.
    #[cfg(debug_assertions)]
    async fn prepare_project_owned_review_provider(&self) -> Result<(), VisualRuntimeError> {
        if !self.broker.project_owned_review_target_selected() {
            return Err(VisualRuntimeError::NoSelectedTarget);
        }
        let snapshot = self
            .broker
            .debug_synthetic_replay_capture_diagnostics()
            .await?;
        let evidence = snapshot
            .capture_evidence
            .ok_or(VisualRuntimeError::CaptureAuthority)?;
        if evidence.pixel_source != CapturePixelSource::WindowsGraphicsCaptureTexture
            || evidence.pixel_scope != CapturePixelScope::ExactSelectedWindow
            || !evidence.external_display_overlay_pixels_excluded
            || !evidence.desktop_luminance_excluded_from_pixel_evidence
            || evidence.selected_process_id != snapshot.target_process_id
            || evidence.selected_window_handle != snapshot.target_window_handle
            || !evidence
                .selected_executable_name
                .eq_ignore_ascii_case(&snapshot.target_executable_basename)
        {
            return Err(VisualRuntimeError::CaptureAuthority);
        }
        let generation = snapshot.diagnostics.cancellation_generation;
        if generation == 0 {
            return Err(VisualRuntimeError::NoSelectedTarget);
        }
        let launch = resolve_project_owned_review_provider(
            self.config.review_openseeface_root.as_deref(),
            evidence.selected_process_id,
        )?;
        let (client, _) = self.ensure_ready(generation).await?;
        self.configure_project_owned_review_provider(&client, generation, &launch)
            .await
    }

    #[cfg(debug_assertions)]
    pub(crate) async fn present_project_owned_review_frame(
        &self,
        request_id: u64,
        audio: &VisualAudioBinding,
        sentence_id: u64,
        now_ns: i64,
    ) -> Option<VisualPresentationReceipt> {
        if !self.broker.project_owned_review_target_selected() {
            return None;
        }
        let Ok(_frame) = self.frame_gate.try_lock() else {
            return Some(VisualPresentationReceipt::fail_open(
                request_id,
                "review_visual_queue_pressure_bypass",
            ));
        };
        Some(
            match self
                .present_project_owned_review_frame_inner(request_id, audio, sentence_id, now_ns)
                .await
            {
                Ok(receipt) => receipt,
                Err(error) => VisualPresentationReceipt::fail_open(
                    request_id,
                    format!("review_visual_only_bypass:{error}"),
                ),
            },
        )
    }

    /// Product entrypoint for a trusted native landmark/audio producer. It
    /// binds pack admission, current resource pressure, exact target pixels,
    /// generation/frame scheduling, and the worker attempt. Every rejected
    /// gate is visual-only and leaves PCM/subtitles untouched.
    pub async fn present_governed_current_frame(
        &self,
        resources: &LocalResourceManager,
        mut request: VisualFrameRequest,
        now_monotonic_millis: u64,
    ) -> VisualPresentationReceipt {
        let fail =
            |detail: String| VisualPresentationReceipt::fail_open(request.request_id, detail);
        let launch = match resources.resolve_admitted_openseeface_launch() {
            Ok(launch) => launch,
            Err(error) => return fail(format!("visual_admission_bypass:{error}")),
        };
        let evidence = match self.broker.native_capture_evidence().await {
            Ok(evidence) => evidence,
            Err(error) => return fail(format!("visual_capture_evidence_bypass:{error}")),
        };
        if evidence.selected_process_id != launch.exact_target_pid
            || evidence.pixel_source != CapturePixelSource::WindowsGraphicsCaptureTexture
            || evidence.pixel_scope != CapturePixelScope::ExactSelectedWindow
            || !evidence.external_display_overlay_pixels_excluded
            || !evidence.desktop_luminance_excluded_from_pixel_evidence
            || evidence.latest_frame_sequence != request.landmarks.expected_frame_sequence
        {
            return fail("visual_exact_window_binding_bypass".into());
        }
        let diagnostics = match self.broker.diagnostics().await {
            Ok(diagnostics) => diagnostics,
            Err(error) => return fail(format!("visual_generation_bypass:{error}")),
        };
        let generation_id = diagnostics.cancellation_generation;
        let frame_id = request.landmarks.expected_frame_sequence;
        let clock = resources.advance_visual_clock(generation_id, frame_id, now_monotonic_millis);
        if clock.disposition != NativeVisualScheduleDispositionV1::Ready {
            return fail(format!("visual_clock_bypass:{}", clock.detail));
        }
        let pressure = resources
            .apply_current_pressure(Some(evidence.selected_process_id), now_monotonic_millis);
        if pressure.disposition != NativeVisualScheduleDispositionV1::Ready {
            return fail(format!("visual_pressure_bypass:{}", pressure.detail));
        }
        let deadline_monotonic_millis = u64::try_from(request.deadline_ns)
            .ok()
            .map(|value| value / 1_000_000)
            .unwrap_or_default();
        let work = NativeScreenSpaceLipSyncWorkV1 {
            actor_id: request.track.actor_id.to_string(),
            track_id: request.track.track_id.to_string(),
            session_epoch: request.track.track_epoch,
            generation_id,
            frame_id,
            deadline_monotonic_millis,
        };
        let queued = resources.submit_screen_space_lip_sync(work.clone(), now_monotonic_millis);
        if queued.disposition != NativeVisualScheduleDispositionV1::Queued {
            return fail(format!("visual_queue_bypass:{}", queued.detail));
        }
        let dispatched = resources.pop_next_screen_space_lip_sync(now_monotonic_millis);
        if dispatched.disposition != NativeVisualScheduleDispositionV1::Dispatched
            || dispatched.work.as_ref() != Some(&work)
        {
            if let Some(work_id) = queued.work_id.as_deref() {
                let _ = resources.cancel_screen_space_lip_sync(work_id);
            }
            return fail(format!("visual_dispatch_bypass:{}", dispatched.detail));
        }
        // These fields are authority outputs here, not caller assertions.
        request.local_visuals_admitted = true;
        request.admitted_signal_rate_hz = request.admitted_signal_rate_hz.min(15);
        let mut receipt = self.present_current_frame(request).await;
        receipt.pixel_source = Some(evidence.pixel_source);
        receipt.pixel_scope = Some(evidence.pixel_scope);
        receipt.external_display_overlay_pixels_excluded =
            evidence.external_display_overlay_pixels_excluded;
        receipt.desktop_luminance_excluded_from_pixel_evidence =
            evidence.desktop_luminance_excluded_from_pixel_evidence;
        receipt.external_display_overlays_may_change_perceived_brightness =
            evidence.external_display_overlays_may_change_perceived_brightness;
        receipt
    }

    /// Ordinary clean-install product entrypoint. The caller supplies only
    /// native identity/appearance evidence, a seed ROI, and causal audio
    /// timing. Pack paths and landmark output are resolved inside the native
    /// control plane and never cross the WebView boundary.
    pub(crate) async fn present_admitted_current_frame(
        &self,
        resources: &LocalResourceManager,
        request: AdmittedVisualFrameRequest,
        now_monotonic_millis: u64,
    ) -> VisualPresentationReceipt {
        let Ok(_frame) = self.frame_gate.try_lock() else {
            return VisualPresentationReceipt::fail_open(
                request.request_id,
                "visual_queue_pressure_bypass",
            );
        };
        match self
            .present_admitted_current_frame_inner(resources, &request, now_monotonic_millis)
            .await
        {
            Ok(receipt) => receipt,
            Err(error) => VisualPresentationReceipt::fail_open(
                request.request_id,
                format!("visual_only_bypass:{error}"),
            ),
        }
    }

    async fn present_admitted_current_frame_inner(
        &self,
        resources: &LocalResourceManager,
        request: &AdmittedVisualFrameRequest,
        now_monotonic_millis: u64,
    ) -> Result<VisualPresentationReceipt, VisualRuntimeError> {
        validate_admitted_frame_request(request)?;
        let launch = resources
            .resolve_admitted_openseeface_launch()
            .map_err(|error| VisualRuntimeError::Admission(error.to_string()))?;
        validate_admitted_launch(&launch)?;
        let evidence = self.broker.native_capture_evidence().await?;
        let authority_checked_at_ns = monotonic_ns()?;
        if evidence.selected_process_id != launch.exact_target_pid
            || evidence.selected_process_id != request.selected_process_id
            || evidence.selected_window_handle != request.selected_window_handle
            || evidence.selected_executable_name != request.selected_executable_name
            || evidence.pixel_source != CapturePixelSource::WindowsGraphicsCaptureTexture
            || evidence.pixel_scope != CapturePixelScope::ExactSelectedWindow
            || !evidence.external_display_overlay_pixels_excluded
            || !evidence.desktop_luminance_excluded_from_pixel_evidence
            || !bounded_frame_progression(
                request.expected_frame_sequence,
                request.expected_frame_qpc,
                evidence.latest_frame_sequence,
                evidence.latest_frame_qpc,
                request.qpc_frequency,
                authority_checked_at_ns,
            )
            || evidence.device_generation != request.expected_device_generation
            || evidence.geometry_epoch != request.expected_geometry_epoch
            || evidence.content_width != request.expected_source_width
            || evidence.content_height != request.expected_source_height
            || request.qpc_frequency == 0
        {
            return Err(VisualRuntimeError::CaptureAuthority);
        }
        let generation = self.broker.diagnostics().await?.cancellation_generation;
        if generation == 0 || generation != request.cancellation_generation {
            return Err(VisualRuntimeError::NoSelectedTarget);
        }
        let clock = resources.advance_visual_clock(
            generation,
            request.expected_frame_sequence,
            now_monotonic_millis,
        );
        if clock.disposition != NativeVisualScheduleDispositionV1::Ready {
            return Err(VisualRuntimeError::Scheduling(clock.detail));
        }
        let pressure = resources
            .apply_current_pressure(Some(evidence.selected_process_id), now_monotonic_millis);
        if pressure.disposition != NativeVisualScheduleDispositionV1::Ready {
            return Err(VisualRuntimeError::Scheduling(pressure.detail));
        }
        let deadline_monotonic_millis = u64::try_from(request.deadline_ns)
            .ok()
            .map(|value| value / 1_000_000)
            .unwrap_or_default();
        let work = NativeScreenSpaceLipSyncWorkV1 {
            actor_id: request.track.actor_id.to_string(),
            track_id: request.track.track_id.to_string(),
            session_epoch: request.track.track_epoch,
            generation_id: generation,
            frame_id: request.expected_frame_sequence,
            deadline_monotonic_millis,
        };
        let queued = resources.submit_screen_space_lip_sync(work.clone(), now_monotonic_millis);
        if queued.disposition != NativeVisualScheduleDispositionV1::Queued {
            return Err(VisualRuntimeError::Scheduling(queued.detail));
        }
        let dispatched = resources.pop_next_screen_space_lip_sync(now_monotonic_millis);
        if dispatched.disposition != NativeVisualScheduleDispositionV1::Dispatched
            || dispatched.work.as_ref() != Some(&work)
        {
            if let Some(work_id) = queued.work_id.as_deref() {
                let _ = resources.cancel_screen_space_lip_sync(work_id);
            }
            return Err(VisualRuntimeError::Scheduling(dispatched.detail));
        }

        let (client, worker) = self.ensure_ready(generation).await?;
        self.configure_admitted_provider(&client, generation, &launch)
            .await?;
        // Finish every fallible/awaiting audio operation before asking the
        // broker to duplicate a source texture into the worker. Once a lease
        // exists, no cancellation point is crossed until the worker request
        // has been submitted; the blocking request keeps running even if its
        // outer future is dropped.
        let audio = self
            .broker
            .query_visual_audio_envelope(VisualAudioEnvelopeQuery {
                session_id: request.audio.session_id.clone(),
                turn_id: request.audio.turn_id.clone(),
                generation: request.audio.generation,
                stream_id: request.audio.stream_id.clone(),
                segment_id: request.audio.stream_id.clone(),
            })
            .await?;
        let drive = drive_from_visual_audio_envelope(&audio, monotonic_ns()?)?;
        let lease = self
            .broker
            .allocate_visual_source(&worker, &request.track)
            .await?;
        let frame_bound_at_ns = match monotonic_ns() {
            Ok(value) => value,
            Err(error) => {
                self.abandon_unsubmitted_visual_source(&lease).await;
                return Err(error);
            }
        };
        if lease.cancellation_generation != generation
            || lease.qpc_frequency != request.qpc_frequency
            || lease.source_device_generation != request.expected_device_generation
            || lease.source_geometry_epoch != request.expected_geometry_epoch
            || lease.width != request.expected_source_width
            || lease.height != request.expected_source_height
            || !bounded_frame_progression(
                request.expected_frame_sequence,
                request.expected_frame_qpc,
                lease.source_frame_sequence,
                lease.source_frame_qpc,
                lease.qpc_frequency,
                frame_bound_at_ns,
            )
        {
            self.abandon_unsubmitted_visual_source(&lease).await;
            return Err(VisualRuntimeError::StaleLandmarks);
        }
        let mut render_request = request.clone();
        render_request.expected_frame_sequence = lease.source_frame_sequence;
        render_request.expected_frame_qpc = lease.source_frame_qpc;
        render_request.deadline_ns =
            match qpc_value_to_ns(lease.source_frame_qpc, lease.qpc_frequency).and_then(
                |captured| {
                    captured
                        .checked_add(VISUAL_FRAME_DEADLINE_NS)
                        .ok_or(VisualRuntimeError::Clock)
                },
            ) {
                Ok(value) => value,
                Err(error) => {
                    self.abandon_unsubmitted_visual_source(&lease).await;
                    return Err(error);
                }
            };
        let command =
            match encode_admitted_render_command(&render_request, &drive, &lease, client.session())
            {
                Ok(value) => value,
                Err(error) => {
                    self.abandon_unsubmitted_visual_source(&lease).await;
                    return Err(error);
                }
            };
        let render = client
            .request(
                WorkerCommand::RenderWithAdmittedLandmarks,
                generation,
                command,
            )
            .await;
        let response = match render {
            Ok(value) => value,
            Err(error) => {
                self.abandon_unsubmitted_visual_source(&lease).await;
                return Err(error);
            }
        };
        let presentation = self
            .present_worker_response(
                response,
                request.request_id,
                &request.track,
                &lease,
                &worker,
                &client,
                generation,
            )
            .await;
        let release = self.broker.release_visual_source(&lease).await;
        let mut receipt = presentation?;
        release?;
        receipt.pixel_source = Some(evidence.pixel_source);
        receipt.pixel_scope = Some(evidence.pixel_scope);
        receipt.external_display_overlay_pixels_excluded =
            evidence.external_display_overlay_pixels_excluded;
        receipt.desktop_luminance_excluded_from_pixel_evidence =
            evidence.desktop_luminance_excluded_from_pixel_evidence;
        receipt.external_display_overlays_may_change_perceived_brightness =
            evidence.external_display_overlays_may_change_perceived_brightness;
        Ok(receipt)
    }

    #[cfg(debug_assertions)]
    async fn present_project_owned_review_frame_inner(
        &self,
        request_id: u64,
        audio_binding: &VisualAudioBinding,
        sentence_id: u64,
        now_ns: i64,
    ) -> Result<VisualPresentationReceipt, VisualRuntimeError> {
        if request_id == 0 || sentence_id == 0 || now_ns <= 0 {
            return Err(VisualRuntimeError::InvalidRequest);
        }
        let snapshot = self
            .broker
            .debug_synthetic_replay_capture_diagnostics()
            .await?;
        let evidence = snapshot
            .capture_evidence
            .ok_or(VisualRuntimeError::CaptureAuthority)?;
        if evidence.pixel_source != CapturePixelSource::WindowsGraphicsCaptureTexture
            || evidence.pixel_scope != CapturePixelScope::ExactSelectedWindow
            || !evidence.external_display_overlay_pixels_excluded
            || !evidence.desktop_luminance_excluded_from_pixel_evidence
            || evidence.selected_process_id != snapshot.target_process_id
            || evidence.selected_window_handle != snapshot.target_window_handle
            || !evidence
                .selected_executable_name
                .eq_ignore_ascii_case(&snapshot.target_executable_basename)
        {
            return Err(VisualRuntimeError::CaptureAuthority);
        }
        let generation = snapshot.diagnostics.cancellation_generation;
        if generation == 0 || audio_binding.generation == 0 {
            return Err(VisualRuntimeError::AudioAuthority);
        }
        let launch = resolve_project_owned_review_provider(
            self.config.review_openseeface_root.as_deref(),
            evidence.selected_process_id,
        )?;
        let (client, worker) = self.ensure_ready(generation).await?;
        self.configure_project_owned_review_provider(&client, generation, &launch)
            .await?;

        // Provider startup is the expensive cold-path operation. Read the
        // current causal envelope after it is ready, then lease the exact frame
        // last so audio/frame skew is dominated only by the two broker calls.
        let audio = self
            .broker
            .query_visual_audio_envelope(VisualAudioEnvelopeQuery {
                session_id: audio_binding.session_id.clone(),
                turn_id: audio_binding.turn_id.clone(),
                generation: audio_binding.generation,
                stream_id: audio_binding.stream_id.clone(),
                segment_id: audio_binding.stream_id.clone(),
            })
            .await
            .map_err(|source| VisualRuntimeError::BrokerStage {
                stage: "query_visual_audio_envelope",
                source,
            })?;

        let track = VisualTrackBinding {
            actor_id: stable_nonzero_id("eclipse-harbor:mara-venn"),
            track_id: stable_nonzero_id("eclipse-harbor:mara-venn:project-owned-review-track"),
            track_epoch: generation,
        };
        let lease = self
            .broker
            .allocate_visual_source(&worker, &track)
            .await
            .map_err(|source| VisualRuntimeError::BrokerStage {
                stage: "allocate_visual_source",
                source,
            })?;
        let captured_ns = match qpc_to_ns(lease.source_frame_qpc, &lease) {
            Ok(value) => value,
            Err(error) => {
                self.abandon_unsubmitted_visual_source(&lease).await;
                return Err(error);
            }
        };
        let frame_bound_at_ns = match monotonic_ns() {
            Ok(value) => value,
            Err(error) => {
                self.abandon_unsubmitted_visual_source(&lease).await;
                return Err(error);
            }
        };
        if frame_bound_at_ns < captured_ns
            || frame_bound_at_ns - captured_ns > REVIEW_VISUAL_CAPTURE_MAX_AGE_NS
            || lease.cancellation_generation != generation
            || lease.width != evidence.content_width
            || lease.height != evidence.content_height
        {
            self.abandon_unsubmitted_visual_source(&lease).await;
            return Err(VisualRuntimeError::StaleLandmarks);
        }
        // WGC frame freshness and model runtime are separate budgets. Starting
        // the OpenSeeFace budget when the immutable broker lease is bound avoids
        // charging event-driven WGC delivery time against CPU inference while
        // the broker still caps the frame's total source-to-presentation age.
        let Some(deadline_ns) = frame_bound_at_ns.checked_add(REVIEW_VISUAL_INFERENCE_BUDGET_NS)
        else {
            self.abandon_unsubmitted_visual_source(&lease).await;
            return Err(VisualRuntimeError::Clock);
        };
        let drive_at_ns = match monotonic_ns() {
            Ok(value) => value,
            Err(error) => {
                self.abandon_unsubmitted_visual_source(&lease).await;
                return Err(error);
            }
        };
        let drive = match drive_from_visual_audio_envelope(&audio, drive_at_ns) {
            Ok(drive) => drive,
            Err(error) => {
                self.abandon_unsubmitted_visual_source(&lease).await;
                return Err(error);
            }
        };
        let (session_id_high, session_id_low) = correlation_words(&audio_binding.session_id);
        let (turn_id_high, turn_id_low) = correlation_words(&audio_binding.turn_id);
        let appearance_high = stable_nonzero_id("mara-venn-portrait-sha256-high");
        let appearance_low = stable_nonzero_id("mara-venn-portrait-sha256-low");
        let request = AdmittedVisualFrameRequest {
            request_id,
            session_id_high,
            session_id_low,
            turn_id_high,
            turn_id_low,
            sentence_id,
            selected_process_id: evidence.selected_process_id,
            selected_window_handle: evidence.selected_window_handle,
            selected_executable_name: evidence.selected_executable_name.clone(),
            track: track.clone(),
            cancellation_generation: generation,
            expected_frame_sequence: lease.source_frame_sequence,
            expected_frame_qpc: lease.source_frame_qpc,
            qpc_frequency: lease.qpc_frequency,
            expected_device_generation: lease.source_device_generation,
            expected_geometry_epoch: lease.source_geometry_epoch,
            expected_source_width: lease.width,
            expected_source_height: lease.height,
            // The seed only bounds detector search. The model produces all
            // face/mouth landmarks from the exact current frame.
            seed_face_x: 0.20,
            seed_face_y: 0.06,
            seed_face_width: 0.62,
            seed_face_height: 0.68,
            appearance: AppearanceGateEvidence {
                descriptor_revision: 1,
                expected_digest_high: appearance_high,
                expected_digest_low: appearance_low,
                observed_digest_high: appearance_high,
                observed_digest_low: appearance_low,
                similarity: 1.0,
                temporal_iou: 1.0,
                blocker_coverage: 0.0,
                identity_locked: true,
                target_visible: true,
                scene_transition: false,
            },
            pressure: VisualPressure::Nominal,
            admitted_signal_rate_hz: 15,
            audio: audio_binding.clone(),
            deadline_ns,
        };
        if let Err(error) = validate_admitted_frame_request(&request) {
            self.abandon_unsubmitted_visual_source(&lease).await;
            return Err(error);
        }
        let command =
            match encode_admitted_render_command(&request, &drive, &lease, client.session()) {
                Ok(value) => value,
                Err(error) => {
                    self.abandon_unsubmitted_visual_source(&lease).await;
                    return Err(error);
                }
            };
        let render = client
            .request(
                WorkerCommand::RenderWithAdmittedLandmarks,
                generation,
                command,
            )
            .await;
        let response = match render {
            Ok(value) => value,
            Err(error) => {
                self.abandon_unsubmitted_visual_source(&lease).await;
                return Err(error);
            }
        };
        // Retain the broker's immutable source-lease authority until the
        // residual has been validated and presented. The worker has already
        // consumed its duplicated texture handle, but the broker-side record
        // proves that a bounded-recent frame was genuinely leased to this
        // exact worker/actor/track rather than invented by a response.
        let presentation = self
            .present_worker_response(
                response, request_id, &track, &lease, &worker, &client, generation,
            )
            .await;
        let release = self
            .broker
            .release_visual_source(&lease)
            .await
            .map_err(|source| VisualRuntimeError::BrokerStage {
                stage: "release_visual_source",
                source,
            });
        let mut receipt = presentation?;
        release?;
        receipt.pixel_source = Some(evidence.pixel_source);
        receipt.pixel_scope = Some(evidence.pixel_scope);
        receipt.external_display_overlay_pixels_excluded =
            evidence.external_display_overlay_pixels_excluded;
        receipt.desktop_luminance_excluded_from_pixel_evidence =
            evidence.desktop_luminance_excluded_from_pixel_evidence;
        receipt.external_display_overlays_may_change_perceived_brightness =
            evidence.external_display_overlays_may_change_perceived_brightness;
        Ok(receipt)
    }

    async fn present_current_frame_inner(
        &self,
        request: &VisualFrameRequest,
    ) -> Result<VisualPresentationReceipt, VisualRuntimeError> {
        validate_frame_request(request)?;
        let generation = self.broker.diagnostics().await?.cancellation_generation;
        if generation == 0 {
            return Err(VisualRuntimeError::NoSelectedTarget);
        }
        let (client, worker) = self.ensure_ready(generation).await?;
        let lease = self
            .broker
            .allocate_visual_source(&worker, &request.track)
            .await?;
        if lease.cancellation_generation != generation
            || lease.source_frame_sequence != request.landmarks.expected_frame_sequence
        {
            self.abandon_unsubmitted_visual_source(&lease).await;
            return Err(VisualRuntimeError::StaleLandmarks);
        }
        let command = match encode_render_command(request, &lease, client.session()) {
            Ok(value) => value,
            Err(error) => {
                self.abandon_unsubmitted_visual_source(&lease).await;
                return Err(error);
            }
        };
        let render = client
            .request(WorkerCommand::RenderCurrentFrame, generation, command)
            .await;
        // The source handle is worker-owned after broker duplication. By the
        // time a worker response arrives it has consumed/closed the handle; the
        // broker release is metadata-only and never closes a remote value.
        let response = match render {
            Ok(value) => value,
            Err(error) => {
                const {
                    assert!(WORKER_FAILURE_POLICY.release_visual_source_metadata);
                }
                self.abandon_unsubmitted_visual_source(&lease).await;
                return Err(error);
            }
        };
        let presentation = self
            .present_worker_response(
                response,
                request.request_id,
                &request.track,
                &lease,
                &worker,
                &client,
                generation,
            )
            .await;
        let release = self.broker.release_visual_source(&lease).await;
        let receipt = presentation?;
        release?;
        Ok(receipt)
    }

    #[allow(clippy::too_many_arguments)]
    async fn present_worker_response(
        &self,
        response: WorkerResponse,
        request_id: u64,
        track: &VisualTrackBinding,
        lease: &VisualSourceLease,
        worker: &VisualWorkerIdentity,
        client: &WorkerClient,
        generation: u64,
    ) -> Result<VisualPresentationReceipt, VisualRuntimeError> {
        validate_worker_response(&response, request_id, track, lease, generation)?;
        let mut receipt = VisualPresentationReceipt {
            schema_version: 1,
            request_id,
            source_frame_sequence: lease.source_frame_sequence,
            cancellation_generation: generation,
            product_disposition: response.receipt.disposition,
            worker_disposition: response.receipt.worker_disposition,
            signal_disposition: response.receipt.signal_disposition,
            queue_replacements: response.receipt.queue_replacements,
            residual_proposed: response.residual.is_some(),
            presented: false,
            degraded: response.residual.is_none(),
            pixel_source: None,
            pixel_scope: None,
            external_display_overlay_pixels_excluded: false,
            desktop_luminance_excluded_from_pixel_evidence: false,
            external_display_overlays_may_change_perceived_brightness: false,
            detail: response.detail.clone(),
            broker: None,
        };
        let Some(residual) = response.residual else {
            return Ok(receipt);
        };

        let proposal = residual.to_broker_proposal(worker, lease)?;
        let occlusion = BrokerOcclusionEvidence {
            face_confidence: residual.detector_confidence,
            landmark_confidence: residual.landmark_confidence,
            visibility_ratio: residual.visibility_ratio,
            mouth_occluded: residual.mouth_occluded,
            measured_qpc: ns_to_qpc(residual.landmarks_measured_at_ns, lease)?,
        };
        let broker_receipt = match self.broker.submit_visual_occlusion(lease, &occlusion).await {
            Ok(()) => self
                .broker
                .submit_visual_patch(lease, &proposal)
                .await
                .map_err(|source| VisualRuntimeError::BrokerStage {
                    stage: "submit_visual_patch",
                    source,
                })?,
            Err(error) => {
                let _ = client
                    .acknowledge(
                        generation,
                        residual.lease_nonce_high,
                        residual.lease_nonce_low,
                        false,
                    )
                    .await;
                return Err(VisualRuntimeError::BrokerStage {
                    stage: "submit_visual_occlusion",
                    source: error,
                });
            }
        };
        let presented = broker_receipt.presented;
        client
            .acknowledge(
                generation,
                residual.lease_nonce_high,
                residual.lease_nonce_low,
                presented,
            )
            .await?;
        receipt.presented = presented;
        receipt.degraded = !presented;
        receipt.detail = if presented {
            "broker_presented_current_frame_residual".into()
        } else {
            "broker_bypassed_current_frame_residual".into()
        };
        receipt.broker = Some(broker_receipt);
        Ok(receipt)
    }

    async fn configure_admitted_provider(
        &self,
        client: &WorkerClient,
        generation: u64,
        launch: &NativeAdmittedVisualPackLaunchV1,
    ) -> Result<(), VisualRuntimeError> {
        let binding = ProviderBinding {
            installed_content_tree_sha256: launch.installed_content_tree_sha256.to_string(),
            measured_envelope_sha256: launch.measured_envelope_sha256.to_string(),
            exact_target_pid: launch.exact_target_pid,
        };
        if self
            .managed
            .lock()
            .await
            .as_ref()
            .and_then(|worker| worker.provider_binding.as_ref())
            == Some(&binding)
        {
            return Ok(());
        }
        client
            .configure_provider(generation, encode_provider_configuration(launch)?)
            .await?;
        let mut managed = self.managed.lock().await;
        let worker = managed.as_mut().ok_or(VisualRuntimeError::State)?;
        if worker.generation != generation {
            return Err(VisualRuntimeError::State);
        }
        worker.provider_binding = Some(binding);
        Ok(())
    }

    #[cfg(debug_assertions)]
    async fn configure_project_owned_review_provider(
        &self,
        client: &WorkerClient,
        generation: u64,
        launch: &ReviewOpenSeeFaceLaunch,
    ) -> Result<(), VisualRuntimeError> {
        let binding = ProviderBinding {
            installed_content_tree_sha256: launch.content_tree_sha256.clone(),
            measured_envelope_sha256: launch.measurement_sha256.clone(),
            exact_target_pid: launch.exact_target_pid,
        };
        if self
            .managed
            .lock()
            .await
            .as_ref()
            .and_then(|worker| worker.provider_binding.as_ref())
            == Some(&binding)
        {
            return Ok(());
        }
        client
            .configure_provider(generation, encode_review_provider_configuration(launch)?)
            .await?;
        let mut managed = self.managed.lock().await;
        let worker = managed.as_mut().ok_or(VisualRuntimeError::State)?;
        if worker.generation != generation {
            return Err(VisualRuntimeError::State);
        }
        worker.provider_binding = Some(binding);
        Ok(())
    }

    pub async fn shutdown(&self) {
        if let Some(mut managed) = self.managed.lock().await.take() {
            let _ = managed.client.shutdown(managed.generation).await;
            if tokio::time::timeout(SHUTDOWN_TIMEOUT, managed.child.wait())
                .await
                .is_err()
            {
                let _ = managed.child.kill().await;
                let _ = managed.child.wait().await;
            }
        }
    }

    async fn ensure_ready(
        &self,
        generation: u64,
    ) -> Result<(WorkerClient, VisualWorkerIdentity), VisualRuntimeError> {
        let _startup = self.startup_gate.lock().await;
        {
            let mut managed = self.managed.lock().await;
            if let Some(worker) = managed.as_mut() {
                if worker.child.try_wait()?.is_none() {
                    if worker.generation < generation {
                        worker
                            .client
                            .cancel_to(worker.generation, generation)
                            .await?;
                        worker.generation = generation;
                    } else if worker.generation > generation {
                        drop(managed);
                        self.discard_worker().await;
                        return self.launch(generation).await;
                    }
                    worker.client.health(generation).await?;
                    return Ok((worker.client.clone(), worker.identity.clone()));
                }
            }
        }
        self.discard_worker().await;
        self.launch(generation).await
    }

    async fn launch(
        &self,
        generation: u64,
    ) -> Result<(WorkerClient, VisualWorkerIdentity), VisualRuntimeError> {
        let executable = validate_fixed_worker(&self.config.executable)?;
        let mut nonce = [0_u8; 32];
        getrandom::fill(&mut nonce).map_err(|_| VisualRuntimeError::Random)?;
        let mut session_bytes = [0_u8; 16];
        getrandom::fill(&mut session_bytes).map_err(|_| VisualRuntimeError::Random)?;
        let mut high = u64::from_le_bytes(session_bytes[..8].try_into().unwrap_or([0; 8]));
        let low = u64::from_le_bytes(session_bytes[8..].try_into().unwrap_or([0; 8]));
        if high == 0 && low == 0 {
            high = 1;
        }
        let session = SessionBinding { nonce, high, low };
        let pipe = format!(r"\\.\pipe\npc-mouth-worker-{high:016x}-{low:016x}");
        let nonce_hex = nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let args = [
            "--pipe".to_owned(),
            pipe.clone(),
            "--nonce-hex".to_owned(),
            nonce_hex,
            "--session-high".to_owned(),
            high.to_string(),
            "--session-low".to_owned(),
            low.to_string(),
            "--controller-pid".to_owned(),
            std::process::id().to_string(),
            "--generation".to_owned(),
            generation.to_string(),
        ];
        let child = spawn_worker_process(&executable, &args, &self.parent_job)?;
        let identity = VisualWorkerIdentity {
            process_id: child.id(),
            process_creation_time: child.creation_time()?,
            executable_name: WORKER_FILE_NAME.into(),
        };
        let stream = connect_worker_pipe(&pipe).await?;
        let client = WorkerClient::new(stream, session);
        client.health(generation).await?;
        let managed = ManagedWorker {
            child,
            client: client.clone(),
            identity: identity.clone(),
            generation,
            provider_binding: None,
        };
        *self.managed.lock().await = Some(managed);
        Ok((client, identity))
    }

    async fn discard_worker(&self) {
        if let Some(mut worker) = self.managed.lock().await.take() {
            let _ = worker.child.kill().await;
            let _ = worker.child.wait().await;
        }
    }

    /// The broker duplicates source textures directly into the worker process.
    /// Before a request is submitted, metadata release alone cannot close that
    /// remote handle value. Terminating the isolated worker first gives Windows
    /// deterministic ownership cleanup; the broker record can then be released.
    async fn abandon_unsubmitted_visual_source(&self, lease: &VisualSourceLease) {
        self.discard_worker().await;
        let _ = self.broker.release_visual_source(lease).await;
    }
}

#[derive(Debug)]
struct ManagedWorker {
    child: WorkerChild,
    client: WorkerClient,
    identity: VisualWorkerIdentity,
    generation: u64,
    provider_binding: Option<ProviderBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProviderBinding {
    installed_content_tree_sha256: String,
    measured_envelope_sha256: String,
    exact_target_pid: u32,
}

#[cfg(debug_assertions)]
#[derive(Clone, Debug)]
struct ReviewVerifiedFile {
    path: PathBuf,
    size_bytes: u64,
    sha256: String,
}

#[cfg(debug_assertions)]
#[derive(Clone, Debug)]
struct ReviewOpenSeeFaceLaunch {
    artifact_root: PathBuf,
    detector: ReviewVerifiedFile,
    landmark: ReviewVerifiedFile,
    runtime: ReviewVerifiedFile,
    runtime_shared: ReviewVerifiedFile,
    content_tree_sha256: String,
    measurement_sha256: String,
    exact_target_pid: u32,
}

#[derive(Clone)]
struct WorkerClient(Arc<Mutex<WorkerConnection>>);

impl std::fmt::Debug for WorkerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("WorkerClient { authenticated: true, credentials: [REDACTED] }")
    }
}

struct WorkerConnection {
    stream: std::fs::File,
    session: SessionBinding,
    next_sequence: u64,
}

impl WorkerClient {
    fn new(stream: std::fs::File, session: SessionBinding) -> Self {
        Self(Arc::new(Mutex::new(WorkerConnection {
            stream,
            session,
            next_sequence: 1,
        })))
    }

    fn session(&self) -> SessionBinding {
        self.0
            .lock()
            .map(|value| value.session)
            .unwrap_or(SessionBinding {
                nonce: [0; 32],
                high: 0,
                low: 0,
            })
    }

    async fn health(&self, generation: u64) -> Result<(), VisualRuntimeError> {
        let response = self
            .request(WorkerCommand::Health, generation, Vec::new())
            .await?;
        (response.status == 0 && response.detail == "ready")
            .then_some(())
            .ok_or(VisualRuntimeError::Malformed)
    }

    async fn cancel_to(&self, current: u64, next: u64) -> Result<(), VisualRuntimeError> {
        let mut payload = WireWriter::default();
        payload.u64(next);
        let response = self
            .request(WorkerCommand::CancelGeneration, current, payload.take())
            .await?;
        (response.status == 0 && response.generation == next)
            .then_some(())
            .ok_or(VisualRuntimeError::Malformed)
    }

    async fn acknowledge(
        &self,
        generation: u64,
        high: u64,
        low: u64,
        presented: bool,
    ) -> Result<(), VisualRuntimeError> {
        let mut payload = WireWriter::default();
        payload.u64(high);
        payload.u64(low);
        payload.boolean(presented);
        let response = self
            .request(
                WorkerCommand::AcknowledgeResidual,
                generation,
                payload.take(),
            )
            .await?;
        (response.status == 0)
            .then_some(())
            .ok_or(VisualRuntimeError::Worker(response.detail))
    }

    async fn configure_provider(
        &self,
        generation: u64,
        payload: Vec<u8>,
    ) -> Result<(), VisualRuntimeError> {
        let response = self
            .request(
                WorkerCommand::ConfigureAdmittedLandmarkProvider,
                generation,
                payload,
            )
            .await?;
        (response.status == 0 && response.detail == "admitted_landmark_provider_ready")
            .then_some(())
            .ok_or(VisualRuntimeError::Worker(response.detail))
    }

    async fn shutdown(&self, generation: u64) -> Result<(), VisualRuntimeError> {
        self.request(WorkerCommand::Shutdown, generation, Vec::new())
            .await
            .map(|_| ())
    }

    async fn request(
        &self,
        command: WorkerCommand,
        generation: u64,
        payload: Vec<u8>,
    ) -> Result<WorkerResponse, VisualRuntimeError> {
        let connection = Arc::clone(&self.0);
        tokio::time::timeout(
            REQUEST_TIMEOUT,
            tauri::async_runtime::spawn_blocking(move || {
                worker_request_blocking(&connection, command, generation, payload)
            }),
        )
        .await
        .map_err(|_| VisualRuntimeError::Timeout)?
        .map_err(|_| VisualRuntimeError::Connection)?
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
enum WorkerCommand {
    Health = 1,
    RenderCurrentFrame = 2,
    CancelGeneration = 3,
    AcknowledgeResidual = 4,
    Shutdown = 5,
    RenderWithAdmittedLandmarks = 6,
    ConfigureAdmittedLandmarkProvider = 7,
}

fn worker_request_blocking(
    connection: &Mutex<WorkerConnection>,
    command: WorkerCommand,
    generation: u64,
    payload: Vec<u8>,
) -> Result<WorkerResponse, VisualRuntimeError> {
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(VisualRuntimeError::Payload);
    }
    let response_generation = if command == WorkerCommand::CancelGeneration {
        if payload.len() != std::mem::size_of::<u64>() {
            return Err(VisualRuntimeError::Malformed);
        }
        u64::from_le_bytes(
            payload
                .as_slice()
                .try_into()
                .map_err(|_| VisualRuntimeError::Malformed)?,
        )
    } else {
        generation
    };
    let mut connection = connection.lock().map_err(|_| VisualRuntimeError::State)?;
    let sequence = connection.next_sequence;
    connection.next_sequence = sequence.saturating_add(1);
    let mut body = WireWriter::default();
    body.u32(PROTOCOL_MAGIC);
    body.u16(PROTOCOL_VERSION);
    body.u16(command as u16);
    body.raw(&connection.session.nonce);
    body.u64(connection.session.high);
    body.u64(connection.session.low);
    body.u64(sequence);
    body.u64(generation);
    body.i64(monotonic_ns()?.saturating_add(400_000_000));
    body.bytes(&payload)?;
    write_wire_frame(&mut connection.stream, &body.take())?;
    let response = decode_worker_response(&read_wire_frame(&mut connection.stream)?)?;
    if response.response_to != sequence || response.generation != response_generation {
        return Err(VisualRuntimeError::Malformed);
    }
    if response.status != 0
        && command != WorkerCommand::RenderCurrentFrame
        && command != WorkerCommand::RenderWithAdmittedLandmarks
    {
        return Err(VisualRuntimeError::Worker(response.detail));
    }
    Ok(response)
}

#[derive(Clone, Debug)]
struct WorkerReceipt {
    request_id: u64,
    source_frame_sequence: u64,
    disposition: u8,
    signal_disposition: u8,
    worker_disposition: u8,
    queue_replacements: u64,
}

#[derive(Clone, Debug)]
struct WorkerResidual {
    schema_version: u32,
    request_id: u64,
    generation: u64,
    actor_id: u64,
    track_id: u64,
    track_epoch: u64,
    source_frame_sequence: u64,
    source_device_generation: u64,
    source_geometry_epoch: u64,
    source_frame_qpc: u64,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    lease_nonce_high: u64,
    lease_nonce_low: u64,
    owner_process_id: u32,
    intended_consumer_process_id: u32,
    worker_handle_value: u64,
    adapter_luid: u64,
    acquire_key: u64,
    release_key: u64,
    texture_width: u32,
    texture_height: u32,
    stride_bytes: u32,
    format: u32,
    expires_at_ns: i64,
    confidence: f64,
    detector_confidence: f64,
    landmark_confidence: f64,
    visibility_ratio: f64,
    mouth_occluded: bool,
    landmarks_measured_at_ns: i64,
    produced_at_ns: i64,
}

impl WorkerResidual {
    fn to_broker_proposal(
        &self,
        worker: &VisualWorkerIdentity,
        source: &VisualSourceLease,
    ) -> Result<BrokerResidualProposal, VisualRuntimeError> {
        if self.owner_process_id != worker.process_id
            || self.intended_consumer_process_id != source.broker_process_id
            || self.adapter_luid != source.adapter_luid
            || self.request_id == 0
        {
            return Err(VisualRuntimeError::Malformed);
        }
        Ok(BrokerResidualProposal {
            worker: worker.clone(),
            lease_nonce_high: self.lease_nonce_high,
            lease_nonce_low: self.lease_nonce_low,
            worker_handle_value: self.worker_handle_value,
            adapter_luid: self.adapter_luid,
            keyed_mutex_acquire_key: self.acquire_key,
            keyed_mutex_release_key: self.release_key,
            width: self.texture_width,
            height: self.texture_height,
            stride_bytes: self.stride_bytes,
            dxgi_format: self.format,
            alpha_mode: 1,
            expires_qpc: ns_to_qpc(self.expires_at_ns, source)?,
            left: self.x,
            top: self.y,
            right: self.x + self.width,
            bottom: self.y + self.height,
            confidence: self.confidence,
            produced_qpc: ns_to_qpc(self.produced_at_ns, source)?,
        })
    }
}

#[derive(Clone, Debug)]
struct WorkerResponse {
    status: u16,
    response_to: u64,
    generation: u64,
    receipt: WorkerReceipt,
    residual: Option<WorkerResidual>,
    detail: String,
}

// Remaining codec/process helpers are kept below so the public product API
// cannot expose raw handles or authentication material.

fn validate_frame_request(request: &VisualFrameRequest) -> Result<(), VisualRuntimeError> {
    let probability = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
    let unit_rect = |x: f64, y: f64, width: f64, height: f64| {
        [x, y, width, height].into_iter().all(f64::is_finite)
            && x >= 0.0
            && y >= 0.0
            && width > 0.0
            && height > 0.0
            && x + width <= 1.0
            && y + height <= 1.0
    };
    if request.request_id == 0
        || (request.session_id_high == 0 && request.session_id_low == 0)
        || (request.turn_id_high == 0 && request.turn_id_low == 0)
        || request.sentence_id == 0
        || request.track.actor_id == 0
        || request.track.track_id == 0
        || request.track.track_epoch == 0
        || request.landmarks.provider_instance_id == 0
        || request.landmarks.expected_frame_sequence == 0
        || !unit_rect(
            request.landmarks.face_x,
            request.landmarks.face_y,
            request.landmarks.face_width,
            request.landmarks.face_height,
        )
        || !probability(request.landmarks.detector_confidence)
        || !probability(request.landmarks.landmark_confidence)
        || !probability(request.landmarks.visibility_ratio)
        || request.landmarks.measured_at_ns <= 0
        || request.appearance.descriptor_revision == 0
        || !probability(request.appearance.similarity)
        || !probability(request.appearance.temporal_iou)
        || !probability(request.appearance.blocker_coverage)
        || request.admitted_signal_rate_hz > 15
        || request.deadline_ns <= 0
        || request.landmarks.landmarks.iter().any(|point| {
            !probability(point.x) || !probability(point.y) || !probability(point.confidence)
        })
    {
        return Err(VisualRuntimeError::InvalidRequest);
    }
    match &request.drive {
        MouthDrive::CausalEnvelopeCoefficients {
            stream_generation,
            segment_id,
            sample_rate,
            channels,
            playback_at_ns,
            coefficients,
            ..
        } => {
            if *stream_generation == 0
                || *segment_id == 0
                || !(8_000..=192_000).contains(sample_rate)
                || !(1..=2).contains(channels)
                || *playback_at_ns <= 0
                || coefficients.iter().any(|value| !probability(*value))
            {
                return Err(VisualRuntimeError::InvalidRequest);
            }
        }
        MouthDrive::TimedViseme {
            stream_generation,
            segment_id,
            sample_rate,
            channels,
            playback_at_ns,
            viseme,
            strength,
            ..
        } => {
            if *stream_generation == 0
                || *segment_id == 0
                || !(8_000..=192_000).contains(sample_rate)
                || !(1..=2).contains(channels)
                || *playback_at_ns <= 0
                || *viseme > 10
                || !probability(*strength)
            {
                return Err(VisualRuntimeError::InvalidRequest);
            }
        }
        MouthDrive::PcmWindow {
            stream_generation,
            segment_id,
            sample_rate,
            channels,
            playback_at_ns,
            interleaved_pcm,
            ..
        } => {
            if *stream_generation == 0
                || *segment_id == 0
                || !(8_000..=192_000).contains(sample_rate)
                || !(1..=2).contains(channels)
                || *playback_at_ns <= 0
                || interleaved_pcm.is_empty()
                || interleaved_pcm.len() > 32 * 1024
                || interleaved_pcm.iter().any(|sample| !sample.is_finite())
            {
                return Err(VisualRuntimeError::InvalidRequest);
            }
        }
    }
    Ok(())
}

fn validate_admitted_frame_request(
    request: &AdmittedVisualFrameRequest,
) -> Result<(), VisualRuntimeError> {
    let probability = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
    let unit_rect = |x: f64, y: f64, width: f64, height: f64| {
        [x, y, width, height].into_iter().all(f64::is_finite)
            && x >= 0.0
            && y >= 0.0
            && width > 0.0
            && height > 0.0
            && x + width <= 1.0
            && y + height <= 1.0
    };
    if request.request_id == 0
        || (request.session_id_high == 0 && request.session_id_low == 0)
        || (request.turn_id_high == 0 && request.turn_id_low == 0)
        || request.sentence_id == 0
        || request.selected_process_id == 0
        || request.selected_window_handle == 0
        || request.selected_executable_name.is_empty()
        || request.selected_executable_name.len() > 260
        || request.track.actor_id == 0
        || request.track.track_id == 0
        || request.track.track_epoch == 0
        || request.cancellation_generation == 0
        || request.expected_frame_sequence == 0
        || request.expected_frame_qpc == 0
        || request.qpc_frequency == 0
        || request.expected_device_generation == 0
        || request.expected_geometry_epoch == 0
        || request.expected_source_width == 0
        || request.expected_source_height == 0
        || !unit_rect(
            request.seed_face_x,
            request.seed_face_y,
            request.seed_face_width,
            request.seed_face_height,
        )
        || request.appearance.descriptor_revision == 0
        || !probability(request.appearance.similarity)
        || !probability(request.appearance.temporal_iou)
        || !probability(request.appearance.blocker_coverage)
        || request.admitted_signal_rate_hz == 0
        || request.admitted_signal_rate_hz > 15
        || request.deadline_ns <= 0
        || request.audio.session_id.is_empty()
        || request.audio.session_id.len() > 128
        || request.audio.turn_id.is_empty()
        || request.audio.turn_id.len() > 128
        || request.audio.generation == 0
        || request.audio.stream_id.is_empty()
        || request.audio.stream_id.len() > 128
    {
        return Err(VisualRuntimeError::InvalidRequest);
    }
    Ok(())
}

fn validate_admitted_launch(
    launch: &NativeAdmittedVisualPackLaunchV1,
) -> Result<(), VisualRuntimeError> {
    let exact = launch.identity.pack_id.as_str() == OPENSEEFACE_PACK_ID
        && launch.identity.revision.as_str() == OPENSEEFACE_PACK_REVISION
        && launch.runtime == "onnxruntime"
        && launch.runtime_revision == OPENSEEFACE_RUNTIME_REVISION
        && launch.backend == OPENSEEFACE_BACKEND
        && launch.exact_target_pid != 0
        && launch.artifact_root.is_absolute();
    exact.then_some(()).ok_or(VisualRuntimeError::Admission(
        "admitted OpenSeeFace launch authority is not the frozen product contract".into(),
    ))
}

fn encode_admitted_render_command(
    request: &AdmittedVisualFrameRequest,
    drive: &MouthDrive,
    source: &VisualSourceLease,
    session: SessionBinding,
) -> Result<Vec<u8>, VisualRuntimeError> {
    let captured_at_ns = qpc_to_ns(source.source_frame_qpc, source)?;
    let expires_at_ns = qpc_to_ns(source.expires_qpc, source)?;
    let mut wire = WireWriter::default();
    wire.u64(request.request_id);
    wire.u64(request.session_id_high);
    wire.u64(request.session_id_low);
    wire.u64(request.turn_id_high);
    wire.u64(request.turn_id_low);
    wire.u64(request.sentence_id);
    wire.u32(1);
    write_session(&mut wire, session);
    write_texture(
        &mut wire,
        TextureWire {
            transport: 1,
            nonce_high: source.lease_nonce_high,
            nonce_low: source.lease_nonce_low,
            owner_process_id: source.broker_process_id,
            intended_consumer_process_id: source.worker.process_id,
            handle: source.worker_handle_value,
            adapter_luid: source.adapter_luid,
            acquire_key: source.keyed_mutex_acquire_key,
            release_key: source.keyed_mutex_release_key,
            width: source.width,
            height: source.height,
            stride: source.stride_bytes,
            format: source.dxgi_format,
            expires_at_ns,
        },
    );
    wire.u32(source.broker_process_id);
    wire.u64(source.broker_process_creation_time);
    wire.string(&source.broker_executable_name)?;
    wire.u64(source.source_frame_qpc);
    wire.u64(source.qpc_frequency);
    write_track(&mut wire, source.cancellation_generation, &source.track);
    write_frame(&mut wire, source, captured_at_ns);
    wire.f64(request.seed_face_x);
    wire.f64(request.seed_face_y);
    wire.f64(request.seed_face_width);
    wire.f64(request.seed_face_height);
    write_appearance(&mut wire, source.track.actor_id, &request.appearance);
    wire.u32(1);
    wire.u8(request.pressure as u8);
    wire.u32(request.admitted_signal_rate_hz);
    wire.boolean(true);
    write_drive(&mut wire, drive);
    wire.i64(request.deadline_ns);
    let bytes = wire.take();
    (bytes.len() <= MAX_MESSAGE_BYTES)
        .then_some(bytes)
        .ok_or(VisualRuntimeError::Payload)
}

fn write_appearance(wire: &mut WireWriter, actor_id: u64, appearance: &AppearanceGateEvidence) {
    wire.u32(1);
    wire.u64(actor_id);
    wire.u64(appearance.descriptor_revision);
    wire.u64(appearance.expected_digest_high);
    wire.u64(appearance.expected_digest_low);
    wire.u64(appearance.observed_digest_high);
    wire.u64(appearance.observed_digest_low);
    wire.f64(appearance.similarity);
    wire.f64(appearance.temporal_iou);
    wire.f64(appearance.blocker_coverage);
    wire.boolean(appearance.identity_locked);
    wire.boolean(appearance.target_visible);
    wire.boolean(appearance.scene_transition);
}

fn encode_provider_configuration(
    launch: &NativeAdmittedVisualPackLaunchV1,
) -> Result<Vec<u8>, VisualRuntimeError> {
    validate_admitted_launch(launch)?;
    let path = |value: &Path| {
        value
            .to_str()
            .filter(|text| text.len() <= 2048)
            .map(str::to_owned)
            .ok_or(VisualRuntimeError::Admission(
                "admitted pack path is not bounded UTF-8".into(),
            ))
    };
    let mut wire = WireWriter::default();
    wire.u32(1);
    wire.string(launch.identity.pack_id.as_str())?;
    wire.string(launch.identity.revision.as_str())?;
    wire.string(&path(&launch.artifact_root)?)?;
    wire.string(&path(&launch.detector_model.path)?)?;
    wire.string(&path(&launch.landmark_model.path)?)?;
    wire.string(&path(&launch.runtime_library.path)?)?;
    wire.string(&path(&launch.runtime_shared_library.path)?)?;
    wire.u64(launch.detector_model.size_bytes);
    wire.u64(launch.landmark_model.size_bytes);
    wire.u64(launch.runtime_library.size_bytes);
    wire.u64(launch.runtime_shared_library.size_bytes);
    wire.string(launch.detector_model.sha256.as_str())?;
    wire.string(launch.landmark_model.sha256.as_str())?;
    wire.string(launch.runtime_library.sha256.as_str())?;
    wire.string(launch.runtime_shared_library.sha256.as_str())?;
    wire.string(launch.measured_envelope_sha256.as_str())?;
    wire.string(&launch.runtime_revision)?;
    wire.string(&launch.backend)?;
    wire.u32(15);
    wire.u32(1);
    wire.u32(launch.exact_target_pid);
    let bytes = wire.take();
    (bytes.len() <= MAX_MESSAGE_BYTES)
        .then_some(bytes)
        .ok_or(VisualRuntimeError::Payload)
}

#[cfg(debug_assertions)]
fn encode_review_provider_configuration(
    launch: &ReviewOpenSeeFaceLaunch,
) -> Result<Vec<u8>, VisualRuntimeError> {
    let path = |value: &Path| {
        value
            .to_str()
            .filter(|text| text.len() <= 2048)
            .map(str::to_owned)
            .ok_or_else(|| {
                VisualRuntimeError::Admission("review pack path is not bounded UTF-8".into())
            })
    };
    let mut wire = WireWriter::default();
    wire.u32(1);
    wire.string(OPENSEEFACE_PACK_ID)?;
    wire.string(OPENSEEFACE_PACK_REVISION)?;
    wire.string(&path(&launch.artifact_root)?)?;
    wire.string(&path(&launch.detector.path)?)?;
    wire.string(&path(&launch.landmark.path)?)?;
    wire.string(&path(&launch.runtime.path)?)?;
    wire.string(&path(&launch.runtime_shared.path)?)?;
    wire.u64(launch.detector.size_bytes);
    wire.u64(launch.landmark.size_bytes);
    wire.u64(launch.runtime.size_bytes);
    wire.u64(launch.runtime_shared.size_bytes);
    wire.string(&launch.detector.sha256)?;
    wire.string(&launch.landmark.sha256)?;
    wire.string(&launch.runtime.sha256)?;
    wire.string(&launch.runtime_shared.sha256)?;
    wire.string(&launch.measurement_sha256)?;
    wire.string(OPENSEEFACE_RUNTIME_REVISION)?;
    wire.string(OPENSEEFACE_BACKEND)?;
    wire.u32(15);
    wire.u32(1);
    wire.u32(launch.exact_target_pid);
    let bytes = wire.take();
    (bytes.len() <= MAX_MESSAGE_BYTES)
        .then_some(bytes)
        .ok_or(VisualRuntimeError::Payload)
}

fn encode_render_command(
    request: &VisualFrameRequest,
    source: &VisualSourceLease,
    session: SessionBinding,
) -> Result<Vec<u8>, VisualRuntimeError> {
    let captured_at_ns = qpc_to_ns(source.source_frame_qpc, source)?;
    let expires_at_ns = qpc_to_ns(source.expires_qpc, source)?;
    let mut wire = WireWriter::default();
    write_request_identity(&mut wire, request);
    wire.u32(1); // source schema
    write_session(&mut wire, session);
    write_texture(
        &mut wire,
        TextureWire {
            transport: 1,
            nonce_high: source.lease_nonce_high,
            nonce_low: source.lease_nonce_low,
            owner_process_id: source.broker_process_id,
            intended_consumer_process_id: source.worker.process_id,
            handle: source.worker_handle_value,
            adapter_luid: source.adapter_luid,
            acquire_key: source.keyed_mutex_acquire_key,
            release_key: source.keyed_mutex_release_key,
            width: source.width,
            height: source.height,
            stride: source.stride_bytes,
            format: source.dxgi_format,
            expires_at_ns,
        },
    );
    wire.u32(source.broker_process_id);
    wire.u64(source.broker_process_creation_time);
    wire.string(&source.broker_executable_name)?;
    wire.u64(source.source_frame_qpc);
    wire.u64(source.qpc_frequency);
    write_track(&mut wire, source.cancellation_generation, &source.track);
    write_frame(&mut wire, source, captured_at_ns);

    wire.u32(1); // OpenSeeFace packet schema
    wire.u64(request.landmarks.provider_instance_id);
    write_track(&mut wire, source.cancellation_generation, &source.track);
    write_frame(&mut wire, source, captured_at_ns);
    wire.u64(source.source_frame_qpc);
    wire.u64(source.qpc_frequency);
    wire.f64(request.landmarks.face_x);
    wire.f64(request.landmarks.face_y);
    wire.f64(request.landmarks.face_width);
    wire.f64(request.landmarks.face_height);
    for point in &request.landmarks.landmarks {
        wire.f64(point.x);
        wire.f64(point.y);
        wire.f64(point.confidence);
    }
    wire.f64(request.landmarks.yaw_degrees);
    wire.f64(request.landmarks.pitch_degrees);
    wire.f64(request.landmarks.roll_degrees);
    wire.f64(request.landmarks.detector_confidence);
    wire.f64(request.landmarks.landmark_confidence);
    wire.f64(request.landmarks.visibility_ratio);
    wire.boolean(request.landmarks.mouth_occluded);
    wire.i64(request.landmarks.measured_at_ns);

    wire.u32(1); // appearance schema
    wire.u64(source.track.actor_id);
    wire.u64(request.appearance.descriptor_revision);
    wire.u64(request.appearance.expected_digest_high);
    wire.u64(request.appearance.expected_digest_low);
    wire.u64(request.appearance.observed_digest_high);
    wire.u64(request.appearance.observed_digest_low);
    wire.f64(request.appearance.similarity);
    wire.f64(request.appearance.temporal_iou);
    wire.f64(request.appearance.blocker_coverage);
    wire.boolean(request.appearance.identity_locked);
    wire.boolean(request.appearance.target_visible);
    wire.boolean(request.appearance.scene_transition);

    wire.u32(1); // resource state schema
    wire.u8(request.pressure as u8);
    wire.u32(request.admitted_signal_rate_hz);
    wire.boolean(request.local_visuals_admitted);
    write_drive(&mut wire, &request.drive);
    wire.i64(request.deadline_ns);
    let bytes = wire.take();
    (bytes.len() <= MAX_MESSAGE_BYTES)
        .then_some(bytes)
        .ok_or(VisualRuntimeError::Payload)
}

fn write_request_identity(writer: &mut WireWriter, request: &VisualFrameRequest) {
    writer.u64(request.request_id);
    writer.u64(request.session_id_high);
    writer.u64(request.session_id_low);
    writer.u64(request.turn_id_high);
    writer.u64(request.turn_id_low);
    writer.u64(request.sentence_id);
}

fn write_session(writer: &mut WireWriter, session: SessionBinding) {
    writer.raw(&session.nonce);
    writer.u64(session.high);
    writer.u64(session.low);
}

fn write_track(writer: &mut WireWriter, generation: u64, track: &VisualTrackBinding) {
    writer.u64(generation);
    writer.u64(track.actor_id);
    writer.u64(track.track_id);
    writer.u64(track.track_epoch);
}

fn write_frame(writer: &mut WireWriter, source: &VisualSourceLease, captured_at_ns: i64) {
    writer.u64(source.source_frame_sequence);
    writer.u64(source.source_device_generation);
    writer.u64(source.source_geometry_epoch);
    writer.i64(captured_at_ns);
}

struct TextureWire {
    transport: u32,
    nonce_high: u64,
    nonce_low: u64,
    owner_process_id: u32,
    intended_consumer_process_id: u32,
    handle: u64,
    adapter_luid: u64,
    acquire_key: u64,
    release_key: u64,
    width: u32,
    height: u32,
    stride: u32,
    format: u32,
    expires_at_ns: i64,
}

fn write_texture(writer: &mut WireWriter, texture: TextureWire) {
    writer.u32(1);
    writer.u32(texture.transport);
    writer.u64(texture.nonce_high);
    writer.u64(texture.nonce_low);
    writer.u32(texture.owner_process_id);
    writer.u32(texture.intended_consumer_process_id);
    writer.u64(texture.handle);
    writer.u32(texture.adapter_luid as u32);
    writer.i32((texture.adapter_luid >> 32) as i32);
    writer.u64(texture.acquire_key);
    writer.u64(texture.release_key);
    writer.u32(texture.width);
    writer.u32(texture.height);
    writer.u32(texture.stride);
    writer.u32(texture.format);
    writer.i64(texture.expires_at_ns);
}

fn write_drive(writer: &mut WireWriter, drive: &MouthDrive) {
    let (
        kind,
        stream_generation,
        segment_id,
        first_sample_index,
        sample_rate,
        channels,
        playback_at_ns,
    ) = match drive {
        MouthDrive::CausalEnvelopeCoefficients {
            stream_generation,
            segment_id,
            first_sample_index,
            sample_rate,
            channels,
            playback_at_ns,
            ..
        } => (
            0_u8,
            *stream_generation,
            *segment_id,
            *first_sample_index,
            *sample_rate,
            *channels,
            *playback_at_ns,
        ),
        MouthDrive::TimedViseme {
            stream_generation,
            segment_id,
            first_sample_index,
            sample_rate,
            channels,
            playback_at_ns,
            ..
        } => (
            1_u8,
            *stream_generation,
            *segment_id,
            *first_sample_index,
            *sample_rate,
            *channels,
            *playback_at_ns,
        ),
        MouthDrive::PcmWindow {
            stream_generation,
            segment_id,
            first_sample_index,
            sample_rate,
            channels,
            playback_at_ns,
            ..
        } => (
            2_u8,
            *stream_generation,
            *segment_id,
            *first_sample_index,
            *sample_rate,
            *channels,
            *playback_at_ns,
        ),
    };
    writer.u8(kind);
    writer.u64(stream_generation);
    writer.u64(segment_id);
    writer.u64(first_sample_index);
    writer.u32(sample_rate);
    writer.u16(channels);
    writer.i64(playback_at_ns);
    match drive {
        MouthDrive::CausalEnvelopeCoefficients { coefficients, .. } => {
            for coefficient in coefficients {
                writer.f64(*coefficient);
            }
        }
        MouthDrive::TimedViseme { .. } | MouthDrive::PcmWindow { .. } => {
            for _ in 0..8 {
                writer.f64(0.0);
            }
        }
    }
    match drive {
        MouthDrive::CausalEnvelopeCoefficients { .. } => {
            writer.u8(0);
            writer.f64(1.0);
            writer.u32(0);
        }
        MouthDrive::TimedViseme {
            viseme, strength, ..
        } => {
            writer.u8(*viseme);
            writer.f64(*strength);
            writer.u32(0);
        }
        MouthDrive::PcmWindow {
            interleaved_pcm, ..
        } => {
            writer.u8(0);
            writer.f64(1.0);
            writer.u32(interleaved_pcm.len() as u32);
            for sample in interleaved_pcm {
                writer.u32(sample.to_bits());
            }
        }
    }
}

fn decode_worker_response(bytes: &[u8]) -> Result<WorkerResponse, VisualRuntimeError> {
    let mut reader = WireReader::new(bytes);
    if reader.u32()? != PROTOCOL_MAGIC || reader.u16()? != PROTOCOL_VERSION {
        return Err(VisualRuntimeError::Malformed);
    }
    let status = reader.u16()?;
    let response_to = reader.u64()?;
    let generation = reader.u64()?;
    let receipt_schema = reader.u32()?;
    let request_id = reader.u64()?;
    for _ in 0..5 {
        let _ = reader.u64()?;
    }
    for _ in 0..4 {
        let _ = reader.u64()?;
    }
    let source_frame_sequence = reader.u64()?;
    let _source_device_generation = reader.u64()?;
    let _source_geometry_epoch = reader.u64()?;
    let _captured_at_ns = reader.i64()?;
    let disposition = reader.u8()?;
    let signal_disposition = reader.u8()?;
    let worker_disposition = reader.u8()?;
    let _drive_kind = reader.u8()?;
    let _admitted_rate = reader.u32()?;
    let _latch_generation = reader.u64()?;
    let _submitted_at_ns = reader.i64()?;
    let _completed_at_ns = reader.i64()?;
    let queue_replacements = reader.u64()?;
    if receipt_schema != 1 {
        return Err(VisualRuntimeError::Malformed);
    }
    let residual = if reader.boolean()? {
        let schema = reader.u32()?;
        let residual_request_id = reader.u64()?;
        for _ in 0..5 {
            let _ = reader.u64()?;
        }
        let residual_generation = reader.u64()?;
        let actor_id = reader.u64()?;
        let track_id = reader.u64()?;
        let track_epoch = reader.u64()?;
        let frame_sequence = reader.u64()?;
        let frame_device_generation = reader.u64()?;
        let frame_geometry_epoch = reader.u64()?;
        let _frame_captured_at_ns = reader.i64()?;
        let source_frame_qpc = reader.u64()?;
        let x = reader.f64()?;
        let y = reader.f64()?;
        let width = reader.f64()?;
        let height = reader.f64()?;
        let texture_schema = reader.u32()?;
        let transport = reader.u32()?;
        let lease_nonce_high = reader.u64()?;
        let lease_nonce_low = reader.u64()?;
        let owner_process_id = reader.u32()?;
        let intended_consumer_process_id = reader.u32()?;
        let worker_handle_value = reader.u64()?;
        let luid_low = reader.u32()?;
        let luid_high = reader.i32()?;
        let acquire_key = reader.u64()?;
        let release_key = reader.u64()?;
        let texture_width = reader.u32()?;
        let texture_height = reader.u32()?;
        let stride_bytes = reader.u32()?;
        let format = reader.u32()?;
        let expires_at_ns = reader.i64()?;
        let confidence = reader.f64()?;
        let (
            detector_confidence,
            landmark_confidence,
            visibility_ratio,
            mouth_occluded,
            landmarks_measured_at_ns,
        ) = if schema >= 2 {
            (
                reader.f64()?,
                reader.f64()?,
                reader.f64()?,
                reader.boolean()?,
                reader.i64()?,
            )
        } else {
            (confidence, confidence, 1.0, false, _frame_captured_at_ns)
        };
        let produced_at_ns = reader.i64()?;
        if !(1..=2).contains(&schema) || texture_schema != 1 || transport != 1 {
            return Err(VisualRuntimeError::Malformed);
        }
        Some(WorkerResidual {
            schema_version: schema,
            request_id: residual_request_id,
            generation: residual_generation,
            actor_id,
            track_id,
            track_epoch,
            source_frame_sequence: frame_sequence,
            source_device_generation: frame_device_generation,
            source_geometry_epoch: frame_geometry_epoch,
            source_frame_qpc,
            x,
            y,
            width,
            height,
            lease_nonce_high,
            lease_nonce_low,
            owner_process_id,
            intended_consumer_process_id,
            worker_handle_value,
            adapter_luid: u64::from(luid_low) | (u64::from(luid_high as u32) << 32),
            acquire_key,
            release_key,
            texture_width,
            texture_height,
            stride_bytes,
            format,
            expires_at_ns,
            confidence,
            detector_confidence,
            landmark_confidence,
            visibility_ratio,
            mouth_occluded,
            landmarks_measured_at_ns,
            produced_at_ns,
        })
    } else {
        None
    };
    let detail = reader.string(1024)?;
    if !reader.done() {
        return Err(VisualRuntimeError::Malformed);
    }
    Ok(WorkerResponse {
        status,
        response_to,
        generation,
        receipt: WorkerReceipt {
            request_id,
            source_frame_sequence,
            disposition,
            signal_disposition,
            worker_disposition,
            queue_replacements,
        },
        residual,
        detail,
    })
}

fn validate_worker_response(
    response: &WorkerResponse,
    request_id: u64,
    track: &VisualTrackBinding,
    lease: &VisualSourceLease,
    generation: u64,
) -> Result<(), VisualRuntimeError> {
    if response.status != 0
        || response.generation != generation
        || response.receipt.request_id != request_id
        || response.receipt.source_frame_sequence != lease.source_frame_sequence
    {
        return Err(VisualRuntimeError::Worker(response.detail.clone()));
    }
    if let Some(residual) = &response.residual {
        let probability = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
        if residual.schema_version != 2
            || residual.request_id != request_id
            || residual.generation != generation
            || residual.actor_id != track.actor_id
            || residual.track_id != track.track_id
            || residual.track_epoch != track.track_epoch
            || residual.source_frame_sequence != lease.source_frame_sequence
            || residual.source_device_generation != lease.source_device_generation
            || residual.source_geometry_epoch != lease.source_geometry_epoch
            || residual.source_frame_qpc != lease.source_frame_qpc
            || !probability(residual.detector_confidence)
            || !probability(residual.landmark_confidence)
            || !probability(residual.visibility_ratio)
            || residual.landmarks_measured_at_ns <= 0
        {
            return Err(VisualRuntimeError::Malformed);
        }
    }
    Ok(())
}

#[derive(Default)]
struct WireWriter(Vec<u8>);

impl WireWriter {
    fn raw(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }
    fn boolean(&mut self, value: bool) {
        self.u8(u8::from(value));
    }
    fn u16(&mut self, value: u16) {
        self.raw(&value.to_le_bytes());
    }
    fn u32(&mut self, value: u32) {
        self.raw(&value.to_le_bytes());
    }
    fn i32(&mut self, value: i32) {
        self.raw(&value.to_le_bytes());
    }
    fn u64(&mut self, value: u64) {
        self.raw(&value.to_le_bytes());
    }
    fn i64(&mut self, value: i64) {
        self.raw(&value.to_le_bytes());
    }
    fn f64(&mut self, value: f64) {
        self.u64(value.to_bits());
    }
    fn bytes(&mut self, bytes: &[u8]) -> Result<(), VisualRuntimeError> {
        self.u32(u32::try_from(bytes.len()).map_err(|_| VisualRuntimeError::Payload)?);
        self.raw(bytes);
        Ok(())
    }
    fn string(&mut self, value: &str) -> Result<(), VisualRuntimeError> {
        self.bytes(value.as_bytes())
    }
    fn take(self) -> Vec<u8> {
        self.0
    }
}

struct WireReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> WireReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn read<const N: usize>(&mut self) -> Result<[u8; N], VisualRuntimeError> {
        let end = self
            .offset
            .checked_add(N)
            .ok_or(VisualRuntimeError::Malformed)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(VisualRuntimeError::Malformed)?
            .try_into()
            .map_err(|_| VisualRuntimeError::Malformed)?;
        self.offset = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, VisualRuntimeError> {
        Ok(self.read::<1>()?[0])
    }
    fn boolean(&mut self) -> Result<bool, VisualRuntimeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(VisualRuntimeError::Malformed),
        }
    }
    fn u16(&mut self) -> Result<u16, VisualRuntimeError> {
        Ok(u16::from_le_bytes(self.read()?))
    }
    fn u32(&mut self) -> Result<u32, VisualRuntimeError> {
        Ok(u32::from_le_bytes(self.read()?))
    }
    fn i32(&mut self) -> Result<i32, VisualRuntimeError> {
        Ok(i32::from_le_bytes(self.read()?))
    }
    fn u64(&mut self) -> Result<u64, VisualRuntimeError> {
        Ok(u64::from_le_bytes(self.read()?))
    }
    fn i64(&mut self) -> Result<i64, VisualRuntimeError> {
        Ok(i64::from_le_bytes(self.read()?))
    }
    fn f64(&mut self) -> Result<f64, VisualRuntimeError> {
        Ok(f64::from_bits(self.u64()?))
    }
    fn string(&mut self, maximum: usize) -> Result<String, VisualRuntimeError> {
        let length = self.u32()? as usize;
        if length > maximum {
            return Err(VisualRuntimeError::Malformed);
        }
        let end = self
            .offset
            .checked_add(length)
            .ok_or(VisualRuntimeError::Malformed)?;
        let value = std::str::from_utf8(
            self.bytes
                .get(self.offset..end)
                .ok_or(VisualRuntimeError::Malformed)?,
        )
        .map_err(|_| VisualRuntimeError::Malformed)?
        .to_owned();
        self.offset = end;
        Ok(value)
    }
    fn done(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

fn write_wire_frame(writer: &mut std::fs::File, body: &[u8]) -> Result<(), VisualRuntimeError> {
    if body.is_empty() || body.len() > MAX_MESSAGE_BYTES {
        return Err(VisualRuntimeError::Payload);
    }
    writer
        .write_all(&(body.len() as u32).to_le_bytes())
        .and_then(|()| writer.write_all(body))
        .and_then(|()| writer.flush())
        .map_err(|_| VisualRuntimeError::Connection)
}

fn read_wire_frame(reader: &mut std::fs::File) -> Result<Vec<u8>, VisualRuntimeError> {
    let mut prefix = [0_u8; 4];
    reader
        .read_exact(&mut prefix)
        .map_err(|_| VisualRuntimeError::Connection)?;
    let length = u32::from_le_bytes(prefix) as usize;
    if length == 0 || length > MAX_MESSAGE_BYTES {
        return Err(VisualRuntimeError::Payload);
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| VisualRuntimeError::Connection)?;
    Ok(body)
}

#[cfg(windows)]
fn monotonic_ns() -> Result<i64, VisualRuntimeError> {
    use windows_sys::Win32::System::Performance::{
        QueryPerformanceCounter, QueryPerformanceFrequency,
    };
    let mut counter = 0_i64;
    let mut frequency = 0_i64;
    // SAFETY: both pointers refer to initialized writable i64 values for the
    // duration of these synchronous Windows API calls.
    let clock_available = unsafe {
        QueryPerformanceCounter(&mut counter) != 0 && QueryPerformanceFrequency(&mut frequency) != 0
    };
    if !clock_available || counter <= 0 || frequency <= 0 {
        return Err(VisualRuntimeError::Clock);
    }
    let value = i128::from(counter)
        .saturating_mul(1_000_000_000)
        .checked_div(i128::from(frequency))
        .ok_or(VisualRuntimeError::Clock)?;
    i64::try_from(value).map_err(|_| VisualRuntimeError::Clock)
}

#[cfg(not(windows))]
fn monotonic_ns() -> Result<i64, VisualRuntimeError> {
    Err(VisualRuntimeError::Unsupported)
}

fn qpc_to_ns(value: u64, lease: &VisualSourceLease) -> Result<i64, VisualRuntimeError> {
    if value == 0 || lease.qpc_frequency == 0 {
        return Err(VisualRuntimeError::Clock);
    }
    let nanoseconds = i128::from(value)
        .saturating_mul(1_000_000_000)
        .checked_div(i128::from(lease.qpc_frequency))
        .ok_or(VisualRuntimeError::Clock)?;
    i64::try_from(nanoseconds).map_err(|_| VisualRuntimeError::Clock)
}

fn drive_from_visual_audio_envelope(
    envelope: &VisualAudioEnvelope,
    render_at_ns: i64,
) -> Result<MouthDrive, VisualRuntimeError> {
    if envelope.schema_version != 1
        || envelope.session_id.is_empty()
        || envelope.turn_id.is_empty()
        || envelope.generation == 0
        || envelope.stream_id.is_empty()
        || envelope.segment_id != envelope.stream_id
        || envelope.source_sample_count == 0
        || !(8_000..=192_000).contains(&envelope.sample_rate)
        || !(1..=2).contains(&envelope.channels)
        || envelope.device_write_qpc == 0
        || envelope.qpc_frequency == 0
        || render_at_ns <= 0
        || !envelope.active
        || envelope.draining
        || envelope.cancelled
        || envelope
            .mono_rms_q15
            .iter()
            .chain(envelope.mono_peak_q15.iter())
            .any(|value| *value > 32_767)
    {
        return Err(VisualRuntimeError::AudioAuthority);
    }
    let device_write_ns = qpc_value_to_ns(envelope.device_write_qpc, envelope.qpc_frequency)?;
    let block_duration_ns = i128::from(envelope.source_sample_count)
        .saturating_mul(1_000_000_000)
        .checked_div(i128::from(envelope.sample_rate))
        .and_then(|value| i64::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or(VisualRuntimeError::Clock)?;
    // A visual query consumes the time bin that corresponds to its current
    // playback instant. The prior whole-block average flattened speech motion
    // and incorrectly carried the old ReleaseBuffer timestamp into each new
    // captured frame. A short tail covers normal shared-mode endpoint jitter;
    // anything older still fails open.
    const AUDIO_ENVELOPE_TAIL_NS: i64 = 250_000_000;
    if render_at_ns < device_write_ns
        || render_at_ns
            > device_write_ns
                .saturating_add(block_duration_ns)
                .saturating_add(AUDIO_ENVELOPE_TAIL_NS)
    {
        return Err(VisualRuntimeError::AudioAuthority);
    }
    let elapsed_ns = render_at_ns
        .saturating_sub(device_write_ns)
        .clamp(0, block_duration_ns.saturating_sub(1));
    let bin_count = envelope.mono_rms_q15.len();
    let bin_index = usize::try_from(
        i128::from(elapsed_ns)
            .saturating_mul(bin_count as i128)
            .checked_div(i128::from(block_duration_ns))
            .unwrap_or_default(),
    )
    .unwrap_or_default()
    .min(bin_count.saturating_sub(1));
    let rms = f64::from(envelope.mono_rms_q15[bin_index]) / 32_767.0;
    let peak = f64::from(envelope.mono_peak_q15[bin_index]) / 32_767.0;
    // This is intentionally an amplitude-only procedural drive. The broker's
    // eight bins are temporal energy evidence, not phoneme classes; mapping
    // them to invented visemes would overclaim the signal.
    let coefficients = [
        (rms * 1.10).clamp(0.0, 1.0),
        (1.0 - peak * 1.40).clamp(0.0, 1.0),
        0.0,
        0.0,
        0.0,
        0.0,
        (peak * 0.15).clamp(0.0, 1.0),
        (peak * 0.20).clamp(0.0, 1.0),
    ];
    let bin_sample_offset = u64::try_from(bin_index)
        .unwrap_or_default()
        .saturating_mul(u64::from(envelope.source_sample_count))
        / u64::try_from(bin_count).unwrap_or(1);
    Ok(MouthDrive::CausalEnvelopeCoefficients {
        stream_generation: envelope.generation,
        segment_id: stable_nonzero_id(&envelope.stream_id),
        first_sample_index: envelope
            .source_sample_start
            .saturating_add(bin_sample_offset),
        sample_rate: envelope.sample_rate,
        channels: envelope.channels,
        playback_at_ns: render_at_ns,
        coefficients,
    })
}

fn qpc_value_to_ns(value: u64, frequency: u64) -> Result<i64, VisualRuntimeError> {
    if value == 0 || frequency == 0 {
        return Err(VisualRuntimeError::Clock);
    }
    let nanoseconds = i128::from(value)
        .saturating_mul(1_000_000_000)
        .checked_div(i128::from(frequency))
        .ok_or(VisualRuntimeError::Clock)?;
    i64::try_from(nanoseconds).map_err(|_| VisualRuntimeError::Clock)
}

fn bounded_frame_progression(
    authority_sequence: u64,
    authority_qpc: u64,
    candidate_sequence: u64,
    candidate_qpc: u64,
    qpc_frequency: u64,
    observed_at_ns: i64,
) -> bool {
    if authority_sequence == 0
        || authority_qpc == 0
        || candidate_sequence < authority_sequence
        || candidate_qpc < authority_qpc
    {
        return false;
    }
    let Ok(authority_at_ns) = qpc_value_to_ns(authority_qpc, qpc_frequency) else {
        return false;
    };
    let Ok(candidate_at_ns) = qpc_value_to_ns(candidate_qpc, qpc_frequency) else {
        return false;
    };
    let Some(deadline_ns) = authority_at_ns.checked_add(VISUAL_FRAME_DEADLINE_NS) else {
        return false;
    };
    let Some(maximum_clock_lead_ns) = observed_at_ns.checked_add(2_000_000) else {
        return false;
    };
    observed_at_ns >= authority_at_ns
        && observed_at_ns <= deadline_ns
        && candidate_at_ns <= maximum_clock_lead_ns
}

fn stable_nonzero_id(value: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    if hash == 0 {
        1
    } else {
        hash
    }
}

fn ns_to_qpc(value: i64, lease: &VisualSourceLease) -> Result<u64, VisualRuntimeError> {
    if value <= 0 || lease.qpc_frequency == 0 {
        return Err(VisualRuntimeError::Clock);
    }
    let counter = i128::from(value)
        .saturating_mul(i128::from(lease.qpc_frequency))
        .checked_div(1_000_000_000)
        .ok_or(VisualRuntimeError::Clock)?;
    u64::try_from(counter).map_err(|_| VisualRuntimeError::Clock)
}

#[cfg(debug_assertions)]
fn resolve_project_owned_review_provider(
    configured_root: Option<&Path>,
    exact_target_pid: u32,
) -> Result<ReviewOpenSeeFaceLaunch, VisualRuntimeError> {
    if exact_target_pid == 0 {
        return Err(VisualRuntimeError::Admission(
            "review target process is unavailable".into(),
        ));
    }
    let root = configured_root.ok_or_else(|| {
        VisualRuntimeError::Admission("review OpenSeeFace root is not configured".into())
    })?;
    let root_metadata = std::fs::symlink_metadata(root).map_err(|_| {
        VisualRuntimeError::Admission("review OpenSeeFace pack is not installed".into())
    })?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(VisualRuntimeError::Admission(
            "review OpenSeeFace root is not a normal directory".into(),
        ));
    }
    let root = root.canonicalize().map_err(|_| {
        VisualRuntimeError::Admission("review OpenSeeFace root cannot be resolved".into())
    })?;
    let detector = verify_review_provider_file(
        &root,
        Path::new("models/mnv3_detection_opt.onnx"),
        568_302,
        REVIEW_DETECTOR_SHA256,
    )?;
    let landmark = verify_review_provider_file(
        &root,
        Path::new("models/lm_model1_opt.onnx"),
        4_842_329,
        REVIEW_LANDMARK_SHA256,
    )?;
    let runtime = verify_review_provider_file(
        &root,
        Path::new("runtime/onnxruntime-1.22.1-cpu/lib/onnxruntime.dll"),
        14_854_728,
        REVIEW_ORT_SHA256,
    )?;
    let runtime_shared = verify_review_provider_file(
        &root,
        Path::new("runtime/onnxruntime-1.22.1-cpu/lib/onnxruntime_providers_shared.dll"),
        22_072,
        REVIEW_ORT_SHARED_SHA256,
    )?;
    let content_tree_sha256 = sha256_hex(
        [
            detector.sha256.as_str(),
            landmark.sha256.as_str(),
            runtime.sha256.as_str(),
            runtime_shared.sha256.as_str(),
        ]
        .join("\n")
        .as_bytes(),
    );
    let measurement_sha256 = sha256_hex(
        format!(
            "npc.project-owned-review-openseeface/v1\n{content_tree_sha256}\n{exact_target_pid}"
        )
        .as_bytes(),
    );
    Ok(ReviewOpenSeeFaceLaunch {
        artifact_root: root,
        detector,
        landmark,
        runtime,
        runtime_shared,
        content_tree_sha256,
        measurement_sha256,
        exact_target_pid,
    })
}

#[cfg(debug_assertions)]
fn verify_review_provider_file(
    root: &Path,
    relative: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<ReviewVerifiedFile, VisualRuntimeError> {
    let path = root.join(relative);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| {
        VisualRuntimeError::Admission(format!(
            "review OpenSeeFace file is missing: {}",
            relative.display()
        ))
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != expected_size {
        return Err(VisualRuntimeError::Admission(format!(
            "review OpenSeeFace file authority is invalid: {}",
            relative.display()
        )));
    }
    let canonical = path.canonicalize().map_err(|_| {
        VisualRuntimeError::Admission(format!(
            "review OpenSeeFace file cannot be resolved: {}",
            relative.display()
        ))
    })?;
    if !canonical.starts_with(root) {
        return Err(VisualRuntimeError::Admission(
            "review OpenSeeFace path escaped its exact version root".into(),
        ));
    }
    let mut file = std::fs::File::open(&canonical).map_err(|_| {
        VisualRuntimeError::Admission(format!(
            "review OpenSeeFace file cannot be opened: {}",
            relative.display()
        ))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| {
            VisualRuntimeError::Admission(format!(
                "review OpenSeeFace file cannot be hashed: {}",
                relative.display()
            ))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual != expected_sha256 {
        return Err(VisualRuntimeError::Admission(format!(
            "review OpenSeeFace file hash mismatch: {}",
            relative.display()
        )));
    }
    Ok(ReviewVerifiedFile {
        path: canonical,
        size_bytes: expected_size,
        sha256: actual,
    })
}

#[cfg(debug_assertions)]
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validate_fixed_worker(path: &Path) -> Result<PathBuf, VisualRuntimeError> {
    if !path.is_absolute()
        || path.file_name().and_then(|value| value.to_str()) != Some(WORKER_FILE_NAME)
    {
        return Err(VisualRuntimeError::InvalidBundle);
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| VisualRuntimeError::Missing)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(VisualRuntimeError::InvalidBundle);
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| VisualRuntimeError::InvalidBundle)?;
    let parent = path
        .parent()
        .ok_or(VisualRuntimeError::InvalidBundle)?
        .canonicalize()
        .map_err(|_| VisualRuntimeError::InvalidBundle)?;
    if canonical.parent() != Some(parent.as_path()) {
        return Err(VisualRuntimeError::InvalidBundle);
    }
    Ok(canonical)
}

#[cfg(windows)]
#[derive(Debug)]
struct WorkerChild {
    handle: windows_sys::Win32::Foundation::HANDLE,
    process_id: u32,
}

#[cfg(windows)]
// SAFETY: this wrapper owns a process kernel handle and never exposes process
// memory; the handle may be waited or terminated from the async runtime thread.
unsafe impl Send for WorkerChild {}

#[cfg(windows)]
impl WorkerChild {
    fn id(&self) -> u32 {
        self.process_id
    }
    fn creation_time(&self) -> Result<u64, VisualRuntimeError> {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::GetProcessTimes;
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: handle is live and every FILETIME pointer is writable.
        if unsafe {
            GetProcessTimes(
                self.handle,
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return Err(VisualRuntimeError::Process);
        }
        Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
    fn try_wait(&mut self) -> Result<Option<u32>, VisualRuntimeError> {
        use windows_sys::Win32::Foundation::STILL_ACTIVE;
        use windows_sys::Win32::System::Threading::GetExitCodeProcess;
        let mut code = 0_u32;
        // SAFETY: handle is live and code is writable.
        if unsafe { GetExitCodeProcess(self.handle, &mut code) } == 0 {
            return Err(VisualRuntimeError::Process);
        }
        Ok((code != STILL_ACTIVE as u32).then_some(code))
    }
    async fn wait(&mut self) -> Result<u32, VisualRuntimeError> {
        loop {
            if let Some(code) = self.try_wait()? {
                return Ok(code);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    async fn kill(&mut self) -> Result<(), VisualRuntimeError> {
        use windows_sys::Win32::System::Threading::TerminateProcess;
        // SAFETY: handle is the live process handle owned by this wrapper.
        (unsafe { TerminateProcess(self.handle, 1) } != 0)
            .then_some(())
            .ok_or(VisualRuntimeError::Process)
    }
}

#[cfg(windows)]
impl Drop for WorkerChild {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::TerminateProcess;
        if matches!(self.try_wait(), Ok(None)) {
            // SAFETY: kill-on-drop is the last-resort no-orphan guarantee.
            unsafe { TerminateProcess(self.handle, 1) };
        }
        // SAFETY: this wrapper exclusively owns the process handle.
        unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(windows)]
fn spawn_worker_process(
    executable: &Path,
    args: &[String],
    parent_job: &RuntimeSupervisor,
) -> Result<WorkerChild, VisualRuntimeError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, ResumeThread, TerminateProcess, CREATE_NO_WINDOW, CREATE_SUSPENDED,
        CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
    };
    let application = executable
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let executable_text = executable.to_string_lossy().replace('"', "");
    let quoted_args = args
        .iter()
        .map(|value| format!("\"{}\"", value.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ");
    let mut command_line = format!("\"{executable_text}\" {quoted_args}")
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
    let current_directory = executable
        .parent()
        .ok_or(VisualRuntimeError::InvalidBundle)?
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let environment = minimal_environment_block();
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..STARTUPINFOW::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: fixed executable and quoted argument buffers are NUL-terminated;
    // no handles are inherited and the process starts suspended.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            current_directory.as_ptr(),
            &startup,
            &mut process,
        )
    };
    if created == 0 || process.hProcess.is_null() || process.hThread.is_null() {
        return Err(VisualRuntimeError::Process);
    }
    if let Err(error) = parent_job.assign_raw_process_to_parent_job(process.hProcess as usize) {
        // SAFETY: both returned handles are exclusively owned on this path.
        unsafe {
            TerminateProcess(process.hProcess, 1);
            CloseHandle(process.hThread);
            CloseHandle(process.hProcess);
        }
        return Err(VisualRuntimeError::Job(error.to_string()));
    }
    // SAFETY: primary thread is suspended exactly once.
    let resumed = unsafe { ResumeThread(process.hThread) };
    // SAFETY: primary thread handle is no longer needed.
    unsafe { CloseHandle(process.hThread) };
    if resumed == u32::MAX {
        // SAFETY: process handle is still live and exclusively owned.
        unsafe {
            TerminateProcess(process.hProcess, 1);
            CloseHandle(process.hProcess);
        }
        return Err(VisualRuntimeError::Process);
    }
    Ok(WorkerChild {
        handle: process.hProcess,
        process_id: process.dwProcessId,
    })
}

#[cfg(windows)]
async fn connect_worker_pipe(pipe: &str) -> Result<std::fs::File, VisualRuntimeError> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING};
    let pipe = std::ffi::OsStr::new(pipe)
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    loop {
        // SAFETY: pipe is a fixed NUL-terminated per-launch name.
        let handle = unsafe {
            CreateFileW(
                pipe.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            // SAFETY: successful handle ownership transfers once to File.
            return Ok(unsafe { std::fs::File::from_raw_handle(handle) });
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(VisualRuntimeError::Connection);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(not(windows))]
#[derive(Debug)]
struct WorkerChild;

#[cfg(not(windows))]
impl WorkerChild {
    fn id(&self) -> u32 {
        0
    }
    fn creation_time(&self) -> Result<u64, VisualRuntimeError> {
        Err(VisualRuntimeError::Unsupported)
    }
    fn try_wait(&mut self) -> Result<Option<u32>, VisualRuntimeError> {
        Ok(Some(1))
    }
    async fn wait(&mut self) -> Result<u32, VisualRuntimeError> {
        Ok(1)
    }
    async fn kill(&mut self) -> Result<(), VisualRuntimeError> {
        Ok(())
    }
}

#[cfg(not(windows))]
fn spawn_worker_process(
    _executable: &Path,
    _args: &[String],
    _parent_job: &RuntimeSupervisor,
) -> Result<WorkerChild, VisualRuntimeError> {
    Err(VisualRuntimeError::Unsupported)
}

#[cfg(not(windows))]
async fn connect_worker_pipe(_pipe: &str) -> Result<std::fs::File, VisualRuntimeError> {
    Err(VisualRuntimeError::Unsupported)
}

#[cfg(windows)]
fn minimal_environment_block() -> Vec<u16> {
    let mut entries = [
        "ALLUSERSPROFILE",
        "APPDATA",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "CommonProgramW6432",
        "COMPUTERNAME",
        "ComSpec",
        "DriverData",
        "HOMEDRIVE",
        "HOMEPATH",
        "LOCALAPPDATA",
        "NUMBER_OF_PROCESSORS",
        "OS",
        "Path",
        "PATHEXT",
        "PROCESSOR_ARCHITECTURE",
        "ProgramData",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "PUBLIC",
        "SystemDrive",
        "SystemRoot",
        "TEMP",
        "TMP",
        "USERDOMAIN",
        "USERNAME",
        "USERPROFILE",
        "WINDIR",
    ]
    .into_iter()
    .filter_map(|key| {
        std::env::var_os(key).map(|value| format!("{key}={}", value.to_string_lossy()))
    })
    .collect::<Vec<_>>();
    entries.sort_by_key(|value| value.to_ascii_uppercase());
    let mut block = Vec::new();
    for entry in entries {
        block.extend(entry.encode_utf16());
        block.push(0);
    }
    block.push(0);
    block
}

#[derive(Debug, thiserror::Error)]
pub enum VisualRuntimeError {
    #[error("bundled mouth worker is missing")]
    Missing,
    #[error("bundled mouth worker layout is invalid")]
    InvalidBundle,
    #[error("mouth worker launch randomness is unavailable")]
    Random,
    #[error("mouth worker process could not be started or inspected")]
    Process,
    #[error("mouth worker could not join the parent Job Object: {0}")]
    Job(String),
    #[error("mouth worker control pipe is unavailable")]
    Connection,
    #[error("mouth worker request timed out")]
    Timeout,
    #[error("mouth worker frame exceeds its protocol bound")]
    Payload,
    #[error("mouth worker response is malformed")]
    Malformed,
    #[error("mouth worker rejected the request: {0}")]
    Worker(String),
    #[error("visual request violates the typed actor/frame/audio contract")]
    InvalidRequest,
    #[error("native visual pack admission is unavailable: {0}")]
    Admission(String),
    #[error("capture evidence is not exact selected-window WGC authority")]
    CaptureAuthority,
    #[error("causal WASAPI visual envelope is unavailable or inactive")]
    AudioAuthority,
    #[error("optional visual scheduling bypassed the request: {0}")]
    Scheduling(String),
    #[error("tracker output does not match the exact leased WGC frame")]
    StaleLandmarks,
    #[error("no selected target generation is available")]
    NoSelectedTarget,
    #[error("monotonic clock conversion failed")]
    Clock,
    #[error("mouth worker state is unavailable")]
    State,
    #[error("mouth worker is Windows-only")]
    Unsupported,
    #[error("visual broker stage {stage} failed: {source}")]
    BrokerStage {
        stage: &'static str,
        #[source]
        source: crate::media_broker::MediaBrokerError,
    },
    #[error(transparent)]
    Broker(#[from] crate::media_broker::MediaBrokerError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_audio_binding_accepts_the_full_runtime_playback_pool() {
        let leases = (0..crate::media_broker::MAX_PLAYBACK_LEASES_PER_TURN)
            .map(|index| {
                crate::media_broker::test_audio_playback_lease(format!("fixture-stream-{index}"), 7)
            })
            .collect::<Vec<_>>();

        let (generation, bindings) =
            visual_audio_bindings(&leases).expect("the runtime pool must drive visual speech");
        assert_eq!(generation, 7);
        assert_eq!(
            bindings.len(),
            crate::media_broker::MAX_PLAYBACK_LEASES_PER_TURN
        );
    }

    #[test]
    fn visual_audio_binding_reports_an_unsupported_legacy_lease_schema() {
        let mut lease = crate::media_broker::test_audio_playback_lease("fixture-stream", 7);
        lease.schema_version = 1;

        assert_eq!(
            visual_audio_bindings(&[lease]).expect_err("legacy playback lease must fail closed"),
            "visual_audio_lease_schema_unsupported"
        );
    }

    #[test]
    fn coordinator_retains_the_best_receipt_for_each_generation() {
        let coordinator = VisualCoordinator::unavailable_for_tests();
        let receipt = |request_id: u64, generation: u64, detail: &str| {
            let mut receipt = VisualPresentationReceipt::fail_open(request_id, detail);
            receipt.cancellation_generation = generation;
            receipt
        };

        coordinator.record_receipt(receipt(1, 7, "visual_audio_coordinator_waiting"));

        let mut exact_frame = receipt(2, 7, "worker_bypass_after_exact_frame");
        exact_frame.source_frame_sequence = 41;
        exact_frame.product_disposition = 2;
        coordinator.record_receipt(exact_frame.clone());

        coordinator.record_receipt(receipt(3, 7, "audio_already_inactive"));
        assert_eq!(
            coordinator
                .best_receipt_for_generation(7)
                .expect("generation seven receipt"),
            exact_frame
        );

        let mut presented = receipt(4, 7, "presented");
        presented.source_frame_sequence = 42;
        presented.product_disposition = 0;
        presented.residual_proposed = true;
        presented.presented = true;
        presented.degraded = false;
        coordinator.record_receipt(presented.clone());
        coordinator.record_receipt(receipt(5, 7, "late_inactive_sample"));
        coordinator.record_receipt(receipt(6, 8, "other_turn"));

        assert_eq!(
            coordinator
                .best_receipt_for_generation(7)
                .expect("presented generation seven receipt"),
            presented
        );
        assert_eq!(
            coordinator
                .best_receipt_for_generation(8)
                .expect("generation eight receipt")
                .detail,
            "other_turn"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn governed_product_entrypoint_bypasses_before_worker_without_native_admission() {
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repository root");
        let scratch = tempfile::tempdir().expect("private test directory");
        let current = std::env::current_exe().expect("test executable");
        let parent = RuntimeSupervisor::try_new(crate::sidecar_supervisor::RuntimeLaunchConfig {
            executable: current.clone(),
            resource_root: repository,
            app_data: scratch.path().join("runtime-host-data"),
            development_fixture_allowed: true,
        })
        .expect("native parent job");
        let broker = MediaBrokerSupervisor::new(
            crate::media_broker::MediaBrokerLaunchConfig {
                executable: current.clone(),
                development_fixture_allowed: true,
                audio_output_selection_path: scratch.path().join("audio-output-selection-v1.json"),
                #[cfg(debug_assertions)]
                debug_synthetic_metadata_path: scratch.path().join("synthetic-target.json"),
            },
            parent.clone(),
        );
        let visual = MouthWorkerSupervisor::new(
            MouthWorkerLaunchConfig {
                executable: current,
                development_fixture_allowed: true,
                #[cfg(debug_assertions)]
                review_openseeface_root: None,
            },
            parent,
            broker,
        );
        let resources = LocalResourceManager::new(scratch.path(), None).expect("local resources");
        let receipt = visual
            .present_governed_current_frame(&resources, valid_request(), 1)
            .await;
        assert!(receipt.degraded);
        assert!(!receipt.presented);
        assert!(!receipt.residual_proposed);
        assert!(receipt.detail.starts_with("visual_admission_bypass:"));
        assert_eq!(receipt.pixel_source, None);
    }

    /// Opt-in product proof for the real Windows sidecars. The normal source
    /// test suite remains artifact-independent; release/qualification jobs set
    /// `NPC_RUN_NATIVE_VISUAL_PRODUCT_TEST=1` after building both native
    /// targets. This exercises the Rust supervisor's authenticated GUI-worker
    /// launch and generation handoff, then runs the native broker proof that
    /// covers exact WGC leasing, worker death during live PCM, drain, restart,
    /// and residual presentation.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread")]
    async fn rust_supervisor_authenticates_gui_worker_and_native_broker_proof() {
        if std::env::var_os("NPC_RUN_NATIVE_VISUAL_PRODUCT_TEST").as_deref()
            != Some(std::ffi::OsStr::new("1"))
        {
            return;
        }

        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repository root");
        let worker = repository
            .join("out/build/native-mouth-worker-product/Release/npc-mouth-worker.exe")
            .canonicalize()
            .expect("built GUI mouth worker");
        let broker_smoke = repository
            .join(
                "out/build/native-media-broker-visual/Release/\
                 npc_media_broker_windows_smoke_tests.exe",
            )
            .canonicalize()
            .expect("built native visual broker proof");
        let scratch = tempfile::tempdir().expect("private test directory");
        let parent = RuntimeSupervisor::try_new(crate::sidecar_supervisor::RuntimeLaunchConfig {
            executable: std::env::current_exe().expect("test executable"),
            resource_root: repository.clone(),
            app_data: scratch.path().join("runtime-host-data"),
            development_fixture_allowed: true,
        })
        .expect("kill-on-close parent job");
        let broker = MediaBrokerSupervisor::new(
            crate::media_broker::MediaBrokerLaunchConfig {
                executable: broker_smoke.clone(),
                development_fixture_allowed: false,
                audio_output_selection_path: scratch.path().join("audio-output-selection-v1.json"),
                #[cfg(debug_assertions)]
                debug_synthetic_metadata_path: scratch.path().join("synthetic-target.json"),
            },
            parent.clone(),
        );
        let visual = MouthWorkerSupervisor::new(
            MouthWorkerLaunchConfig {
                executable: worker,
                development_fixture_allowed: false,
                #[cfg(debug_assertions)]
                review_openseeface_root: None,
            },
            parent,
            broker,
        );

        let (_, first_identity) = visual.ensure_ready(1).await.expect("authenticated worker");
        assert!(first_identity.process_id > 0);
        assert!(first_identity.process_creation_time > 0);
        let (_, next_identity) = visual
            .ensure_ready(2)
            .await
            .expect("worker generation handoff");
        assert_eq!(next_identity.process_id, first_identity.process_id);
        assert_eq!(
            next_identity.process_creation_time,
            first_identity.process_creation_time
        );
        visual.shutdown().await;

        let proof = std::process::Command::new(&broker_smoke)
            .arg("--service-only")
            .output()
            .expect("run native visual product proof");
        let output = String::from_utf8_lossy(&proof.stdout);
        let errors = String::from_utf8_lossy(&proof.stderr);
        assert!(
            proof.status.success(),
            "native proof failed:\nstdout:\n{output}\nstderr:\n{errors}"
        );
        assert!(output.contains("exact current WGC source texture"));
        assert!(output.contains("endpoint-drained without global broker cancellation"));
        assert!(output.contains("presented through the broker service"));
        assert!(output.contains("service lifecycle smoke passed"));
    }

    #[test]
    fn worker_crash_mid_spoken_turn_is_visual_only_and_never_emits_broker_cancel() {
        const {
            assert!(WORKER_FAILURE_POLICY.release_visual_source_metadata);
            assert!(WORKER_FAILURE_POLICY.drop_visual_worker);
            assert!(!WORKER_FAILURE_POLICY.cancel_playback);
            assert!(!WORKER_FAILURE_POLICY.advance_broker_generation);
        }
        let broker_source = include_str!("media_broker.rs");
        assert!(!broker_source.contains("cancel_visual_generation"));
    }

    #[test]
    fn exact_frame_and_bounded_pcm_are_required_before_any_native_handle_request() {
        let mut request = valid_request();
        assert!(validate_frame_request(&request).is_ok());
        request.landmarks.expected_frame_sequence = 0;
        assert!(matches!(
            validate_frame_request(&request),
            Err(VisualRuntimeError::InvalidRequest)
        ));
        request = valid_request();
        request.drive = MouthDrive::PcmWindow {
            stream_generation: 1,
            segment_id: 1,
            first_sample_index: 0,
            sample_rate: 48_000,
            channels: 1,
            playback_at_ns: 1,
            interleaved_pcm: vec![0.0; 32 * 1024 + 1],
        };
        assert!(matches!(
            validate_frame_request(&request),
            Err(VisualRuntimeError::InvalidRequest)
        ));
    }

    #[test]
    fn admitted_actor_authority_accepts_newer_frames_only_inside_the_freshness_window() {
        const FREQUENCY: u64 = 1_000_000_000;
        const AUTHORITY_QPC: u64 = 10_000_000_000;
        let authority_qpc_ns =
            i64::try_from(AUTHORITY_QPC).expect("test authority QPC must fit in i64");
        assert!(bounded_frame_progression(
            40,
            AUTHORITY_QPC,
            43,
            AUTHORITY_QPC + 30_000_000,
            FREQUENCY,
            authority_qpc_ns + 100_000_000,
        ));
        assert!(!bounded_frame_progression(
            40,
            AUTHORITY_QPC,
            39,
            AUTHORITY_QPC + 30_000_000,
            FREQUENCY,
            authority_qpc_ns + 100_000_000,
        ));
        assert!(!bounded_frame_progression(
            40,
            AUTHORITY_QPC,
            43,
            AUTHORITY_QPC + 30_000_000,
            FREQUENCY,
            authority_qpc_ns + VISUAL_FRAME_DEADLINE_NS + 1,
        ));
        assert!(!bounded_frame_progression(
            40,
            AUTHORITY_QPC,
            43,
            AUTHORITY_QPC + 103_000_000,
            FREQUENCY,
            authority_qpc_ns + 100_000_000,
        ));
    }

    #[test]
    fn command29_envelope_becomes_bounded_amplitude_coefficients_without_pcm_or_visemes() {
        let mut envelope = VisualAudioEnvelope {
            schema_version: 1,
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            generation: 7,
            stream_id: "pcm-001".into(),
            segment_id: "pcm-001".into(),
            source_sample_start: 960,
            source_sample_count: 480,
            sample_rate: 48_000,
            channels: 1,
            device_write_qpc: 10_000,
            qpc_frequency: 10_000_000,
            source_frames: 1_440,
            device_frames: 1_440,
            mono_rms_q15: [1_024, 2_048, 3_072, 4_096, 5_120, 6_144, 7_168, 8_192],
            mono_peak_q15: [2_048, 4_096, 6_144, 8_192, 10_240, 12_288, 14_336, 16_384],
            active: true,
            draining: false,
            cancelled: false,
        };
        let drive = drive_from_visual_audio_envelope(&envelope, 1_500_000).expect("causal drive");
        match drive {
            MouthDrive::CausalEnvelopeCoefficients {
                stream_generation,
                first_sample_index,
                sample_rate,
                playback_at_ns,
                coefficients,
                ..
            } => {
                assert_eq!(stream_generation, 7);
                assert_eq!(first_sample_index, 960);
                assert_eq!(sample_rate, 48_000);
                assert_eq!(playback_at_ns, 1_500_000);
                assert!(coefficients.iter().all(|value| (0.0..=1.0).contains(value)));
                assert!(coefficients[0] > 0.0);
            }
            _ => panic!("command 29 must not be converted to PCM or a fabricated viseme"),
        }
        let late_drive =
            drive_from_visual_audio_envelope(&envelope, 10_500_000).expect("late causal drive");
        match late_drive {
            MouthDrive::CausalEnvelopeCoefficients {
                first_sample_index,
                playback_at_ns,
                coefficients,
                ..
            } => {
                assert_eq!(first_sample_index, 1_380);
                assert_eq!(playback_at_ns, 10_500_000);
                assert!(
                    coefficients[0] > 0.25,
                    "the current temporal bin drives the jaw"
                );
            }
            _ => panic!("late command 29 drive must stay amplitude-only"),
        }
        assert!(matches!(
            drive_from_visual_audio_envelope(&envelope, 270_000_001),
            Err(VisualRuntimeError::AudioAuthority)
        ));
        envelope.draining = true;
        assert!(matches!(
            drive_from_visual_audio_envelope(&envelope, 1_500_000),
            Err(VisualRuntimeError::AudioAuthority)
        ));
    }

    #[test]
    fn admitted_request_accepts_only_exact_typed_seed_and_audio_address() {
        let mut request = valid_admitted_request();
        assert!(validate_admitted_frame_request(&request).is_ok());
        request.seed_face_width = 0.0;
        assert!(matches!(
            validate_admitted_frame_request(&request),
            Err(VisualRuntimeError::InvalidRequest)
        ));
        request = valid_admitted_request();
        request.audio.stream_id.clear();
        assert!(matches!(
            validate_admitted_frame_request(&request),
            Err(VisualRuntimeError::InvalidRequest)
        ));
    }

    struct FakeActorLockSource {
        lock: Mutex<Option<Arc<NativeSelectedActorLockV1>>>,
    }

    impl ActorLockSource for FakeActorLockSource {
        fn selected_current(&self, now_unix_ms: u64) -> Option<Arc<NativeSelectedActorLockV1>> {
            self.lock.lock().ok().and_then(|lock| {
                lock.as_ref()
                    .filter(|selected| selected.expires_at_unix_ms > now_unix_ms)
                    .cloned()
            })
        }
    }

    #[derive(Default)]
    struct RecordingPresenter {
        requests: Mutex<Vec<AdmittedVisualFrameRequest>>,
    }

    #[async_trait]
    impl AdmittedVisualPresenter for RecordingPresenter {
        async fn present(
            &self,
            request: AdmittedVisualFrameRequest,
            _now_monotonic_millis: u64,
        ) -> VisualPresentationReceipt {
            let request_id = request.request_id;
            if let Ok(mut requests) = self.requests.lock() {
                requests.push(request);
            }
            let mut receipt = VisualPresentationReceipt::fail_open(request_id, "fake_presented");
            receipt.product_disposition = 0;
            receipt.degraded = false;
            receipt
        }

        #[cfg(debug_assertions)]
        async fn present_project_owned_review(
            &self,
            _request_id: u64,
            _audio: &VisualAudioBinding,
            _sentence_id: u64,
            _now_ns: i64,
        ) -> Option<VisualPresentationReceipt> {
            None
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn coordinator_decouples_dialogue_generation_from_current_capture_lock_and_fails_open_on_stale_expiry(
    ) {
        let generation = 7;
        let audio = VisualAudioBinding {
            session_id: "session-native".into(),
            turn_id: "turn-native".into(),
            generation,
            stream_id: "pcm-native-001".into(),
        };
        let presenter = Arc::new(RecordingPresenter::default());
        let source = Arc::new(FakeActorLockSource {
            lock: Mutex::new(None),
        });
        let coordinator = VisualCoordinator::from_parts(source.clone(), presenter.clone());

        coordinator
            .start_audio_bindings(generation, vec![audio.clone()])
            .await;
        tokio::time::sleep(Duration::from_millis(85)).await;
        coordinator.stop_turn(Some(generation)).await;
        assert!(presenter.requests.lock().expect("requests").is_empty());
        let unavailable = coordinator.latest_receipt().expect("fail-open receipt");
        assert!(unavailable.degraded);
        assert!(!unavailable.presented);
        assert_eq!(unavailable.detail, "visual_actor_lock_unavailable");
        assert_eq!(unavailable.cancellation_generation, generation);

        let now_ns = monotonic_ns().expect("QPC clock");
        let current = Arc::new(native_actor_lock_fixture(
            generation,
            now_ns as u64,
            current_unix_millis() + 1_000,
        ));
        *source.lock.lock().expect("actor lock") = Some(current.clone());
        coordinator
            .start_audio_bindings(generation, vec![audio.clone()])
            .await;
        tokio::time::sleep(Duration::from_millis(85)).await;
        coordinator.stop_turn(Some(generation)).await;
        {
            let requests = presenter.requests.lock().expect("requests");
            assert!(!requests.is_empty());
            let request = &requests[0];
            assert_eq!(request.track.actor_id, current.runtime_actor_id);
            assert_eq!(request.track.track_id, current.track_id);
            assert_eq!(request.track.track_epoch, current.track_epoch);
            assert_eq!(request.selected_process_id, current.selected_process_id);
            assert_eq!(
                request.selected_window_handle,
                current.selected_window_handle
            );
            assert_eq!(
                request.expected_frame_sequence,
                current.source_frame_sequence
            );
            assert_eq!(request.expected_frame_qpc, current.source_frame_qpc);
        }

        let baseline = presenter.requests.lock().expect("requests").len();
        let expired = Arc::new(native_actor_lock_fixture(
            generation,
            monotonic_ns().expect("QPC clock") as u64,
            current_unix_millis().saturating_sub(1),
        ));
        *source.lock.lock().expect("actor lock") = Some(expired);
        coordinator
            .start_audio_bindings(generation, vec![audio.clone()])
            .await;
        tokio::time::sleep(Duration::from_millis(85)).await;
        coordinator.stop_turn(Some(generation)).await;
        assert_eq!(presenter.requests.lock().expect("requests").len(), baseline);

        let cancelled = Arc::new(native_actor_lock_fixture(
            generation + 1,
            monotonic_ns().expect("QPC clock") as u64,
            current_unix_millis() + 1_000,
        ));
        *source.lock.lock().expect("actor lock") = Some(cancelled);
        coordinator
            .start_audio_bindings(generation, vec![audio.clone()])
            .await;
        tokio::time::sleep(Duration::from_millis(85)).await;
        coordinator.stop_turn(Some(generation)).await;
        let decoupled_count = presenter.requests.lock().expect("requests").len();
        assert!(decoupled_count > baseline);
        {
            let requests = presenter.requests.lock().expect("requests");
            let request = requests.last().expect("decoupled request");
            assert_eq!(request.cancellation_generation, generation + 1);
            assert_eq!(request.audio.generation, generation);
        }

        let stale = Arc::new(native_actor_lock_fixture(
            generation,
            (monotonic_ns().expect("QPC clock") - VISUAL_FRAME_DEADLINE_NS - 1) as u64,
            current_unix_millis() + 1_000,
        ));
        *source.lock.lock().expect("actor lock") = Some(stale);
        coordinator
            .start_audio_bindings(generation, vec![audio])
            .await;
        tokio::time::sleep(Duration::from_millis(85)).await;
        coordinator.stop_turn(Some(generation)).await;
        assert_eq!(
            presenter.requests.lock().expect("requests").len(),
            decoupled_count
        );
    }

    #[cfg(windows)]
    fn native_actor_lock_fixture(
        cancellation_generation: u64,
        source_frame_qpc: u64,
        expires_at_unix_ms: u64,
    ) -> NativeSelectedActorLockV1 {
        NativeSelectedActorLockV1 {
            schema_version: 1,
            lock_generation: 1,
            provenance: crate::identity_runtime::NativeActorLockProvenanceV1::QualifiedIdentity {
                qualification_id: "qualified-identity-fixture".into(),
                catalog_admission_sha256: "a".repeat(64),
                admission_receipt_sha256: "b".repeat(64),
            },
            game_profile_id: "game-fixture".into(),
            capture_session_id: "capture-fixture".into(),
            selected_process_id: 42,
            selected_window_handle: 43,
            selected_executable_name: "game.exe".into(),
            character_id: "character-fixture".into(),
            selection_authority:
                crate::identity_runtime::NativeActorSelectionAuthorityV1::Consensus,
            actor_id: "encounter-fixture".into(),
            runtime_actor_id: 44,
            track_id: 45,
            track_epoch: 46,
            full_source_roi: crate::identity_runtime::NativeFullSourceRoiV1 {
                x: 0.2,
                y: 0.2,
                width: 0.4,
                height: 0.5,
                source_width: 1_920,
                source_height: 1_080,
            },
            appearance_hysteresis_latched: true,
            appearance_descriptor_revision: 1,
            expected_appearance_digest_high: 2,
            expected_appearance_digest_low: 3,
            observed_appearance_digest_high: 2,
            observed_appearance_digest_low: 3,
            appearance_similarity: 0.99,
            temporal_iou: 0.98,
            blocker_coverage: 0.01,
            scene_transition_detected: false,
            identity_confidence: 0.99,
            identity_margin: 0.4,
            source_content_sha256: "b".repeat(64),
            source_frame_sequence: 47,
            source_frame_qpc,
            qpc_frequency: 1_000_000_000,
            captured_at_unix_ms: current_unix_millis(),
            device_generation: 48,
            geometry_epoch: 49,
            expires_at_unix_ms,
            cancellation_generation,
        }
    }

    #[cfg(windows)]
    #[test]
    fn sealed_native_click_provenance_is_admitted_without_identity_fields() {
        let generation = 7;
        let audio = VisualAudioBinding {
            session_id: "session-native-click".into(),
            turn_id: "turn-native-click".into(),
            generation,
            stream_id: "pcm-native-click".into(),
        };
        let now_ns = monotonic_ns().expect("QPC clock");
        let mut lock =
            native_actor_lock_fixture(generation, now_ns as u64, current_unix_millis() + 200);
        lock.character_id.clear();
        lock.selection_authority = NativeActorSelectionAuthorityV1::SealedNativeClick;
        lock.provenance = NativeActorLockProvenanceV1::SealedNativeClick {
            visual_pack_id: "openseeface-mnv3-lm1-mouth-signal".into(),
            visual_pack_admission_sha256: "c".repeat(64),
            native_click_receipt_sha256: "d".repeat(64),
            candidate_set_sha256: "e".repeat(64),
            receipt_nonce_high: 1,
            receipt_nonce_low: 2,
        };
        assert!(admitted_request_from_actor_lock(1, &lock, &audio, 1, now_ns).is_some());

        lock.provenance = NativeActorLockProvenanceV1::QualifiedIdentity {
            qualification_id: "must-not-cross-domains".into(),
            catalog_admission_sha256: "a".repeat(64),
            admission_receipt_sha256: "b".repeat(64),
        };
        assert!(admitted_request_from_actor_lock(2, &lock, &audio, 1, now_ns).is_none());
    }

    #[test]
    fn worker_response_codec_rejects_trailing_or_oversized_detail() {
        let mut wire = empty_response_wire();
        wire.u32(0);
        let decoded = decode_worker_response(&wire.take()).expect("valid response");
        assert_eq!(decoded.status, 0);
        assert_eq!(decoded.detail, "");

        let mut trailing = empty_response_wire();
        trailing.u32(0);
        trailing.u8(9);
        assert!(matches!(
            decode_worker_response(&trailing.take()),
            Err(VisualRuntimeError::Malformed)
        ));
    }

    fn valid_request() -> VisualFrameRequest {
        VisualFrameRequest {
            request_id: 1,
            session_id_high: 1,
            session_id_low: 2,
            turn_id_high: 3,
            turn_id_low: 4,
            sentence_id: 1,
            track: VisualTrackBinding {
                actor_id: 5,
                track_id: 6,
                track_epoch: 7,
            },
            landmarks: OpenSeeFacePacket {
                provider_instance_id: 8,
                expected_frame_sequence: 9,
                face_x: 0.2,
                face_y: 0.2,
                face_width: 0.4,
                face_height: 0.5,
                landmarks: std::array::from_fn(|_| NormalizedLandmark {
                    x: 0.4,
                    y: 0.5,
                    confidence: 0.99,
                }),
                yaw_degrees: 0.0,
                pitch_degrees: 0.0,
                roll_degrees: 0.0,
                detector_confidence: 0.99,
                landmark_confidence: 0.99,
                visibility_ratio: 0.99,
                mouth_occluded: false,
                measured_at_ns: 1,
            },
            appearance: AppearanceGateEvidence {
                descriptor_revision: 1,
                expected_digest_high: 1,
                expected_digest_low: 2,
                observed_digest_high: 1,
                observed_digest_low: 2,
                similarity: 0.99,
                temporal_iou: 0.99,
                blocker_coverage: 0.0,
                identity_locked: true,
                target_visible: true,
                scene_transition: false,
            },
            pressure: VisualPressure::Nominal,
            admitted_signal_rate_hz: 15,
            local_visuals_admitted: true,
            drive: MouthDrive::TimedViseme {
                stream_generation: 1,
                segment_id: 1,
                first_sample_index: 0,
                sample_rate: 48_000,
                channels: 1,
                playback_at_ns: 1,
                viseme: 9,
                strength: 0.8,
            },
            deadline_ns: 2,
        }
    }

    fn valid_admitted_request() -> AdmittedVisualFrameRequest {
        AdmittedVisualFrameRequest {
            request_id: 1,
            session_id_high: 1,
            session_id_low: 2,
            turn_id_high: 3,
            turn_id_low: 4,
            sentence_id: 1,
            selected_process_id: 42,
            selected_window_handle: 43,
            selected_executable_name: "game.exe".into(),
            track: VisualTrackBinding {
                actor_id: 5,
                track_id: 6,
                track_epoch: 7,
            },
            cancellation_generation: 1,
            expected_frame_sequence: 9,
            expected_frame_qpc: 10,
            qpc_frequency: 10_000_000,
            expected_device_generation: 11,
            expected_geometry_epoch: 12,
            expected_source_width: 1_920,
            expected_source_height: 1_080,
            seed_face_x: 0.2,
            seed_face_y: 0.2,
            seed_face_width: 0.4,
            seed_face_height: 0.5,
            appearance: AppearanceGateEvidence {
                descriptor_revision: 1,
                expected_digest_high: 1,
                expected_digest_low: 2,
                observed_digest_high: 1,
                observed_digest_low: 2,
                similarity: 0.99,
                temporal_iou: 0.99,
                blocker_coverage: 0.0,
                identity_locked: true,
                target_visible: true,
                scene_transition: false,
            },
            pressure: VisualPressure::Nominal,
            admitted_signal_rate_hz: 15,
            audio: VisualAudioBinding {
                session_id: "session-1".into(),
                turn_id: "turn-1".into(),
                generation: 1,
                stream_id: "pcm-001".into(),
            },
            deadline_ns: 2_000_000,
        }
    }

    fn empty_response_wire() -> WireWriter {
        let mut wire = WireWriter::default();
        wire.u32(PROTOCOL_MAGIC);
        wire.u16(PROTOCOL_VERSION);
        wire.u16(0);
        wire.u64(1);
        wire.u64(1);
        wire.u32(1);
        for value in [1_u64, 1, 2, 3, 4, 1] {
            wire.u64(value);
        }
        for value in [1_u64, 5, 6, 7] {
            wire.u64(value);
        }
        wire.u64(9);
        wire.u64(0);
        wire.u64(1);
        wire.i64(1);
        wire.u8(4);
        wire.u8(4);
        wire.u8(1);
        wire.u8(1);
        wire.u32(15);
        wire.u64(1);
        wire.i64(1);
        wire.i64(2);
        wire.u64(0);
        wire.boolean(false);
        wire
    }
}
