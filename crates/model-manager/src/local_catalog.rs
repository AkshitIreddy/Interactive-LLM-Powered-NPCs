use crate::experimental_visual_pack::{
    openseeface_guarded_report_sha256_v1, OPENSEEFACE_VISUAL_SIGNAL_INSTALLED_BYTES,
    OPENSEEFACE_VISUAL_SIGNAL_PACK_ID, OPENSEEFACE_VISUAL_SIGNAL_REVISION,
};
use crate::{
    CatalogError, LicensePermission, ModelPackKindV1, ModelPackScopeV1, PackRevision,
    QualifiedResourceEnvelopeV1, Sha256Digest, TrustedCatalog,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const LOCAL_MODEL_CANDIDATE_SCHEMA_V1: &str = "npc.local-model-candidate/v1";
pub const PLANNING_RESOURCE_ESTIMATE_SCHEMA_V1: &str = "npc.planning-resource-estimate/v1";
pub const OPTIONAL_LOCAL_CHANNEL_V1: &str = "optional-local";

/// Product research metadata for an option that may become a signed model pack.
///
/// This is deliberately not an install manifest. In particular, source version
/// labels and planning estimates are not trusted artifact pins. Qualification
/// requires a non-revoked entry from a verified catalog, whose manifest binds
/// every artifact to an exact byte length and SHA-256 digest, plus a verified
/// measured resource envelope for the current device.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalModelCandidateV1 {
    pub schema: String,
    pub candidate_id: String,
    pub display_name: String,
    pub capability: ModelPackKindV1,
    pub source: CandidateSourceV1,
    pub expected_license: ExpectedLicenseV1,
    pub install: CandidateInstallMetadataV1,
    pub disposition: CandidateDispositionV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning_estimate: Option<PlanningResourceEstimateV1>,
    pub qualification_gates: BTreeSet<QualificationGateV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CandidateSourceV1 {
    pub project_url: String,
    /// A release/model identifier expected from the eventual manifest. It is
    /// not treated as a cryptographic pin; the signed manifest and artifact
    /// SHA-256 values remain authoritative.
    pub expected_source_revision: String,
    pub model_card_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExpectedLicenseV1 {
    pub spdx_expression: Option<String>,
    pub license_name: String,
    pub license_url: String,
    pub redistributable_signal: bool,
    pub acceptance_required: bool,
    /// Model, tokenizer, dictionaries, voices and runtime must all be reviewed
    /// even when the top-level weights use a permissive license.
    pub transitive_assets_review_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CandidateInstallMetadataV1 {
    pub runtime: String,
    pub abi: String,
    pub model_format: String,
    pub delivery: CandidateDeliveryV1,
    pub preferred_device: PreferredDeviceV1,
    pub cpu_fallback_supported: bool,
    pub stock_voice_count: Option<u16>,
    pub explicit_download_required: bool,
    pub dependency_of_candidate_ids: BTreeSet<String>,
    pub required_self_test_kind: Option<String>,
    pub required_self_test_suite_revision: Option<String>,
    pub required_self_test_output_sha256: Option<Sha256Digest>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateDeliveryV1 {
    DirectFromUpstream,
    CuratedMirrorAfterLicenseReview,
    ProjectOwned,
    AccessControlled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferredDeviceV1 {
    Cpu,
    Gpu,
    CpuFirstWithOptionalGpuOffload,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateDispositionV1 {
    QualificationCandidate,
    EngineeringBaseline,
    /// Catalogued for research and private evaluation, but never installable
    /// until the candidate record itself is updated after rights review.
    PrivateEvaluationOnly,
    Blocked,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationGateV1 {
    ImmutableSourceAndArtifactHashes,
    TransitiveLicenseReview,
    WindowsSelfTest,
    WindowsLatencyMeasurement,
    WindowsResourceMeasurement,
    WholeLoadoutGameContention,
    PretrainedWeightsAndTrainingDataRights,
    TemporalAppearanceOcclusionGuard,
    CancellationAndStaleness,
    RenderedVisualQuality,
}

/// A research estimate may guide what to benchmark, but it is structurally
/// impossible to pass it to the resource governor as measured evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanningResourceEstimateV1 {
    pub schema: String,
    pub provenance_url: String,
    pub assumptions: String,
    pub estimated_installed_bytes: EstimateRangeV1,
    pub estimated_resident_ram_bytes: EstimateRangeV1,
    pub estimated_resident_vram_bytes: EstimateRangeV1,
    pub estimated_peak_vram_bytes: EstimateRangeV1,
    pub estimated_load_millis: EstimateRangeV1,
    /// Must remain true. Validation rejects an estimate that purports to be
    /// admissible evidence.
    pub not_for_admission: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EstimateRangeV1 {
    pub minimum: u64,
    pub maximum: u64,
}

impl EstimateRangeV1 {
    fn validate(&self) -> Result<(), LocalCatalogError> {
        if self.minimum > self.maximum {
            return Err(LocalCatalogError::InvertedEstimateRange);
        }
        Ok(())
    }
}

impl LocalModelCandidateV1 {
    pub fn validate(&self) -> Result<(), LocalCatalogError> {
        if self.schema != LOCAL_MODEL_CANDIDATE_SCHEMA_V1 {
            return Err(LocalCatalogError::UnsupportedCandidateSchema(
                self.schema.clone(),
            ));
        }
        validate_token("candidate_id", &self.candidate_id, 3, 128)?;
        validate_text("display_name", &self.display_name, 1, 160)?;
        validate_https(&self.source.project_url)?;
        validate_https(&self.source.model_card_url)?;
        validate_text(
            "expected_source_revision",
            &self.source.expected_source_revision,
            1,
            256,
        )?;
        if matches!(
            self.source
                .expected_source_revision
                .to_ascii_lowercase()
                .as_str(),
            "main" | "master" | "head" | "latest"
        ) {
            return Err(LocalCatalogError::MutableCandidateRevision);
        }
        validate_text("license_name", &self.expected_license.license_name, 1, 256)?;
        validate_https(&self.expected_license.license_url)?;
        validate_token("runtime", &self.install.runtime, 1, 128)?;
        validate_token("abi", &self.install.abi, 1, 128)?;
        validate_token("model_format", &self.install.model_format, 1, 64)?;
        for dependency in &self.install.dependency_of_candidate_ids {
            validate_token("dependency_of_candidate_id", dependency, 3, 128)?;
            if dependency == &self.candidate_id {
                return Err(LocalCatalogError::SelfDependency);
            }
        }
        if let Some(kind) = &self.install.required_self_test_kind {
            validate_token("required_self_test_kind", kind, 1, 64)?;
        }
        if let Some(revision) = &self.install.required_self_test_suite_revision {
            validate_token("required_self_test_suite_revision", revision, 1, 128)?;
        }
        if self.qualification_gates.is_empty() {
            return Err(LocalCatalogError::MissingQualificationGates);
        }
        if let Some(estimate) = &self.planning_estimate {
            if estimate.schema != PLANNING_RESOURCE_ESTIMATE_SCHEMA_V1
                || !estimate.not_for_admission
            {
                return Err(LocalCatalogError::EstimateClaimedAsMeasurement);
            }
            validate_https(&estimate.provenance_url)?;
            validate_text("estimate.assumptions", &estimate.assumptions, 1, 4_096)?;
            estimate.estimated_installed_bytes.validate()?;
            estimate.estimated_resident_ram_bytes.validate()?;
            estimate.estimated_resident_vram_bytes.validate()?;
            estimate.estimated_peak_vram_bytes.validate()?;
            estimate.estimated_load_millis.validate()?;
        }
        Ok(())
    }
}

/// A candidate becomes qualified only through this private-field result.
/// Callers cannot turn built-in research metadata into an installable option.
#[derive(Clone, Debug)]
pub struct QualifiedLocalModelV1 {
    candidate: LocalModelCandidateV1,
    identity: PackRevision,
    manifest_sha256: Sha256Digest,
    catalog_payload_sha256: Sha256Digest,
    catalog_version: u64,
    measurement: QualifiedResourceEnvelopeV1,
}

impl QualifiedLocalModelV1 {
    pub fn candidate(&self) -> &LocalModelCandidateV1 {
        &self.candidate
    }

    pub fn identity(&self) -> &PackRevision {
        &self.identity
    }

    pub fn manifest_sha256(&self) -> &Sha256Digest {
        &self.manifest_sha256
    }

    pub fn catalog_payload_sha256(&self) -> &Sha256Digest {
        &self.catalog_payload_sha256
    }

    pub fn catalog_version(&self) -> u64 {
        self.catalog_version
    }

    pub fn measurement(&self) -> &QualifiedResourceEnvelopeV1 {
        &self.measurement
    }
}

pub fn qualify_local_model(
    candidate: &LocalModelCandidateV1,
    catalog: &TrustedCatalog,
    identity: &PackRevision,
    measurement: QualifiedResourceEnvelopeV1,
) -> Result<QualifiedLocalModelV1, LocalCatalogError> {
    candidate.validate()?;
    if candidate.disposition == CandidateDispositionV1::Blocked {
        return Err(LocalCatalogError::CandidateBlocked);
    }
    if candidate.disposition == CandidateDispositionV1::PrivateEvaluationOnly {
        return Err(LocalCatalogError::CandidatePrivateEvaluationOnly);
    }
    let entry = catalog
        .installable_entry(identity)
        .map_err(LocalCatalogError::Catalog)?;
    let manifest = &entry.manifest;
    if !entry.channels.contains(OPTIONAL_LOCAL_CHANNEL_V1) {
        return Err(LocalCatalogError::MissingOptionalLocalChannel);
    }
    if manifest.capability.kind != candidate.capability
        || manifest.capability.scope != ModelPackScopeV1::Generic
        || manifest.source_project != candidate.source.project_url
        || manifest.source_revision != candidate.source.expected_source_revision
        || manifest.runtime.runtime != candidate.install.runtime
        || manifest.runtime.abi != candidate.install.abi
        || candidate
            .install
            .required_self_test_kind
            .as_ref()
            .is_some_and(|kind| kind != &manifest.self_test.kind)
        || candidate
            .install
            .required_self_test_suite_revision
            .as_ref()
            .is_some_and(|revision| revision != &manifest.self_test.suite_revision)
        || candidate
            .install
            .required_self_test_output_sha256
            .as_ref()
            .is_some_and(|digest| {
                manifest.self_test.expected_output_sha256.as_ref() != Some(digest)
            })
        || manifest.license.spdx_expression != candidate.expected_license.spdx_expression
        || manifest.license.license_url != candidate.expected_license.license_url
        || manifest.license.redistributable != candidate.expected_license.redistributable_signal
        || manifest.license.acceptance_required != candidate.expected_license.acceptance_required
    {
        return Err(LocalCatalogError::CandidateManifestMismatch);
    }
    if manifest.license.commercial_use == LicensePermission::Unknown
        || manifest.license.derivative_use == LicensePermission::Unknown
    {
        return Err(LocalCatalogError::UnresolvedLicensePermission);
    }
    let manifest_sha256 = manifest.digest().map_err(LocalCatalogError::Manifest)?;
    if manifest_sha256 != entry.manifest_sha256
        || measurement.identity() != identity
        || measurement.manifest_sha256() != &manifest_sha256
    {
        return Err(LocalCatalogError::MeasurementBindingMismatch);
    }
    Ok(QualifiedLocalModelV1 {
        candidate: candidate.clone(),
        identity: identity.clone(),
        manifest_sha256,
        catalog_payload_sha256: catalog.payload_digest().clone(),
        catalog_version: catalog.payload().version,
        measurement,
    })
}

/// Concrete qualification candidates. Their status is intentionally honest:
/// none is installable until an exact signed manifest and current-device
/// measured envelope pass `qualify_local_model`.
pub fn built_in_local_model_candidates_v1() -> Vec<LocalModelCandidateV1> {
    let common_gates = BTreeSet::from([
        QualificationGateV1::ImmutableSourceAndArtifactHashes,
        QualificationGateV1::TransitiveLicenseReview,
        QualificationGateV1::WindowsSelfTest,
        QualificationGateV1::WindowsLatencyMeasurement,
        QualificationGateV1::WindowsResourceMeasurement,
        QualificationGateV1::WholeLoadoutGameContention,
    ]);
    vec![
        candidate(
            "qwen3-4b-instruct-2507-q4-k-m",
            "Qwen3 4B Instruct 2507 Q4_K_M",
            ModelPackKindV1::LanguageModel,
            "https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507",
            "Qwen3-4B-Instruct-2507",
            "https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507",
            Some("Apache-2.0"),
            "Apache License 2.0",
            "https://www.apache.org/licenses/LICENSE-2.0",
            true,
            false,
            "llama.cpp",
            "npc-gguf-v1",
            "gguf-q4-k-m",
            PreferredDeviceV1::CpuFirstWithOptionalGpuOffload,
            true,
            None,
            common_gates.clone(),
        ),
        candidate(
            "moonshine-v2-tiny-streaming",
            "Moonshine v2 Tiny Streaming",
            ModelPackKindV1::SpeechRecognition,
            "https://github.com/moonshine-ai/moonshine",
            "moonshine-v2-tiny-streaming-34m",
            "https://github.com/moonshine-ai/moonshine",
            Some("MIT"),
            "MIT License",
            "https://opensource.org/license/mit",
            true,
            false,
            "onnxruntime",
            "npc-moonshine-v2",
            "ort",
            PreferredDeviceV1::Cpu,
            true,
            None,
            common_gates.clone(),
        ),
        candidate(
            "kokoro-82m-v1-int8",
            "Kokoro 82M v1.0 INT8",
            ModelPackKindV1::SpeechSynthesis,
            "https://huggingface.co/hexgrad/Kokoro-82M",
            "v1.0-496dba11",
            "https://huggingface.co/hexgrad/Kokoro-82M",
            Some("Apache-2.0"),
            "Apache License 2.0",
            "https://www.apache.org/licenses/LICENSE-2.0",
            true,
            false,
            "sherpa-onnx",
            "npc-kokoro-v1",
            "onnx-int8",
            PreferredDeviceV1::Cpu,
            true,
            Some(54),
            common_gates.clone(),
        ),
        candidate(
            "bge-small-en-v1-5-int8",
            "BGE Small English v1.5 INT8",
            ModelPackKindV1::Embedding,
            "https://huggingface.co/BAAI/bge-small-en-v1.5",
            "bge-small-en-v1.5",
            "https://huggingface.co/BAAI/bge-small-en-v1.5",
            Some("MIT"),
            "MIT License",
            "https://opensource.org/license/mit",
            true,
            false,
            "onnxruntime",
            "npc-embedding-onnx-v1",
            "onnx-int8",
            PreferredDeviceV1::Cpu,
            true,
            None,
            common_gates.clone(),
        ),
        LocalModelCandidateV1 {
            schema: LOCAL_MODEL_CANDIDATE_SCHEMA_V1.to_owned(),
            candidate_id: OPENSEEFACE_VISUAL_SIGNAL_PACK_ID.to_owned(),
            display_name: "OpenSeeFace MNV3 + LM1 current-frame mouth signal".to_owned(),
            capability: ModelPackKindV1::Vision,
            source: CandidateSourceV1 {
                project_url: "https://github.com/emilianavt/OpenSeeFace".to_owned(),
                expected_source_revision: OPENSEEFACE_VISUAL_SIGNAL_REVISION.to_owned(),
                model_card_url: "https://github.com/emilianavt/OpenSeeFace".to_owned(),
            },
            expected_license: ExpectedLicenseV1 {
                spdx_expression: Some("BSD-2-Clause".to_owned()),
                license_name: "BSD 2-Clause License".to_owned(),
                license_url: format!(
                    "https://github.com/emilianavt/OpenSeeFace/blob/{}/LICENSE",
                    OPENSEEFACE_VISUAL_SIGNAL_REVISION
                ),
                redistributable_signal: true,
                acceptance_required: false,
                transitive_assets_review_required: true,
            },
            install: CandidateInstallMetadataV1 {
                runtime: "onnxruntime".to_owned(),
                abi: "npc-openseeface-mouth-signal-v1".to_owned(),
                model_format: "onnx-pinned-pair".to_owned(),
                delivery: CandidateDeliveryV1::DirectFromUpstream,
                preferred_device: PreferredDeviceV1::Cpu,
                cpu_fallback_supported: true,
                stock_voice_count: None,
                explicit_download_required: true,
                dependency_of_candidate_ids: BTreeSet::from([
                    "npc-causal-viseme-mouth-warp-v1".to_owned(),
                ]),
                required_self_test_kind: Some("openseeface-temporal-guard".to_owned()),
                required_self_test_suite_revision: Some(
                    "eclipse-harbor-guard-2026-08-30".to_owned(),
                ),
                required_self_test_output_sha256: Some(openseeface_guarded_report_sha256_v1()),
                notes: vec![
                    format!(
                        "Exact explicit-download payload is {OPENSEEFACE_VISUAL_SIGNAL_INSTALLED_BYTES} bytes; shared ONNX Runtime is excluded."
                    ),
                    "Experimental CPU-only landmark/signal dependency capped at 15 Hz and queue depth one."
                        .to_owned(),
                    "Not complete lip-sync, identity recognition, lip reading, or talking-head generation."
                        .to_owned(),
                ],
            },
            disposition: CandidateDispositionV1::QualificationCandidate,
            planning_estimate: None,
            qualification_gates: common_gates
                .iter()
                .cloned()
                .chain([
                    QualificationGateV1::CancellationAndStaleness,
                    QualificationGateV1::RenderedVisualQuality,
                    QualificationGateV1::TemporalAppearanceOcclusionGuard,
                ])
                .collect(),
        },
        LocalModelCandidateV1 {
            schema: LOCAL_MODEL_CANDIDATE_SCHEMA_V1.to_owned(),
            candidate_id: "opencv-sface-2021dec".to_owned(),
            display_name: "OpenCV SFace 2021DEC (private evaluation only)".to_owned(),
            capability: ModelPackKindV1::Vision,
            source: CandidateSourceV1 {
                project_url: "https://github.com/opencv/opencv_zoo".to_owned(),
                expected_source_revision: "face-recognition-sface-2021dec".to_owned(),
                model_card_url:
                    "https://github.com/opencv/opencv_zoo/tree/main/models/face_recognition_sface"
                        .to_owned(),
            },
            expected_license: ExpectedLicenseV1 {
                // The repository's Apache label does not by itself resolve
                // the exact pretrained weights and training-data rights.
                spdx_expression: Some("Apache-2.0".to_owned()),
                license_name: "Repository Apache-2.0 claim; pretrained-weight rights unresolved"
                    .to_owned(),
                license_url: "https://github.com/opencv/opencv_zoo/blob/main/LICENSE".to_owned(),
                redistributable_signal: false,
                acceptance_required: false,
                transitive_assets_review_required: true,
            },
            install: CandidateInstallMetadataV1 {
                runtime: "onnxruntime".to_owned(),
                abi: "npc-face-embedding-v1".to_owned(),
                model_format: "onnx".to_owned(),
                delivery: CandidateDeliveryV1::AccessControlled,
                preferred_device: PreferredDeviceV1::Cpu,
                cpu_fallback_supported: true,
                stock_voice_count: None,
                explicit_download_required: true,
                dependency_of_candidate_ids: BTreeSet::new(),
                required_self_test_kind: None,
                required_self_test_suite_revision: None,
                required_self_test_output_sha256: None,
                notes: vec![
                    "Research/private-evaluation entry; redistribution and commercial qualification are blocked."
                        .to_owned(),
                    "A generic repository Apache label is insufficient; exact pretrained-weight and training-data permissions must be resolved before this candidate can be promoted."
                        .to_owned(),
                ],
            },
            disposition: CandidateDispositionV1::PrivateEvaluationOnly,
            planning_estimate: None,
            qualification_gates: common_gates
                .iter()
                .cloned()
                .chain([QualificationGateV1::PretrainedWeightsAndTrainingDataRights])
                .collect(),
        },
        LocalModelCandidateV1 {
            schema: LOCAL_MODEL_CANDIDATE_SCHEMA_V1.to_owned(),
            candidate_id: "npc-causal-viseme-mouth-warp-v1".to_owned(),
            display_name: "NPC Causal Viseme Mouth Warp v1".to_owned(),
            capability: ModelPackKindV1::LipSync,
            source: CandidateSourceV1 {
                project_url: "https://github.com/AkshitIreddy/Interactive-LLM-Powered-NPCs"
                    .to_owned(),
                expected_source_revision: "npc-causal-viseme-mouth-warp-v1".to_owned(),
                model_card_url:
                    "https://github.com/AkshitIreddy/Interactive-LLM-Powered-NPCs".to_owned(),
            },
            expected_license: ExpectedLicenseV1 {
                spdx_expression: Some("MIT".to_owned()),
                license_name: "MIT License".to_owned(),
                license_url: "https://opensource.org/license/mit".to_owned(),
                redistributable_signal: true,
                acceptance_required: false,
                transitive_assets_review_required: true,
            },
            install: CandidateInstallMetadataV1 {
                runtime: "npc-viseme-runtime".to_owned(),
                abi: "npc-viseme-mouth-warp-v1".to_owned(),
                model_format: "model-free-reference-code".to_owned(),
                delivery: CandidateDeliveryV1::ProjectOwned,
                preferred_device: PreferredDeviceV1::Cpu,
                cpu_fallback_supported: true,
                stock_voice_count: None,
                explicit_download_required: true,
                dependency_of_candidate_ids: BTreeSet::new(),
                required_self_test_kind: Some("causal-viseme-mouth-warp".to_owned()),
                required_self_test_suite_revision: None,
                required_self_test_output_sha256: None,
                notes: vec![
                    "Engineering baseline, not a native game-rig integration.".to_owned(),
                    "Model-free reference CPU code; the observed ~1.5 ms CPU result is planning context only until imported as a signed measured envelope."
                        .to_owned(),
                    "Must modify only a tracked current-frame mouth region and bypass on stale input."
                        .to_owned(),
                ],
            },
            disposition: CandidateDispositionV1::EngineeringBaseline,
            planning_estimate: None,
            qualification_gates: BTreeSet::from([
                QualificationGateV1::ImmutableSourceAndArtifactHashes,
                QualificationGateV1::TransitiveLicenseReview,
                QualificationGateV1::WindowsSelfTest,
                QualificationGateV1::WindowsLatencyMeasurement,
                QualificationGateV1::WindowsResourceMeasurement,
                QualificationGateV1::WholeLoadoutGameContention,
                QualificationGateV1::CancellationAndStaleness,
                QualificationGateV1::RenderedVisualQuality,
            ]),
        },
    ]
}

#[allow(clippy::too_many_arguments)]
fn candidate(
    candidate_id: &str,
    display_name: &str,
    capability: ModelPackKindV1,
    project_url: &str,
    source_revision: &str,
    model_card_url: &str,
    spdx: Option<&str>,
    license_name: &str,
    license_url: &str,
    redistributable: bool,
    acceptance_required: bool,
    runtime: &str,
    abi: &str,
    model_format: &str,
    preferred_device: PreferredDeviceV1,
    cpu_fallback_supported: bool,
    stock_voice_count: Option<u16>,
    qualification_gates: BTreeSet<QualificationGateV1>,
) -> LocalModelCandidateV1 {
    LocalModelCandidateV1 {
        schema: LOCAL_MODEL_CANDIDATE_SCHEMA_V1.to_owned(),
        candidate_id: candidate_id.to_owned(),
        display_name: display_name.to_owned(),
        capability,
        source: CandidateSourceV1 {
            project_url: project_url.to_owned(),
            expected_source_revision: source_revision.to_owned(),
            model_card_url: model_card_url.to_owned(),
        },
        expected_license: ExpectedLicenseV1 {
            spdx_expression: spdx.map(str::to_owned),
            license_name: license_name.to_owned(),
            license_url: license_url.to_owned(),
            redistributable_signal: redistributable,
            acceptance_required,
            transitive_assets_review_required: true,
        },
        install: CandidateInstallMetadataV1 {
            runtime: runtime.to_owned(),
            abi: abi.to_owned(),
            model_format: model_format.to_owned(),
            delivery: CandidateDeliveryV1::DirectFromUpstream,
            preferred_device,
            cpu_fallback_supported,
            stock_voice_count,
            explicit_download_required: true,
            dependency_of_candidate_ids: BTreeSet::new(),
            required_self_test_kind: None,
            required_self_test_suite_revision: None,
            required_self_test_output_sha256: None,
            notes: vec![
                "Optional local pack; never a first-run default.".to_owned(),
                "Exact runtime and auxiliary-asset licenses remain part of qualification."
                    .to_owned(),
            ],
        },
        disposition: CandidateDispositionV1::QualificationCandidate,
        planning_estimate: None,
        qualification_gates,
    }
}

fn validate_token(
    field: &'static str,
    value: &str,
    min: usize,
    max: usize,
) -> Result<(), LocalCatalogError> {
    if !(min..=max).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(LocalCatalogError::InvalidField(field));
    }
    Ok(())
}

fn validate_text(
    field: &'static str,
    value: &str,
    min: usize,
    max: usize,
) -> Result<(), LocalCatalogError> {
    let trimmed = value.trim();
    if !(min..=max).contains(&trimmed.len()) || value.contains('\0') {
        return Err(LocalCatalogError::InvalidField(field));
    }
    Ok(())
}

fn validate_https(value: &str) -> Result<(), LocalCatalogError> {
    let rest = value
        .strip_prefix("https://")
        .ok_or_else(|| LocalCatalogError::InsecureUrl(value.to_owned()))?;
    if rest.is_empty()
        || rest.starts_with('/')
        || rest.contains('@')
        || rest.chars().any(char::is_whitespace)
    {
        return Err(LocalCatalogError::InsecureUrl(value.to_owned()));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum LocalCatalogError {
    #[error("unsupported local candidate schema: {0}")]
    UnsupportedCandidateSchema(String),
    #[error("invalid local candidate field: {0}")]
    InvalidField(&'static str),
    #[error("candidate URL must be a valid HTTPS URL: {0}")]
    InsecureUrl(String),
    #[error("candidate source revision cannot be mutable")]
    MutableCandidateRevision,
    #[error("candidate has no qualification gates")]
    MissingQualificationGates,
    #[error("candidate cannot depend on itself")]
    SelfDependency,
    #[error("planning resource estimate range is inverted")]
    InvertedEstimateRange,
    #[error("planning estimate attempted to claim measured/admissible status")]
    EstimateClaimedAsMeasurement,
    #[error("candidate is explicitly blocked")]
    CandidateBlocked,
    #[error("candidate is restricted to private evaluation until exact pretrained-asset rights are resolved")]
    CandidatePrivateEvaluationOnly,
    #[error("signed catalog entry is invalid: {0}")]
    Catalog(CatalogError),
    #[error("signed catalog entry is not in the optional-local channel")]
    MissingOptionalLocalChannel,
    #[error("candidate metadata does not match the signed manifest")]
    CandidateManifestMismatch,
    #[error("manifest retains unresolved license permissions")]
    UnresolvedLicensePermission,
    #[error("measured envelope is not bound to this exact manifest")]
    MeasurementBindingMismatch,
    #[error("manifest validation failed: {0}")]
    Manifest(crate::ManifestError),
}
