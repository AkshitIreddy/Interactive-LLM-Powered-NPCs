//! Native-only control path for the optional CPU identity observation worker.
//!
//! Mapping names, lease nonces, pixels, and worker embeddings never cross the
//! WebView boundary. The worker result is untrusted until it is rebound to the
//! broker-issued target/frame/digest and accepted by `TrustedWgcEvidenceAdapterV1`.

// Product activation is deliberately withheld until Model Manager supplies a
// measured v2 pack. Keep the complete native bridge compiled meanwhile without
// creating a WebView affordance or pretending the optional model is available.
#![allow(dead_code)]

use crate::identity_worker_transport::{
    FramedIdentityWorkerTransport, IdentityReferenceImportTransport, IdentityWorkerLaunchConfig,
    NativeReferenceExtractionV1, ReferencePixelLeaseV1,
};
use crate::local_resources::{LocalResourceManager, NativeAdmittedIdentityPackLaunchV1};
use crate::media_broker::{
    IdentityCrop, IdentityFrameLease, IdentityReferenceImportRequest, IdentityReferenceSourceClass,
    IdentityWorkerIdentity, MediaBrokerError, MediaBrokerSupervisor,
    NativeManualActorPickerReceiptV1, NativeManualActorPickerRequestV1,
    NativeManualActorPickerStatusV1,
};
use crate::sidecar_supervisor::RuntimeSupervisor;
use async_trait::async_trait;
use npc_identity_engine::{
    ActorDetectionV1, ActorIdentityEngineV1, BoundingBoxV1, FrameActorsV1, FrameIdentityUpdateV1,
    IdentityDecisionV1, PinnedIdentityQualificationV1, QualifiedIdentityError,
    QualifiedIdentityGalleryV1, QualifiedReferenceProvenanceV1, ReferenceSourceClassV1,
    TrackPhaseV1, TrustedCaptureTargetV1, TrustedWgcEvidenceAdapterV1, TrustedWgcIdentityFrameV1,
    QUALIFIED_IDENTITY_SCHEMA_VERSION,
};
use npc_protocol::{TurnSafetyContextErrorV1, TurnSafetyContextV1, TurnSafetyEvidenceStateV1};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{watch, Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

const IDENTITY_INFERENCE_TIMEOUT: Duration = Duration::from_millis(500);
const OBSERVATION_CONTRACT: &str = "npc.identity-observations/v1";
const REQUEST_CONTRACT: &str = "npc.identity-observation-request/v1";
const MAX_GALLERY_BYTES: u64 = 8 * 1024 * 1024;
const ACTOR_LOCK_SCHEMA_VERSION: u32 = 1;
const MAX_ACTOR_LOCK_LIFETIME_MS: u64 = 250;
const IDENTITY_OBSERVATION_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PrivateEvaluationLoad {
    pub lease_id: String,
    pub manifest_path: std::path::PathBuf,
    pub manifest_sha256: String,
    pub artifact_root: std::path::PathBuf,
    pub cpu_threads: u8,
    pub explicit_user_confirmation: bool,
    pub activation_mode: IdentityActivationMode,
    pub verified_catalog_admission_sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IdentityActivationMode {
    PrivateEvaluation,
    QualifiedCatalog,
}

#[derive(Debug, Serialize)]
struct WorkerCaptureTarget<'a> {
    capture_session_id: &'a str,
    process_id: u32,
    window_handle: u64,
    executable_name: &'a str,
}

#[derive(Debug, Serialize)]
struct WorkerPixelLease<'a> {
    lease_id: &'a str,
    shared_memory_name: &'a str,
    lease_nonce: &'a str,
    byte_length: u64,
    width: u32,
    height: u32,
    stride_bytes: u32,
    pixel_format: &'a str,
    content_sha256: &'a str,
}

#[derive(Debug, Serialize)]
struct WorkerFrameRequest<'a> {
    contract_version: &'static str,
    mode: &'static str,
    target: WorkerCaptureTarget<'a>,
    frame_sequence: u64,
    device_generation: u64,
    geometry_epoch: u64,
    source_frame_qpc: u64,
    qpc_frequency: u64,
    captured_at_ms: u64,
    content_sha256: &'a str,
    advancing_frame_verified: bool,
    overlay_capture_excluded: bool,
    protected_online_detected: bool,
    anti_cheat_detected: bool,
    pixel_lease: WorkerPixelLease<'a>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UntrustedWorkerObservationsV1 {
    contract_version: String,
    authority: String,
    native_revalidation_required: bool,
    schema_version: u32,
    target: TrustedCaptureTargetV1,
    frame_sequence: u64,
    device_generation: u64,
    geometry_epoch: u64,
    source_frame_qpc: u64,
    qpc_frequency: u64,
    captured_at_ms: u64,
    content_sha256: String,
    advancing_frame_verified: bool,
    overlay_capture_excluded: bool,
    protected_online_detected: bool,
    anti_cheat_detected: bool,
    observations: Vec<ActorDetectionV1>,
}

#[async_trait]
pub(crate) trait IdentityWorkerTransport: Send + Sync + 'static {
    fn identity(&self) -> IdentityWorkerIdentity;
    /// True only after the hidden child completed its launch-nonce handshake.
    fn authenticated(&self) -> bool;
    /// True only when the exact child belongs to a kill-on-parent-close Job.
    fn parent_death_bound(&self) -> bool;
    async fn load_private_evaluation(
        &self,
        request: &PrivateEvaluationLoad,
    ) -> Result<(), IdentityRuntimeError>;
    async fn infer_wgc_frame(
        &self,
        payload: serde_json::Value,
        cancellation_generation: u64,
    ) -> Result<UntrustedWorkerObservationsV1, IdentityRuntimeError>;
    /// Advances exactly one worker generation. Implementations must terminate
    /// and recreate the child when a framed cancel cannot complete in time.
    async fn cancel_and_restart(
        &self,
        cancellation_generation: u64,
    ) -> Result<(), IdentityRuntimeError>;
    async fn shutdown(&self);
}

pub(crate) struct IdentityControlBridge<W: IdentityWorkerTransport> {
    broker: MediaBrokerSupervisor,
    worker: Arc<W>,
    qualification: PinnedIdentityQualificationV1,
    actor_locks: Arc<NativeActorLockBusV1>,
    one_request: Semaphore,
    cancellation_generation: Mutex<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AttestedIdentityObservation {
    pub target: TrustedCaptureTargetV1,
    pub crop: IdentityCrop,
    pub source_width: u32,
    pub source_height: u32,
    pub frame_sequence: u64,
    pub device_generation: u64,
    pub geometry_epoch: u64,
    pub source_frame_qpc: u64,
    pub qpc_frequency: u64,
    pub captured_at_ms: u64,
    pub content_sha256: String,
    pub actors: FrameActorsV1,
}

/// Opaque result of running the native tracker/resolver over an observation
/// that already passed broker and qualification revalidation. Callers cannot
/// supply a `FrameIdentityUpdateV1` independently of its attested frame.
pub(crate) struct AdmittedIdentityTrackerResultV1 {
    observation: AttestedIdentityObservation,
    update: FrameIdentityUpdateV1,
    appearance: NativeTrackerAppearanceEvidenceV1,
}

/// Native tracker/occlusion evidence required by VisualCoordinator. Fields are
/// private so neither WebView DTOs nor arbitrary visual callers can manufacture
/// an appearance gate.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeTrackerAppearanceEvidenceV1 {
    descriptor_revision: u64,
    expected_digest_high: u64,
    expected_digest_low: u64,
    observed_digest_high: u64,
    observed_digest_low: u64,
    similarity: f32,
    temporal_iou: f32,
    blocker_coverage: f32,
    scene_transition_detected: bool,
}

impl NativeTrackerAppearanceEvidenceV1 {
    // The tracker receipt is an explicit closed-world native boundary.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_native_tracker(
        descriptor_revision: u64,
        expected_digest_high: u64,
        expected_digest_low: u64,
        observed_digest_high: u64,
        observed_digest_low: u64,
        similarity: f32,
        temporal_iou: f32,
        blocker_coverage: f32,
        scene_transition_detected: bool,
    ) -> Result<Self, IdentityRuntimeError> {
        if descriptor_revision == 0
            || (expected_digest_high == 0 && expected_digest_low == 0)
            || (observed_digest_high == 0 && observed_digest_low == 0)
            || !similarity.is_finite()
            || !(-1.0..=1.0).contains(&similarity)
            || !temporal_iou.is_finite()
            || !(0.0..=1.0).contains(&temporal_iou)
            || !blocker_coverage.is_finite()
            || !(0.0..=1.0).contains(&blocker_coverage)
        {
            return Err(IdentityRuntimeError::ActorLockEvidenceMismatch);
        }
        Ok(Self {
            descriptor_revision,
            expected_digest_high,
            expected_digest_low,
            observed_digest_high,
            observed_digest_low,
            similarity,
            temporal_iou,
            blocker_coverage,
            scene_transition_detected,
        })
    }
}

pub(crate) fn process_attested_identity_tracker(
    engine: &mut ActorIdentityEngineV1,
    observation: AttestedIdentityObservation,
    appearance: NativeTrackerAppearanceEvidenceV1,
) -> Result<AdmittedIdentityTrackerResultV1, IdentityRuntimeError> {
    let update = engine
        .process_frame(observation.actors.clone())
        .map_err(|error| IdentityRuntimeError::IdentityEngine(error.to_string()))?;
    Ok(AdmittedIdentityTrackerResultV1 {
        observation,
        update,
        appearance,
    })
}

/// Native authority for a selected actor. This type intentionally has no
/// serde implementation and is never accepted by a Tauri command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeActorSelectionAuthorityV1 {
    Explicit,
    Consensus,
    SealedNativeClick,
}

/// Provenance domains are intentionally disjoint. A native click can never be
/// mistaken for qualified automatic identity evidence, and qualified identity
/// fields are not populated or repurposed for manual selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeActorLockProvenanceV1 {
    QualifiedIdentity {
        qualification_id: String,
        catalog_admission_sha256: String,
        admission_receipt_sha256: String,
    },
    SealedNativeClick {
        visual_pack_id: String,
        visual_pack_admission_sha256: String,
        native_click_receipt_sha256: String,
        candidate_set_sha256: String,
        receipt_nonce_high: u64,
        receipt_nonce_low: u64,
    },
}

/// Normalized full-source coordinates. The identity crop is translated back
/// into the WGC source coordinate system before this value is constructed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NativeFullSourceRoiV1 {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub source_width: u32,
    pub source_height: u32,
}

/// Immutable native-only actor lock consumed by visual scheduling. It contains
/// no pixels, mappings, nonces, embeddings, or WebView-supplied coordinates.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeSelectedActorLockV1 {
    pub schema_version: u32,
    pub lock_generation: u64,
    pub provenance: NativeActorLockProvenanceV1,
    pub game_profile_id: String,
    pub capture_session_id: String,
    pub selected_process_id: u32,
    pub selected_window_handle: u64,
    pub selected_executable_name: String,
    pub character_id: String,
    pub selection_authority: NativeActorSelectionAuthorityV1,
    pub actor_id: String,
    pub runtime_actor_id: u64,
    pub track_id: u64,
    pub track_epoch: u64,
    pub full_source_roi: NativeFullSourceRoiV1,
    pub appearance_hysteresis_latched: bool,
    pub appearance_descriptor_revision: u64,
    pub expected_appearance_digest_high: u64,
    pub expected_appearance_digest_low: u64,
    pub observed_appearance_digest_high: u64,
    pub observed_appearance_digest_low: u64,
    pub appearance_similarity: f32,
    pub temporal_iou: f32,
    pub blocker_coverage: f32,
    pub scene_transition_detected: bool,
    pub identity_confidence: f32,
    pub identity_margin: f32,
    pub source_content_sha256: String,
    pub source_frame_sequence: u64,
    pub source_frame_qpc: u64,
    pub qpc_frequency: u64,
    pub captured_at_unix_ms: u64,
    pub device_generation: u64,
    pub geometry_epoch: u64,
    pub expires_at_unix_ms: u64,
    pub cancellation_generation: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NativeActorLockStateV1 {
    IdentityPackUnqualified,
    QualifiedNoSelection {
        qualification_id: String,
        catalog_admission_sha256: String,
        admission_receipt_sha256: String,
        exact_target_pid: u32,
        cancellation_generation: u64,
    },
    Selected(Arc<NativeSelectedActorLockV1>),
}

