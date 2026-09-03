use crate::{
    Accelerator, Architecture, ArtifactKind, ArtifactV1, CatalogInstallBindingV1, CatalogKeyError,
    CatalogSignatureV1, CatalogSignatureVerifier, CatalogTrustDomainV1, Ed25519CatalogVerifier,
    HardwareRequirementsV1, LicenseMetadataV1, LicensePermission, ModelPackCapabilityV1,
    ModelPackKindV1, ModelPackManifestV1, ModelPackScopeV1, PackId, Platform, QualityTier,
    ResourceEnvelopeV1, Revision, RuntimeCompatibilityV1, SelfTestV1, Sha256Digest,
    ED25519_CATALOG_ALGORITHM, MODEL_PACK_MANIFEST_SCHEMA_V1,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use thiserror::Error;

pub const OPENSEEFACE_VISUAL_SIGNAL_PACK_ID: &str = "openseeface-mnv3-lm1-mouth-signal";
pub const OPENSEEFACE_VISUAL_SIGNAL_REVISION: &str = "85aa70fc67582d046e771ea73625182a0d8f7475";
pub const OPENSEEFACE_VISUAL_SIGNAL_INSTALLED_BYTES: u64 = 5_411_995;
pub const OPENSEEFACE_GUARDED_REPORT_SHA256: &str =
    "7bbd78e9c2aae7b760fb6258878437009cf6f31c7bf13df34ae5867f6be8cf97";
pub const OPENSEEFACE_VISUAL_SIGNAL_CONTRACT_SCHEMA_V1: &str =
    "npc.experimental-visual-signal-contract/v1";
pub const LOCAL_REVIEW_CATALOG_SCHEMA_V1: &str = "npc.local-review-catalog/dev-v1";
pub const LOCAL_REVIEW_TEST_KEY_ID_V1: &str = "local-review-rfc8032-test-key-1";

/// RFC 8032 test-vector public key. Its private seed is public knowledge and is
/// intentionally used only to make checked-in local-review evidence
/// cryptographically testable. Production catalog roots must never contain it.
pub const LOCAL_REVIEW_TEST_PUBLIC_KEY_V1: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

pub const UI_COMMAND_INSTALL_EXPERIMENTAL_PACK: &str = "install_experimental_model_pack";
pub const UI_COMMAND_ACTIVATE_EXPERIMENTAL_PACK: &str = "activate_experimental_model_pack";
pub const UI_COMMAND_REPAIR_MODEL_PACK: &str = "repair_model_pack";
pub const UI_COMMAND_REMOVE_MODEL_PACK: &str = "remove_model_pack";

/// Inert UI/native bridge contract. These values describe lifecycle requests;
/// they are never interpreted as shell commands by model-manager.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ExperimentalVisualPackUiCommandV1 {
    InstallExperimentalModelPack {
        pack_id: PackId,
        revision: Revision,
        explicit_user_confirmation: bool,
    },
    ActivateExperimentalModelPack {
        pack_id: PackId,
        revision: Revision,
        mouth_warp_profile_id: String,
    },
    RepairModelPack {
        pack_id: PackId,
        revision: Revision,
    },
    RemoveModelPack {
        pack_id: PackId,
        require_unreferenced: bool,
    },
}

