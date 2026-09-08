use crate::optional_pack_activation::{
    activate_trusted_yunet_provider_pack_v1, SystemProviderLoadSelfTestClockV1,
    TrustedProviderLoadActivationReceiptV1, TrustedProviderLoadSelfTestProbeV1,
};
use npc_identity_engine::PinnedIdentityQualificationV1;
use npc_model_manager::{
    load_release_catalog_bundle_v1, openseeface_visual_signal_contract_v1,
    openseeface_visual_signal_manifest_v1, verify_artifact, verify_release_catalog_bundle_v1,
    verify_release_envelope_files_v1, verify_release_manifest_files_v1, ArchivePolicy,
    CancellationReasonV1, CatalogTrustDomainV1, CatalogTrustState, DownloadPolicy,
    Ed25519CatalogVerifier, FileDownloadJournalStore, FilesystemPackStorage,
    HttpsArtifactDownloader, InstallState, InstalledPackInventoryV1, LoadoutAdmissionV1,
    MeasuredLocalPackSelectionPolicyV1, MeasuredResidencyDecisionV1, MeasurementTrustPolicyV1,
    ModelPackKindV1, NativeAdmissionContextV1, PackRevision, PackSelectionActionV1,
    PackSelectionOriginV1, PackSelectionRequestV1, ReleaseCatalogTrustScopeV1, ResidencyModeV1,
    ResourceGovernorPolicyV1, ResourceGovernorV1, ResourcePressureLevelV1,
    SelectedLoadoutDecisionV1, SelectedLoadoutManagerV1, SelectedLoadoutPlannerSnapshotV1,
    SelectedLoadoutSelectionV1, Sha256Digest, SignedMeasuredResourceEnvelopeV1,
    SignedResourceEnvelopeSourceV1, TrustedOptionalPackLifecycleV1, TrustedReleasePackSnapshotV1,
    VisualWorkAddressV1, WorkCancellationV1, WorkItemV1, WorkKindV1, WorkQueuePolicyV1,
    OPENSEEFACE_VISUAL_SIGNAL_PACK_ID, OPENSEEFACE_VISUAL_SIGNAL_REVISION,
};
use npc_system_telemetry::{collect, ResourceTelemetrySnapshotV1, TelemetryRequest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

const SETTINGS_FILE_NAME: &str = "local-resource-policy-v1.json";
const PACK_STATE_FILE_NAME: &str = "experimental-visual-pack-v1.json";
const SELECTED_LOADOUT_FILE_NAME: &str = "selected-local-loadout-v1.json";
const MAX_TRUST_METADATA_BYTES: u64 = 8 * 1024 * 1024;
const MAX_VISUAL_DEADLINE_MILLIS: u64 = 2_000;
const IDENTITY_PACK_ID: &str = "opencv-yunet-sface-private-eval";
const IDENTITY_PACK_REVISION: &str = "zoo-47534e27-opencv-5.0.0.93";
const IDENTITY_RUNTIME_AUTHORITY_SCHEMA_V1: &str = "npc.identity-runtime-authority/v1";
const YUNET_OPENSEEFACE_VISUAL_SIGNAL_PACK_ID: &str = "openseeface-yunet640-lm1-mouth-signal";
const YUNET_OPENSEEFACE_DETECTOR_PATH: &str = "models/face_detection_yunet_2023mar.onnx";
const YUNET_OPENSEEFACE_DETECTOR_SHA256: &str =
    "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4";
const OPENSEEFACE_LM1_PATH: &str = "models/lm_model1_opt.onnx";
const OPENSEEFACE_LM1_SHA256: &str =
    "5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalResourceSettings {
    pub schema_version: u32,
    pub governor: ResourceGovernorPolicyV1,
    pub game_reserve_vram_bytes: u64,
    pub game_additional_reserve_ram_bytes: u64,
    pub preferred_residency: ResidencyModeV1,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LocalResourceSettingsPersistenceV1 {
    NativePersisted,
    BuiltInDefault,
    RecoveredDefault,
}

impl Default for LocalResourceSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            governor: ResourceGovernorPolicyV1::default(),
            game_reserve_vram_bytes: 4 * 1024 * 1024 * 1024,
            game_additional_reserve_ram_bytes: 2 * 1024 * 1024 * 1024,
            preferred_residency: ResidencyModeV1::CpuResidentGpuCold,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentalPackPhase {
    NotInstalled,
    Downloading,
    InstalledInactive,
    ActivationBlockedMissingMeasuredEnvelope,
    RepairRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExperimentalPackState {
    pub schema_version: u32,
    pub pack_id: String,
    pub revision: String,
    pub phase: ExperimentalPackPhase,
    pub installed_artifact_sha256: Vec<String>,
    pub trust_domain: String,
    pub experimental: bool,
    pub explicit_download_required: bool,
    pub complete_lip_sync_model: bool,
    pub detail: String,
}

impl Default for ExperimentalPackState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            pack_id: OPENSEEFACE_VISUAL_SIGNAL_PACK_ID.into(),
            revision: OPENSEEFACE_VISUAL_SIGNAL_REVISION.into(),
            phase: ExperimentalPackPhase::NotInstalled,
            installed_artifact_sha256: vec![],
            trust_domain: "localReviewDevOnly".into(),
            experimental: true,
            explicit_download_required: true,
            complete_lip_sync_model: false,
            detail: "Not installed. This optional CPU face/landmark signal dependency is not a complete lip-sync model.".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExperimentalPackMutationRequest {
    pub pack_id: String,
    pub revision: String,
    pub explicit_user_confirmation: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTelemetryResult {
    pub settings: LocalResourceSettings,
    pub snapshot: ResourceTelemetrySnapshotV1,
    pub admission_ready: bool,
    pub admission_detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedLoadoutPlannerResult {
    pub schema_version: u32,
    pub ready: bool,
    pub detail: String,
    pub selected: Option<SelectedLoadoutSelectionV1>,
    pub planner: Option<SelectedLoadoutPlannerSnapshotV1>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedLoadoutAdmissionResult {
    pub schema_version: u32,
    pub ready: bool,
    pub detail: String,
    pub persisted: bool,
    pub decision: Option<SelectedLoadoutDecisionV1>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedLocalPackCatalogResult {
    pub schema_version: u32,
    pub ready: bool,
    pub detail: String,
    pub trust_scope: Option<ReleaseCatalogTrustScopeV1>,
    pub production_trust: bool,
    pub rotation_required_before_release: bool,
    pub promotion_supported: bool,
    pub publication_supported: bool,
    pub packs: Vec<TrustedReleasePackSnapshotV1>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustedOptionalPackMutationRequestV1 {
    pub pack_id: String,
    pub revision: String,
    pub explicit_user_confirmation: bool,
    pub license_accepted: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustedOptionalPackActivationRequestV1 {
    pub pack_id: String,
    pub revision: String,
    pub explicit_user_confirmation: bool,
    pub selection: SelectedLoadoutSelectionV1,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrustedOptionalPackActivationResultV1 {
    pub schema_version: u32,
    pub receipt: TrustedProviderLoadActivationReceiptV1,
    pub lifecycle: TrustedOptionalPackLifecycleResultV1,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrustedOptionalPackPhaseV1 {
    NotInstalled,
    Downloading,
    Verifying,
    Staging,
    InstalledInactiveAwaitingSelfTest,
    Active,
    Repairing,
    RepairRequired,
    Removing,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrustedOptionalPackStateV1 {
    pub identity: PackRevision,
    pub phase: TrustedOptionalPackPhaseV1,
    pub explicit_download_required: bool,
    pub automatic_download_allowed: bool,
    pub license_acceptance_required: bool,
    pub can_install: bool,
    pub can_repair: bool,
    pub can_remove: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrustedOptionalPackLifecycleResultV1 {
    pub schema_version: u32,
    pub ready: bool,
    pub detail: String,
    pub packs: Vec<TrustedOptionalPackStateV1>,
}

#[derive(Clone)]
struct TrustedLocalCatalogAuthorityV1 {
    trust_scope: ReleaseCatalogTrustScopeV1,
    production_trust: bool,
    rotation_required_before_release: bool,
    promotion_supported: bool,
    publication_supported: bool,
}

/// Native-only exact visual launch authority. It deliberately contains the
/// governor-minted, non-deserializable receipt rather than any WebView state.
#[derive(Clone, Debug)]
pub struct NativeAdmittedVisualPackLaunchV1 {
    pub identity: PackRevision,
    pub artifact_root: PathBuf,
    pub installed_content_tree_sha256: Sha256Digest,
    pub detector_model: NativeVerifiedPackFileV1,
    pub landmark_model: NativeVerifiedPackFileV1,
    pub openseeface_license: NativeVerifiedPackFileV1,
    pub runtime_library: NativeVerifiedPackFileV1,
    pub runtime_shared_library: NativeVerifiedPackFileV1,
    pub runtime_license: NativeVerifiedPackFileV1,
    pub runtime_third_party_notices: NativeVerifiedPackFileV1,
    pub runtime_version_file: NativeVerifiedPackFileV1,
    pub runtime_commit_file: NativeVerifiedPackFileV1,
    pub runtime: String,
    pub runtime_revision: String,
    pub backend: String,
    pub placement: ResidencyModeV1,
    pub exact_target_pid: u32,
    pub measured_envelope_sha256: Sha256Digest,
    pub admission_receipt: LoadoutAdmissionV1,
    pub residency_decision: MeasuredResidencyDecisionV1,
}

/// Exact file authority resolved from the immutable installed-version index.
/// Native workers must re-hash before loading to close the path/open race.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeVerifiedPackFileV1 {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub sha256: Sha256Digest,
}

#[derive(Clone, Debug)]
pub struct NativeAdmittedIdentityPackLaunchV1 {
    pub identity: PackRevision,
    pub artifact_root: PathBuf,
    pub installed_content_tree_sha256: Sha256Digest,
    pub yunet_model: NativeVerifiedPackFileV1,
    pub sface_model: NativeVerifiedPackFileV1,
    pub yunet_license: NativeVerifiedPackFileV1,
    pub sface_license: NativeVerifiedPackFileV1,
    pub sface_model_card: NativeVerifiedPackFileV1,
    pub opencv_zoo_commercial_use_report: NativeVerifiedPackFileV1,
    pub python_executable: NativeVerifiedPackFileV1,
    pub worker_script: NativeVerifiedPackFileV1,
    pub manifest_file: NativeVerifiedPackFileV1,
    pub runtime_root: PathBuf,
    pub runtime_tree_sha256: Sha256Digest,
    pub runtime_files: Vec<NativeVerifiedPackFileV1>,
    pub runtime: String,
    pub runtime_revision: String,
    pub backend: String,
    pub placement: ResidencyModeV1,
    pub exact_target_pid: u32,
    pub measured_envelope_sha256: Sha256Digest,
    pub admission_receipt_sha256: Sha256Digest,
    pub catalog_admission_sha256: Sha256Digest,
    pub admission_receipt: LoadoutAdmissionV1,
    pub residency_decision: MeasuredResidencyDecisionV1,
    pub qualification: PinnedIdentityQualificationV1,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum IdentityRuntimeFileRoleV1 {
    PythonExecutable,
    OpencvRuntime,
    NumpyRuntime,
    OpencvLicense,
    OpencvThirdPartyNotices,
    NumpyLicense,
    RuntimeSupport,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityRuntimeFileBindingV1 {
    role: IdentityRuntimeFileRoleV1,
    relative_path: String,
    size_bytes: u64,
    sha256: Sha256Digest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeFileBindingV1 {
    relative_path: String,
    size_bytes: u64,
    sha256: Sha256Digest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityRuntimeAuthorityPayloadV1 {
    schema: String,
    identity: PackRevision,
    manifest_sha256: Sha256Digest,
    manifest_raw_sha256: Sha256Digest,
    catalog_payload_sha256: Sha256Digest,
    measured_envelope_sha256: Sha256Digest,
    admission_receipt_sha256: Sha256Digest,
    runtime: String,
    runtime_revision: String,
    backend: String,
    python_abi: String,
    runtime_tree_sha256: Sha256Digest,
    runtime_files: Vec<IdentityRuntimeFileBindingV1>,
    worker_script: NativeFileBindingV1,
    manifest_file: NativeFileBindingV1,
    qualification: PinnedIdentityQualificationV1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedIdentityRuntimeAuthorityV1 {
    signed: IdentityRuntimeAuthorityPayloadV1,
    signatures: Vec<npc_model_manager::CatalogSignatureV1>,
}

/// Constructed by the native visual runtime from its actor/track/capture
/// authority. This type is intentionally not deserializable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeScreenSpaceLipSyncWorkV1 {
    pub actor_id: String,
    pub track_id: String,
    pub session_epoch: u64,
    pub generation_id: u64,
    pub frame_id: u64,
    pub deadline_monotonic_millis: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeVisualScheduleDispositionV1 {
    Queued,
    Dispatched,
    Cancelled,
    Ready,
    NoWork,
    BypassPlannerUnavailable,
    BypassNoAdmission,
    BypassInvalid,
    BypassStale,
    BypassPressure,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeVisualScheduleResultV1 {
    pub schema_version: u32,
    pub disposition: NativeVisualScheduleDispositionV1,
    pub detail: String,
    pub work_id: Option<String>,
    pub work: Option<NativeScreenSpaceLipSyncWorkV1>,
    pub pressure: Option<ResourcePressureLevelV1>,
    pub cancellations: Vec<WorkCancellationV1>,
}

#[derive(Debug, thiserror::Error)]
pub enum LocalResourceError {
    #[error("local resource request is invalid: {0}")]
    Invalid(String),
    #[error("explicit user confirmation is required")]
    ConfirmationRequired,
    #[error("experimental visual-pack lifecycle is unavailable in production builds")]
    LocalReviewOnly,
    #[error("local resource operation failed: {0}")]
    Operation(String),
    #[error("local resource state is temporarily unavailable")]
    State,
}

pub struct LocalResourceManager {
    root: PathBuf,
    config_root: PathBuf,
    installed_resource_root: Option<PathBuf>,
    settings_path: PathBuf,
    pack_state_path: PathBuf,
    selected_loadout_path: PathBuf,
    settings: Mutex<LocalResourceSettings>,
    pack_state: Mutex<ExperimentalPackState>,
    selected_loadout: Mutex<Option<SelectedLoadoutSelectionV1>>,
    loadout_planner: Mutex<NativeLoadoutPlanner>,
    active_admission: Mutex<Option<NativeActiveLoadoutAdmissionV1>>,
    visual_work: Mutex<BTreeMap<String, NativeScreenSpaceLipSyncWorkV1>>,
    optional_lifecycle: tokio::sync::Mutex<Option<TrustedOptionalPackLifecycleV1>>,
    optional_downloads: Mutex<BTreeMap<PackRevision, NativeOptionalMutationV1>>,
    operation: tokio::sync::Mutex<()>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeOptionalMutationKindV1 {
    Install,
    Repair,
    Activate,
}

#[derive(Clone)]
struct NativeOptionalMutationV1 {
    operation_id: uuid::Uuid,
    kind: NativeOptionalMutationKindV1,
    cancellation: CancellationToken,
}

#[derive(Clone)]
struct NativeActiveLoadoutAdmissionV1 {
    exact_target_pid: u32,
    receipt: LoadoutAdmissionV1,
    residency_decisions: Vec<MeasuredResidencyDecisionV1>,
}

type CoreLoadoutPlanner =
    SelectedLoadoutManagerV1<Ed25519CatalogVerifier, FileResourceEnvelopeSource>;

// The ready state is long-lived and avoids additional indirection on every admission check.
#[allow(clippy::large_enum_variant)]
enum NativeLoadoutPlanner {
    Ready {
        manager: CoreLoadoutPlanner,
        packs: Vec<TrustedReleasePackSnapshotV1>,
        detail: String,
        trust: TrustedLocalCatalogAuthorityV1,
    },
    Unavailable(String),
}

#[derive(Clone)]
struct FileResourceEnvelopeSource {
    roots: Vec<PathBuf>,
    catalog_qualified: Vec<(PackRevision, Sha256Digest, ResidencyModeV1, PathBuf)>,
}

impl SignedResourceEnvelopeSourceV1 for FileResourceEnvelopeSource {
    type Error = LocalResourceError;

    fn load_signed_envelope(
        &self,
        identity: &PackRevision,
        device_fingerprint_sha256: &npc_model_manager::Sha256Digest,
        placement: &ResidencyModeV1,
    ) -> Result<Option<SignedMeasuredResourceEnvelopeV1>, Self::Error> {
        let placement_label = match placement {
            ResidencyModeV1::CpuResident => "cpu_resident",
            ResidencyModeV1::GpuResident => "gpu_resident",
            ResidencyModeV1::CpuResidentGpuCold => "cpu_resident_gpu_cold",
        };
        for root in &self.roots {
            let path = root
                .join(identity.pack_id.as_str())
                .join(identity.revision.as_str())
                .join(device_fingerprint_sha256.as_str())
                .join(format!("{placement_label}.json"));
            if let Some(envelope) = read_bounded_regular_json(&path, MAX_TRUST_METADATA_BYTES)? {
                return Ok(Some(envelope));
            }
        }
        if let Some((_, _, _, path)) = self.catalog_qualified.iter().find(
            |(bound_identity, bound_device, bound_placement, _)| {
                bound_identity == identity
                    && bound_device == device_fingerprint_sha256
                    && bound_placement == placement
            },
        ) {
            return read_bounded_regular_json(path, MAX_TRUST_METADATA_BYTES);
        }
        Ok(None)
    }
}

impl LocalResourceManager {
    pub fn new(
        config_directory: &Path,
        installed_resource_root: Option<&Path>,
    ) -> Result<Self, LocalResourceError> {
        let root = config_directory.join("model-packs");
        fs::create_dir_all(&root)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let settings_path = config_directory.join(SETTINGS_FILE_NAME);
        let pack_state_path = config_directory.join(PACK_STATE_FILE_NAME);
        let selected_loadout_path = config_directory.join(SELECTED_LOADOUT_FILE_NAME);
        let settings: LocalResourceSettings = load_json_or_default(&settings_path);
        let loadout_planner = load_native_planner(
            config_directory,
            installed_resource_root,
            &settings,
            current_unix_seconds(),
        );
        let optional_lifecycle = match &loadout_planner {
            NativeLoadoutPlanner::Ready { manager, trust, .. } => {
                let trust_domain = if trust.production_trust {
                    CatalogTrustDomainV1::ReleaseThreshold
                } else {
                    CatalogTrustDomainV1::LocalReviewDevOnly
                };
                TrustedOptionalPackLifecycleV1::new(
                    root.join("trusted-lifecycle-v1"),
                    manager.trusted_catalog().clone(),
                    trust_domain,
                    DownloadPolicy::default(),
                )
                .ok()
            }
            NativeLoadoutPlanner::Unavailable(_) => None,
        };
        Ok(Self {
            root,
            config_root: config_directory.to_path_buf(),
            installed_resource_root: installed_resource_root.map(Path::to_path_buf),
            settings: Mutex::new(settings),
            pack_state: Mutex::new(load_json_or_default(&pack_state_path)),
            selected_loadout: Mutex::new(load_optional_json(&selected_loadout_path)),
            loadout_planner: Mutex::new(loadout_planner),
            active_admission: Mutex::new(None),
            visual_work: Mutex::new(BTreeMap::new()),
            optional_lifecycle: tokio::sync::Mutex::new(optional_lifecycle),
            optional_downloads: Mutex::new(BTreeMap::new()),
            settings_path,
            pack_state_path,
            selected_loadout_path,
            operation: tokio::sync::Mutex::new(()),
        })
    }

    pub fn settings(&self) -> Result<LocalResourceSettings, LocalResourceError> {
        self.settings
            .lock()
            .map(|settings| settings.clone())
            .map_err(|_| LocalResourceError::State)
    }

    /// Reports where the effective policy came from without changing policy,
    /// loading models, or minting an admission receipt.
    pub(crate) fn settings_persistence_source(
        &self,
    ) -> Result<LocalResourceSettingsPersistenceV1, LocalResourceError> {
        let effective = self.settings()?;
        match read_bounded_regular_json::<LocalResourceSettings>(&self.settings_path, 512 * 1024) {
            Ok(Some(saved)) if saved == effective => {
                Ok(LocalResourceSettingsPersistenceV1::NativePersisted)
            }
            Ok(None) => Ok(LocalResourceSettingsPersistenceV1::BuiltInDefault),
            Ok(Some(_)) | Err(_) => Ok(LocalResourceSettingsPersistenceV1::RecoveredDefault),
        }
    }

    pub fn save_settings(
        &self,
        settings: LocalResourceSettings,
    ) -> Result<LocalResourceSettings, LocalResourceError> {
        validate_settings(&settings)?;
        atomic_write_json(&self.settings_path, &settings)?;
        *self
            .settings
            .lock()
            .map_err(|_| LocalResourceError::State)? = settings.clone();
        *self
            .loadout_planner
            .lock()
            .map_err(|_| LocalResourceError::State)? = load_native_planner(
            &self.config_root,
            self.installed_resource_root.as_deref(),
            &settings,
            current_unix_seconds(),
        );
        *self
            .active_admission
            .lock()
            .map_err(|_| LocalResourceError::State)? = None;
        self.visual_work
            .lock()
            .map_err(|_| LocalResourceError::State)?
            .clear();
        Ok(settings)
    }

    /// Revokes the device/target-bound native admission before a selected game
    /// target or planner policy changes. This is intentionally broader than
    /// identity: every local model receipt was minted for the same exact game
    /// process and must be re-admitted together.
    pub(crate) fn revoke_active_loadout_admission(&self) -> Result<(), LocalResourceError> {
        let mut planner = self
            .loadout_planner
            .lock()
            .map_err(|_| LocalResourceError::State)?;
        if let NativeLoadoutPlanner::Ready { manager, .. } = &mut *planner {
            manager.cancel_all_work(CancellationReasonV1::AdmissionRevoked);
        }
        *self
            .active_admission
            .lock()
            .map_err(|_| LocalResourceError::State)? = None;
        self.visual_work
            .lock()
            .map_err(|_| LocalResourceError::State)?
            .clear();
        Ok(())
    }

    pub fn selected_loadout_planner(
        &self,
    ) -> Result<SelectedLoadoutPlannerResult, LocalResourceError> {
        let selected = self
            .selected_loadout
            .lock()
            .map_err(|_| LocalResourceError::State)?
            .clone();
        let planner = self
            .loadout_planner
            .lock()
            .map_err(|_| LocalResourceError::State)?;
        Ok(match &*planner {
            NativeLoadoutPlanner::Ready {
                manager, detail, ..
            } => SelectedLoadoutPlannerResult {
                schema_version: 1,
                ready: true,
                detail: format!("{detail} Native measurement storage is available. Admission still verifies the exact selected PID, current telemetry, and every role envelope."),
                selected,
                planner: Some(manager.planner_snapshot()),
            },
            NativeLoadoutPlanner::Unavailable(detail) => SelectedLoadoutPlannerResult {
                schema_version: 1,
                ready: false,
                detail: detail.clone(),
                selected,
                planner: None,
            },
        })
    }

    pub fn trusted_pack_catalog(
        &self,
    ) -> Result<TrustedLocalPackCatalogResult, LocalResourceError> {
        let mut planner = self
            .loadout_planner
            .lock()
            .map_err(|_| LocalResourceError::State)?;
        Ok(match &mut *planner {
            NativeLoadoutPlanner::Ready {
                manager,
                packs,
                detail,
                trust,
            } => {
                let mut packs = packs.clone();
                let telemetry = collect(TelemetryRequest::default());
                let device = telemetry
                    .device_fingerprint_sha256
                    .value()
                    .cloned()
                    .and_then(|value| npc_model_manager::Sha256Digest::parse(value).ok());
                if let Some(device) = &device {
                    let now = current_unix_seconds();
                    for pack in &mut packs {
                        manager.refresh_trusted_pack_measurement(pack, device, now);
                    }
                }
                TrustedLocalPackCatalogResult {
                    schema_version: 1,
                    ready: true,
                    detail: match device {
                        Some(_) => format!("{detail} Measurement fields include only signed envelopes verified for the current native device; unavailable fields remain explicit."),
                        None => format!("{detail} The native device fingerprint is unavailable, so all measurement fields remain explicitly unavailable."),
                    },
                    trust_scope: Some(trust.trust_scope.clone()),
                    production_trust: trust.production_trust,
                    rotation_required_before_release: trust.rotation_required_before_release,
                    promotion_supported: trust.promotion_supported,
                    publication_supported: trust.publication_supported,
                    packs,
                }
            }
            NativeLoadoutPlanner::Unavailable(detail) => TrustedLocalPackCatalogResult {
                schema_version: 1,
                ready: false,
                detail: detail.clone(),
                trust_scope: None,
                production_trust: false,
                rotation_required_before_release: true,
                promotion_supported: false,
                publication_supported: false,
                packs: Vec::new(),
            },
        })
    }

    pub async fn trusted_optional_pack_lifecycle(
        &self,
    ) -> Result<TrustedOptionalPackLifecycleResultV1, LocalResourceError> {
        let catalog = self.trusted_pack_catalog()?;
        if !catalog.ready {
            return Ok(TrustedOptionalPackLifecycleResultV1 {
                schema_version: 1,
                ready: false,
                detail: catalog.detail,
                packs: Vec::new(),
            });
        }
        let lifecycle = self.optional_lifecycle.lock().await;
        let Some(lifecycle) = lifecycle.as_ref() else {
            return Ok(TrustedOptionalPackLifecycleResultV1 {
                schema_version: 1,
                ready: false,
                detail: "The verified catalog is present, but native optional-pack storage could not be initialized; mutations remain fail-closed.".into(),
                packs: Vec::new(),
            });
        };
        let packs = catalog
            .packs
            .into_iter()
            .map(|pack| optional_pack_state(&pack, lifecycle.state(&pack.identity)))
            .collect();
        Ok(TrustedOptionalPackLifecycleResultV1 {
            schema_version: 1,
            ready: true,
            detail: "Every optional pack is resolved from the verified signed catalog. Downloads require explicit consent, are resumable and hash-bound, and stop inactive at the attested self-test boundary; live loadout admission remains separate.".into(),
            packs,
        })
    }

    pub async fn install_trusted_optional_pack(
        &self,
        request: TrustedOptionalPackMutationRequestV1,
    ) -> Result<TrustedOptionalPackLifecycleResultV1, LocalResourceError> {
        let (identity, contract) = self.validate_optional_mutation(&request)?;
        if contract.lifecycle.explicit_download_required && !request.explicit_user_confirmation {
            return Err(LocalResourceError::ConfirmationRequired);
        }
        if contract.lifecycle.automatic_download_allowed {
            return Err(LocalResourceError::Invalid(
                "automatic local model downloads are forbidden by the product lifecycle".into(),
            ));
        }
        if contract.license.acceptance_required && !request.license_accepted {
            return Err(LocalResourceError::Invalid(
                "the exact signed license must be accepted before download".into(),
            ));
        }
        let active =
            self.begin_optional_mutation(&identity, NativeOptionalMutationKindV1::Install)?;
        let cancellation = active.cancellation.clone();
        let result = {
            let mut lifecycle = self.optional_lifecycle.lock().await;
            match lifecycle.as_mut() {
                Some(lifecycle) => lifecycle
                    .install_or_resume(
                        &identity,
                        format!("explicit-{}", uuid::Uuid::new_v4().simple()),
                        current_unix_seconds(),
                        request.explicit_user_confirmation,
                        request.license_accepted,
                        &cancellation,
                    )
                    .await
                    .map_err(|error| LocalResourceError::Operation(error.to_string())),
                None => Err(LocalResourceError::Operation(
                    "trusted optional-pack lifecycle is unavailable".into(),
                )),
            }
        };
        self.finish_optional_mutation(&identity, active.operation_id)?;
        result?;
        self.trusted_optional_pack_lifecycle().await
    }

    /// Activates an immutable inactive YuNet provider pack without requiring a
    /// game window to be running. The setup-only admission uses current host
    /// telemetry, the configured game reserves, and the complete explicit
    /// loadout supplied by the UI. It is consumed only for self-test
    /// authorization and is never stored as target-bound runtime authority.
    pub(crate) async fn activate_trusted_optional_pack(
        &self,
        request: TrustedOptionalPackActivationRequestV1,
        probe: &impl TrustedProviderLoadSelfTestProbeV1,
    ) -> Result<TrustedOptionalPackActivationResultV1, LocalResourceError> {
        if !request.explicit_user_confirmation {
            return Err(LocalResourceError::ConfirmationRequired);
        }
        let mutation = TrustedOptionalPackMutationRequestV1 {
            pack_id: request.pack_id.clone(),
            revision: request.revision.clone(),
            explicit_user_confirmation: true,
            license_accepted: false,
        };
        let (identity, _) = self.validate_optional_mutation(&mutation)?;
        let active =
            self.begin_optional_mutation(&identity, NativeOptionalMutationKindV1::Activate)?;
        let operation = async {
            // Registering the Activate mutation first makes it a barrier for
            // synchronous runtime admission. Revocation then prevents an old
            // game/PID-bound receipt from becoming usable when the active
            // installed pointer changes. Failure deliberately leaves it
            // revoked.
            self.revoke_active_loadout_admission()?;
            let settings = self.settings()?;
            let telemetry = collect(TelemetryRequest::default());
            let pressure = native_resource_pressure(&telemetry);
            let decision = {
                let mut planner = self
                    .loadout_planner
                    .lock()
                    .map_err(|_| LocalResourceError::State)?;
                match &mut *planner {
                    NativeLoadoutPlanner::Ready { manager, .. } => manager.admit_for_setup(
                        &request.selection,
                        NativeAdmissionContextV1 {
                            exact_target_pid: None,
                            now_unix_seconds: current_unix_seconds(),
                            now_monotonic_millis: telemetry.captured_monotonic_millis,
                            configured_game_reserve_vram_bytes: settings.game_reserve_vram_bytes,
                            game_additional_reserve_ram_bytes: settings
                                .game_additional_reserve_ram_bytes,
                            resource_pressure: pressure,
                            telemetry: Some(&telemetry),
                        },
                    ),
                    NativeLoadoutPlanner::Unavailable(detail) => {
                        return Err(LocalResourceError::Operation(detail.clone()));
                    }
                }
            };
            if !decision.admitted() || decision.exact_target_pid.is_some() {
                return Err(LocalResourceError::Operation(format!(
                    "setup-only whole-loadout admission was blocked: {}",
                    decision.detail
                )));
            }
            let admission = decision.admission_receipt.ok_or_else(|| {
                LocalResourceError::Operation(
                    "setup-only admission returned no native receipt".into(),
                )
            })?;
            // Persist this only as the user's selected draft. It carries no
            // PID and never populates `active_admission`; gameplay still has
            // to reconstruct and admit the target-bound loadout.
            atomic_write_json(&self.selected_loadout_path, &request.selection)?;
            *self
                .selected_loadout
                .lock()
                .map_err(|_| LocalResourceError::State)? = Some(request.selection.clone());
            let receipt = {
                let mut lifecycle = self.optional_lifecycle.lock().await;
                let lifecycle = lifecycle.as_mut().ok_or_else(|| {
                    LocalResourceError::Operation(
                        "trusted optional-pack lifecycle is unavailable".into(),
                    )
                })?;
                activate_trusted_yunet_provider_pack_v1(
                    lifecycle,
                    &identity,
                    format!("activate-{}", uuid::Uuid::new_v4().simple()),
                    &admission,
                    probe,
                    &SystemProviderLoadSelfTestClockV1,
                )
                .await
                .map_err(|error| LocalResourceError::Operation(error.to_string()))?
            };
            let lifecycle = self.trusted_optional_pack_lifecycle().await?;
            Ok(TrustedOptionalPackActivationResultV1 {
                schema_version: 1,
                receipt,
                lifecycle,
            })
        }
        .await;
        // Keep the activation barrier registered through a final revocation:
        // an admission either finishes before the barrier and is revoked, or
        // observes the barrier and cannot store a receipt.
        let final_revocation = self.revoke_active_loadout_admission();
        let finish = self.finish_optional_mutation(&identity, active.operation_id);
        let result = operation?;
        final_revocation?;
        finish?;
        Ok(result)
    }

    pub fn cancel_trusted_optional_pack_download(
        &self,
        request: TrustedOptionalPackMutationRequestV1,
    ) -> Result<bool, LocalResourceError> {
        let (identity, _) = self.validate_optional_mutation(&request)?;
        self.cancel_optional_mutation(&identity)
    }

    pub async fn repair_trusted_optional_pack(
        &self,
        request: TrustedOptionalPackMutationRequestV1,
    ) -> Result<TrustedOptionalPackLifecycleResultV1, LocalResourceError> {
        let (identity, contract) = self.validate_optional_mutation(&request)?;
        if !request.explicit_user_confirmation {
            return Err(LocalResourceError::ConfirmationRequired);
        }
        if contract.license.acceptance_required && !request.license_accepted {
            return Err(LocalResourceError::Invalid(
                "the exact signed license must be accepted before repair".into(),
            ));
        }
        let active =
            self.begin_optional_mutation(&identity, NativeOptionalMutationKindV1::Repair)?;
        let cancellation = active.cancellation.clone();
        let result = {
            let mut lifecycle = self.optional_lifecycle.lock().await;
            match lifecycle.as_mut() {
                Some(lifecycle) => match lifecycle.begin_repair(&identity) {
                    Ok(assessment) if assessment.healthy => Ok(()),
                    Ok(_) => lifecycle
                        .install_or_resume(
                            &identity,
                            format!("repair-{}", uuid::Uuid::new_v4().simple()),
                            current_unix_seconds(),
                            true,
                            request.license_accepted,
                            &cancellation,
                        )
                        .await
                        .map(|_| ())
                        .map_err(|error| LocalResourceError::Operation(error.to_string())),
                    Err(error) => Err(LocalResourceError::Operation(error.to_string())),
                },
                None => Err(LocalResourceError::Operation(
                    "trusted optional-pack lifecycle is unavailable".into(),
                )),
            }
        };
        self.finish_optional_mutation(&identity, active.operation_id)?;
        result?;
        self.trusted_optional_pack_lifecycle().await
    }

    pub async fn remove_trusted_optional_pack(
        &self,
        request: TrustedOptionalPackMutationRequestV1,
    ) -> Result<TrustedOptionalPackLifecycleResultV1, LocalResourceError> {
        let (identity, _) = self.validate_optional_mutation(&request)?;
        if !request.explicit_user_confirmation {
            return Err(LocalResourceError::ConfirmationRequired);
        }
        if self
            .active_admission
            .lock()
            .map_err(|_| LocalResourceError::State)?
            .as_ref()
            .is_some_and(|active| {
                active
                    .receipt
                    .models()
                    .iter()
                    .any(|model| model.identity == identity)
            })
        {
            return Err(LocalResourceError::Invalid(
                "an admitted loadout still references this pack; unload or change the loadout first"
                    .into(),
            ));
        }
        let mut lifecycle = self.optional_lifecycle.lock().await;
        lifecycle
            .as_mut()
            .ok_or_else(|| {
                LocalResourceError::Operation(
                    "trusted optional-pack lifecycle is unavailable".into(),
                )
            })?
            .remove(&identity, true)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        drop(lifecycle);
        self.trusted_optional_pack_lifecycle().await
    }

    fn validate_optional_mutation(
        &self,
        request: &TrustedOptionalPackMutationRequestV1,
    ) -> Result<(PackRevision, TrustedReleasePackSnapshotV1), LocalResourceError> {
        let identity = PackRevision {
            pack_id: npc_model_manager::PackId::parse(&request.pack_id)
                .map_err(|error| LocalResourceError::Invalid(error.to_string()))?,
            revision: npc_model_manager::Revision::parse(&request.revision)
                .map_err(|error| LocalResourceError::Invalid(error.to_string()))?,
        };
        let catalog = self.trusted_pack_catalog()?;
        if !catalog.ready {
            return Err(LocalResourceError::Operation(catalog.detail));
        }
        let contract = catalog
            .packs
            .into_iter()
            .find(|pack| pack.identity == identity)
            .ok_or_else(|| {
                LocalResourceError::Invalid(
                    "pack identity is absent from the verified signed catalog".into(),
                )
            })?;
        Ok((identity, contract))
    }

    fn begin_optional_mutation(
        &self,
        identity: &PackRevision,
        kind: NativeOptionalMutationKindV1,
    ) -> Result<NativeOptionalMutationV1, LocalResourceError> {
        let mut active = self
            .optional_downloads
            .lock()
            .map_err(|_| LocalResourceError::State)?;
        if let Some(existing) = active.get(identity) {
            return Err(LocalResourceError::Operation(format!(
                "an exact-pack {:?} mutation is already active; concurrent install/repair is rejected",
                existing.kind
            )));
        }
        let mutation = NativeOptionalMutationV1 {
            operation_id: uuid::Uuid::new_v4(),
            kind,
            cancellation: CancellationToken::new(),
        };
        active.insert(identity.clone(), mutation.clone());
        Ok(mutation)
    }

    fn finish_optional_mutation(
        &self,
        identity: &PackRevision,
        operation_id: uuid::Uuid,
    ) -> Result<(), LocalResourceError> {
        let mut active = self
            .optional_downloads
            .lock()
            .map_err(|_| LocalResourceError::State)?;
        if active
            .get(identity)
            .is_some_and(|mutation| mutation.operation_id == operation_id)
        {
            active.remove(identity);
        }
        Ok(())
    }

    fn cancel_optional_mutation(
        &self,
        identity: &PackRevision,
    ) -> Result<bool, LocalResourceError> {
        let cancellation = self
            .optional_downloads
            .lock()
            .map_err(|_| LocalResourceError::State)?
            .get(identity)
            .map(|active| active.cancellation.clone());
        if let Some(cancellation) = cancellation {
            cancellation.cancel();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn admit_selected_loadout(
        &self,
        selection: SelectedLoadoutSelectionV1,
        exact_target_pid: Option<u32>,
    ) -> Result<SelectedLoadoutAdmissionResult, LocalResourceError> {
        // Hold the mutation registry lock until any target-bound receipt is
        // stored. This makes the Activate registration and this entire
        // synchronous admission atomic relative to each other.
        let optional_mutations = self
            .optional_downloads
            .lock()
            .map_err(|_| LocalResourceError::State)?;
        if optional_mutations
            .values()
            .any(|mutation| mutation.kind == NativeOptionalMutationKindV1::Activate)
        {
            return Ok(SelectedLoadoutAdmissionResult {
                schema_version: 1,
                ready: false,
                detail: "A local provider activation self-test is running. Wait for it to finish, then check and admit the selected game loadout again.".into(),
                persisted: false,
                decision: None,
            });
        }
        let settings = self.settings()?;
        let snapshot = collect(TelemetryRequest {
            selected_game_pid: exact_target_pid,
            ..TelemetryRequest::default()
        });
        let pressure = native_resource_pressure(&snapshot);
        let mut planner = self
            .loadout_planner
            .lock()
            .map_err(|_| LocalResourceError::State)?;
        let manager = match &mut *planner {
            NativeLoadoutPlanner::Ready { manager, .. } => manager,
            NativeLoadoutPlanner::Unavailable(detail) => {
                return Ok(SelectedLoadoutAdmissionResult {
                    schema_version: 1,
                    ready: false,
                    detail: detail.clone(),
                    persisted: false,
                    decision: None,
                })
            }
        };
        let decision = manager.admit(
            &selection,
            NativeAdmissionContextV1 {
                exact_target_pid,
                now_unix_seconds: current_unix_seconds(),
                now_monotonic_millis: snapshot.captured_monotonic_millis,
                configured_game_reserve_vram_bytes: settings.game_reserve_vram_bytes,
                game_additional_reserve_ram_bytes: settings.game_additional_reserve_ram_bytes,
                resource_pressure: pressure,
                telemetry: Some(&snapshot),
            },
        );
        let active_admission = decision.admission_receipt.as_ref().and_then(|receipt| {
            decision
                .exact_target_pid
                .map(|exact_target_pid| NativeActiveLoadoutAdmissionV1 {
                    exact_target_pid,
                    receipt: receipt.clone(),
                    residency_decisions: decision.residency_decisions.clone(),
                })
        });
        if active_admission.is_none() {
            manager.cancel_all_work(CancellationReasonV1::AdmissionRevoked);
        }
        *self
            .active_admission
            .lock()
            .map_err(|_| LocalResourceError::State)? = active_admission;
        if !decision.admitted() {
            self.visual_work
                .lock()
                .map_err(|_| LocalResourceError::State)?
                .clear();
        }
        let persisted = if decision.admitted() {
            atomic_write_json(&self.selected_loadout_path, &selection)?;
            *self
                .selected_loadout
                .lock()
                .map_err(|_| LocalResourceError::State)? = Some(selection);
            true
        } else {
            false
        };
        Ok(SelectedLoadoutAdmissionResult {
            schema_version: 1,
            ready: true,
            detail: decision.detail.clone(),
            persisted,
            decision: Some(decision),
        })
    }

    pub fn telemetry(
        &self,
        selected_game_pid: Option<u32>,
    ) -> Result<ResourceTelemetryResult, LocalResourceError> {
        let settings = self.settings()?;
        let snapshot = collect(TelemetryRequest {
            selected_game_pid,
            ..TelemetryRequest::default()
        });
        let admission = snapshot
            .admission_view(
                settings.game_reserve_vram_bytes,
                settings.game_additional_reserve_ram_bytes,
            )
            .and_then(|view| {
                npc_model_manager::LiveResourceSnapshotV1::try_from(view).map_err(|_| {
                    npc_system_telemetry::AdmissionViewError::RequiredMetricUnavailable(
                        "resource_governor_bridge",
                    )
                })
            });
        Ok(ResourceTelemetryResult {
            settings,
            admission_ready: admission.is_ok(),
            admission_detail: match admission {
                Ok(_) => "Live telemetry is complete. Activation still requires signed, device-bound measurements for every selected local role.".into(),
                Err(error) => format!("Local activation is fail-closed because telemetry is incomplete: {error}"),
            },
            snapshot,
        })
    }

    /// Resolves an exact visual provider from a governor-minted admission.
    /// No caller supplies a provider id, path, runtime, placement,
    /// measurement, or receipt. The existing MNV3 pack remains the default;
    /// the YuNet pack can cross this boundary only when it is the exact pack
    /// selected by the sealed loadout.
    pub fn resolve_admitted_openseeface_launch(
        &self,
    ) -> Result<NativeAdmittedVisualPackLaunchV1, LocalResourceError> {
        let planner = self
            .loadout_planner
            .lock()
            .map_err(|_| LocalResourceError::State)?;
        let (manager, packs) = match &*planner {
            NativeLoadoutPlanner::Ready { manager, packs, .. } => (manager, packs),
            NativeLoadoutPlanner::Unavailable(detail) => {
                return Err(LocalResourceError::Operation(format!(
                    "visual launch is unavailable: {detail}"
                )))
            }
        };
        let active = self
            .active_admission
            .lock()
            .map_err(|_| LocalResourceError::State)?
            .clone()
            .ok_or_else(|| {
                LocalResourceError::Operation(
                    "visual launch is fail-closed because no current native admission exists"
                        .into(),
                )
            })?;
        let mut admitted_visual_models = active.receipt.models().iter().filter(|model| {
            model.role == ModelPackKindV1::Vision
                && supported_openseeface_visual_identity(&model.identity)
        });
        let model = admitted_visual_models.next().cloned().ok_or_else(|| {
            LocalResourceError::Operation(
                "the current sealed loadout does not admit an exact supported OpenSeeFace visual pack"
                    .into(),
            )
        })?;
        if admitted_visual_models.next().is_some() {
            return Err(LocalResourceError::Operation(
                "the current sealed loadout ambiguously admits multiple visual providers".into(),
            ));
        }
        let trusted_pack = packs
            .iter()
            .find(|pack| pack.identity == model.identity)
            .ok_or_else(|| {
                LocalResourceError::Operation(
                    "the admitted visual pack is absent from the verified source inventory".into(),
                )
            })?;
        let trusted_entry = manager
            .trusted_catalog()
            .installable_entry(&model.identity)
            .map_err(|error| {
                LocalResourceError::Operation(format!(
                    "the admitted visual pack is no longer installable from the verified catalog: {error}"
                ))
            })?;
        if trusted_entry.manifest_sha256 != model.manifest_sha256 {
            return Err(LocalResourceError::Operation(
                "the admitted visual pack manifest does not match the verified catalog".into(),
            ));
        }
        let measurement = manager.active_measurement(&model.identity).ok_or_else(|| {
            LocalResourceError::Operation(
                "the admitted visual pack has no active verified measurement".into(),
            )
        })?;
        let runtime_revision = trusted_pack.runtime_revision.clone().ok_or_else(|| {
            LocalResourceError::Operation(
                "the admitted visual pack has no immutable runtime revision".into(),
            )
        })?;
        if measurement.runtime != trusted_pack.runtime
            || measurement.runtime_revision != runtime_revision
            || measurement.device_fingerprint_sha256 != *active.receipt.device_fingerprint_sha256()
            || measurement.placements.get(&model.mode) != Some(&model.measurement)
        {
            return Err(LocalResourceError::Operation(
                "the active visual runtime measurement does not match the sealed admission".into(),
            ));
        }
        let residency_decision = active
            .residency_decisions
            .iter()
            .find(|decision| decision.identity == model.identity)
            .cloned()
            .ok_or_else(|| {
                LocalResourceError::Operation(
                    "the admitted visual pack has no measured residency decision".into(),
                )
            })?;
        let storage = FilesystemPackStorage::new(
            self.root.join("trusted-lifecycle-v1"),
            ArchivePolicy::default(),
        )
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let inventory = storage
            .active_installed_inventory(&model.identity.pack_id)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?
            .filter(|inventory| inventory.identity == model.identity)
            .ok_or_else(|| {
                LocalResourceError::Operation(
                    "the admitted visual pack is not the exact attested active installed version"
                        .into(),
                )
            })?;
        if inventory.manifest_sha256 != model.manifest_sha256 {
            return Err(LocalResourceError::Operation(
                "the active installed visual tree is bound to a different manifest".into(),
            ));
        }
        let detector_path =
            if model.identity.pack_id.as_str() == YUNET_OPENSEEFACE_VISUAL_SIGNAL_PACK_ID {
                YUNET_OPENSEEFACE_DETECTOR_PATH
            } else {
                "models/mnv3_detection_opt.onnx"
            };
        let detector_model = native_installed_file(&inventory, detector_path)?;
        let landmark_model = native_installed_file(&inventory, OPENSEEFACE_LM1_PATH)?;
        if landmark_model.sha256.as_str() != OPENSEEFACE_LM1_SHA256
            || (model.identity.pack_id.as_str() == YUNET_OPENSEEFACE_VISUAL_SIGNAL_PACK_ID
                && detector_model.sha256.as_str() != YUNET_OPENSEEFACE_DETECTOR_SHA256)
        {
            return Err(LocalResourceError::Operation(
                "the admitted visual provider model hashes do not match the frozen native contract"
                    .into(),
            ));
        }
        let openseeface_license = native_installed_file(&inventory, "LICENSE")?;
        let runtime_library = native_installed_file(
            &inventory,
            "runtime/onnxruntime-1.22.1-cpu/lib/onnxruntime.dll",
        )?;
        let runtime_shared_library = native_installed_file(
            &inventory,
            "runtime/onnxruntime-1.22.1-cpu/lib/onnxruntime_providers_shared.dll",
        )?;
        let runtime_license =
            native_installed_file(&inventory, "runtime/onnxruntime-1.22.1-cpu/LICENSE")?;
        let runtime_third_party_notices = native_installed_file(
            &inventory,
            "runtime/onnxruntime-1.22.1-cpu/ThirdPartyNotices.txt",
        )?;
        let runtime_version_file =
            native_installed_file(&inventory, "runtime/onnxruntime-1.22.1-cpu/VERSION_NUMBER")?;
        let runtime_commit_file =
            native_installed_file(&inventory, "runtime/onnxruntime-1.22.1-cpu/GIT_COMMIT_ID")?;
        let launch = NativeAdmittedVisualPackLaunchV1 {
            identity: model.identity,
            artifact_root: inventory.root,
            installed_content_tree_sha256: inventory.content_tree_sha256,
            detector_model,
            landmark_model,
            openseeface_license,
            runtime_library,
            runtime_shared_library,
            runtime_license,
            runtime_third_party_notices,
            runtime_version_file,
            runtime_commit_file,
            runtime: measurement.runtime.clone(),
            runtime_revision: measurement.runtime_revision.clone(),
            backend: measurement.backend.clone(),
            placement: model.mode,
            exact_target_pid: active.exact_target_pid,
            measured_envelope_sha256: model.measured_envelope_sha256,
            admission_receipt: active.receipt,
            residency_decision,
        };
        drop(planner);
        Ok(launch)
    }

    /// Resolves the exact qualified identity worker launch exclusively from
    /// native catalog, measurement, admission, installed-pack, and separately
    /// threshold-signed runtime authority. No path or qualification is accepted
    /// from the WebView. The current unqualified identity pack therefore fails
    /// closed before returning any private path.
    pub fn admitted_identity_launch(
        &self,
    ) -> Result<NativeAdmittedIdentityPackLaunchV1, LocalResourceError> {
        let planner = self
            .loadout_planner
            .lock()
            .map_err(|_| LocalResourceError::State)?;
        let (manager, packs, trust) = match &*planner {
            NativeLoadoutPlanner::Ready {
                manager,
                packs,
                trust,
                ..
            } => (manager, packs, trust),
            NativeLoadoutPlanner::Unavailable(detail) => {
                return Err(LocalResourceError::Operation(format!(
                    "identity launch is unavailable: {detail}"
                )))
            }
        };
        if !trust.production_trust {
            return Err(LocalResourceError::Operation(
                "identity launch requires a production-trusted final catalog; local-review bootstrap trust cannot activate identity models".into(),
            ));
        }
        let active = self
            .active_admission
            .lock()
            .map_err(|_| LocalResourceError::State)?
            .clone()
            .ok_or_else(|| {
                LocalResourceError::Operation(
                    "identity launch is fail-closed because no current native admission exists"
                        .into(),
                )
            })?;
        let model = active
            .receipt
            .models()
            .iter()
            .find(|model| {
                model.role == ModelPackKindV1::Vision
                    && model.identity.pack_id.as_str() == IDENTITY_PACK_ID
                    && model.identity.revision.as_str() == IDENTITY_PACK_REVISION
            })
            .cloned()
            .ok_or_else(|| {
                LocalResourceError::Operation(
                    "the sealed loadout does not admit the exact YuNet/SFace identity pack".into(),
                )
            })?;
        let trusted_pack = packs
            .iter()
            .find(|pack| pack.identity == model.identity)
            .ok_or_else(|| {
                LocalResourceError::Operation(
                    "the admitted identity pack is absent from the verified source inventory"
                        .into(),
                )
            })?;
        let trusted_entry = manager
            .trusted_catalog()
            .installable_entry(&model.identity)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        if trusted_entry.manifest_sha256 != model.manifest_sha256 {
            return Err(LocalResourceError::Operation(
                "the admitted identity manifest does not match the verified catalog".into(),
            ));
        }
        let measurement = manager.active_measurement(&model.identity).ok_or_else(|| {
            LocalResourceError::Operation(
                "the admitted identity pack has no active signed current-device measurement".into(),
            )
        })?;
        let runtime_revision = trusted_pack.runtime_revision.clone().ok_or_else(|| {
            LocalResourceError::Operation(
                "the identity pack has no immutable runtime revision".into(),
            )
        })?;
        if model.mode != ResidencyModeV1::CpuResident
            || measurement.runtime != trusted_pack.runtime
            || measurement.runtime_revision != runtime_revision
            || measurement.backend != "opencv-dnn-cpu"
            || measurement.device_fingerprint_sha256 != *active.receipt.device_fingerprint_sha256()
            || measurement.placements.get(&model.mode) != Some(&model.measurement)
        {
            return Err(LocalResourceError::Operation(
                "the active identity measurement does not match the sealed CPU admission".into(),
            ));
        }
        let admission_receipt_sha256 = active
            .receipt
            .digest()
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let residency_decision = active
            .residency_decisions
            .iter()
            .find(|decision| decision.identity == model.identity)
            .cloned()
            .ok_or_else(|| {
                LocalResourceError::Operation(
                    "the admitted identity pack has no measured residency decision".into(),
                )
            })?;

        let storage = FilesystemPackStorage::new(
            self.root.join("trusted-lifecycle-v1"),
            ArchivePolicy::default(),
        )
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let inventory = storage
            .active_installed_inventory(&model.identity.pack_id)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?
            .filter(|inventory| inventory.identity == model.identity)
            .ok_or_else(|| {
                LocalResourceError::Operation(
                    "the exact identity pack is not the protected active installed version".into(),
                )
            })?;
        if inventory.manifest_sha256 != model.manifest_sha256 {
            return Err(LocalResourceError::Operation(
                "the active installed identity tree is bound to a different manifest".into(),
            ));
        }
        let yunet_model =
            native_installed_file(&inventory, "models/face_detection_yunet_2026may.onnx")?;
        let sface_model =
            native_installed_file(&inventory, "models/face_recognition_sface_2021dec.onnx")?;
        let yunet_license = native_installed_file(&inventory, "licenses/YUNET-MIT.txt")?;
        let sface_license = native_installed_file(&inventory, "licenses/SFACE-APACHE-2.0.txt")?;
        let sface_model_card = native_installed_file(&inventory, "licenses/SFACE-MODEL-CARD.md")?;
        let opencv_zoo_commercial_use_report =
            native_installed_file(&inventory, "licenses/OPENCV-ZOO-COMMERCIAL-USE-REPORT.md")?;

        let authority_root = self
            .root
            .join("trusted-identity-runtime-v1")
            .join(model.identity.pack_id.as_str())
            .join(model.identity.revision.as_str());
        let signed: SignedIdentityRuntimeAuthorityV1 = read_bounded_regular_json(
            &authority_root.join("authority.json"),
            MAX_TRUST_METADATA_BYTES,
        )?
        .ok_or_else(|| {
            LocalResourceError::Operation(
                "no threshold-signed identity runtime/qualification authority is installed".into(),
            )
        })?;
        let authority_bytes = serde_json::to_vec(&signed.signed)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        if !manager.verifies_native_authority_payload(&authority_bytes, &signed.signatures) {
            return Err(LocalResourceError::Operation(
                "identity runtime authority signatures do not satisfy the catalog threshold".into(),
            ));
        }
        let authority = signed.signed;
        if authority.schema != IDENTITY_RUNTIME_AUTHORITY_SCHEMA_V1
            || authority.identity != model.identity
            || authority.manifest_sha256 != model.manifest_sha256
            || authority.manifest_raw_sha256 != trusted_pack.manifest_raw_sha256
            || authority.catalog_payload_sha256 != *manager.catalog_payload_sha256()
            || authority.measured_envelope_sha256 != model.measured_envelope_sha256
            || authority.admission_receipt_sha256 != admission_receipt_sha256
            || authority.runtime != measurement.runtime
            || authority.runtime_revision != measurement.runtime_revision
            || authority.backend != measurement.backend
            || authority.python_abi != "cp312-win_amd64"
        {
            return Err(LocalResourceError::Operation(
                "identity runtime authority does not bind the exact catalog, manifest, measurement, receipt, and runtime".into(),
            ));
        }
        authority
            .qualification
            .validate()
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;

        let resource_root = self.installed_resource_root.as_ref().ok_or_else(|| {
            LocalResourceError::Operation(
                "the native application resource root is unavailable for identity launch".into(),
            )
        })?;
        let manifest_file = verify_native_file_binding(
            resource_root,
            &authority.manifest_file,
            "packaging/model-packs/opencv-yunet-sface-private-evaluation.json",
        )?;
        if manifest_file.sha256 != trusted_pack.manifest_raw_sha256 {
            return Err(LocalResourceError::Operation(
                "the exact distributed identity v2 manifest changed".into(),
            ));
        }
        let worker_script = verify_native_file_binding(
            resource_root,
            &authority.worker_script,
            "workers/local-identity/worker.py",
        )?;
        let runtime_root = authority_root.join("runtime");
        let (runtime_files, python_executable) =
            verify_identity_runtime_tree(&runtime_root, &authority)?;
        let catalog_admission_sha256 = Sha256Digest::of_bytes(&authority_bytes);

        Ok(NativeAdmittedIdentityPackLaunchV1 {
            identity: model.identity,
            artifact_root: inventory.root,
            installed_content_tree_sha256: inventory.content_tree_sha256,
            yunet_model,
            sface_model,
            yunet_license,
            sface_license,
            sface_model_card,
            opencv_zoo_commercial_use_report,
            python_executable,
            worker_script,
            manifest_file,
            runtime_root,
            runtime_tree_sha256: authority.runtime_tree_sha256,
            runtime_files,
            runtime: measurement.runtime.clone(),
            runtime_revision: measurement.runtime_revision.clone(),
            backend: measurement.backend.clone(),
            placement: model.mode,
            exact_target_pid: active.exact_target_pid,
            measured_envelope_sha256: model.measured_envelope_sha256,
            admission_receipt_sha256,
            catalog_admission_sha256,
            admission_receipt: active.receipt,
            residency_decision,
            qualification: authority.qualification,
        })
    }

    pub fn advance_visual_clock(
        &self,
        generation_id: u64,
        frame_id: u64,
        now_monotonic_millis: u64,
    ) -> NativeVisualScheduleResultV1 {
        if self
            .active_admission
            .lock()
            .map_or(true, |active| active.is_none())
        {
            return visual_schedule_result(
                NativeVisualScheduleDispositionV1::BypassNoAdmission,
                "visual scheduling is fail-open because no sealed admission is active",
                None,
                None,
                None,
                Vec::new(),
            );
        }
        let mut planner = match self.loadout_planner.lock() {
            Ok(planner) => planner,
            Err(_) => return visual_state_bypass(),
        };
        let manager = match &mut *planner {
            NativeLoadoutPlanner::Ready { manager, .. } => manager,
            NativeLoadoutPlanner::Unavailable(detail) => {
                return visual_schedule_result(
                    NativeVisualScheduleDispositionV1::BypassPlannerUnavailable,
                    detail,
                    None,
                    None,
                    None,
                    Vec::new(),
                )
            }
        };
        match manager.advance_visual_clock(generation_id, frame_id, now_monotonic_millis) {
            Ok(cancellations) => {
                self.remove_visual_metadata(&cancellations);
                visual_schedule_result(
                    if cancellations.is_empty() {
                        NativeVisualScheduleDispositionV1::Ready
                    } else {
                        NativeVisualScheduleDispositionV1::BypassStale
                    },
                    if cancellations.is_empty() {
                        "authoritative visual generation/frame clock advanced"
                    } else {
                        "stale or expired visual work was dropped"
                    },
                    None,
                    None,
                    None,
                    cancellations,
                )
            }
            Err(error) => visual_schedule_result(
                NativeVisualScheduleDispositionV1::BypassStale,
                &format!("visual clock update was rejected: {error}"),
                None,
                None,
                None,
                Vec::new(),
            ),
        }
    }

    pub fn submit_screen_space_lip_sync(
        &self,
        work: NativeScreenSpaceLipSyncWorkV1,
        now_monotonic_millis: u64,
    ) -> NativeVisualScheduleResultV1 {
        if let Err(detail) = validate_native_visual_work(&work, now_monotonic_millis) {
            return visual_schedule_result(
                NativeVisualScheduleDispositionV1::BypassInvalid,
                &detail,
                None,
                Some(work),
                None,
                Vec::new(),
            );
        }
        if let Err(error) = self.resolve_admitted_openseeface_launch() {
            return visual_schedule_result(
                NativeVisualScheduleDispositionV1::BypassNoAdmission,
                &error.to_string(),
                None,
                Some(work),
                None,
                Vec::new(),
            );
        }
        let work_id = visual_work_id(&work);
        let mut planner = match self.loadout_planner.lock() {
            Ok(planner) => planner,
            Err(_) => return visual_state_bypass(),
        };
        let manager = match &mut *planner {
            NativeLoadoutPlanner::Ready { manager, .. } => manager,
            NativeLoadoutPlanner::Unavailable(detail) => {
                return visual_schedule_result(
                    NativeVisualScheduleDispositionV1::BypassPlannerUnavailable,
                    detail,
                    Some(work_id),
                    Some(work),
                    None,
                    Vec::new(),
                )
            }
        };
        let mut cancellations = Vec::new();
        if let Some(cancelled) = manager.cancel_work_kind(
            &work_id,
            &WorkKindV1::LipSync,
            CancellationReasonV1::CancelledByNativeRuntime,
        ) {
            cancellations.push(cancelled);
        }
        let item = WorkItemV1 {
            work_id: work_id.clone(),
            kind: WorkKindV1::LipSync,
            submitted_monotonic_millis: now_monotonic_millis,
            deadline_monotonic_millis: work.deadline_monotonic_millis,
            visual: Some(VisualWorkAddressV1 {
                generation_id: work.generation_id,
                frame_id: work.frame_id,
            }),
        };
        match manager.submit_work(item, now_monotonic_millis) {
            Ok(mut result) => {
                cancellations.append(&mut result.cancellations);
                self.remove_visual_metadata(&cancellations);
                if result.accepted {
                    if let Ok(mut metadata) = self.visual_work.lock() {
                        metadata.insert(work_id.clone(), work.clone());
                    } else {
                        manager.cancel_work_kind(
                            &work_id,
                            &WorkKindV1::LipSync,
                            CancellationReasonV1::CancelledByNativeRuntime,
                        );
                        return visual_state_bypass();
                    }
                }
                visual_schedule_result(
                    if result.accepted {
                        NativeVisualScheduleDispositionV1::Queued
                    } else {
                        NativeVisualScheduleDispositionV1::BypassStale
                    },
                    if result.accepted {
                        "exact actor/track/epoch/frame visual work was queued"
                    } else {
                        "visual work was stale, superseded, or over queue capacity"
                    },
                    Some(work_id),
                    Some(work),
                    None,
                    cancellations,
                )
            }
            Err(error) => visual_schedule_result(
                NativeVisualScheduleDispositionV1::BypassInvalid,
                &format!("visual work was rejected: {error}"),
                Some(work_id),
                Some(work),
                None,
                cancellations,
            ),
        }
    }

    /// Re-collects native pressure and drops only queued optional visual work
    /// in addition to the core priority policy. It never emits a PCM/global
    /// cancellation token.
    pub fn apply_current_pressure(
        &self,
        exact_target_pid: Option<u32>,
        now_monotonic_millis: u64,
    ) -> NativeVisualScheduleResultV1 {
        let snapshot = collect(TelemetryRequest {
            selected_game_pid: exact_target_pid,
            ..TelemetryRequest::default()
        });
        let pressure = native_resource_pressure(&snapshot);
        let mut planner = match self.loadout_planner.lock() {
            Ok(planner) => planner,
            Err(_) => return visual_state_bypass(),
        };
        let manager = match &mut *planner {
            NativeLoadoutPlanner::Ready { manager, .. } => manager,
            NativeLoadoutPlanner::Unavailable(detail) => {
                return visual_schedule_result(
                    NativeVisualScheduleDispositionV1::BypassPlannerUnavailable,
                    detail,
                    None,
                    None,
                    Some(pressure),
                    Vec::new(),
                )
            }
        };
        let mut cancellations = manager.apply_pressure(pressure, now_monotonic_millis);
        if pressure != ResourcePressureLevelV1::Normal {
            cancellations.extend(
                manager.cancel_all_work_kind(
                    &WorkKindV1::LipSync,
                    CancellationReasonV1::QueuePressure,
                ),
            );
        }
        self.remove_visual_metadata(&cancellations);
        visual_schedule_result(
            if pressure == ResourcePressureLevelV1::Normal {
                NativeVisualScheduleDispositionV1::Ready
            } else {
                NativeVisualScheduleDispositionV1::BypassPressure
            },
            if pressure == ResourcePressureLevelV1::Normal {
                "native resource pressure is within the configured ceiling"
            } else {
                "optional visual work was shed to protect the game and audio path"
            },
            None,
            None,
            Some(pressure),
            cancellations,
        )
    }

    pub fn pop_next_screen_space_lip_sync(
        &self,
        now_monotonic_millis: u64,
    ) -> NativeVisualScheduleResultV1 {
        let mut planner = match self.loadout_planner.lock() {
            Ok(planner) => planner,
            Err(_) => return visual_state_bypass(),
        };
        let manager = match &mut *planner {
            NativeLoadoutPlanner::Ready { manager, .. } => manager,
            NativeLoadoutPlanner::Unavailable(detail) => {
                return visual_schedule_result(
                    NativeVisualScheduleDispositionV1::BypassPlannerUnavailable,
                    detail,
                    None,
                    None,
                    None,
                    Vec::new(),
                )
            }
        };
        let Some(item) = manager.pop_next_work_kind(&WorkKindV1::LipSync, now_monotonic_millis)
        else {
            return visual_schedule_result(
                NativeVisualScheduleDispositionV1::NoWork,
                "no current visual work is queued",
                None,
                None,
                None,
                Vec::new(),
            );
        };
        let work = self
            .visual_work
            .lock()
            .ok()
            .and_then(|mut metadata| metadata.remove(&item.work_id));
        match work {
            Some(work) => visual_schedule_result(
                NativeVisualScheduleDispositionV1::Dispatched,
                "the highest-priority current visual work was dispatched",
                Some(item.work_id),
                Some(work),
                None,
                Vec::new(),
            ),
            None => visual_schedule_result(
                NativeVisualScheduleDispositionV1::BypassInvalid,
                "visual queue metadata was unavailable; the work was dropped",
                Some(item.work_id),
                None,
                None,
                Vec::new(),
            ),
        }
    }

    pub fn cancel_screen_space_lip_sync(&self, work_id: &str) -> NativeVisualScheduleResultV1 {
        let mut planner = match self.loadout_planner.lock() {
            Ok(planner) => planner,
            Err(_) => return visual_state_bypass(),
        };
        let manager = match &mut *planner {
            NativeLoadoutPlanner::Ready { manager, .. } => manager,
            NativeLoadoutPlanner::Unavailable(detail) => {
                return visual_schedule_result(
                    NativeVisualScheduleDispositionV1::BypassPlannerUnavailable,
                    detail,
                    Some(work_id.to_owned()),
                    None,
                    None,
                    Vec::new(),
                )
            }
        };
        let cancelled = manager.cancel_work_kind(
            work_id,
            &WorkKindV1::LipSync,
            CancellationReasonV1::CancelledByNativeRuntime,
        );
        let cancellations = cancelled.into_iter().collect::<Vec<_>>();
        self.remove_visual_metadata(&cancellations);
        visual_schedule_result(
            if cancellations.is_empty() {
                NativeVisualScheduleDispositionV1::NoWork
            } else {
                NativeVisualScheduleDispositionV1::Cancelled
            },
            if cancellations.is_empty() {
                "the exact visual work was not queued"
            } else {
                "the exact visual work was cancelled without touching audio"
            },
            Some(work_id.to_owned()),
            None,
            None,
            cancellations,
        )
    }

    fn remove_visual_metadata(&self, cancellations: &[WorkCancellationV1]) {
        if let Ok(mut metadata) = self.visual_work.lock() {
            for cancellation in cancellations {
                metadata.remove(&cancellation.work_id);
            }
        }
    }

    pub fn pack_state(&self) -> Result<ExperimentalPackState, LocalResourceError> {
        self.pack_state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| LocalResourceError::State)
    }

    /// Returns whether native state proves that this exact local lip-sync
    /// route is installed, hash-verified, device-admitted, and complete.
    ///
    /// The only lifecycle currently exposed here is OpenSeeFace, which is an
    /// experimental landmark/signal dependency and explicitly not a complete
    /// lip-sync model. It can therefore never satisfy this guard.
    pub(crate) fn admits_complete_lipsync_route(
        &self,
        _provider_id: &str,
        _model_id: &str,
    ) -> Result<bool, LocalResourceError> {
        let _state = self.pack_state()?;
        // No current state variant represents a signed whole-loadout
        // admission for a complete lip-sync runtime, so the proof is absent.
        Ok(false)
    }

    pub async fn install(
        &self,
        request: ExperimentalPackMutationRequest,
    ) -> Result<ExperimentalPackState, LocalResourceError> {
        validate_mutation(&request)?;
        ensure_local_review_build()?;
        let _operation = self.operation.lock().await;
        self.update_pack_state(|state| {
            state.phase = ExperimentalPackPhase::Downloading;
            state.detail = "Explicit user-authorized download is in progress; every artifact is pinned by byte length and SHA-256.".into();
        })?;
        match self.download_exact_pack().await {
            Ok(digests) => self.update_pack_state(|state| {
                state.phase = ExperimentalPackPhase::InstalledInactive;
                state.installed_artifact_sha256 = digests;
                state.detail = "Exact local-review artifacts are installed but inactive. Activation requires fresh whole-loadout telemetry and a signed device-bound measured envelope.".into();
            }),
            Err(error) => {
                let detail = error.to_string();
                let _ = self.update_pack_state(|state| {
                    state.phase = ExperimentalPackPhase::RepairRequired;
                    state.detail = format!("Install did not complete and remains inactive: {detail}");
                });
                Err(error)
            }
        }
    }

    pub fn activate(
        &self,
        request: ExperimentalPackMutationRequest,
        selected_game_pid: Option<u32>,
    ) -> Result<ExperimentalPackState, LocalResourceError> {
        validate_mutation(&request)?;
        ensure_local_review_build()?;
        let current = self.pack_state()?;
        if current.phase != ExperimentalPackPhase::InstalledInactive
            && current.phase != ExperimentalPackPhase::ActivationBlockedMissingMeasuredEnvelope
        {
            return Err(LocalResourceError::Invalid(
                "the exact pack must be installed and verified before activation".into(),
            ));
        }
        let settings = self.settings()?;
        ResourceGovernorV1::new(settings.governor.clone())
            .map_err(|error| LocalResourceError::Invalid(error.to_string()))?;
        let snapshot = collect(TelemetryRequest {
            selected_game_pid,
            ..TelemetryRequest::default()
        });
        let telemetry_detail = snapshot
            .admission_view(
                settings.game_reserve_vram_bytes,
                settings.game_additional_reserve_ram_bytes,
            )
            .map(|_| "live telemetry was complete")
            .unwrap_or("live telemetry was incomplete");
        self.update_pack_state(|state| {
            state.phase = ExperimentalPackPhase::ActivationBlockedMissingMeasuredEnvelope;
            state.detail = format!("Activation remains fail-closed: {telemetry_detail}, but the local-review measurement is not a signed current-device resource envelope and cannot authorize a beside-game loadout.");
        })
    }

    pub async fn repair(
        &self,
        request: ExperimentalPackMutationRequest,
    ) -> Result<ExperimentalPackState, LocalResourceError> {
        validate_mutation(&request)?;
        ensure_local_review_build()?;
        let _operation = self.operation.lock().await;
        match verify_installed(&self.pack_directory()) {
            Ok(digests) => self.update_pack_state(|state| {
                state.phase = ExperimentalPackPhase::InstalledInactive;
                state.installed_artifact_sha256 = digests;
                state.detail = "All exact artifacts passed byte-length and SHA-256 verification; the pack remains inactive pending admission.".into();
            }),
            Err(_) => self.install_without_lock().await,
        }
    }

    pub async fn remove(
        &self,
        request: ExperimentalPackMutationRequest,
    ) -> Result<ExperimentalPackState, LocalResourceError> {
        validate_mutation(&request)?;
        ensure_local_review_build()?;
        let _operation = self.operation.lock().await;
        let directory = self.pack_directory();
        match tokio::fs::remove_dir_all(&directory).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(LocalResourceError::Operation(error.to_string())),
        }
        self.update_pack_state(|state| *state = ExperimentalPackState::default())
    }

    async fn install_without_lock(&self) -> Result<ExperimentalPackState, LocalResourceError> {
        self.update_pack_state(|state| {
            state.phase = ExperimentalPackPhase::Downloading;
            state.detail = "Explicit repair download is in progress.".into();
        })?;
        let digests = self.download_exact_pack().await?;
        self.update_pack_state(|state| {
            state.phase = ExperimentalPackPhase::InstalledInactive;
            state.installed_artifact_sha256 = digests;
            state.detail = "Repair restored every exact artifact; the pack remains inactive pending admission.".into();
        })
    }

    async fn download_exact_pack(&self) -> Result<Vec<String>, LocalResourceError> {
        let manifest = openseeface_visual_signal_manifest_v1()
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        openseeface_visual_signal_contract_v1()
            .validate()
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let selection = MeasuredLocalPackSelectionPolicyV1
            .authorize(
                &manifest,
                PackSelectionRequestV1 {
                    selection_id: format!("explicit-{}", uuid::Uuid::new_v4().simple()),
                    origin: PackSelectionOriginV1::ExplicitUser,
                    action: PackSelectionActionV1::InstallOnly,
                    selected_unix_seconds: current_unix_seconds(),
                },
                None,
            )
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let directory = self.pack_directory();
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let journals = FileDownloadJournalStore::new(self.root.join("journals"))
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let downloader = HttpsArtifactDownloader::new(DownloadPolicy {
            maximum_artifact_bytes: 16 * 1024 * 1024,
            ..DownloadPolicy::default()
        })
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let cancellation = CancellationToken::new();
        let mut digests = Vec::new();
        for artifact in &manifest.artifacts {
            let destination = directory.join(&artifact.destination);
            let parent = destination.parent().ok_or_else(|| {
                LocalResourceError::Operation("artifact destination has no parent".into())
            })?;
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
            downloader
                .download(
                    &manifest.identity(),
                    artifact,
                    &selection,
                    &destination,
                    &journals,
                    &cancellation,
                )
                .await
                .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
            digests.push(artifact.sha256.as_str().to_owned());
        }
        verify_installed(&directory)
    }

    fn pack_directory(&self) -> PathBuf {
        self.root
            .join(OPENSEEFACE_VISUAL_SIGNAL_PACK_ID)
            .join(OPENSEEFACE_VISUAL_SIGNAL_REVISION)
    }

    fn update_pack_state(
        &self,
        update: impl FnOnce(&mut ExperimentalPackState),
    ) -> Result<ExperimentalPackState, LocalResourceError> {
        let mut state = self
            .pack_state
            .lock()
            .map_err(|_| LocalResourceError::State)?;
        update(&mut state);
        atomic_write_json(&self.pack_state_path, &*state)?;
        Ok(state.clone())
    }
}

fn validate_native_visual_work(
    work: &NativeScreenSpaceLipSyncWorkV1,
    now_monotonic_millis: u64,
) -> Result<(), String> {
    let valid_token = |value: &str| {
        (1..=128).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    };
    if !valid_token(&work.actor_id)
        || !valid_token(&work.track_id)
        || work.session_epoch == 0
        || work.generation_id == 0
        || work.frame_id == 0
        || work.deadline_monotonic_millis <= now_monotonic_millis
        || work
            .deadline_monotonic_millis
            .saturating_sub(now_monotonic_millis)
            > MAX_VISUAL_DEADLINE_MILLIS
    {
        return Err(
            "visual work requires bounded actor/track tokens, non-zero epoch/generation/frame, and a deadline within two seconds"
                .into(),
        );
    }
    Ok(())
}

fn visual_work_id(work: &NativeScreenSpaceLipSyncWorkV1) -> String {
    let bytes = serde_json::to_vec(work).expect("native visual work serialization is infallible");
    format!("visual-{}", Sha256Digest::of_bytes(&bytes).as_str())
}

fn optional_pack_state(
    pack: &TrustedReleasePackSnapshotV1,
    state: Option<&InstallState>,
) -> TrustedOptionalPackStateV1 {
    let phase = match state {
        None => TrustedOptionalPackPhaseV1::NotInstalled,
        Some(InstallState::Downloading) => TrustedOptionalPackPhaseV1::Downloading,
        Some(InstallState::Verifying) => TrustedOptionalPackPhaseV1::Verifying,
        Some(InstallState::Staging) => TrustedOptionalPackPhaseV1::Staging,
        Some(InstallState::AwaitingSelfTest | InstallState::Activating) => {
            TrustedOptionalPackPhaseV1::InstalledInactiveAwaitingSelfTest
        }
        Some(InstallState::Active) => TrustedOptionalPackPhaseV1::Active,
        Some(InstallState::Repairing) => TrustedOptionalPackPhaseV1::Repairing,
        Some(InstallState::Removing) => TrustedOptionalPackPhaseV1::Removing,
        Some(InstallState::RolledBack) => TrustedOptionalPackPhaseV1::RolledBack,
        Some(InstallState::Quarantined { .. }) => TrustedOptionalPackPhaseV1::RepairRequired,
    };
    let can_install = phase == TrustedOptionalPackPhaseV1::NotInstalled;
    let can_repair = matches!(
        phase,
        TrustedOptionalPackPhaseV1::InstalledInactiveAwaitingSelfTest
            | TrustedOptionalPackPhaseV1::Active
            | TrustedOptionalPackPhaseV1::RepairRequired
    );
    let can_remove = phase != TrustedOptionalPackPhaseV1::NotInstalled;
    let detail = match state {
        None => "Not installed. No download occurs without an explicit user mutation.".to_owned(),
        Some(InstallState::AwaitingSelfTest) => "Exact artifacts are verified and atomically installed as an immutable inactive version. No active pointer exists until a trusted runtime self-test and separate live whole-loadout admission succeed.".to_owned(),
        Some(InstallState::Quarantined { reason, .. }) => {
            format!("Verification quarantined this pack: {reason}")
        }
        Some(other) => format!("Native lifecycle state: {other:?}."),
    };
    TrustedOptionalPackStateV1 {
        identity: pack.identity.clone(),
        phase,
        explicit_download_required: pack.lifecycle.explicit_download_required,
        automatic_download_allowed: pack.lifecycle.automatic_download_allowed,
        license_acceptance_required: pack.license.acceptance_required,
        can_install,
        can_repair,
        can_remove,
        detail,
    }
}

fn visual_state_bypass() -> NativeVisualScheduleResultV1 {
    visual_schedule_result(
        NativeVisualScheduleDispositionV1::BypassPlannerUnavailable,
        "native visual scheduling state is unavailable",
        None,
        None,
        None,
        Vec::new(),
    )
}

fn visual_schedule_result(
    disposition: NativeVisualScheduleDispositionV1,
    detail: &str,
    work_id: Option<String>,
    work: Option<NativeScreenSpaceLipSyncWorkV1>,
    pressure: Option<ResourcePressureLevelV1>,
    cancellations: Vec<WorkCancellationV1>,
) -> NativeVisualScheduleResultV1 {
    NativeVisualScheduleResultV1 {
        schema_version: 1,
        disposition,
        detail: detail.to_owned(),
        work_id,
        work,
        pressure,
        cancellations,
    }
}

fn load_native_planner(
    config_directory: &Path,
    installed_resource_root: Option<&Path>,
    settings: &LocalResourceSettings,
    now_unix_seconds: u64,
) -> NativeLoadoutPlanner {
    let Some(resource_root) = installed_resource_root else {
        return NativeLoadoutPlanner::Unavailable(
            "Whole-loadout admission is unavailable because no native application resource root was resolved; no model is activated.".into(),
        );
    };
    let model_metadata_root = resource_root.join("packaging").join("model-packs");
    let result = (|| -> Result<
        (
            CoreLoadoutPlanner,
            Vec<TrustedReleasePackSnapshotV1>,
            String,
            TrustedLocalCatalogAuthorityV1,
        ),
        LocalResourceError,
    > {
        let (root, bundle) = load_release_catalog_bundle_v1(&model_metadata_root)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let verified = verify_release_catalog_bundle_v1(
            &root,
            &bundle,
            &CatalogTrustState::default(),
            now_unix_seconds,
        )
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        verify_release_manifest_files_v1(&model_metadata_root, &verified)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        verify_release_envelope_files_v1(&model_metadata_root, &verified, now_unix_seconds)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let packs = verified
            .trusted_pack_snapshots()
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let detail = if root.production_trust {
            "Production release-threshold model catalog is verified.".to_owned()
        } else {
            "Automated local-review bootstrap catalog is verified; production trust is false, rotation is required before release, and promotion/publication are unsupported.".to_owned()
        };
        let trust = TrustedLocalCatalogAuthorityV1 {
            trust_scope: root.trust_scope.clone(),
            production_trust: root.production_trust,
            rotation_required_before_release: root.rotation_required_before_release,
            promotion_supported: root.promotion_supported,
            publication_supported: root.publication_supported,
        };
        let catalog_qualified = verified
            .source_inventory
            .manifests
            .iter()
            .flat_map(|manifest| {
                manifest.qualified_envelopes.iter().map(|envelope| {
                    (
                        manifest.identity.clone(),
                        envelope.device_fingerprint_sha256.clone(),
                        envelope.placement.clone(),
                        model_metadata_root
                            .join("qual")
                            .join(format!("{}.json", envelope.envelope_sha256.as_str())),
                    )
                })
            })
            .collect();
        let manager = SelectedLoadoutManagerV1::new(
            verified.catalog,
            verified.verifier,
            FileResourceEnvelopeSource {
                roots: vec![
                    config_directory.join("model-measurements-v1"),
                    model_metadata_root.join("qual"),
                ],
                catalog_qualified,
            },
            MeasurementTrustPolicyV1 {
                signature_threshold: verified.signature_threshold,
                ..MeasurementTrustPolicyV1::default()
            },
            settings.governor.clone(),
            WorkQueuePolicyV1::default(),
        )
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        Ok((manager, packs, detail, trust))
    })();
    match result {
        Ok((manager, packs, detail, trust)) => NativeLoadoutPlanner::Ready {
            manager,
            packs,
            detail,
            trust,
        },
        Err(error) => NativeLoadoutPlanner::Unavailable(format!(
            "Whole-loadout admission is fail-closed: {error}. Installable catalog identities, signed current-device measurements, target PID, and live telemetry are all required."
        )),
    }
}

fn native_resource_pressure(snapshot: &ResourceTelemetrySnapshotV1) -> ResourcePressureLevelV1 {
    let Some(used) = snapshot.total_device_pressure_vram_bytes.value().copied() else {
        return ResourcePressureLevelV1::Critical;
    };
    let Some(budget) = snapshot.os_local_vram_budget_bytes.value().copied() else {
        return ResourcePressureLevelV1::Critical;
    };
    if budget == 0 || used >= budget {
        ResourcePressureLevelV1::Critical
    } else if used.saturating_mul(10) >= budget.saturating_mul(9) {
        ResourcePressureLevelV1::Elevated
    } else {
        ResourcePressureLevelV1::Normal
    }
}

fn validate_settings(settings: &LocalResourceSettings) -> Result<(), LocalResourceError> {
    if settings.schema_version != 1
        || settings.game_reserve_vram_bytes > 64 * 1024 * 1024 * 1024
        || settings.game_additional_reserve_ram_bytes > 128 * 1024 * 1024 * 1024
    {
        return Err(LocalResourceError::Invalid(
            "resource settings are outside their bounded schema".into(),
        ));
    }
    ResourceGovernorV1::new(settings.governor.clone())
        .map_err(|error| LocalResourceError::Invalid(error.to_string()))?;
    Ok(())
}

fn validate_mutation(request: &ExperimentalPackMutationRequest) -> Result<(), LocalResourceError> {
    if !request.explicit_user_confirmation {
        return Err(LocalResourceError::ConfirmationRequired);
    }
    if request.pack_id != OPENSEEFACE_VISUAL_SIGNAL_PACK_ID
        || request.revision != OPENSEEFACE_VISUAL_SIGNAL_REVISION
    {
        return Err(LocalResourceError::Invalid(
            "pack identity does not match the exact reviewed experimental option".into(),
        ));
    }
    Ok(())
}

fn ensure_local_review_build() -> Result<(), LocalResourceError> {
    if cfg!(debug_assertions) {
        Ok(())
    } else {
        Err(LocalResourceError::LocalReviewOnly)
    }
}

fn native_installed_file(
    inventory: &InstalledPackInventoryV1,
    relative_path: &str,
) -> Result<NativeVerifiedPackFileV1, LocalResourceError> {
    let file = inventory
        .files
        .iter()
        .find(|file| file.relative_path == relative_path)
        .ok_or_else(|| {
            LocalResourceError::Operation(format!(
                "the verified installed visual inventory is missing {relative_path}"
            ))
        })?;
    let path = inventory.root.join(relative_path);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != file.size_bytes
    {
        return Err(LocalResourceError::Operation(format!(
            "the verified installed visual file changed before launch: {relative_path}"
        )));
    }
    Ok(NativeVerifiedPackFileV1 {
        path,
        size_bytes: file.size_bytes,
        sha256: file.sha256.clone(),
    })
}

fn supported_openseeface_visual_identity(identity: &PackRevision) -> bool {
    identity.revision.as_str() == OPENSEEFACE_VISUAL_SIGNAL_REVISION
        && matches!(
            identity.pack_id.as_str(),
            OPENSEEFACE_VISUAL_SIGNAL_PACK_ID | YUNET_OPENSEEFACE_VISUAL_SIGNAL_PACK_ID
        )
}

fn verify_native_file_binding(
    root: &Path,
    binding: &NativeFileBindingV1,
    expected_relative_path: &str,
) -> Result<NativeVerifiedPackFileV1, LocalResourceError> {
    if binding.relative_path != expected_relative_path {
        return Err(LocalResourceError::Operation(format!(
            "native authority path does not match the required file: {expected_relative_path}"
        )));
    }
    let relative = npc_model_manager::validate_relative_archive_path(&binding.relative_path)
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    verify_exact_bound_file(root, relative.as_str(), binding.size_bytes, &binding.sha256)
}

fn verify_exact_bound_file(
    root: &Path,
    relative_path: &str,
    size_bytes: u64,
    sha256: &Sha256Digest,
) -> Result<NativeVerifiedPackFileV1, LocalResourceError> {
    let path = root.join(relative_path);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != size_bytes {
        return Err(LocalResourceError::Operation(format!(
            "native authority file is linked, absent, or changed: {relative_path}"
        )));
    }
    let file =
        fs::File::open(&path).map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    verify_artifact(relative_path, size_bytes, sha256, file)
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    Ok(NativeVerifiedPackFileV1 {
        path,
        size_bytes,
        sha256: sha256.clone(),
    })
}

fn verify_identity_runtime_tree(
    runtime_root: &Path,
    authority: &IdentityRuntimeAuthorityPayloadV1,
) -> Result<(Vec<NativeVerifiedPackFileV1>, NativeVerifiedPackFileV1), LocalResourceError> {
    let metadata = fs::symlink_metadata(runtime_root)
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(LocalResourceError::Operation(
            "identity runtime root is linked or not a directory".into(),
        ));
    }
    if !(6..=4_096).contains(&authority.runtime_files.len()) {
        return Err(LocalResourceError::Operation(
            "identity runtime authority has an invalid closed-world file count".into(),
        ));
    }
    let mut declared = authority.runtime_files.clone();
    declared.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let mut paths = BTreeSet::new();
    let roles = declared
        .iter()
        .map(|file| file.role.clone())
        .collect::<BTreeSet<_>>();
    let required_roles = BTreeSet::from([
        IdentityRuntimeFileRoleV1::PythonExecutable,
        IdentityRuntimeFileRoleV1::OpencvRuntime,
        IdentityRuntimeFileRoleV1::NumpyRuntime,
        IdentityRuntimeFileRoleV1::OpencvLicense,
        IdentityRuntimeFileRoleV1::OpencvThirdPartyNotices,
        IdentityRuntimeFileRoleV1::NumpyLicense,
    ]);
    if !required_roles.is_subset(&roles)
        || declared
            .iter()
            .filter(|file| file.role == IdentityRuntimeFileRoleV1::PythonExecutable)
            .count()
            != 1
    {
        return Err(LocalResourceError::Operation(
            "identity runtime authority omits a required Python/OpenCV/NumPy runtime, license, or notice role".into(),
        ));
    }
    for file in &declared {
        let relative = npc_model_manager::validate_relative_archive_path(&file.relative_path)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        if file.size_bytes == 0 || !paths.insert(relative.as_str().to_ascii_lowercase()) {
            return Err(LocalResourceError::Operation(
                "identity runtime authority contains an empty or duplicate file".into(),
            ));
        }
    }
    let encoded = serde_json::to_vec(&declared)
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    let mut tree_bytes = b"npc.identity-runtime-tree/v1\0".to_vec();
    tree_bytes.extend_from_slice(&encoded);
    if Sha256Digest::of_bytes(&tree_bytes) != authority.runtime_tree_sha256 {
        return Err(LocalResourceError::Operation(
            "identity runtime authority tree digest is inconsistent".into(),
        ));
    }
    let mut actual_paths = BTreeSet::new();
    collect_runtime_files(runtime_root, runtime_root, &mut actual_paths)?;
    if actual_paths != paths {
        return Err(LocalResourceError::Operation(
            "identity runtime directory contains missing or unlisted files".into(),
        ));
    }
    let mut verified = Vec::with_capacity(declared.len());
    let mut python = None;
    for file in declared {
        let exact = verify_exact_bound_file(
            runtime_root,
            &file.relative_path,
            file.size_bytes,
            &file.sha256,
        )?;
        if file.role == IdentityRuntimeFileRoleV1::PythonExecutable {
            python = Some(exact.clone());
        }
        verified.push(exact);
    }
    Ok((
        verified,
        python.ok_or_else(|| {
            LocalResourceError::Operation(
                "identity runtime authority has no exact Python executable".into(),
            )
        })?,
    ))
}

fn collect_runtime_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<String>,
) -> Result<(), LocalResourceError> {
    if files.len() > 4_096 {
        return Err(LocalResourceError::Operation(
            "identity runtime contains too many files".into(),
        ));
    }
    for entry in
        fs::read_dir(directory).map_err(|error| LocalResourceError::Operation(error.to_string()))?
    {
        let entry = entry.map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(LocalResourceError::Operation(
                "identity runtime contains a linked file or directory".into(),
            ));
        }
        if metadata.is_dir() {
            collect_runtime_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| LocalResourceError::Operation("runtime file escaped root".into()))?
                .to_str()
                .ok_or_else(|| LocalResourceError::Operation("runtime path is not UTF-8".into()))?
                .replace('\\', "/");
            let relative = npc_model_manager::validate_relative_archive_path(&relative)
                .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
            if !files.insert(relative.as_str().to_ascii_lowercase()) {
                return Err(LocalResourceError::Operation(
                    "identity runtime contains a case-fold path collision".into(),
                ));
            }
        } else {
            return Err(LocalResourceError::Operation(
                "identity runtime contains a non-regular entry".into(),
            ));
        }
    }
    Ok(())
}

fn verify_installed(directory: &Path) -> Result<Vec<String>, LocalResourceError> {
    let manifest = openseeface_visual_signal_manifest_v1()
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    let mut digests = Vec::new();
    for artifact in manifest.artifacts {
        let path = directory.join(&artifact.destination);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(LocalResourceError::Operation(
                "installed artifact is not a regular file".into(),
            ));
        }
        let file = fs::File::open(&path)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        verify_artifact(&artifact.id, artifact.size_bytes, &artifact.sha256, file)
            .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
        digests.push(artifact.sha256.as_str().to_owned());
    }
    Ok(digests)
}

fn load_json_or_default<T>(path: &Path) -> T
where
    T: for<'de> Deserialize<'de> + Default,
{
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return T::default();
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 512 * 1024 {
        return T::default();
    }
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn load_optional_json<T>(path: &Path) -> Option<T>
where
    T: for<'de> Deserialize<'de>,
{
    read_bounded_regular_json(path, 512 * 1024).ok().flatten()
}

fn read_bounded_regular_json<T>(
    path: &Path,
    maximum_bytes: u64,
) -> Result<Option<T>, LocalResourceError>
where
    T: for<'de> Deserialize<'de>,
{
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(LocalResourceError::Operation(error.to_string())),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > maximum_bytes {
        return Err(LocalResourceError::Operation(format!(
            "trusted metadata path is linked, oversized, or not a regular file: {}",
            path.display()
        )));
    }
    let bytes = fs::read(path).map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| LocalResourceError::Operation(error.to_string()))
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), LocalResourceError> {
    let parent = path.parent().ok_or_else(|| {
        LocalResourceError::Operation("persistent state path has no parent".into())
    })?;
    fs::create_dir_all(parent).map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| LocalResourceError::Operation(error.to_string()))?;
    temporary
        .persist(path)
        .map_err(|error| LocalResourceError::Operation(error.error.to_string()))?;
    Ok(())
}

fn current_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(confirmed: bool) -> ExperimentalPackMutationRequest {
        ExperimentalPackMutationRequest {
            pack_id: OPENSEEFACE_VISUAL_SIGNAL_PACK_ID.into(),
            revision: OPENSEEFACE_VISUAL_SIGNAL_REVISION.into(),
            explicit_user_confirmation: confirmed,
        }
    }

    #[test]
    fn mutations_require_exact_identity_and_explicit_confirmation() {
        assert!(matches!(
            validate_mutation(&request(false)),
            Err(LocalResourceError::ConfirmationRequired)
        ));
        let mut wrong = request(true);
        wrong.revision = "latest".into();
        assert!(validate_mutation(&wrong).is_err());
        assert!(validate_mutation(&request(true)).is_ok());
    }

    #[test]
    fn admitted_visual_identity_allows_only_the_two_frozen_provider_ids() {
        let identity = |pack_id: &str, revision: &str| PackRevision {
            pack_id: npc_model_manager::PackId::parse(pack_id).expect("pack id"),
            revision: npc_model_manager::Revision::parse(revision).expect("revision"),
        };
        assert!(supported_openseeface_visual_identity(&identity(
            OPENSEEFACE_VISUAL_SIGNAL_PACK_ID,
            OPENSEEFACE_VISUAL_SIGNAL_REVISION,
        )));
        assert!(supported_openseeface_visual_identity(&identity(
            YUNET_OPENSEEFACE_VISUAL_SIGNAL_PACK_ID,
            OPENSEEFACE_VISUAL_SIGNAL_REVISION,
        )));
        assert!(!supported_openseeface_visual_identity(&identity(
            "openseeface-yunet640-lm1-unreviewed",
            OPENSEEFACE_VISUAL_SIGNAL_REVISION,
        )));
        assert!(!supported_openseeface_visual_identity(&identity(
            YUNET_OPENSEEFACE_VISUAL_SIGNAL_PACK_ID,
            "latest",
        )));
    }

    #[test]
    fn same_pack_optional_mutations_reject_concurrency_cancel_active_and_cleanup_exactly() {
        fn race(left: NativeOptionalMutationKindV1, right: NativeOptionalMutationKindV1) {
            let directory = tempfile::tempdir().expect("tempdir");
            let manager = std::sync::Arc::new(
                LocalResourceManager::new(directory.path(), None).expect("manager"),
            );
            let identity = PackRevision {
                pack_id: npc_model_manager::PackId::parse("optional.fixture").expect("pack"),
                revision: npc_model_manager::Revision::parse("r1").expect("revision"),
            };
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
            let spawn = |kind| {
                let manager = std::sync::Arc::clone(&manager);
                let identity = identity.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    manager.begin_optional_mutation(&identity, kind)
                })
            };
            let first = spawn(left);
            let second = spawn(right);
            barrier.wait();
            let results = [first.join().expect("first"), second.join().expect("second")];
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
            assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);

            let active = manager
                .optional_downloads
                .lock()
                .expect("downloads")
                .get(&identity)
                .cloned()
                .expect("active mutation");
            assert!(manager.cancel_optional_mutation(&identity).expect("cancel"));
            assert!(active.cancellation.is_cancelled());

            manager
                .finish_optional_mutation(&identity, uuid::Uuid::new_v4())
                .expect("ignore stale cleanup");
            assert!(manager
                .optional_downloads
                .lock()
                .expect("downloads")
                .contains_key(&identity));
            manager
                .finish_optional_mutation(&identity, active.operation_id)
                .expect("cleanup active");
            assert!(!manager
                .optional_downloads
                .lock()
                .expect("downloads")
                .contains_key(&identity));
        }

        race(
            NativeOptionalMutationKindV1::Install,
            NativeOptionalMutationKindV1::Install,
        );
        race(
            NativeOptionalMutationKindV1::Install,
            NativeOptionalMutationKindV1::Repair,
        );
        race(
            NativeOptionalMutationKindV1::Install,
            NativeOptionalMutationKindV1::Activate,
        );
        race(
            NativeOptionalMutationKindV1::Repair,
            NativeOptionalMutationKindV1::Activate,
        );
        race(
            NativeOptionalMutationKindV1::Activate,
            NativeOptionalMutationKindV1::Activate,
        );
    }

    #[test]
    fn activation_mutation_blocks_target_admission_until_final_revocation() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager = LocalResourceManager::new(directory.path(), None).expect("manager");
        let identity = PackRevision {
            pack_id: npc_model_manager::PackId::parse(YUNET_OPENSEEFACE_VISUAL_SIGNAL_PACK_ID)
                .expect("pack"),
            revision: npc_model_manager::Revision::parse(OPENSEEFACE_VISUAL_SIGNAL_REVISION)
                .expect("revision"),
        };
        let mutation = manager
            .begin_optional_mutation(&identity, NativeOptionalMutationKindV1::Activate)
            .expect("activation barrier");
        let result = manager
            .admit_selected_loadout(
                SelectedLoadoutSelectionV1 {
                    selection_id: "blocked-during-activation".into(),
                    roles: Vec::new(),
                    expected_idle_millis: 0,
                },
                Some(42),
            )
            .expect("actionable blocked result");
        assert!(!result.ready);
        assert!(!result.persisted);
        assert!(result.decision.is_none());
        assert!(result.detail.contains("activation self-test is running"));
        assert!(manager
            .active_admission
            .lock()
            .expect("admission")
            .is_none());
        manager
            .finish_optional_mutation(&identity, mutation.operation_id)
            .expect("finish barrier");
    }

    #[test]
    fn policy_persistence_round_trips_and_rejects_invalid_governor() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager = LocalResourceManager::new(directory.path(), None).expect("manager");
        let mut settings = LocalResourceSettings::default();
        settings.governor.vram_soft_ceiling_basis_points = 8_000;
        manager.save_settings(settings.clone()).expect("save");
        let reopened = LocalResourceManager::new(directory.path(), None).expect("reopen");
        assert_eq!(reopened.settings().expect("settings"), settings);

        let mut invalid = settings;
        invalid.governor.vram_soft_ceiling_basis_points = 0;
        assert!(reopened.save_settings(invalid).is_err());
    }

    #[test]
    fn installed_artifacts_are_all_exact_hashes_or_rejected() {
        let directory = tempfile::tempdir().expect("tempdir");
        assert!(verify_installed(directory.path()).is_err());
    }

    #[test]
    fn openseeface_signal_pack_never_authorizes_complete_lipsync() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager = LocalResourceManager::new(directory.path(), None).expect("manager");
        assert!(!manager
            .admits_complete_lipsync_route("local-visual-worker", "musetalk")
            .expect("admission state"));
        let state = manager.pack_state().expect("pack state");
        assert_eq!(state.pack_id, OPENSEEFACE_VISUAL_SIGNAL_PACK_ID);
        assert!(!state.complete_lip_sync_model);
    }

    #[test]
    fn identity_launch_is_product_reachable_but_returns_no_private_path_without_trust() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager = LocalResourceManager::new(directory.path(), None).expect("manager");
        let error = manager.admitted_identity_launch().expect_err("blocked");
        assert!(error.to_string().contains("identity launch is unavailable"));
    }

    #[test]
    fn identity_runtime_closed_world_rehash_rejects_tamper_and_unlisted_files() {
        use npc_identity_engine::{EmbeddingModelV1, IdentityConfigV1};

        let directory = tempfile::tempdir().expect("tempdir");
        let runtime_root = directory.path().join("runtime");
        fs::create_dir_all(runtime_root.join("bin")).expect("runtime dirs");
        fs::create_dir_all(runtime_root.join("licenses")).expect("license dirs");
        let specifications = [
            (
                IdentityRuntimeFileRoleV1::PythonExecutable,
                "bin/python.exe",
                b"python".as_slice(),
            ),
            (
                IdentityRuntimeFileRoleV1::OpencvRuntime,
                "bin/cv2.pyd",
                b"opencv".as_slice(),
            ),
            (
                IdentityRuntimeFileRoleV1::NumpyRuntime,
                "bin/numpy.pyd",
                b"numpy".as_slice(),
            ),
            (
                IdentityRuntimeFileRoleV1::OpencvLicense,
                "licenses/OPENCV.txt",
                b"opencv license".as_slice(),
            ),
            (
                IdentityRuntimeFileRoleV1::OpencvThirdPartyNotices,
                "licenses/OPENCV-THIRD-PARTY.txt",
                b"opencv notices".as_slice(),
            ),
            (
                IdentityRuntimeFileRoleV1::NumpyLicense,
                "licenses/NUMPY.txt",
                b"numpy license".as_slice(),
            ),
        ];
        let mut files = Vec::new();
        for (role, path, bytes) in specifications {
            fs::write(runtime_root.join(path), bytes).expect("runtime file");
            files.push(IdentityRuntimeFileBindingV1 {
                role,
                relative_path: path.into(),
                size_bytes: bytes.len() as u64,
                sha256: Sha256Digest::of_bytes(bytes),
            });
        }
        files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let mut tree = b"npc.identity-runtime-tree/v1\0".to_vec();
        tree.extend_from_slice(&serde_json::to_vec(&files).expect("tree json"));
        let authority = IdentityRuntimeAuthorityPayloadV1 {
            schema: IDENTITY_RUNTIME_AUTHORITY_SCHEMA_V1.into(),
            identity: PackRevision {
                pack_id: npc_model_manager::PackId::parse(IDENTITY_PACK_ID).expect("pack"),
                revision: npc_model_manager::Revision::parse(IDENTITY_PACK_REVISION)
                    .expect("revision"),
            },
            manifest_sha256: Sha256Digest::of_bytes(b"manifest"),
            manifest_raw_sha256: Sha256Digest::of_bytes(b"raw manifest"),
            catalog_payload_sha256: Sha256Digest::of_bytes(b"catalog"),
            measured_envelope_sha256: Sha256Digest::of_bytes(b"envelope"),
            admission_receipt_sha256: Sha256Digest::of_bytes(b"receipt"),
            runtime: "opencv-python-headless".into(),
            runtime_revision: "cp312-opencv-5.0.0.93-numpy-2.5.2".into(),
            backend: "opencv-dnn-cpu".into(),
            python_abi: "cp312-win_amd64".into(),
            runtime_tree_sha256: Sha256Digest::of_bytes(&tree),
            runtime_files: files,
            worker_script: NativeFileBindingV1 {
                relative_path: "workers/local-identity/worker.py".into(),
                size_bytes: 1,
                sha256: Sha256Digest::of_bytes(b"worker"),
            },
            manifest_file: NativeFileBindingV1 {
                relative_path: "packaging/model-packs/opencv-yunet-sface-private-evaluation.json"
                    .into(),
                size_bytes: 1,
                sha256: Sha256Digest::of_bytes(b"manifest file"),
            },
            qualification: PinnedIdentityQualificationV1 {
                qualification_id: "identity-fixture".into(),
                model: EmbeddingModelV1 {
                    provider: "opencv-zoo".into(),
                    model_id: "sface".into(),
                    revision: "fixture".into(),
                    dimensions: 128,
                },
                detector_id: "yunet".into(),
                detector_revision: "fixture".into(),
                preprocessing: "fixture".into(),
                calibration_fixture_sha256: "ab".repeat(32),
                calibrated_config: IdentityConfigV1::default(),
            },
        };

        let (verified, python) =
            verify_identity_runtime_tree(&runtime_root, &authority).expect("verified runtime");
        assert_eq!(verified.len(), 6);
        assert!(python.path.ends_with("python.exe"));

        fs::write(runtime_root.join("bin/python.exe"), b"tampered").expect("tamper");
        assert!(verify_identity_runtime_tree(&runtime_root, &authority).is_err());
        fs::write(runtime_root.join("bin/python.exe"), b"python").expect("restore");
        fs::write(runtime_root.join("bin/unlisted.dll"), b"unlisted").expect("unlisted");
        assert!(verify_identity_runtime_tree(&runtime_root, &authority).is_err());
    }

    #[test]
    fn native_visual_scheduler_fails_open_without_a_sealed_admission() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager = LocalResourceManager::new(directory.path(), None).expect("manager");
        let work = NativeScreenSpaceLipSyncWorkV1 {
            actor_id: "actor-1".into(),
            track_id: "track-1".into(),
            session_epoch: 1,
            generation_id: 1,
            frame_id: 1,
            deadline_monotonic_millis: 1_100,
        };

        let submitted = manager.submit_screen_space_lip_sync(work, 1_000);
        assert_eq!(
            submitted.disposition,
            NativeVisualScheduleDispositionV1::BypassNoAdmission
        );
        assert!(submitted.cancellations.is_empty());

        let advanced = manager.advance_visual_clock(1, 1, 1_000);
        assert_eq!(
            advanced.disposition,
            NativeVisualScheduleDispositionV1::BypassNoAdmission
        );
        assert!(advanced.cancellations.is_empty());
    }

    #[test]
    fn native_visual_scheduler_rejects_unbounded_or_stale_native_work() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager = LocalResourceManager::new(directory.path(), None).expect("manager");
        let invalid = NativeScreenSpaceLipSyncWorkV1 {
            actor_id: String::new(),
            track_id: "track-1".into(),
            session_epoch: 1,
            generation_id: 1,
            frame_id: 1,
            deadline_monotonic_millis: 10_000,
        };

        let result = manager.submit_screen_space_lip_sync(invalid, 1_000);
        assert_eq!(
            result.disposition,
            NativeVisualScheduleDispositionV1::BypassInvalid
        );
        assert!(result.cancellations.is_empty());
    }

    #[test]
    fn selected_loadout_command_cannot_mint_native_admission_context() {
        let crafted = r#"{
            "selection_id":"crafted-selection",
            "roles":[],
            "expected_idle_millis":100,
            "exact_target_pid":4242
        }"#;
        assert!(serde_json::from_str::<SelectedLoadoutSelectionV1>(crafted).is_err());

        let directory = tempfile::tempdir().expect("tempdir");
        let manager = LocalResourceManager::new(directory.path(), None).expect("manager");
        let planner = manager.selected_loadout_planner().expect("planner state");
        assert!(!planner.ready);
        assert!(planner.planner.is_none());
        assert!(planner.detail.contains("fail") || planner.detail.contains("unavailable"));
        let catalog = manager
            .trusted_pack_catalog()
            .expect("trusted catalog state");
        assert!(!catalog.ready);
        assert!(catalog.packs.is_empty());
        assert!(catalog.detail.contains("fail") || catalog.detail.contains("unavailable"));
    }

    #[test]
    fn checked_bootstrap_catalog_makes_native_planner_reachable_but_not_measured() {
        let config = tempfile::tempdir().expect("config");
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let (_, bundle) =
            load_release_catalog_bundle_v1(&repo_root.join("packaging").join("model-packs"))
                .expect("bundled catalog");
        let verified_now = bundle.signed.generated_unix_seconds + 1;
        let planner = load_native_planner(
            config.path(),
            Some(&repo_root),
            &LocalResourceSettings::default(),
            verified_now,
        );
        match planner {
            NativeLoadoutPlanner::Ready { packs, detail, .. } => {
                assert_eq!(packs.len(), 6);
                assert!(detail.contains("local-review bootstrap"));
                assert!(packs.iter().all(|pack| {
                    pack.measurement_status
                        == npc_model_manager::TrustedPackMeasurementStatusV1::Unavailable
                        && pack.qualified_measurement.is_none()
                }));
                assert_eq!(
                    packs
                        .iter()
                        .map(|pack| pack.qualified_envelope_count)
                        .sum::<usize>(),
                    2
                );
            }
            NativeLoadoutPlanner::Unavailable(detail) => {
                panic!("checked bootstrap catalog must verify: {detail}")
            }
        }

        let missing = tempfile::tempdir().expect("missing resource root");
        assert!(matches!(
            load_native_planner(
                config.path(),
                Some(missing.path()),
                &LocalResourceSettings::default(),
                verified_now,
            ),
            NativeLoadoutPlanner::Unavailable(_)
        ));
    }

    #[cfg(windows)]
    fn real_provider_probe(
        worker: PathBuf,
        resource_root: &Path,
        state_root: &Path,
    ) -> crate::visual_runtime::MouthWorkerSupervisor {
        let current = std::env::current_exe().expect("test executable");
        let parent = crate::sidecar_supervisor::RuntimeSupervisor::try_new(
            crate::sidecar_supervisor::RuntimeLaunchConfig {
                executable: current.clone(),
                resource_root: resource_root.to_path_buf(),
                app_data: state_root.join("runtime-host-data"),
                development_fixture_allowed: true,
                application_namespace:
                    interactive_npcs_credential_vault::PRODUCTION_APPLICATION_NAMESPACE.into(),
            },
        )
        .expect("kill-on-close parent job");
        let broker = crate::media_broker::MediaBrokerSupervisor::new(
            crate::media_broker::MediaBrokerLaunchConfig {
                executable: current,
                development_fixture_allowed: false,
                audio_output_selection_path: state_root.join("audio-output-selection-v1.json"),
                #[cfg(debug_assertions)]
                debug_synthetic_metadata_path: state_root.join("synthetic-target.json"),
            },
            parent.clone(),
        );
        crate::visual_runtime::MouthWorkerSupervisor::new(
            crate::visual_runtime::MouthWorkerLaunchConfig {
                executable: worker,
                development_fixture_allowed: false,
                #[cfg(debug_assertions)]
                review_openseeface_root: None,
                #[cfg(debug_assertions)]
                review_mouth_atlas_root: None,
            },
            parent,
            broker,
        )
    }

    /// Opt-in proof for the private signed YuNet catalog and cached official
    /// artifacts. It intentionally leaves its isolated state directory intact
    /// so the inactive/active inventories and receipt can be reviewed after
    /// the test. No game, GPU, audio device, WebView, or provider API is used.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires the private signed catalog, exact cached official artifacts, and a built mouth worker"]
    async fn real_private_catalog_provider_activation_is_hidden_retryable_and_inventory_bound() {
        fn required_path(name: &str) -> PathBuf {
            std::env::var_os(name)
                .map(PathBuf::from)
                .unwrap_or_else(|| panic!("{name} is required"))
        }

        let catalog_root = required_path("NPC_REAL_PROVIDER_CATALOG_ROOT");
        let artifact_root = required_path("NPC_REAL_PROVIDER_ARTIFACT_ROOT");
        let yunet_license = required_path("NPC_REAL_YUNET_LICENSE");
        let openseeface_license = required_path("NPC_REAL_OPENSEEFACE_LICENSE");
        let worker = required_path("NPC_REAL_MOUTH_WORKER");
        let state_root = required_path("NPC_REAL_PROVIDER_STATE_ROOT");
        assert!(catalog_root.is_absolute());
        assert!(artifact_root.is_absolute());
        assert!(worker.is_absolute());
        assert!(state_root.is_absolute());
        assert!(worker.is_file(), "built worker is missing");
        assert!(
            !state_root.exists(),
            "state root must be fresh so no prior active pointer can satisfy the proof"
        );

        let model_catalog_root = state_root
            .join("resource-root")
            .join("packaging")
            .join("model-packs");
        fs::create_dir_all(model_catalog_root.join("qual")).expect("catalog directories");
        for relative in [
            "model-catalog-root-v1.json",
            "model-catalog-v1.json",
            "openseeface-yunet640-lm1-mouth-signal.json",
            "qual/c35d184bc8f69b833c1da3fdf38a731b977b2612b565bc58cae1989f75990896.json",
        ] {
            let source = catalog_root.join(relative);
            let destination = model_catalog_root.join(relative);
            fs::copy(&source, &destination)
                .unwrap_or_else(|error| panic!("copy {}: {error}", source.display()));
        }

        let config_root = state_root.join("config");
        let resource_root = state_root.join("resource-root");
        let manager = LocalResourceManager::new(&config_root, Some(&resource_root))
            .expect("private catalog manager");
        let identity = PackRevision {
            pack_id: npc_model_manager::PackId::parse(YUNET_OPENSEEFACE_VISUAL_SIGNAL_PACK_ID)
                .expect("pack id"),
            revision: npc_model_manager::Revision::parse(OPENSEEFACE_VISUAL_SIGNAL_REVISION)
                .expect("revision"),
        };

        let artifact_sources = [
            (
                "yunet-2023mar-onnx",
                artifact_root.join("models/face_detection_yunet_2023mar.onnx"),
                232_589,
                YUNET_OPENSEEFACE_DETECTOR_SHA256,
            ),
            (
                "yunet-license",
                yunet_license,
                1_085,
                "c83b8120c50ccbd4c4f96edf53141bdd566ebb8f8e9227e415326aa1b1aba958",
            ),
            (
                "lm-model1-opt",
                artifact_root.join("models/lm_model1_opt.onnx"),
                4_842_329,
                OPENSEEFACE_LM1_SHA256,
            ),
            (
                "openseeface-license",
                openseeface_license,
                1_364,
                "28612834d7ca038a9009550e3869a67e6be3a87c238d997f58c0907e08744146",
            ),
            (
                "onnxruntime-1.22.1-cpu-windows-x64",
                artifact_root.join("onnxruntime-win-x64-1.22.1.zip"),
                73_731_806,
                "855276cd4be3cda14fe636c69eb038d75bf5bcd552bda1193a5d79c51f436dfe",
            ),
        ];
        let mut evidence = Vec::with_capacity(artifact_sources.len());
        {
            let mut lifecycle = manager.optional_lifecycle.lock().await;
            let lifecycle = lifecycle.as_mut().expect("trusted optional lifecycle");
            for (artifact_id, source, size, sha256) in artifact_sources {
                let expected = Sha256Digest::parse(sha256).expect("artifact digest");
                let verified = verify_artifact(
                    artifact_id,
                    size,
                    &expected,
                    fs::File::open(&source)
                        .unwrap_or_else(|error| panic!("open {}: {error}", source.display())),
                )
                .unwrap_or_else(|error| panic!("verify {}: {error}", source.display()));
                let destination = lifecycle
                    .manager()
                    .storage()
                    .download_path(&identity, artifact_id)
                    .expect("download destination");
                fs::create_dir_all(destination.parent().expect("download parent"))
                    .expect("download parent directory");
                fs::copy(&source, &destination)
                    .unwrap_or_else(|error| panic!("stage {}: {error}", source.display()));
                evidence.push(verified);
            }
            let state = lifecycle
                .import_verified_downloads(
                    &identity,
                    "real-private-yunet-install".into(),
                    current_unix_seconds(),
                    true,
                    false,
                    evidence,
                )
                .expect("verified offline import");
            assert_eq!(state, InstallState::AwaitingSelfTest);
            assert!(lifecycle
                .manager()
                .storage()
                .active_installed_inventory(&identity.pack_id)
                .expect("inactive inventory query")
                .is_none());
        }

        let selection = SelectedLoadoutSelectionV1 {
            selection_id: "real-private-yunet-setup-selection".into(),
            roles: vec![npc_model_manager::SelectedPackV1 {
                role: ModelPackKindV1::Vision,
                identity: identity.clone(),
                preferred_residency: ResidencyModeV1::CpuResident,
            }],
            expected_idle_millis: 1_000,
        };
        let request = TrustedOptionalPackActivationRequestV1 {
            pack_id: identity.pack_id.as_str().into(),
            revision: identity.revision.as_str().into(),
            explicit_user_confirmation: true,
            selection: selection.clone(),
        };

        // Seed an older target-shaped receipt around the same setup fit. This
        // cannot be reached from production APIs without target telemetry; it
        // is used here only to prove activation revokes stale runtime storage.
        let telemetry = collect(TelemetryRequest::default());
        let settings = manager.settings().expect("settings");
        let setup_decision = {
            let mut planner = manager.loadout_planner.lock().expect("planner");
            match &mut *planner {
                NativeLoadoutPlanner::Ready { manager, .. } => manager.admit_for_setup(
                    &selection,
                    NativeAdmissionContextV1 {
                        exact_target_pid: None,
                        now_unix_seconds: current_unix_seconds(),
                        now_monotonic_millis: telemetry.captured_monotonic_millis,
                        configured_game_reserve_vram_bytes: settings.game_reserve_vram_bytes,
                        game_additional_reserve_ram_bytes: settings
                            .game_additional_reserve_ram_bytes,
                        resource_pressure: native_resource_pressure(&telemetry),
                        telemetry: Some(&telemetry),
                    },
                ),
                NativeLoadoutPlanner::Unavailable(detail) => {
                    panic!("planner unavailable: {detail}")
                }
            }
        };
        assert!(setup_decision.admitted(), "{}", setup_decision.detail);
        *manager.active_admission.lock().expect("active admission") =
            Some(NativeActiveLoadoutAdmissionV1 {
                exact_target_pid: 42,
                receipt: setup_decision.admission_receipt.expect("setup receipt"),
                residency_decisions: setup_decision.residency_decisions,
            });

        let broken_probe = real_provider_probe(
            state_root.join("missing-mouth-worker.exe"),
            &resource_root,
            &state_root,
        );
        let first_error = manager
            .activate_trusted_optional_pack(request.clone(), &broken_probe)
            .await
            .expect_err("missing worker must fail");
        assert!(first_error.to_string().contains("provider-load"));
        assert!(manager
            .active_admission
            .lock()
            .expect("admission")
            .is_none());
        assert_eq!(
            manager
                .selected_loadout
                .lock()
                .expect("selected draft")
                .as_ref(),
            Some(&selection)
        );
        assert_eq!(
            manager
                .optional_lifecycle
                .lock()
                .await
                .as_ref()
                .and_then(|lifecycle| lifecycle.state(&identity)),
            Some(&InstallState::AwaitingSelfTest)
        );

        let real_probe = real_provider_probe(worker, &resource_root, &state_root);
        let result = manager
            .activate_trusted_optional_pack(request.clone(), &real_probe)
            .await
            .expect("real hidden provider activation");
        assert_eq!(result.receipt.identity, identity);
        assert!(result.receipt.detail.contains("provider load only"));
        assert!(manager
            .active_admission
            .lock()
            .expect("admission")
            .is_none());
        let active = manager
            .optional_lifecycle
            .lock()
            .await
            .as_ref()
            .expect("lifecycle")
            .manager()
            .storage()
            .active_installed_inventory(&identity.pack_id)
            .expect("active inventory query")
            .expect("active inventory");
        assert_eq!(active.identity, identity);
        assert_eq!(
            active.content_tree_sha256,
            result.receipt.installed_content_tree_sha256
        );

        let retry_error = manager
            .activate_trusted_optional_pack(request, &real_probe)
            .await
            .expect_err("an active immutable revision cannot be activated twice");
        assert!(retry_error.to_string().contains("not awaiting"));
        let evidence_path = state_root.join("real-provider-activation-evidence.json");
        fs::write(
            &evidence_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "initialFailure": first_error.to_string(),
                "retryRejected": retry_error.to_string(),
                "receipt": result.receipt,
                "lifecycle": result.lifecycle,
                "activeInventory": {
                    "identity": active.identity,
                    "root": active.root,
                    "manifestSha256": active.manifest_sha256,
                    "contentTreeSha256": active.content_tree_sha256,
                    "totalFileBytes": active.total_file_bytes,
                    "files": active.files,
                }
            }))
            .expect("evidence JSON"),
        )
        .expect("write activation evidence");
        assert!(evidence_path.is_file());
    }
}