/// Opaque proof that Model Manager admitted the exact signed identity pack.
/// It is native-only and cannot be deserialized from the WebView. No product
/// code constructs it while catalog admission remains blocked.
#[derive(Clone, Debug)]
pub(crate) struct QualifiedIdentityActorLockAdmissionV1 {
    qualification: PinnedIdentityQualificationV1,
    catalog_admission_sha256: String,
    admission_receipt_sha256: String,
    exact_target_pid: u32,
}

impl QualifiedIdentityActorLockAdmissionV1 {
    fn from_admitted_launch(
        launch: &NativeAdmittedIdentityPackLaunchV1,
    ) -> Result<Self, IdentityRuntimeError> {
        launch.qualification.validate()?;
        if launch.exact_target_pid == 0
            || !valid_lower_sha256(launch.catalog_admission_sha256.as_str())
            || !valid_lower_sha256(launch.admission_receipt_sha256.as_str())
        {
            return Err(IdentityRuntimeError::ActorLockNotAdmitted);
        }
        Ok(Self {
            qualification: launch.qualification.clone(),
            catalog_admission_sha256: launch.catalog_admission_sha256.as_str().to_owned(),
            admission_receipt_sha256: launch.admission_receipt_sha256.as_str().to_owned(),
            exact_target_pid: launch.exact_target_pid,
        })
    }
}

/// AppState owns one bus; Identity publishes and VisualCoordinator subscribes.
/// `watch` gives each consumer an immutable `Arc` snapshot and coalesces stale
/// frames, so native backpressure is bounded to one current actor lock.
pub(crate) struct NativeActorLockBusV1 {
    state: watch::Sender<NativeActorLockStateV1>,
    next_lock_generation: AtomicU64,
    next_runtime_actor_id: AtomicU64,
    consumed_manual_receipts: StdMutex<std::collections::BTreeSet<(u64, u64)>>,
}