pub fn openseeface_ui_command_handoff_v1(
) -> Result<Vec<ExperimentalVisualPackUiCommandV1>, ExperimentalVisualPackError> {
    let pack_id = PackId::parse(OPENSEEFACE_VISUAL_SIGNAL_PACK_ID)?;
    let revision = Revision::parse(OPENSEEFACE_VISUAL_SIGNAL_REVISION)?;
    Ok(vec![
        ExperimentalVisualPackUiCommandV1::InstallExperimentalModelPack {
            pack_id: pack_id.clone(),
            revision: revision.clone(),
            explicit_user_confirmation: true,
        },
        ExperimentalVisualPackUiCommandV1::ActivateExperimentalModelPack {
            pack_id: pack_id.clone(),
            revision: revision.clone(),
            mouth_warp_profile_id: "npc-causal-viseme-mouth-warp-v1".to_owned(),
        },
        ExperimentalVisualPackUiCommandV1::RepairModelPack {
            pack_id: pack_id.clone(),
            revision,
        },
        ExperimentalVisualPackUiCommandV1::RemoveModelPack {
            pack_id,
            require_unreferenced: true,
        },
    ])
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LatencyDistributionNanosV1 {
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MandatoryTemporalGuardV1 {
    pub actor_lock_required: bool,
    pub track_id_required: bool,
    pub captured_frame_id_required: bool,
    pub scene_epoch_required: bool,
    pub appearance_and_occlusion_latch_required: bool,
    pub queue_depth_one_required: bool,
    pub stale_late_cancelled_scene_mismatch_drop_required: bool,
    pub rejected_frame_must_remain_untouched: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExperimentalVisualSignalMeasurementV1 {
    pub benchmark_date: String,
    pub benchmark_host: String,
    pub replay_sha256: Sha256Digest,
    pub replay_frames: u32,
    pub load_nanos: u64,
    pub loaded_rss_delta_mib_x1000: u64,
    pub peak_rss_delta_mib_x1000: u64,
    pub dedicated_vram_bytes: u64,
    pub transient_vram_bytes: u64,
    pub sustained_fps_x1000: u64,
    pub cpu_percent_of_one_logical_core_x1000: u64,
    pub detector_nanos: LatencyDistributionNanosV1,
    pub landmark_nanos: LatencyDistributionNanosV1,
    pub detector_plus_landmark_nanos: LatencyDistributionNanosV1,
    pub full_frame_nanos: LatencyDistributionNanosV1,
    pub static_validation_report_sha256: Sha256Digest,
    pub comparator_report_sha256: Sha256Digest,
    pub guarded_replay_report_sha256: Sha256Digest,
}

/// Narrow contract for a current-frame landmark/signal dependency. These
/// explicit negative claims prevent UI or runtime code from promoting it into
/// a complete lipsync, identity-recognition, or talking-head model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExperimentalVisualSignalContractV1 {
    pub schema: String,
    pub pack_id: String,
    pub dependency_of_candidate_id: String,
    pub explicit_user_download_required: bool,
    pub experimental: bool,
    pub cpu_only: bool,
    pub inference_threads: u16,
    pub maximum_signal_rate_hz: u16,
    pub queue_depth: u16,
    pub is_complete_lip_sync_model: bool,
    pub is_identity_recognition_model: bool,
    pub is_talking_head_generator: bool,
    pub static_onnx_validation_before_session_required: bool,
    pub provider_timed_visemes_preferred: bool,
    pub temporal_guard: MandatoryTemporalGuardV1,
    pub measurement: ExperimentalVisualSignalMeasurementV1,
}

impl ExperimentalVisualSignalContractV1 {
    pub fn validate(&self) -> Result<(), ExperimentalVisualPackError> {
        if self.schema != OPENSEEFACE_VISUAL_SIGNAL_CONTRACT_SCHEMA_V1
            || self.pack_id != OPENSEEFACE_VISUAL_SIGNAL_PACK_ID
            || self.dependency_of_candidate_id != "npc-causal-viseme-mouth-warp-v1"
            || !self.explicit_user_download_required
            || !self.experimental
            || !self.cpu_only
            || self.inference_threads != 1
            || self.maximum_signal_rate_hz != 15
            || self.queue_depth != 1
            || self.is_complete_lip_sync_model
            || self.is_identity_recognition_model
            || self.is_talking_head_generator
            || !self.static_onnx_validation_before_session_required
            || !self.provider_timed_visemes_preferred
            || !self.temporal_guard.actor_lock_required
            || !self.temporal_guard.track_id_required
            || !self.temporal_guard.captured_frame_id_required
            || !self.temporal_guard.scene_epoch_required
            || !self.temporal_guard.appearance_and_occlusion_latch_required
            || !self.temporal_guard.queue_depth_one_required
            || !self
                .temporal_guard
                .stale_late_cancelled_scene_mismatch_drop_required
            || !self.temporal_guard.rejected_frame_must_remain_untouched
            || self.measurement.replay_frames != 453
            || self.measurement.peak_rss_delta_mib_x1000 != 88_945
            || self.measurement.dedicated_vram_bytes != 0
            || self.measurement.transient_vram_bytes != 0
        {
            return Err(ExperimentalVisualPackError::ContractMismatch);
        }
        for distribution in [
            &self.measurement.detector_nanos,
            &self.measurement.landmark_nanos,
            &self.measurement.detector_plus_landmark_nanos,
            &self.measurement.full_frame_nanos,
        ] {
            if distribution.p50 == 0
                || distribution.p50 > distribution.p95
                || distribution.p95 > distribution.p99
            {
                return Err(ExperimentalVisualPackError::InvalidMeasurement);
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<Sha256Digest, ExperimentalVisualPackError> {
        self.validate()?;
        serde_json::to_vec(self)
            .map(|bytes| Sha256Digest::of_bytes(&bytes))
            .map_err(ExperimentalVisualPackError::Serialization)
    }
}

pub fn openseeface_visual_signal_contract_v1() -> ExperimentalVisualSignalContractV1 {
    ExperimentalVisualSignalContractV1 {
        schema: OPENSEEFACE_VISUAL_SIGNAL_CONTRACT_SCHEMA_V1.to_owned(),
        pack_id: OPENSEEFACE_VISUAL_SIGNAL_PACK_ID.to_owned(),
        dependency_of_candidate_id: "npc-causal-viseme-mouth-warp-v1".to_owned(),
        explicit_user_download_required: true,
        experimental: true,
        cpu_only: true,
        inference_threads: 1,
        maximum_signal_rate_hz: 15,
        queue_depth: 1,
        is_complete_lip_sync_model: false,
        is_identity_recognition_model: false,
        is_talking_head_generator: false,
        static_onnx_validation_before_session_required: true,
        provider_timed_visemes_preferred: true,
        temporal_guard: MandatoryTemporalGuardV1 {
            actor_lock_required: true,
            track_id_required: true,
            captured_frame_id_required: true,
            scene_epoch_required: true,
            appearance_and_occlusion_latch_required: true,
            queue_depth_one_required: true,
            stale_late_cancelled_scene_mismatch_drop_required: true,
            rejected_frame_must_remain_untouched: true,
        },
        measurement: ExperimentalVisualSignalMeasurementV1 {
            benchmark_date: "2026-08-30".to_owned(),
            benchmark_host: "Windows 11 build 26200; Intel Core i9-13980HX; ONNX Runtime 1.22.1 CPU EP, one inference thread; synthetic replay only".to_owned(),
            replay_sha256: digest("a27480c3cfeca36b72f3a2b8baff204fbe04e23d7ccd3a762f719d0da350a3a3"),
            replay_frames: 453,
            load_nanos: 71_220_000,
            loaded_rss_delta_mib_x1000: 15_133,
            peak_rss_delta_mib_x1000: 88_945,
            dedicated_vram_bytes: 0,
            transient_vram_bytes: 0,
            sustained_fps_x1000: 23_316,
            cpu_percent_of_one_logical_core_x1000: 104_466,
            detector_nanos: LatencyDistributionNanosV1 {
                p50: 11_878_600,
                p95: 37_405_700,
                p99: 38_788_600,
            },
            landmark_nanos: LatencyDistributionNanosV1 {
                p50: 47_047_600,
                p95: 52_158_900,
                p99: 56_566_900,
            },
            detector_plus_landmark_nanos: LatencyDistributionNanosV1 {
                p50: 47_726_100,
                p95: 84_445_500,
                p99: 87_738_400,
            },
            full_frame_nanos: LatencyDistributionNanosV1 {
                p50: 48_497_800,
                p95: 86_210_200,
                p99: 92_424_500,
            },
            static_validation_report_sha256: digest("859cecb2ca18fc252b8e9601949d0be73a3700415287abcbf87e94c601f44c91"),
            comparator_report_sha256: digest(
                "41efeb8f773dc277e0790c995252d3fe41a1a031d6b6dd3cd8bdb3bdfdb0b3ec",
            ),
            guarded_replay_report_sha256: openseeface_guarded_report_sha256_v1(),
        },
    }
}

pub fn openseeface_visual_signal_manifest_v1(
) -> Result<ModelPackManifestV1, ExperimentalVisualPackError> {
    let revision = OPENSEEFACE_VISUAL_SIGNAL_REVISION;
    let manifest = ModelPackManifestV1 {
        schema: MODEL_PACK_MANIFEST_SCHEMA_V1.to_owned(),
        pack_id: PackId::parse(OPENSEEFACE_VISUAL_SIGNAL_PACK_ID)?,
        revision: Revision::parse(revision)?,
        display_name: "OpenSeeFace MNV3 + LM1 current-frame mouth signal".to_owned(),
        description: "Explicit-download experimental 15 Hz CPU face/landmark signal dependency for the project-owned causal residual mouth-warp baseline. It is not complete lip-sync, identity recognition, or talking-head generation.".to_owned(),
        source_project: "https://github.com/emilianavt/OpenSeeFace".to_owned(),
        source_revision: revision.to_owned(),
        capability: ModelPackCapabilityV1 {
            kind: ModelPackKindV1::Vision,
            scope: ModelPackScopeV1::Generic,
        },
        artifacts: vec![
            artifact(
                "mnv3-detector",
                "models/mnv3_detection_opt.onnx",
                568_302,
                "0e8e4806766d85ab067a52c7af0dcb59eb7f9dfe580b44f20a8e6ab712d89809",
            ),
            artifact(
                "lm1-landmarks",
                "models/lm_model1_opt.onnx",
                4_842_329,
                "5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f",
            ),
            artifact(
                "bsd-license",
                "LICENSE",
                1_364,
                "28612834d7ca038a9009550e3869a67e6be3a87c238d997f58c0907e08744146",
            ),
        ],
        runtime: RuntimeCompatibilityV1 {
            runtime: "onnxruntime".to_owned(),
            abi: "npc-openseeface-mouth-signal-v1".to_owned(),
            minimum_runtime_revision: Some("1.22.1".to_owned()),
            supported_platforms: BTreeSet::from([Platform::Windows10, Platform::Windows11]),
            supported_architectures: BTreeSet::from([Architecture::X86_64]),
        },
        hardware: HardwareRequirementsV1 {
            minimum_ram_bytes: 134_217_728,
            recommended_ram_bytes: 268_435_456,
            minimum_vram_bytes: Some(0),
            recommended_vram_bytes: Some(0),
            minimum_cpu_threads: 1,
            accelerators: BTreeSet::from([Accelerator::Cpu]),
            required_cpu_features: BTreeSet::new(),
        },
        resources: ResourceEnvelopeV1 {
            storage_bytes: OPENSEEFACE_VISUAL_SIGNAL_INSTALLED_BYTES,
            peak_install_bytes: OPENSEEFACE_VISUAL_SIGNAL_INSTALLED_BYTES,
            measured_ram_bytes: Some(93_265_592),
            measured_vram_bytes: Some(0),
            measured_load_millis: Some(72),
            benchmark_hardware: Some("Planning display only: signed admission uses a separate device-bound envelope. Windows 11 build 26200, i9-13980HX, ORT 1.22.1 CPU EP one thread, Eclipse Harbor synthetic replay.".to_owned()),
            quality_tier: QualityTier::Experimental,
            languages: BTreeSet::from(["language-independent-landmarks".to_owned()]),
        },
        license: LicenseMetadataV1 {
            spdx_expression: Some("BSD-2-Clause".to_owned()),
            license_name: "BSD 2-Clause License".to_owned(),
            license_url: format!(
                "https://github.com/emilianavt/OpenSeeFace/blob/{revision}/LICENSE"
            ),
            attribution: "OpenSeeFace by Emiliana Torrano; retain the pinned BSD-2-Clause LICENSE with redistributed detector and landmark models.".to_owned(),
            redistributable: true,
            commercial_use: LicensePermission::Allowed,
            derivative_use: LicensePermission::Allowed,
            acceptance_required: false,
            notices: vec![
                "The pinned upstream LICENSE states that OpenSeeFace code and models are BSD-2-Clause.".to_owned(),
                "Shared ONNX Runtime 1.22.1 CPU runtime is excluded from the 5,411,995-byte pack and requires its own MIT notice.".to_owned(),
            ],
        },
        self_test: SelfTestV1 {
            kind: "openseeface-temporal-guard".to_owned(),
            suite_revision: "eclipse-harbor-guard-2026-08-30".to_owned(),
            allowed_runtime_backends: BTreeSet::from(["onnxruntime-1.22.1-cpu-1thread".to_owned()]),
            input_fixture: "tests/eclipse-harbor-guard.json".to_owned(),
            expected_output_sha256: Some(openseeface_guarded_report_sha256_v1()),
            timeout_millis: 300_000,
        },
    };
    manifest.validate()?;
    Ok(manifest)
}

pub fn openseeface_guarded_report_sha256_v1() -> Sha256Digest {
    digest(OPENSEEFACE_GUARDED_REPORT_SHA256)
}

fn artifact(id: &str, path: &str, size_bytes: u64, sha256: &str) -> ArtifactV1 {
    ArtifactV1 {
        id: id.to_owned(),
        kind: ArtifactKind::File,
        archive_format: None,
        source_urls: vec![format!(
            "https://raw.githubusercontent.com/emilianavt/OpenSeeFace/{}/{path}",
            OPENSEEFACE_VISUAL_SIGNAL_REVISION
        )],
        size_bytes,
        sha256: digest(sha256),
        destination: path.to_owned(),
        strip_prefix: None,
        required_paths: vec![],
    }
}

#[allow(clippy::expect_used)]
fn digest(value: &str) -> Sha256Digest {
    // All values in this module are reviewed, fixed lowercase SHA-256 strings.
    Sha256Digest::parse(value).expect("reviewed SHA-256 constant")
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalReviewCatalogPayloadV1 {
    pub schema: String,
    pub review_id: String,
    pub review_sequence: u64,
    pub generated_unix_seconds: u64,
    pub expires_unix_seconds: u64,
    pub manifest: ModelPackManifestV1,
    pub manifest_sha256: Sha256Digest,
    pub contract: ExperimentalVisualSignalContractV1,
    pub contract_sha256: Sha256Digest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SignedLocalReviewCatalogV1 {
    pub signed: LocalReviewCatalogPayloadV1,
    pub signatures: Vec<CatalogSignatureV1>,
}

pub fn canonical_local_review_catalog_bytes(
    payload: &LocalReviewCatalogPayloadV1,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(payload)
}

#[derive(Clone, Debug)]
pub struct VerifiedLocalReviewCatalogV1 {
    payload: LocalReviewCatalogPayloadV1,
    payload_sha256: Sha256Digest,
}

impl VerifiedLocalReviewCatalogV1 {
    pub fn payload(&self) -> &LocalReviewCatalogPayloadV1 {
        &self.payload
    }

    pub fn payload_sha256(&self) -> &Sha256Digest {
        &self.payload_sha256
    }

    pub fn install_binding(&self) -> CatalogInstallBindingV1 {
        CatalogInstallBindingV1 {
            catalog_payload_sha256: self.payload_sha256.clone(),
            catalog_version: self.payload.review_sequence,
            trust_domain: CatalogTrustDomainV1::LocalReviewDevOnly,
        }
    }
}

pub fn verify_local_review_catalog(
    catalog: &SignedLocalReviewCatalogV1,
    now_unix_seconds: u64,
) -> Result<VerifiedLocalReviewCatalogV1, ExperimentalVisualPackError> {
    let payload = &catalog.signed;
    if payload.schema != LOCAL_REVIEW_CATALOG_SCHEMA_V1
        || payload.review_id != "openseeface-visual-pack-qualification-2026-08-30"
        || payload.review_sequence == 0
        || payload.generated_unix_seconds == 0
        || payload.generated_unix_seconds > now_unix_seconds.saturating_add(600)
        || payload.expires_unix_seconds <= payload.generated_unix_seconds
        || payload.expires_unix_seconds <= now_unix_seconds
        || payload
            .expires_unix_seconds
            .saturating_sub(payload.generated_unix_seconds)
            > 31 * 24 * 60 * 60
    {
        return Err(ExperimentalVisualPackError::InvalidLocalReviewCatalog);
    }
    let expected_manifest = openseeface_visual_signal_manifest_v1()?;
    let expected_contract = openseeface_visual_signal_contract_v1();
    expected_contract.validate()?;
    if payload.manifest != expected_manifest
        || payload.manifest_sha256 != expected_manifest.digest()?
        || payload.contract != expected_contract
        || payload.contract_sha256 != expected_contract.digest()?
    {
        return Err(ExperimentalVisualPackError::EvidenceMismatch);
    }
    let bytes = canonical_local_review_catalog_bytes(payload)
        .map_err(ExperimentalVisualPackError::Serialization)?;
    let verifier = Ed25519CatalogVerifier::new([(
        LOCAL_REVIEW_TEST_KEY_ID_V1.to_owned(),
        LOCAL_REVIEW_TEST_PUBLIC_KEY_V1,
    )])?;
    let valid = catalog
        .signatures
        .iter()
        .filter(|signature| signature.key_id == LOCAL_REVIEW_TEST_KEY_ID_V1)
        .filter(|signature| signature.algorithm == ED25519_CATALOG_ALGORITHM)
        .filter(|signature| {
            verifier.verify(
                &signature.key_id,
                &signature.algorithm,
                &bytes,
                &signature.signature,
            )
        })
        .map(|signature| signature.key_id.clone())
        .collect::<HashSet<_>>();
    if valid.len() != 1 {
        return Err(ExperimentalVisualPackError::InvalidDevSignature);
    }
    Ok(VerifiedLocalReviewCatalogV1 {
        payload: payload.clone(),
        payload_sha256: Sha256Digest::of_bytes(&bytes),
    })
}

#[derive(Debug, Error)]
pub enum ExperimentalVisualPackError {
    #[error("experimental visual signal contract does not enforce the qualified limits")]
    ContractMismatch,
    #[error("experimental visual signal measurement is invalid")]
    InvalidMeasurement,
    #[error("local-review catalog metadata is invalid")]
    InvalidLocalReviewCatalog,
    #[error("local-review evidence does not match the exact reviewed pack and contract")]
    EvidenceMismatch,
    #[error("local-review signature is invalid")]
    InvalidDevSignature,
    #[error("manifest is invalid: {0}")]
    Manifest(#[from] crate::ManifestError),
    #[error("local-review public key is invalid: {0}")]
    CatalogKey(#[from] CatalogKeyError),
    #[error("local-review evidence serialization failed: {0}")]
    Serialization(serde_json::Error),
}