impl NativeActorLockBusV1 {
    pub(crate) fn new_unqualified() -> Arc<Self> {
        let (state, _) = watch::channel(NativeActorLockStateV1::IdentityPackUnqualified);
        Arc::new(Self {
            state,
            next_lock_generation: AtomicU64::new(1),
            next_runtime_actor_id: AtomicU64::new(1),
            consumed_manual_receipts: StdMutex::new(std::collections::BTreeSet::new()),
        })
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<NativeActorLockStateV1> {
        self.state.subscribe()
    }

    pub(crate) fn snapshot(&self) -> NativeActorLockStateV1 {
        self.state.borrow().clone()
    }

    pub(crate) fn activate_qualified(
        &self,
        admission: &QualifiedIdentityActorLockAdmissionV1,
        cancellation_generation: u64,
    ) {
        self.state
            .send_replace(NativeActorLockStateV1::QualifiedNoSelection {
                qualification_id: admission.qualification.qualification_id.clone(),
                catalog_admission_sha256: admission.catalog_admission_sha256.clone(),
                admission_receipt_sha256: admission.admission_receipt_sha256.clone(),
                exact_target_pid: admission.exact_target_pid,
                cancellation_generation,
            });
    }

    pub(crate) fn publish_from_admitted_tracker(
        &self,
        admission: &QualifiedIdentityActorLockAdmissionV1,
        game_profile_id: &str,
        character_id: &str,
        result: &AdmittedIdentityTrackerResultV1,
        expires_at_unix_ms: u64,
        cancellation_generation: u64,
    ) -> Result<Arc<NativeSelectedActorLockV1>, IdentityRuntimeError> {
        let observation = &result.observation;
        let update = &result.update;
        let appearance = &result.appearance;
        let state = self.state.borrow().clone();
        let (
            active_qualification,
            active_catalog,
            active_receipt,
            active_target_pid,
            active_generation,
        ) = match state {
            NativeActorLockStateV1::QualifiedNoSelection {
                qualification_id,
                catalog_admission_sha256,
                admission_receipt_sha256,
                exact_target_pid,
                cancellation_generation,
            } => (
                qualification_id,
                catalog_admission_sha256,
                admission_receipt_sha256,
                exact_target_pid,
                cancellation_generation,
            ),
            NativeActorLockStateV1::Selected(ref selected) => {
                let NativeActorLockProvenanceV1::QualifiedIdentity {
                    qualification_id,
                    catalog_admission_sha256,
                    admission_receipt_sha256,
                } = &selected.provenance
                else {
                    return Err(IdentityRuntimeError::ActorLockNotAdmitted);
                };
                (
                    qualification_id.clone(),
                    catalog_admission_sha256.clone(),
                    admission_receipt_sha256.clone(),
                    selected.selected_process_id,
                    selected.cancellation_generation,
                )
            }
            NativeActorLockStateV1::IdentityPackUnqualified => {
                return Err(IdentityRuntimeError::ActorLockNotAdmitted)
            }
        };
        if active_qualification != admission.qualification.qualification_id
            || active_catalog != admission.catalog_admission_sha256
            || active_receipt != admission.admission_receipt_sha256
            || active_target_pid != admission.exact_target_pid
            || active_generation != cancellation_generation
        {
            return Err(IdentityRuntimeError::ActorLockNotAdmitted);
        }

        let Some(selected_track_id) = update.selected_track_id else {
            self.revoke(cancellation_generation);
            return Err(IdentityRuntimeError::ActorLockEvidenceMismatch);
        };
        let Some(actor) = update
            .actors
            .iter()
            .find(|actor| actor.track.track_id == selected_track_id)
        else {
            self.revoke(cancellation_generation);
            return Err(IdentityRuntimeError::ActorLockEvidenceMismatch);
        };
        if update.frame_index != observation.actors.frame_index
            || actor.track.last_seen_frame != update.frame_index
            || !actor.track.selected
            || actor.track.phase != TrackPhaseV1::Visible
            || observation.target.process_id == 0
            || observation.target.process_id != admission.exact_target_pid
            || observation.target.window_handle == 0
            || observation.target.capture_session_id.is_empty()
            || observation.target.executable_name.is_empty()
            || observation.frame_sequence == 0
            || observation.source_frame_qpc == 0
            || observation.qpc_frequency == 0
            || observation.device_generation == 0
            || observation.geometry_epoch == 0
            || !valid_lower_sha256(&observation.content_sha256)
            || !valid_enrollment_identifier(game_profile_id, 128)
            || !valid_enrollment_identifier(character_id, 128)
            || expires_at_unix_ms <= observation.captured_at_ms
            || expires_at_unix_ms - observation.captured_at_ms > MAX_ACTOR_LOCK_LIFETIME_MS
        {
            self.revoke(cancellation_generation);
            return Err(IdentityRuntimeError::ActorLockEvidenceMismatch);
        }
        let (authority, actor_id, confidence, margin, latched) = match &actor.identity {
            IdentityDecisionV1::Explicit {
                encounter_id,
                subject_id,
            } if subject_id == character_id => (
                NativeActorSelectionAuthorityV1::Explicit,
                encounter_id.clone(),
                1.0,
                1.0,
                false,
            ),
            IdentityDecisionV1::Matched {
                encounter_id,
                subject_id,
                subject_similarity,
                top1_top2_margin,
                held_by_hysteresis,
                ..
            } if subject_id == character_id
                && subject_similarity.is_finite()
                && (-1.0..=1.0).contains(subject_similarity)
                && top1_top2_margin.is_finite()
                && *top1_top2_margin >= 0.0 =>
            {
                (
                    NativeActorSelectionAuthorityV1::Consensus,
                    encounter_id.clone(),
                    *subject_similarity,
                    *top1_top2_margin,
                    *held_by_hysteresis,
                )
            }
            _ => {
                self.revoke(cancellation_generation);
                return Err(IdentityRuntimeError::ActorLockEvidenceMismatch);
            }
        };
        if authority == NativeActorSelectionAuthorityV1::Consensus
            && (appearance.similarity - confidence).abs() > f32::EPSILON * 8.0
        {
            self.revoke(cancellation_generation);
            return Err(IdentityRuntimeError::ActorLockEvidenceMismatch);
        }
        let roi = match normalized_full_source_roi(
            actor.track.bounds,
            observation.source_width,
            observation.source_height,
        ) {
            Ok(roi) => roi,
            Err(error) => {
                self.revoke(cancellation_generation);
                return Err(error);
            }
        };
        let lock_generation = self
            .next_lock_generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| IdentityRuntimeError::GenerationExhausted)?;
        let runtime_actor_id = match self.state.borrow().clone() {
            NativeActorLockStateV1::Selected(previous)
                if previous.game_profile_id == game_profile_id
                    && previous.capture_session_id == observation.target.capture_session_id
                    && previous.track_id == actor.track.track_id.0
                    && previous.track_epoch == actor.track.track_epoch.0 =>
            {
                previous.runtime_actor_id
            }
            _ => self
                .next_runtime_actor_id
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                    current.checked_add(1)
                })
                .map_err(|_| IdentityRuntimeError::GenerationExhausted)?,
        };
        let selected = Arc::new(NativeSelectedActorLockV1 {
            schema_version: ACTOR_LOCK_SCHEMA_VERSION,
            lock_generation,
            provenance: NativeActorLockProvenanceV1::QualifiedIdentity {
                qualification_id: admission.qualification.qualification_id.clone(),
                catalog_admission_sha256: admission.catalog_admission_sha256.clone(),
                admission_receipt_sha256: admission.admission_receipt_sha256.clone(),
            },
            game_profile_id: game_profile_id.to_owned(),
            capture_session_id: observation.target.capture_session_id.clone(),
            selected_process_id: observation.target.process_id,
            selected_window_handle: observation.target.window_handle,
            selected_executable_name: observation.target.executable_name.clone(),
            character_id: character_id.to_owned(),
            selection_authority: authority,
            actor_id,
            runtime_actor_id,
            track_id: actor.track.track_id.0,
            track_epoch: actor.track.track_epoch.0,
            full_source_roi: roi,
            appearance_hysteresis_latched: latched,
            appearance_descriptor_revision: appearance.descriptor_revision,
            expected_appearance_digest_high: appearance.expected_digest_high,
            expected_appearance_digest_low: appearance.expected_digest_low,
            observed_appearance_digest_high: appearance.observed_digest_high,
            observed_appearance_digest_low: appearance.observed_digest_low,
            appearance_similarity: appearance.similarity,
            temporal_iou: appearance.temporal_iou,
            blocker_coverage: appearance.blocker_coverage,
            scene_transition_detected: appearance.scene_transition_detected,
            identity_confidence: confidence,
            identity_margin: margin,
            source_content_sha256: observation.content_sha256.clone(),
            source_frame_sequence: observation.frame_sequence,
            source_frame_qpc: observation.source_frame_qpc,
            qpc_frequency: observation.qpc_frequency,
            captured_at_unix_ms: observation.captured_at_ms,
            device_generation: observation.device_generation,
            geometry_epoch: observation.geometry_epoch,
            expires_at_unix_ms,
            cancellation_generation,
        });
        self.state
            .send_replace(NativeActorLockStateV1::Selected(selected.clone()));
        Ok(selected)
    }

    /// Single-consumes one selected command-31 receipt. The native click proof
    /// is bound to the exact private candidate set and admitted visual pack;
    /// it never borrows or populates qualified identity provenance.
    pub(crate) fn publish_from_sealed_native_click(
        &self,
        request: &NativeManualActorPickerRequestV1,
        receipt: &NativeManualActorPickerReceiptV1,
        now_unix_ms: u64,
        current_cancellation_generation: u64,
    ) -> Result<Arc<NativeSelectedActorLockV1>, IdentityRuntimeError> {
        let candidate_digest =
            crate::media_broker::manual_actor_candidate_set_sha256(&request.candidates);
        let selected_candidate = request.candidates.iter().find(|candidate| {
            candidate.actor_id == receipt.selected_actor_id
                && candidate.track_id == receipt.selected_track_id
                && candidate.track_epoch == receipt.selected_track_epoch
        });
        if receipt.schema_version != 1
            || receipt.status != NativeManualActorPickerStatusV1::Selected
            || receipt.request_id != request.request_id
            || receipt.capture_session_id != request.capture_session_id
            || receipt.cancellation_generation != request.cancellation_generation
            || receipt.cancellation_generation != current_cancellation_generation
            || receipt.selected_process_id != request.selected_process_id
            || receipt.selected_window_handle != request.selected_window_handle
            || receipt.selected_executable_name != request.selected_executable_name
            || receipt.source_device_generation != request.source_device_generation
            || receipt.source_geometry_epoch != request.source_geometry_epoch
            || receipt.source_frame_sequence != request.source_frame_sequence
            || receipt.source_frame_qpc != request.source_frame_qpc
            || receipt.candidate_count as usize != request.candidates.len()
            || receipt.candidate_set_sha256 != candidate_digest
            || !valid_lower_sha256(&request.visual_pack_admission_sha256)
            || !valid_lower_sha256(&receipt.receipt_sha256)
            || request.visual_pack_id.is_empty()
            || request.game_profile_id.is_empty()
            || request.source_width == 0
            || request.source_height == 0
            || request.qpc_frequency == 0
            || request.qpc_frequency != receipt.qpc_frequency
            || request.captured_at_unix_ms == 0
            || now_unix_ms < request.captured_at_unix_ms
            || now_unix_ms >= request.expires_at_unix_ms
            || request.expires_at_unix_ms - request.captured_at_unix_ms > MAX_ACTOR_LOCK_LIFETIME_MS
            || receipt.receipt_nonce_high == 0 && receipt.receipt_nonce_low == 0
            || receipt.began_qpc < receipt.source_frame_qpc
            || receipt.clicked_qpc < receipt.began_qpc
            || receipt.attested_at_qpc < receipt.clicked_qpc
            || receipt.pointer_kind == 0
            || !receipt.frozen_wgc_frame_verified
            || !receipt.overlay_capture_excluded
            || !receipt.overlay_nonactivating
            || !receipt.single_hardware_pointer_click
            || !receipt.pixels_withheld_from_webview
            || !receipt.coordinates_withheld_from_webview
            || selected_candidate.is_none()
        {
            return Err(IdentityRuntimeError::ManualActorClickEvidenceMismatch);
        }
        let candidate = selected_candidate.expect("checked selected candidate");
        let width = candidate.right - candidate.left;
        let height = candidate.bottom - candidate.top;
        if ![
            candidate.left,
            candidate.top,
            candidate.right,
            candidate.bottom,
            width,
            height,
        ]
        .into_iter()
        .all(|value| value.is_finite())
            || candidate.left < 0.0
            || candidate.top < 0.0
            || candidate.right > 1.0
            || candidate.bottom > 1.0
            || width < 0.005
            || height < 0.005
        {
            return Err(IdentityRuntimeError::ManualActorClickEvidenceMismatch);
        }
        let nonce = (receipt.receipt_nonce_high, receipt.receipt_nonce_low);
        let mut consumed = self
            .consumed_manual_receipts
            .lock()
            .map_err(|_| IdentityRuntimeError::ManualActorClickEvidenceMismatch)?;
        if !consumed.insert(nonce) {
            return Err(IdentityRuntimeError::ManualActorClickReceiptReplayed);
        }
        let lock_generation = self
            .next_lock_generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| IdentityRuntimeError::GenerationExhausted)?;
        let runtime_actor_id = self
            .next_runtime_actor_id
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| IdentityRuntimeError::GenerationExhausted)?;
        let digest = hex::decode(&receipt.candidate_set_sha256)
            .map_err(|_| IdentityRuntimeError::ManualActorClickEvidenceMismatch)?;
        let digest_high = u64::from_le_bytes(
            digest[0..8]
                .try_into()
                .map_err(|_| IdentityRuntimeError::ManualActorClickEvidenceMismatch)?,
        );
        let digest_low = u64::from_le_bytes(
            digest[8..16]
                .try_into()
                .map_err(|_| IdentityRuntimeError::ManualActorClickEvidenceMismatch)?,
        );
        let selected = Arc::new(NativeSelectedActorLockV1 {
            schema_version: ACTOR_LOCK_SCHEMA_VERSION,
            lock_generation,
            provenance: NativeActorLockProvenanceV1::SealedNativeClick {
                visual_pack_id: request.visual_pack_id.clone(),
                visual_pack_admission_sha256: request.visual_pack_admission_sha256.clone(),
                native_click_receipt_sha256: receipt.receipt_sha256.clone(),
                candidate_set_sha256: receipt.candidate_set_sha256.clone(),
                receipt_nonce_high: receipt.receipt_nonce_high,
                receipt_nonce_low: receipt.receipt_nonce_low,
            },
            game_profile_id: request.game_profile_id.clone(),
            capture_session_id: request.capture_session_id.clone(),
            selected_process_id: request.selected_process_id,
            selected_window_handle: request.selected_window_handle,
            selected_executable_name: request.selected_executable_name.clone(),
            character_id: String::new(),
            selection_authority: NativeActorSelectionAuthorityV1::SealedNativeClick,
            actor_id: format!("sealed-click-{}", candidate.actor_id),
            runtime_actor_id,
            track_id: candidate.track_id,
            track_epoch: candidate.track_epoch,
            full_source_roi: NativeFullSourceRoiV1 {
                x: candidate.left as f32,
                y: candidate.top as f32,
                width: width as f32,
                height: height as f32,
                source_width: request.source_width,
                source_height: request.source_height,
            },
            appearance_hysteresis_latched: false,
            appearance_descriptor_revision: 1,
            expected_appearance_digest_high: digest_high,
            expected_appearance_digest_low: digest_low,
            observed_appearance_digest_high: digest_high,
            observed_appearance_digest_low: digest_low,
            appearance_similarity: 1.0,
            temporal_iou: 1.0,
            blocker_coverage: 0.0,
            scene_transition_detected: false,
            identity_confidence: 0.0,
            identity_margin: 0.0,
            source_content_sha256: String::new(),
            source_frame_sequence: request.source_frame_sequence,
            source_frame_qpc: request.source_frame_qpc,
            qpc_frequency: request.qpc_frequency,
            captured_at_unix_ms: request.captured_at_unix_ms,
            device_generation: request.source_device_generation,
            geometry_epoch: request.source_geometry_epoch,
            expires_at_unix_ms: request.expires_at_unix_ms,
            cancellation_generation: request.cancellation_generation,
        });
        self.state
            .send_replace(NativeActorLockStateV1::Selected(selected.clone()));
        Ok(selected)
    }

    pub(crate) fn selected_if_current(
        &self,
        now_unix_ms: u64,
        cancellation_generation: u64,
    ) -> Option<Arc<NativeSelectedActorLockV1>> {
        self.selected_current(now_unix_ms)
            .filter(|selected| selected.cancellation_generation == cancellation_generation)
    }

    /// Returns the current broker-bound actor authority without conflating its
    /// capture cancellation generation with an independent dialogue-turn
    /// generation. Consumers still validate the lock's own generation against
    /// every leased frame before presentation.
    pub(crate) fn selected_current(
        &self,
        now_unix_ms: u64,
    ) -> Option<Arc<NativeSelectedActorLockV1>> {
        match self.state.borrow().clone() {
            NativeActorLockStateV1::Selected(selected)
                if selected.expires_at_unix_ms > now_unix_ms =>
            {
                Some(selected)
            }
            _ => None,
        }
    }

    pub(crate) fn revoke(&self, cancellation_generation: u64) {
        let current = self.state.borrow().clone();
        match current {
            NativeActorLockStateV1::QualifiedNoSelection {
                qualification_id,
                catalog_admission_sha256,
                admission_receipt_sha256,
                exact_target_pid,
                ..
            } => {
                self.state
                    .send_replace(NativeActorLockStateV1::QualifiedNoSelection {
                        qualification_id,
                        catalog_admission_sha256,
                        admission_receipt_sha256,
                        exact_target_pid,
                        cancellation_generation,
                    });
            }
            NativeActorLockStateV1::Selected(selected) => match &selected.provenance {
                NativeActorLockProvenanceV1::QualifiedIdentity {
                    qualification_id,
                    catalog_admission_sha256,
                    admission_receipt_sha256,
                } => {
                    self.state
                        .send_replace(NativeActorLockStateV1::QualifiedNoSelection {
                            qualification_id: qualification_id.clone(),
                            catalog_admission_sha256: catalog_admission_sha256.clone(),
                            admission_receipt_sha256: admission_receipt_sha256.clone(),
                            exact_target_pid: selected.selected_process_id,
                            cancellation_generation,
                        });
                }
                NativeActorLockProvenanceV1::SealedNativeClick { .. } => {
                    self.state
                        .send_replace(NativeActorLockStateV1::IdentityPackUnqualified);
                }
            },
            NativeActorLockStateV1::IdentityPackUnqualified => {}
        }
    }

    pub(crate) fn deactivate_unqualified(&self) {
        self.state
            .send_replace(NativeActorLockStateV1::IdentityPackUnqualified);
    }
}

fn normalized_full_source_roi(
    bounds: BoundingBoxV1,
    source_width: u32,
    source_height: u32,
) -> Result<NativeFullSourceRoiV1, IdentityRuntimeError> {
    bounds
        .validate()
        .map_err(|_| IdentityRuntimeError::ActorLockEvidenceMismatch)?;
    if source_width == 0
        || source_height == 0
        || bounds.x < 0.0
        || bounds.y < 0.0
        || bounds.x + bounds.width > source_width as f32
        || bounds.y + bounds.height > source_height as f32
    {
        return Err(IdentityRuntimeError::ActorLockEvidenceMismatch);
    }
    Ok(NativeFullSourceRoiV1 {
        x: bounds.x / source_width as f32,
        y: bounds.y / source_height as f32,
        width: bounds.width / source_width as f32,
        height: bounds.height / source_height as f32,
        source_width,
        source_height,
    })
}

fn valid_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityReferenceEnrollmentCommandRequest {
    pub game_profile_id: String,
    pub character_id: String,
    pub reference_id: String,
    pub subject_display_name: String,
    pub source_class: ReferenceSourceClassV1,
    pub owner_user_id: Option<String>,
    pub original_work_license: Option<String>,
    pub explicit_user_consent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IdentityReferenceEnrollmentRequest {
    pub game_profile_id: String,
    pub character_id: String,
    pub reference_id: String,
    pub subject_display_name: String,
    pub source_class: ReferenceSourceClassV1,
    pub owner_user_id: Option<String>,
    pub original_work_license: Option<String>,
    pub explicit_user_consent: bool,
    pub imported_at_unix_ms: u64,
}

impl IdentityReferenceEnrollmentCommandRequest {
    pub(crate) fn into_native(
        self,
    ) -> Result<IdentityReferenceEnrollmentRequest, IdentityRuntimeError> {
        let imported_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| IdentityRuntimeError::InvalidReferenceEnrollment)?
            .as_millis()
            .try_into()
            .map_err(|_| IdentityRuntimeError::InvalidReferenceEnrollment)?;
        let request = IdentityReferenceEnrollmentRequest {
            game_profile_id: self.game_profile_id,
            character_id: self.character_id,
            reference_id: self.reference_id,
            subject_display_name: self.subject_display_name,
            source_class: self.source_class,
            owner_user_id: self.owner_user_id,
            original_work_license: self.original_work_license,
            explicit_user_consent: self.explicit_user_consent,
            imported_at_unix_ms,
        };
        validate_enrollment_request(&request)?;
        Ok(request)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityReferenceEnrollmentReceipt {
    pub gallery_schema_version: u32,
    pub gallery_generation: u64,
    pub game_profile_id: String,
    pub character_id: String,
    pub reference_id: String,
    pub source_asset_sha256: String,
    pub normalized_pixel_sha256: String,
    pub reference_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdentityGalleryDocumentV1 {
    schema_version: u32,
    generation: u64,
    gallery: QualifiedIdentityGalleryV1,
}

#[derive(Clone, Debug)]
pub(crate) struct IdentityGalleryStore {
    root: PathBuf,
}

impl IdentityGalleryStore {
    pub(crate) fn new(config_directory: &Path) -> Result<Self, IdentityRuntimeError> {
        let root = crate::private_directory::ensure_private_directory(
            &config_directory.join("identity-galleries"),
        )
        .map_err(|_| IdentityRuntimeError::GalleryPersistence)?;
        Ok(Self { root })
    }

    pub(crate) fn load_or_create(
        &self,
        game_profile_id: &str,
        qualification: &PinnedIdentityQualificationV1,
    ) -> Result<QualifiedIdentityGalleryV1, IdentityRuntimeError> {
        if !valid_enrollment_identifier(game_profile_id, 128) {
            return Err(IdentityRuntimeError::InvalidReferenceEnrollment);
        }
        qualification.validate()?;
        let path = self.gallery_path(game_profile_id);
        if !path.exists() {
            return QualifiedIdentityGalleryV1::new(game_profile_id, qualification.clone())
                .map_err(Into::into);
        }
        reject_unsafe_gallery_file(&path)?;
        let metadata = fs::metadata(&path).map_err(|_| IdentityRuntimeError::GalleryPersistence)?;
        if metadata.len() == 0 || metadata.len() > MAX_GALLERY_BYTES {
            return Err(IdentityRuntimeError::GalleryPersistence);
        }
        let bytes = fs::read(&path).map_err(|_| IdentityRuntimeError::GalleryPersistence)?;
        let document: IdentityGalleryDocumentV1 =
            serde_json::from_slice(&bytes).map_err(|_| IdentityRuntimeError::GalleryPersistence)?;
        document.gallery.validate()?;
        if document.schema_version != 1
            || document.generation == 0
            || document.gallery.game_profile_id != game_profile_id
            || document.gallery.qualification != *qualification
        {
            return Err(IdentityRuntimeError::GalleryQualificationMismatch);
        }
        Ok(document.gallery)
    }

    pub(crate) fn persist(
        &self,
        gallery: &QualifiedIdentityGalleryV1,
    ) -> Result<u64, IdentityRuntimeError> {
        gallery.validate()?;
        if !valid_enrollment_identifier(&gallery.game_profile_id, 128) {
            return Err(IdentityRuntimeError::InvalidReferenceEnrollment);
        }
        let path = self.gallery_path(&gallery.game_profile_id);
        let prior_generation = if path.exists() {
            reject_unsafe_gallery_file(&path)?;
            let metadata =
                fs::metadata(&path).map_err(|_| IdentityRuntimeError::GalleryPersistence)?;
            if metadata.len() == 0 || metadata.len() > MAX_GALLERY_BYTES {
                return Err(IdentityRuntimeError::GalleryPersistence);
            }
            let prior: IdentityGalleryDocumentV1 = serde_json::from_slice(
                &fs::read(&path).map_err(|_| IdentityRuntimeError::GalleryPersistence)?,
            )
            .map_err(|_| IdentityRuntimeError::GalleryPersistence)?;
            prior.gallery.validate()?;
            if prior.schema_version != 1
                || prior.generation == 0
                || prior.gallery.game_profile_id != gallery.game_profile_id
                || prior.gallery.qualification != gallery.qualification
            {
                return Err(IdentityRuntimeError::GalleryQualificationMismatch);
            }
            prior.generation
        } else {
            0
        };
        let generation = prior_generation
            .checked_add(1)
            .ok_or(IdentityRuntimeError::GalleryPersistence)?;
        let document = IdentityGalleryDocumentV1 {
            schema_version: 1,
            generation,
            gallery: gallery.clone(),
        };
        let bytes =
            serde_json::to_vec(&document).map_err(|_| IdentityRuntimeError::GalleryPersistence)?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_GALLERY_BYTES {
            return Err(IdentityRuntimeError::GalleryPersistence);
        }
        let mut temporary = tempfile::NamedTempFile::new_in(&self.root)
            .map_err(|_| IdentityRuntimeError::GalleryPersistence)?;
        temporary
            .write_all(&bytes)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|_| IdentityRuntimeError::GalleryPersistence)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o600))
                .map_err(|_| IdentityRuntimeError::GalleryPersistence)?;
        }
        temporary
            .persist(&path)
            .map_err(|_| IdentityRuntimeError::GalleryPersistence)?;
        Ok(generation)
    }

    fn gallery_path(&self, game_profile_id: &str) -> PathBuf {
        self.root
            .join(format!("{game_profile_id}.identity-gallery.v1.json"))
    }
}

impl<W: IdentityWorkerTransport> IdentityControlBridge<W> {
    pub(crate) fn new(
        broker: MediaBrokerSupervisor,
        worker: Arc<W>,
        qualification: PinnedIdentityQualificationV1,
        actor_locks: Arc<NativeActorLockBusV1>,
    ) -> Self {
        Self {
            broker,
            worker,
            qualification,
            actor_locks,
            one_request: Semaphore::new(1),
            cancellation_generation: Mutex::new(0),
        }
    }

    pub(crate) async fn load(
        &self,
        request: &PrivateEvaluationLoad,
    ) -> Result<(), IdentityRuntimeError> {
        validate_private_evaluation_load(request)?;
        if !self.worker.authenticated() || !self.worker.parent_death_bound() {
            return Err(IdentityRuntimeError::UntrustedWorkerProcess);
        }
        self.worker.load_private_evaluation(request).await
    }

    pub(crate) async fn observe(
        &self,
        crop: IdentityCrop,
        safety: TurnSafetyContextV1,
    ) -> Result<AttestedIdentityObservation, IdentityRuntimeError> {
        let _permit = self
            .one_request
            .try_acquire()
            .map_err(|_| IdentityRuntimeError::Backpressure)?;
        if !self.worker.authenticated() || !self.worker.parent_death_bound() {
            return Err(IdentityRuntimeError::UntrustedWorkerProcess);
        }
        validate_identity_safety(safety)?;
        let worker_identity = self.worker.identity();
        let lease = self
            .broker
            .allocate_identity_frame(&worker_identity, crop)
            .await?;
        let payload = worker_frame_payload(&lease)?;
        // Worker cancellation is an independent generation barrier. The
        // broker generation remains immutable frame provenance and must never
        // be reused as authority to advance the worker scheduler.
        let generation = *self.cancellation_generation.lock().await;
        let result = tokio::time::timeout(
            IDENTITY_INFERENCE_TIMEOUT,
            self.worker.infer_wgc_frame(payload, generation),
        )
        .await;
        let release = self.broker.release_identity_frame(&lease).await;
        let observations = match result {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                let _ = release;
                return Err(error);
            }
            Err(_) => {
                let _ = release;
                self.cancel_and_restart().await?;
                return Err(IdentityRuntimeError::Timeout);
            }
        };
        release?;
        let expected_target = TrustedCaptureTargetV1 {
            capture_session_id: lease.capture_session_id.clone(),
            process_id: lease.selected_process_id,
            window_handle: lease.selected_window_handle,
            executable_name: lease.selected_executable_name.clone(),
        };
        let mut adapter =
            TrustedWgcEvidenceAdapterV1::new(expected_target.clone(), self.qualification.clone())?;
        let actors = revalidate_worker_observations(&lease, observations, &mut adapter)?;
        Ok(AttestedIdentityObservation {
            target: expected_target,
            crop: lease.crop,
            source_width: lease.source_width,
            source_height: lease.source_height,
            frame_sequence: lease.source_frame_sequence,
            device_generation: lease.source_device_generation,
            geometry_epoch: lease.source_geometry_epoch,
            source_frame_qpc: lease.source_frame_qpc,
            qpc_frequency: lease.qpc_frequency,
            captured_at_ms: lease.captured_at_unix_ms,
            content_sha256: lease.content_sha256.clone(),
            actors,
        })
    }

    pub(crate) async fn cancel_and_restart(&self) -> Result<(), IdentityRuntimeError> {
        let mut generation = self.cancellation_generation.lock().await;
        *generation = generation
            .checked_add(1)
            .ok_or(IdentityRuntimeError::GenerationExhausted)?;
        self.actor_locks.revoke(*generation);
        self.worker.cancel_and_restart(*generation).await
    }

    pub(crate) async fn shutdown(&self) {
        let cancellation_generation = {
            let mut generation = self.cancellation_generation.lock().await;
            *generation = generation.saturating_add(1);
            *generation
        };
        self.actor_locks.revoke(cancellation_generation);
        self.worker.shutdown().await;
    }

    async fn cancellation_generation(&self) -> u64 {
        *self.cancellation_generation.lock().await
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QualifiedIdentityAuthorityFingerprintV1 {
    catalog_admission_sha256: String,
    admission_receipt_sha256: String,
    installed_content_tree_sha256: String,
    runtime_tree_sha256: String,
    exact_target_pid: u32,
}

struct QualifiedIdentityWorkerPlanV1 {
    fingerprint: QualifiedIdentityAuthorityFingerprintV1,
    launch: IdentityWorkerLaunchConfig,
    load: PrivateEvaluationLoad,
    admission: QualifiedIdentityActorLockAdmissionV1,
    qualification: PinnedIdentityQualificationV1,
}

struct ActiveQualifiedIdentityRuntimeV1 {
    fingerprint: QualifiedIdentityAuthorityFingerprintV1,
    admission: QualifiedIdentityActorLockAdmissionV1,
    bridge: Arc<IdentityControlBridge<FramedIdentityWorkerTransport>>,
}

/// App-owned native identity service. It is deliberately absent from every
/// Tauri DTO: only this service sees installed paths, interpreter/model
/// bindings, worker handles, pixel leases, qualifications, or activation
/// receipts. The actor-lock bus remains unqualified until the exact admitted
/// worker has launched, authenticated, parent-death-bound, and loaded.
pub(crate) struct QualifiedIdentityRuntimeServiceV1 {
    resources: Arc<LocalResourceManager>,
    broker: MediaBrokerSupervisor,
    parent: RuntimeSupervisor,
    actor_locks: Arc<NativeActorLockBusV1>,
    reconcile_gate: Mutex<()>,
    active: Mutex<Option<ActiveQualifiedIdentityRuntimeV1>>,
    authority_generation: AtomicU64,
    shutdown: CancellationToken,
}

impl QualifiedIdentityRuntimeServiceV1 {
    pub(crate) fn new(
        resources: Arc<LocalResourceManager>,
        broker: MediaBrokerSupervisor,
        parent: RuntimeSupervisor,
        actor_locks: Arc<NativeActorLockBusV1>,
    ) -> Arc<Self> {
        Arc::new(Self {
            resources,
            broker,
            parent,
            actor_locks,
            reconcile_gate: Mutex::new(()),
            active: Mutex::new(None),
            authority_generation: AtomicU64::new(1),
            shutdown: CancellationToken::new(),
        })
    }

    /// Revalidates native authority in the background. Revocation is
    /// synchronous: no old actor lock survives a target, settings, catalog, or
    /// loadout transition while the expensive closed-world checks run.
    pub(crate) fn invalidate_and_reconcile_background(self: &Arc<Self>) {
        self.actor_locks.deactivate_unqualified();
        let generation = self
            .authority_generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map(|previous| previous + 1)
            .unwrap_or(u64::MAX);
        let service = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let _ = service.reconcile_generation(generation).await;
        });
    }

    pub(crate) async fn reconcile_qualified_authority(&self) -> Result<bool, IdentityRuntimeError> {
        let generation = self.authority_generation.load(Ordering::Acquire);
        self.reconcile_generation(generation).await
    }

    async fn reconcile_generation(
        &self,
        expected_generation: u64,
    ) -> Result<bool, IdentityRuntimeError> {
        let _gate = self.reconcile_gate.lock().await;
        if self.shutdown.is_cancelled() {
            self.stop_active().await;
            self.actor_locks.deactivate_unqualified();
            return Ok(false);
        }
        if self.authority_generation.load(Ordering::Acquire) != expected_generation {
            return Ok(false);
        }

        // This method returns no launch on every stale/missing/untrusted case.
        // The LocalResourceManager re-hashes the complete declared runtime tree
        // and exact distributed worker/manifest before yielding the native
        // capability.
        let admitted = match self.resources.admitted_identity_launch() {
            Ok(admitted) => admitted,
            Err(_) => {
                self.stop_active().await;
                self.actor_locks.deactivate_unqualified();
                return Ok(false);
            }
        };
        let plan = qualified_identity_worker_plan(admitted, self.parent.clone())?;

        {
            let active = self.active.lock().await;
            if let Some(active) = active.as_ref() {
                if active.fingerprint == plan.fingerprint {
                    if self.authority_generation.load(Ordering::Acquire) != expected_generation {
                        return Ok(false);
                    }
                    let generation = active.bridge.cancellation_generation().await;
                    self.actor_locks
                        .activate_qualified(&active.admission, generation);
                    return Ok(true);
                }
            }
        }

        self.stop_active().await;
        self.actor_locks.deactivate_unqualified();
        let worker = FramedIdentityWorkerTransport::launch(plan.launch).await?;
        let bridge = Arc::new(IdentityControlBridge::new(
            self.broker.clone(),
            worker,
            plan.qualification,
            Arc::clone(&self.actor_locks),
        ));
        if let Err(error) = bridge.load(&plan.load).await {
            bridge.shutdown().await;
            self.actor_locks.deactivate_unqualified();
            return Err(error);
        }
        if self.shutdown.is_cancelled()
            || self.authority_generation.load(Ordering::Acquire) != expected_generation
        {
            bridge.shutdown().await;
            self.actor_locks.deactivate_unqualified();
            return Ok(false);
        }
        let generation = bridge.cancellation_generation().await;
        self.actor_locks
            .activate_qualified(&plan.admission, generation);
        *self.active.lock().await = Some(ActiveQualifiedIdentityRuntimeV1 {
            fingerprint: plan.fingerprint,
            admission: plan.admission,
            bridge,
        });
        Ok(true)
    }

    pub(crate) async fn observe(
        &self,
        crop: IdentityCrop,
        safety: TurnSafetyContextV1,
    ) -> Result<AttestedIdentityObservation, IdentityRuntimeError> {
        if !self.reconcile_qualified_authority().await? {
            return Err(IdentityRuntimeError::ActorLockNotAdmitted);
        }
        let bridge = self
            .active
            .lock()
            .await
            .as_ref()
            .map(|active| Arc::clone(&active.bridge))
            .ok_or(IdentityRuntimeError::ActorLockNotAdmitted)?;
        bridge.observe(crop, safety).await
    }

    pub(crate) async fn shutdown(&self) {
        self.shutdown.cancel();
        self.actor_locks.deactivate_unqualified();
        let _gate = self.reconcile_gate.lock().await;
        self.stop_active().await;
    }

    async fn stop_active(&self) {
        let active = self.active.lock().await.take();
        if let Some(active) = active {
            active.bridge.shutdown().await;
        }
    }
}

fn qualified_identity_worker_plan(
    admitted: NativeAdmittedIdentityPackLaunchV1,
    parent: RuntimeSupervisor,
) -> Result<QualifiedIdentityWorkerPlanV1, IdentityRuntimeError> {
    let admission = QualifiedIdentityActorLockAdmissionV1::from_admitted_launch(&admitted)?;
    if admitted.placement != npc_model_manager::ResidencyModeV1::CpuResident
        || admitted.backend != "opencv-dnn-cpu"
        || admitted.runtime.trim().is_empty()
        || admitted.runtime_revision.trim().is_empty()
    {
        return Err(IdentityRuntimeError::ActorLockNotAdmitted);
    }
    let fingerprint = QualifiedIdentityAuthorityFingerprintV1 {
        catalog_admission_sha256: admitted.catalog_admission_sha256.as_str().to_owned(),
        admission_receipt_sha256: admitted.admission_receipt_sha256.as_str().to_owned(),
        installed_content_tree_sha256: admitted.installed_content_tree_sha256.as_str().to_owned(),
        runtime_tree_sha256: admitted.runtime_tree_sha256.as_str().to_owned(),
        exact_target_pid: admitted.exact_target_pid,
    };
    let load = PrivateEvaluationLoad {
        lease_id: format!("qualified-{}", admitted.admission_receipt_sha256.as_str()),
        manifest_path: admitted.manifest_file.path.clone(),
        manifest_sha256: admitted.manifest_file.sha256.as_str().to_owned(),
        artifact_root: admitted.artifact_root.clone(),
        cpu_threads: 2,
        explicit_user_confirmation: true,
        activation_mode: IdentityActivationMode::QualifiedCatalog,
        verified_catalog_admission_sha256: Some(
            admitted.catalog_admission_sha256.as_str().to_owned(),
        ),
    };
    validate_private_evaluation_load(&load)?;
    let launch = IdentityWorkerLaunchConfig {
        python_executable: admitted.python_executable.path.clone(),
        python_size_bytes: admitted.python_executable.size_bytes,
        python_sha256: admitted.python_executable.sha256.as_str().to_owned(),
        worker_script: admitted.worker_script.path.clone(),
        worker_script_size_bytes: admitted.worker_script.size_bytes,
        worker_script_sha256: admitted.worker_script.sha256.as_str().to_owned(),
        manifest_path: admitted.manifest_file.path.clone(),
        manifest_size_bytes: admitted.manifest_file.size_bytes,
        manifest_sha256: admitted.manifest_file.sha256.as_str().to_owned(),
        parent,
    };
    Ok(QualifiedIdentityWorkerPlanV1 {
        fingerprint,
        launch,
        load,
        admission,
        qualification: admitted.qualification,
    })
}

impl<W: IdentityReferenceImportTransport> IdentityControlBridge<W> {
    pub(crate) async fn enroll_reference_persisted(
        &self,
        request: IdentityReferenceEnrollmentRequest,
        safety: TurnSafetyContextV1,
        store: &IdentityGalleryStore,
        gallery: &mut QualifiedIdentityGalleryV1,
    ) -> Result<IdentityReferenceEnrollmentReceipt, IdentityRuntimeError> {
        let mut candidate = gallery.clone();
        let mut receipt = self
            .enroll_reference(request, safety, &mut candidate)
            .await?;
        receipt.gallery_generation = store.persist(&candidate)?;
        *gallery = candidate;
        Ok(receipt)
    }

    pub(crate) async fn enroll_reference(
        &self,
        request: IdentityReferenceEnrollmentRequest,
        safety: TurnSafetyContextV1,
        gallery: &mut QualifiedIdentityGalleryV1,
    ) -> Result<IdentityReferenceEnrollmentReceipt, IdentityRuntimeError> {
        let _permit = self
            .one_request
            .try_acquire()
            .map_err(|_| IdentityRuntimeError::Backpressure)?;
        if !self.worker.authenticated() || !self.worker.parent_death_bound() {
            return Err(IdentityRuntimeError::UntrustedWorkerProcess);
        }
        validate_identity_safety(safety)?;
        gallery.validate()?;
        validate_enrollment_request(&request)?;
        if gallery.game_profile_id != request.game_profile_id {
            return Err(IdentityRuntimeError::InvalidReferenceEnrollment);
        }
        let worker = self.worker.identity();
        let picker_consent_token = uuid::Uuid::new_v4().simple().to_string();
        let native_request = IdentityReferenceImportRequest {
            worker: worker.clone(),
            picker_consent_token,
            game_profile_id: request.game_profile_id.clone(),
            character_id: request.character_id.clone(),
            subject_id: request.character_id.clone(),
            reference_id: request.reference_id.clone(),
            subject_display_name: request.subject_display_name.clone(),
            source_class: match request.source_class {
                ReferenceSourceClassV1::UserPrivate => IdentityReferenceSourceClass::UserPrivate,
                ReferenceSourceClassV1::OriginalSynthetic => {
                    IdentityReferenceSourceClass::OriginalSynthetic
                }
            },
            owner_user_id: request.owner_user_id.clone(),
            original_work_license: request.original_work_license.clone(),
            explicit_user_consent: request.explicit_user_consent,
            local_only: true,
            imported_at_unix_ms: request.imported_at_unix_ms,
        };
        let lease = self
            .broker
            .allocate_identity_reference_import(&native_request)
            .await?;
        let provenance = QualifiedReferenceProvenanceV1 {
            game_profile_id: lease.game_profile_id.clone(),
            subject_id: lease.subject_id.clone(),
            reference_id: lease.reference_id.clone(),
            source_class: request.source_class,
            source_content_sha256: lease.content_sha256.clone(),
            owner_user_id: lease.owner_user_id.clone(),
            original_work_license: lease.original_work_license.clone(),
            explicit_user_consent: lease.explicit_user_consent,
            local_only: lease.local_only,
            imported_at_ms: lease.imported_at_unix_ms,
        };
        let extraction = NativeReferenceExtractionV1 {
            provenance,
            subject_display_name: lease.subject_display_name.clone(),
            pixel_lease: ReferencePixelLeaseV1::new(
                lease.lease_id().to_owned(),
                lease.shared_memory_name().to_owned(),
                lease.lease_nonce().to_owned(),
                lease.byte_length,
                lease.width,
                lease.height,
                lease.stride_bytes,
                lease.content_sha256.clone(),
            )?,
        };
        let generation = *self.cancellation_generation.lock().await;
        let result = tokio::time::timeout(
            IDENTITY_INFERENCE_TIMEOUT,
            self.worker.extract_reference(&extraction, generation),
        )
        .await;
        let release = self.broker.release_identity_reference_import(&lease).await;
        let import = match result {
            Ok(Ok(import)) => import,
            Ok(Err(error)) => {
                let _ = release;
                return Err(error);
            }
            Err(_) => {
                let _ = release;
                self.cancel_and_restart().await?;
                return Err(IdentityRuntimeError::Timeout);
            }
        };
        release?;
        gallery.import_reference(import)?;
        let reference_count = gallery
            .gallery
            .subjects
            .values()
            .map(|subject| subject.references.len())
            .sum();
        Ok(IdentityReferenceEnrollmentReceipt {
            gallery_schema_version: gallery.schema_version,
            gallery_generation: 0,
            game_profile_id: lease.game_profile_id,
            character_id: lease.character_id,
            reference_id: lease.reference_id,
            source_asset_sha256: lease.source_asset_sha256,
            normalized_pixel_sha256: lease.content_sha256,
            reference_count,
        })
    }
}

fn valid_enrollment_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_enrollment_request(
    request: &IdentityReferenceEnrollmentRequest,
) -> Result<(), IdentityRuntimeError> {
    let private_rights = request.source_class == ReferenceSourceClassV1::UserPrivate
        && request.explicit_user_consent
        && request
            .owner_user_id
            .as_deref()
            .is_some_and(|owner| valid_enrollment_identifier(owner, 128))
        && request.original_work_license.is_none();
    let original_rights = request.source_class == ReferenceSourceClassV1::OriginalSynthetic
        && request.owner_user_id.is_none()
        && request
            .original_work_license
            .as_deref()
            .is_some_and(|license| !license.trim().is_empty() && license.len() <= 512);
    if !valid_enrollment_identifier(&request.game_profile_id, 128)
        || !valid_enrollment_identifier(&request.character_id, 128)
        || !valid_enrollment_identifier(&request.reference_id, 128)
        || request.subject_display_name.trim().is_empty()
        || request.subject_display_name.len() > 256
        || request.imported_at_unix_ms == 0
        || (!private_rights && !original_rights)
    {
        return Err(IdentityRuntimeError::InvalidReferenceEnrollment);
    }
    Ok(())
}

fn reject_unsafe_gallery_file(path: &Path) -> Result<(), IdentityRuntimeError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| IdentityRuntimeError::GalleryPersistence)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(IdentityRuntimeError::GalleryPersistence);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(IdentityRuntimeError::GalleryPersistence);
        }
    }
    Ok(())
}

fn validate_private_evaluation_load(
    request: &PrivateEvaluationLoad,
) -> Result<(), IdentityRuntimeError> {
    const PACK_ID: &str = "opencv-yunet-sface-private-eval";
    const REVISION: &str = "zoo-47534e27-opencv-5.0.0.93";
    if request.lease_id.is_empty()
        || request.lease_id.len() > 256
        || request.cpu_threads == 0
        || request.cpu_threads > 4
        || !request.explicit_user_confirmation
        || request.manifest_sha256.len() != 64
        || request
            .manifest_sha256
            .bytes()
            .any(|byte| !(byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        || request
            .manifest_path
            .file_name()
            .and_then(|value| value.to_str())
            != Some("opencv-yunet-sface-private-evaluation.json")
        || request
            .artifact_root
            .components()
            .rev()
            .take(2)
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            != vec![REVISION.to_owned(), PACK_ID.to_owned()]
        || match request.activation_mode {
            IdentityActivationMode::PrivateEvaluation => {
                request.verified_catalog_admission_sha256.is_some()
            }
            IdentityActivationMode::QualifiedCatalog => {
                match request.verified_catalog_admission_sha256.as_deref() {
                    None => true,
                    Some(digest) => {
                        digest.len() != 64
                            || digest.bytes().any(|byte| {
                                !(byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                            })
                    }
                }
            }
        }
    {
        return Err(IdentityRuntimeError::PrivateEvaluationGate);
    }
    Ok(())
}

fn validate_identity_safety(safety: TurnSafetyContextV1) -> Result<(), IdentityRuntimeError> {
    safety.validate_admitted()?;
    if safety.evidence_state != TurnSafetyEvidenceStateV1::VerifiedSafe || !safety.visuals_allowed {
        return Err(IdentityRuntimeError::VisualSafetyNotAdmitted);
    }
    Ok(())
}

fn worker_frame_payload(
    lease: &IdentityFrameLease,
) -> Result<serde_json::Value, IdentityRuntimeError> {
    serde_json::to_value(WorkerFrameRequest {
        contract_version: REQUEST_CONTRACT,
        mode: "wgc_frame",
        target: WorkerCaptureTarget {
            capture_session_id: &lease.capture_session_id,
            process_id: lease.selected_process_id,
            window_handle: lease.selected_window_handle,
            executable_name: &lease.selected_executable_name,
        },
        frame_sequence: lease.source_frame_sequence,
        device_generation: lease.source_device_generation,
        geometry_epoch: lease.source_geometry_epoch,
        source_frame_qpc: lease.source_frame_qpc,
        qpc_frequency: lease.qpc_frequency,
        captured_at_ms: lease.captured_at_unix_ms,
        content_sha256: &lease.content_sha256,
        advancing_frame_verified: lease.advancing_frame_verified,
        overlay_capture_excluded: lease.overlay_capture_excluded,
        protected_online_detected: lease.protected_online_detected,
        anti_cheat_detected: lease.anti_cheat_detected,
        pixel_lease: WorkerPixelLease {
            lease_id: lease.lease_id(),
            shared_memory_name: lease.shared_memory_name(),
            lease_nonce: lease.lease_nonce(),
            byte_length: lease.byte_length,
            width: lease.width,
            height: lease.height,
            stride_bytes: lease.stride_bytes,
            pixel_format: &lease.pixel_format,
            content_sha256: &lease.content_sha256,
        },
    })
    .map_err(|_| IdentityRuntimeError::MalformedWorkerFrame)
}

fn revalidate_worker_observations(
    lease: &IdentityFrameLease,
    mut worker: UntrustedWorkerObservationsV1,
    adapter: &mut TrustedWgcEvidenceAdapterV1,
) -> Result<FrameActorsV1, IdentityRuntimeError> {
    let expected_target = TrustedCaptureTargetV1 {
        capture_session_id: lease.capture_session_id.clone(),
        process_id: lease.selected_process_id,
        window_handle: lease.selected_window_handle,
        executable_name: lease.selected_executable_name.clone(),
    };
    if worker.contract_version != OBSERVATION_CONTRACT
        || worker.authority != "untrusted_worker_observations"
        || !worker.native_revalidation_required
        || worker.schema_version != QUALIFIED_IDENTITY_SCHEMA_VERSION
        || worker.target != expected_target
        || worker.frame_sequence != lease.source_frame_sequence
        || worker.device_generation != lease.source_device_generation
        || worker.geometry_epoch != lease.source_geometry_epoch
        || worker.source_frame_qpc != lease.source_frame_qpc
        || worker.qpc_frequency != lease.qpc_frequency
        || worker.captured_at_ms != lease.captured_at_unix_ms
        || worker.content_sha256 != lease.content_sha256
        || worker.advancing_frame_verified != lease.advancing_frame_verified
        || worker.overlay_capture_excluded != lease.overlay_capture_excluded
        || worker.protected_online_detected != lease.protected_online_detected
        || worker.anti_cheat_detected != lease.anti_cheat_detected
    {
        return Err(IdentityRuntimeError::EvidenceMismatch);
    }
    translate_crop_observations(lease, &mut worker.observations)?;
    adapter
        .adapt(TrustedWgcIdentityFrameV1 {
            schema_version: worker.schema_version,
            target: worker.target,
            frame_sequence: worker.frame_sequence,
            device_generation: worker.device_generation,
            geometry_epoch: worker.geometry_epoch,
            source_frame_qpc: worker.source_frame_qpc,
            qpc_frequency: worker.qpc_frequency,
            captured_at_ms: worker.captured_at_ms,
            content_sha256: worker.content_sha256,
            advancing_frame_verified: worker.advancing_frame_verified,
            overlay_capture_excluded: worker.overlay_capture_excluded,
            protected_online_detected: worker.protected_online_detected,
            anti_cheat_detected: worker.anti_cheat_detected,
            observations: worker.observations,
        })
        .map_err(Into::into)
}

fn translate_crop_observations(
    lease: &IdentityFrameLease,
    observations: &mut [ActorDetectionV1],
) -> Result<(), IdentityRuntimeError> {
    let crop_width = lease.width as f32;
    let crop_height = lease.height as f32;
    let dx = lease.crop.left as f32;
    let dy = lease.crop.top as f32;
    for observation in observations {
        let bounds = &mut observation.bounds;
        if bounds.x < 0.0
            || bounds.y < 0.0
            || bounds.x + bounds.width > crop_width
            || bounds.y + bounds.height > crop_height
        {
            return Err(IdentityRuntimeError::ObservationOutsideCrop);
        }
        bounds.x += dx;
        bounds.y += dy;
        if let Some(embedding) = &mut observation.embedding {
            let embedded = &mut embedding.metadata.crop_bounds;
            if embedded.x < 0.0
                || embedded.y < 0.0
                || embedded.x + embedded.width > crop_width
                || embedded.y + embedded.height > crop_height
            {
                return Err(IdentityRuntimeError::ObservationOutsideCrop);
            }
            embedded.x += dx;
            embedded.y += dy;
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum IdentityRuntimeError {
    #[error("identity worker queue is full")]
    Backpressure,
    #[error("identity worker exceeded its bounded inference deadline")]
    Timeout,
    #[error("identity private-evaluation model load was not explicitly authorized")]
    PrivateEvaluationGate,
    #[error("identity worker frame payload was malformed")]
    MalformedWorkerFrame,
    #[error("identity worker observations did not match the broker frame evidence")]
    EvidenceMismatch,
    #[error("identity worker observation escaped the broker-issued crop")]
    ObservationOutsideCrop,
    #[error("identity cancellation generation is exhausted")]
    GenerationExhausted,
    #[error("identity worker is not authenticated and parent-death bound")]
    UntrustedWorkerProcess,
    #[error("identity observation is unavailable outside verified visual-safe targeting")]
    VisualSafetyNotAdmitted,
    #[error("identity reference enrollment scope or rights are invalid")]
    InvalidReferenceEnrollment,
    #[error("identity gallery could not be loaded or persisted privately")]
    GalleryPersistence,
    #[error("identity gallery does not match the active pinned qualification")]
    GalleryQualificationMismatch,
    #[error("identity actor lock is unavailable without exact qualified-catalog admission")]
    ActorLockNotAdmitted,
    #[error("identity actor lock did not match the attested selected tracker result")]
    ActorLockEvidenceMismatch,
    #[error("manual actor click did not match the sealed native picker evidence")]
    ManualActorClickEvidenceMismatch,
    #[error("manual actor click receipt has already been consumed")]
    ManualActorClickReceiptReplayed,
    #[error("identity tracker rejected the attested frame: {0}")]
    IdentityEngine(String),
    #[error(transparent)]
    Broker(#[from] MediaBrokerError),
    #[error(transparent)]
    Qualification(#[from] QualifiedIdentityError),
    #[error(transparent)]
    Safety(#[from] TurnSafetyContextErrorV1),
    #[error("identity worker transport failed safely: {0}")]
    Transport(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use npc_identity_engine::{
        ActorIdentityV1, ActorTrackSnapshotV1, EmbeddingModelV1, IdentityConfigV1, TrackEpoch,
        TrackId,
    };
    use zeroize::Zeroizing;

    fn fixture_lease() -> IdentityFrameLease {
        let lease_id = "00112233445566778899aabbccddeeff";
        let lease_nonce = "ffeeddccbbaa99887766554433221100";
        IdentityFrameLease {
            schema_version: 1,
            worker: IdentityWorkerIdentity {
                process_id: 445,
                process_creation_time: 0x2233_4455,
                executable_name: "python.exe".into(),
            },
            lease_id: Zeroizing::new(lease_id.into()),
            shared_memory_name: Zeroizing::new(crate::media_broker::identity_mapping_name(
                lease_id,
                lease_nonce,
            )),
            lease_nonce: Zeroizing::new(lease_nonce.into()),
            byte_length: 800 * 480 * 4,
            width: 800,
            height: 480,
            stride_bytes: 800 * 4,
            pixel_format: "b8g8r8a8_unorm".into(),
            content_sha256: "c6d853177eb0a3fa2d55c09b981550e449a0715853928e84274e54cbb01f7ea2"
                .into(),
            expires_qpc: 9_000,
            qpc_frequency: 10_000_000,
            cancellation_generation: 3,
            capture_session_id: "native-capture-session-1".into(),
            selected_process_id: 42,
            selected_window_handle: 777,
            selected_executable_name: "EclipseHarbor.exe".into(),
            source_device_generation: 1,
            source_geometry_epoch: 1,
            source_frame_sequence: 1,
            source_frame_qpc: 10_001,
            captured_at_unix_ms: 16,
            advancing_frame_verified: true,
            overlay_capture_excluded: true,
            protected_online_detected: false,
            anti_cheat_detected: false,
            crop: IdentityCrop {
                left: 32,
                top: 48,
                right: 832,
                bottom: 528,
            },
            source_width: 1280,
            source_height: 720,
        }
    }

    fn fixture_qualification() -> PinnedIdentityQualificationV1 {
        PinnedIdentityQualificationV1 {
            qualification_id: "identity-private-evaluation-fixture".into(),
            model: EmbeddingModelV1 {
                provider: "opencv-zoo".into(),
                model_id: "sface-2021dec-mobilefacenet".into(),
                revision: "sha256:fixture-sface-revision".into(),
                dimensions: 128,
            },
            detector_id: "opencv-zoo-yunet".into(),
            detector_revision: "sha256:fixture-yunet-revision".into(),
            preprocessing: "opencv-face-recognizer-sf-aligncrop-bgr-112x112-l2-f32-v1".into(),
            calibration_fixture_sha256: "ab".repeat(32),
            calibrated_config: IdentityConfigV1::default(),
        }
    }

    fn fixture_admission(exact_target_pid: u32) -> QualifiedIdentityActorLockAdmissionV1 {
        QualifiedIdentityActorLockAdmissionV1 {
            qualification: fixture_qualification(),
            catalog_admission_sha256: "cd".repeat(32),
            admission_receipt_sha256: "ef".repeat(32),
            exact_target_pid,
        }
    }

    fn fixture_tracker_result(character_id: &str) -> AdmittedIdentityTrackerResultV1 {
        let lease = fixture_lease();
        AdmittedIdentityTrackerResultV1 {
            observation: AttestedIdentityObservation {
                target: TrustedCaptureTargetV1 {
                    capture_session_id: lease.capture_session_id,
                    process_id: lease.selected_process_id,
                    window_handle: lease.selected_window_handle,
                    executable_name: lease.selected_executable_name,
                },
                crop: lease.crop,
                source_width: lease.source_width,
                source_height: lease.source_height,
                frame_sequence: lease.source_frame_sequence,
                device_generation: lease.source_device_generation,
                geometry_epoch: lease.source_geometry_epoch,
                source_frame_qpc: lease.source_frame_qpc,
                qpc_frequency: lease.qpc_frequency,
                captured_at_ms: lease.captured_at_unix_ms,
                content_sha256: lease.content_sha256,
                actors: FrameActorsV1 {
                    frame_index: 1,
                    timestamp_ms: 16,
                    detections: Vec::new(),
                },
            },
            update: FrameIdentityUpdateV1 {
                frame_index: 1,
                actors: vec![ActorIdentityV1 {
                    track: ActorTrackSnapshotV1 {
                        track_id: TrackId(41),
                        track_epoch: TrackEpoch(3),
                        phase: TrackPhaseV1::Visible,
                        bounds: BoundingBoxV1::new(320.0, 180.0, 160.0, 240.0)
                            .expect("full-source bounds"),
                        velocity_x_per_frame: 0.0,
                        velocity_y_per_frame: 0.0,
                        first_seen_frame: 1,
                        last_seen_frame: 1,
                        missed_frames: 0,
                        selected: true,
                        encounter_id: "eclipse-harbor/encounter-7/actor-41".into(),
                    },
                    identity: IdentityDecisionV1::Matched {
                        encounter_id: "eclipse-harbor/encounter-7/actor-41".into(),
                        subject_id: character_id.into(),
                        subject_similarity: 0.91,
                        top_candidate_subject_id: character_id.into(),
                        top_candidate_similarity: 0.91,
                        runner_up_similarity: Some(0.55),
                        top1_top2_margin: 0.36,
                        supporting_frames: 5,
                        window_frames: 5,
                        held_by_hysteresis: true,
                    },
                }],
                track_events: Vec::new(),
                selected_track_id: Some(TrackId(41)),
            },
            appearance: NativeTrackerAppearanceEvidenceV1::from_native_tracker(
                3, 0x1122, 0x3344, 0x5566, 0x7788, 0.91, 0.84, 0.07, false,
            )
            .expect("native tracker appearance evidence"),
        }
    }

    #[test]
    fn actor_lock_is_native_qualified_immutable_and_generation_bounded() {
        let bus = NativeActorLockBusV1::new_unqualified();
        let receiver = bus.subscribe();
        let admission = fixture_admission(42);
        let result = fixture_tracker_result("mara");
        assert!(matches!(
            bus.publish_from_admitted_tracker(
                &admission,
                "eclipse-harbor",
                "mara",
                &result,
                200,
                7,
            ),
            Err(IdentityRuntimeError::ActorLockNotAdmitted)
        ));

        bus.activate_qualified(&admission, 7);
        let selected = bus
            .publish_from_admitted_tracker(&admission, "eclipse-harbor", "mara", &result, 200, 7)
            .expect("selected actor lock");
        assert_eq!(selected.schema_version, ACTOR_LOCK_SCHEMA_VERSION);
        assert_eq!(selected.lock_generation, 1);
        assert_eq!(selected.selected_process_id, 42);
        assert_eq!(selected.selected_window_handle, 777);
        assert_eq!(selected.character_id, "mara");
        assert!(matches!(
            selected.provenance,
            NativeActorLockProvenanceV1::QualifiedIdentity { .. }
        ));
        assert_eq!(
            selected.selection_authority,
            NativeActorSelectionAuthorityV1::Consensus
        );
        assert_eq!(selected.actor_id, "eclipse-harbor/encounter-7/actor-41");
        assert_eq!(selected.runtime_actor_id, 1);
        assert_eq!(selected.track_id, 41);
        assert_eq!(selected.track_epoch, 3);
        assert_eq!(selected.full_source_roi.x, 0.25);
        assert_eq!(selected.full_source_roi.y, 0.25);
        assert!(selected.appearance_hysteresis_latched);
        assert_eq!(selected.appearance_descriptor_revision, 3);
        assert_eq!(selected.appearance_similarity, 0.91);
        assert_eq!(selected.temporal_iou, 0.84);
        assert_eq!(selected.blocker_coverage, 0.07);
        assert!(!selected.scene_transition_detected);
        assert_eq!(selected.identity_confidence, 0.91);
        assert_eq!(selected.identity_margin, 0.36);
        assert!(receiver.has_changed().expect("watch remains open"));
        assert!(bus.selected_if_current(199, 7).is_some());
        assert!(bus.selected_if_current(200, 7).is_none());
        assert!(bus.selected_if_current(199, 8).is_none());
        bus.revoke(8);
        assert!(matches!(
            bus.snapshot(),
            NativeActorLockStateV1::QualifiedNoSelection {
                cancellation_generation: 8,
                ..
            }
        ));
    }

    #[test]
    fn actor_lock_rejects_character_frame_roi_and_admission_mismatches() {
        let bus = NativeActorLockBusV1::new_unqualified();
        let admission = fixture_admission(42);
        bus.activate_qualified(&admission, 9);

        let wrong_character = fixture_tracker_result("not-mara");
        assert!(matches!(
            bus.publish_from_admitted_tracker(
                &admission,
                "eclipse-harbor",
                "mara",
                &wrong_character,
                200,
                9,
            ),
            Err(IdentityRuntimeError::ActorLockEvidenceMismatch)
        ));
        assert!(matches!(
            bus.snapshot(),
            NativeActorLockStateV1::QualifiedNoSelection {
                cancellation_generation: 9,
                ..
            }
        ));
        bus.publish_from_admitted_tracker(
            &admission,
            "eclipse-harbor",
            "mara",
            &fixture_tracker_result("mara"),
            200,
            9,
        )
        .expect("valid lock before selected-track loss");
        let mut lost = fixture_tracker_result("mara");
        lost.update.selected_track_id = None;
        assert!(bus
            .publish_from_admitted_tracker(&admission, "eclipse-harbor", "mara", &lost, 200, 9,)
            .is_err());
        assert!(bus.selected_if_current(199, 9).is_none());
        let mut stale = fixture_tracker_result("mara");
        stale.update.frame_index = 2;
        assert!(bus
            .publish_from_admitted_tracker(&admission, "eclipse-harbor", "mara", &stale, 200, 9,)
            .is_err());
        let mut escaped = fixture_tracker_result("mara");
        escaped.update.actors[0].track.bounds.x = 1_279.0;
        assert!(bus
            .publish_from_admitted_tracker(&admission, "eclipse-harbor", "mara", &escaped, 200, 9,)
            .is_err());
        assert!(bus
            .publish_from_admitted_tracker(
                &admission,
                "eclipse-harbor",
                "mara",
                &fixture_tracker_result("mara"),
                200,
                10,
            )
            .is_err());

        let mut different_receipt = admission.clone();
        different_receipt.admission_receipt_sha256 = "aa".repeat(32);
        assert!(matches!(
            bus.publish_from_admitted_tracker(
                &different_receipt,
                "eclipse-harbor",
                "mara",
                &fixture_tracker_result("mara"),
                200,
                9,
            ),
            Err(IdentityRuntimeError::ActorLockNotAdmitted)
        ));

        let mut wrong_process = fixture_tracker_result("mara");
        wrong_process.observation.target.process_id = 43;
        assert!(matches!(
            bus.publish_from_admitted_tracker(
                &admission,
                "eclipse-harbor",
                "mara",
                &wrong_process,
                200,
                9,
            ),
            Err(IdentityRuntimeError::ActorLockEvidenceMismatch)
        ));
        assert!(bus.selected_if_current(199, 9).is_none());
    }

    fn manual_click_request_fixture() -> NativeManualActorPickerRequestV1 {
        NativeManualActorPickerRequestV1 {
            request_id: "native-actor-click-1".into(),
            visual_pack_id: "openseeface-mnv3-lm1-mouth-signal".into(),
            visual_pack_admission_sha256: "a".repeat(64),
            game_profile_id: "eclipse-harbor".into(),
            capture_session_id: "capture-session-1".into(),
            cancellation_generation: 7,
            selected_process_id: 42,
            selected_window_handle: 777,
            selected_executable_name: "synthetic-game.exe".into(),
            source_device_generation: 3,
            source_geometry_epoch: 4,
            source_frame_sequence: 50,
            source_frame_qpc: 50_000,
            qpc_frequency: 10_000_000,
            captured_at_unix_ms: 1_000,
            expires_at_unix_ms: 1_200,
            source_width: 1_920,
            source_height: 1_080,
            timeout_ms: 5_000,
            candidates: vec![crate::media_broker::NativeManualActorCandidateV1 {
                actor_id: 10,
                track_id: 20,
                track_epoch: 30,
                left: 0.25,
                top: 0.2,
                right: 0.5,
                bottom: 0.7,
            }],
        }
    }

    fn manual_click_receipt_fixture(
        request: &NativeManualActorPickerRequestV1,
    ) -> NativeManualActorPickerReceiptV1 {
        NativeManualActorPickerReceiptV1 {
            schema_version: 1,
            request_id: request.request_id.clone(),
            status: NativeManualActorPickerStatusV1::Selected,
            receipt_nonce_high: 11,
            receipt_nonce_low: 12,
            capture_session_id: request.capture_session_id.clone(),
            cancellation_generation: request.cancellation_generation,
            selected_process_id: request.selected_process_id,
            selected_window_handle: request.selected_window_handle,
            selected_executable_name: request.selected_executable_name.clone(),
            source_device_generation: request.source_device_generation,
            source_geometry_epoch: request.source_geometry_epoch,
            source_frame_sequence: request.source_frame_sequence,
            source_frame_qpc: request.source_frame_qpc,
            selected_actor_id: request.candidates[0].actor_id,
            selected_track_id: request.candidates[0].track_id,
            selected_track_epoch: request.candidates[0].track_epoch,
            candidate_count: request.candidates.len() as u32,
            candidate_set_sha256: crate::media_broker::manual_actor_candidate_set_sha256(
                &request.candidates,
            ),
            began_qpc: 51_000,
            clicked_qpc: 52_000,
            attested_at_qpc: 52_100,
            qpc_frequency: request.qpc_frequency,
            pointer_kind: 1,
            frozen_wgc_frame_verified: true,
            overlay_capture_excluded: true,
            overlay_nonactivating: true,
            single_hardware_pointer_click: true,
            pixels_withheld_from_webview: true,
            coordinates_withheld_from_webview: true,
            receipt_sha256: "b".repeat(64),
        }
    }

    #[test]
    fn sealed_native_click_is_distinct_exact_and_single_consume() {
        let bus = NativeActorLockBusV1::new_unqualified();
        let request = manual_click_request_fixture();
        let receipt = manual_click_receipt_fixture(&request);
        let selected = bus
            .publish_from_sealed_native_click(&request, &receipt, 1_100, 7)
            .expect("sealed click actor lock");
        assert_eq!(
            selected.selection_authority,
            NativeActorSelectionAuthorityV1::SealedNativeClick
        );
        assert!(selected.character_id.is_empty());
        assert_eq!(selected.track_id, 20);
        assert_eq!(selected.track_epoch, 30);
        assert_eq!(selected.full_source_roi.x, 0.25);
        assert_eq!(selected.full_source_roi.width, 0.25);
        assert!(matches!(
            &selected.provenance,
            NativeActorLockProvenanceV1::SealedNativeClick {
                visual_pack_id,
                candidate_set_sha256,
                ..
            } if visual_pack_id == "openseeface-mnv3-lm1-mouth-signal"
                && candidate_set_sha256 == &receipt.candidate_set_sha256
        ));
        assert!(matches!(
            bus.publish_from_sealed_native_click(&request, &receipt, 1_101, 7),
            Err(IdentityRuntimeError::ManualActorClickReceiptReplayed)
        ));
    }

    #[test]
    fn sealed_native_click_rejects_stale_or_mismatched_provenance() {
        let request = manual_click_request_fixture();
        let receipt = manual_click_receipt_fixture(&request);
        assert!(matches!(
            NativeActorLockBusV1::new_unqualified()
                .publish_from_sealed_native_click(&request, &receipt, 1_100, 8),
            Err(IdentityRuntimeError::ManualActorClickEvidenceMismatch)
        ));
        let mut stale_geometry = receipt.clone();
        stale_geometry.source_geometry_epoch += 1;
        assert!(NativeActorLockBusV1::new_unqualified()
            .publish_from_sealed_native_click(&request, &stale_geometry, 1_100, 7)
            .is_err());
        let mut wrong_candidates = request.clone();
        wrong_candidates.candidates[0].left = 0.3;
        assert!(NativeActorLockBusV1::new_unqualified()
            .publish_from_sealed_native_click(&wrong_candidates, &receipt, 1_100, 7)
            .is_err());
        assert!(NativeActorLockBusV1::new_unqualified()
            .publish_from_sealed_native_click(&request, &receipt, 1_200, 7)
            .is_err());
    }

    #[test]
    fn private_evaluation_load_requires_exact_v2_manifest_and_confirmation() {
        let valid = PrivateEvaluationLoad {
            lease_id: "identity-model-lease-01".into(),
            manifest_path: "packaging/model-packs/opencv-yunet-sface-private-evaluation.json"
                .into(),
            manifest_sha256: "a4af4874af77c4dc517fe41990371e96e68a4469ee6817102876b7b074302e67"
                .into(),
            artifact_root: "packs/opencv-yunet-sface-private-eval/zoo-47534e27-opencv-5.0.0.93"
                .into(),
            cpu_threads: 2,
            explicit_user_confirmation: true,
            activation_mode: IdentityActivationMode::PrivateEvaluation,
            verified_catalog_admission_sha256: None,
        };
        assert!(validate_private_evaluation_load(&valid).is_ok());
        let mut unconfirmed = valid.clone();
        unconfirmed.explicit_user_confirmation = false;
        assert!(validate_private_evaluation_load(&unconfirmed).is_err());
        let mut legacy_manifest = valid;
        legacy_manifest.manifest_path = "workers/local-identity/old.manifest.json".into();
        assert!(validate_private_evaluation_load(&legacy_manifest).is_err());

        let mut qualified = legacy_manifest;
        qualified.manifest_path =
            "packaging/model-packs/opencv-yunet-sface-private-evaluation.json".into();
        qualified.activation_mode = IdentityActivationMode::QualifiedCatalog;
        assert!(validate_private_evaluation_load(&qualified).is_err());
        qualified.verified_catalog_admission_sha256 =
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into());
        assert!(validate_private_evaluation_load(&qualified).is_ok());
    }

    #[test]
    fn reference_command_accepts_only_private_or_original_rights_and_native_timestamp() {
        let private: IdentityReferenceEnrollmentCommandRequest =
            serde_json::from_value(serde_json::json!({
                "gameProfileId": "eclipse-harbor",
                "characterId": "mara",
                "referenceId": "mara-private-01",
                "subjectDisplayName": "Mara",
                "sourceClass": "user_private",
                "ownerUserId": "local-user",
                "originalWorkLicense": null,
                "explicitUserConsent": true
            }))
            .expect("bounded private request");
        let native = private.into_native().expect("native private rights");
        assert!(native.imported_at_unix_ms > 0);
        assert_eq!(native.owner_user_id.as_deref(), Some("local-user"));

        let original: IdentityReferenceEnrollmentCommandRequest =
            serde_json::from_value(serde_json::json!({
                "gameProfileId": "eclipse-harbor",
                "characterId": "mara",
                "referenceId": "mara-original-01",
                "subjectDisplayName": "Mara",
                "sourceClass": "original_synthetic",
                "ownerUserId": null,
                "originalWorkLicense": "CC0-1.0",
                "explicitUserConsent": false
            }))
            .expect("bounded original request");
        original.into_native().expect("native original rights");

        let mixed: IdentityReferenceEnrollmentCommandRequest =
            serde_json::from_value(serde_json::json!({
                "gameProfileId": "eclipse-harbor",
                "characterId": "mara",
                "referenceId": "mara-mixed-01",
                "subjectDisplayName": "Mara",
                "sourceClass": "user_private",
                "ownerUserId": "local-user",
                "originalWorkLicense": "CC0-1.0",
                "explicitUserConsent": true
            }))
            .expect("shape parses before rights validation");
        assert!(mixed.into_native().is_err());
        assert!(
            serde_json::from_value::<IdentityReferenceEnrollmentCommandRequest>(
                serde_json::json!({
                    "gameProfileId": "eclipse-harbor",
                    "characterId": "mara",
                    "referenceId": "mara-private-01",
                    "subjectDisplayName": "Mara",
                    "sourceClass": "user_private",
                    "ownerUserId": "local-user",
                    "originalWorkLicense": null,
                    "explicitUserConsent": true,
                    "path": "C:/private/reference.png"
                })
            )
            .is_err()
        );
    }

    #[test]
    fn gallery_store_is_private_game_scoped_atomic_and_qualification_pinned() {
        let directory = tempfile::tempdir().expect("gallery parent");
        let store = IdentityGalleryStore::new(directory.path()).expect("private gallery store");
        let qualification = fixture_qualification();
        let gallery = store
            .load_or_create("eclipse-harbor", &qualification)
            .expect("new pinned gallery");
        assert_eq!(
            store.persist(&gallery).expect("atomic gallery persistence"),
            1
        );
        let reopened = store
            .load_or_create("eclipse-harbor", &qualification)
            .expect("reopen exact qualification");
        assert_eq!(reopened, gallery);
        assert_eq!(
            store
                .persist(&reopened)
                .expect("atomic replacement of existing gallery"),
            2
        );

        let mut changed = qualification;
        changed.qualification_id = "different-qualified-model".into();
        assert!(store.load_or_create("eclipse-harbor", &changed).is_err());
        assert!(store.load_or_create("../other-game", &changed).is_err());
    }

    #[test]
    fn cross_language_observation_fixture_deserializes_as_untrusted_only() {
        let mut fixture: UntrustedWorkerObservationsV1 = serde_json::from_str(include_str!(
            "../../../../fixtures/identity/worker-observations.v1.json"
        ))
        .expect("canonical worker fixture");
        assert_eq!(fixture.contract_version, OBSERVATION_CONTRACT);
        assert_eq!(fixture.authority, "untrusted_worker_observations");
        assert!(fixture.native_revalidation_required);
        assert_eq!(fixture.target.process_id, 42);
        assert_eq!(fixture.target.window_handle, 777);
        assert_eq!(fixture.frame_sequence, 1);
        assert!(fixture.overlay_capture_excluded);
        assert!(!fixture.protected_online_detected);
        assert!(!fixture.anti_cheat_detected);
        let original_x = fixture.observations[0].bounds.x;
        let original_crop_x = fixture.observations[0]
            .embedding
            .as_ref()
            .expect("fixture embedding")
            .metadata
            .crop_bounds
            .x;
        translate_crop_observations(&fixture_lease(), &mut fixture.observations)
            .expect("bounded crop translation");
        assert_eq!(fixture.observations[0].bounds.x, original_x + 32.0);
        assert_eq!(
            fixture.observations[0]
                .embedding
                .as_ref()
                .expect("fixture embedding")
                .metadata
                .crop_bounds
                .x,
            original_crop_x + 32.0
        );
        let mut escaped = fixture.observations.clone();
        escaped[0].bounds.x = 799.0;
        escaped[0].bounds.width = 2.0;
        assert!(matches!(
            translate_crop_observations(&fixture_lease(), &mut escaped),
            Err(IdentityRuntimeError::ObservationOutsideCrop)
        ));
    }

    #[test]
    fn console_unknown_and_detected_risk_cannot_allocate_identity_frames() {
        assert!(
            validate_identity_safety(TurnSafetyContextV1::verified_console_isolated()).is_err()
        );
        assert!(validate_identity_safety(TurnSafetyContextV1::default()).is_err());
        assert!(validate_identity_safety(TurnSafetyContextV1 {
            protected_online_detected: true,
            visuals_allowed: true,
            ..TurnSafetyContextV1::verified_safe()
        })
        .is_err());
        assert!(validate_identity_safety(TurnSafetyContextV1 {
            anti_cheat_detected: true,
            visuals_allowed: true,
            ..TurnSafetyContextV1::verified_safe()
        })
        .is_err());
        assert!(validate_identity_safety(TurnSafetyContextV1 {
            visuals_allowed: true,
            ..TurnSafetyContextV1::verified_safe()
        })
        .is_ok());
    }
}
