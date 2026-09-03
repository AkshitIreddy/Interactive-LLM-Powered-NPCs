use crate::{
    Accelerator, Architecture, ArchiveFormatV1, ArtifactKind, ArtifactV1, HardwareRequirementsV1,
    LicenseMetadataV1, ModelPackCapabilityV1, ModelPackKindV1, ModelPackManifestV1, PackId,
    Platform, QualityTier, ResourceEnvelopeV1, Revision, RuntimeCompatibilityV1, SelfTestV1,
    Sha256Digest, MEASURED_RESOURCE_ENVELOPE_SCHEMA_V1, MODEL_PACK_MANIFEST_SCHEMA_V1,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const MODEL_PACK_MANIFEST_SCHEMA_V2: &str = "npc.model-pack/v2";
pub const MODEL_PACK_MANIFEST_JSON_SCHEMA_V2: &str = "./model-pack-manifest.schema.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackManifestV2 {
    #[serde(rename = "$schema")]
    pub schema_uri: String,
    pub schema: String,
    pub pack_id: PackId,
    pub revision: Revision,
    pub display_name: String,
    pub description: String,
    pub source: ImmutableSourceV2,
    pub capability: ModelPackCapabilityV1,
    pub artifacts: Vec<ModelPackArtifactV2>,
    pub runtime: ModelPackRuntimeV2,
    pub hardware: ModelPackHardwareV2,
    pub resources: ModelPackResourcesV2,
    pub license: ModelPackLicenseV2,
    pub self_test: ModelPackSelfTestV2,
    pub voices: Vec<ModelPackVoiceV2>,
    pub extensions: ModelPackRoleExtensionsV2,
    pub lifecycle: ModelPackLifecycleV2,
    pub trust: ModelPackTrustV2,
    pub admission: ModelPackAdmissionV2,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImmutableSourceV2 {
    pub project_url: String,
    pub immutable_revision: String,
    pub model_card_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelPackArtifactRoleV2 {
    ModelWeights,
    Runtime,
    Tokenizer,
    VoiceData,
    LanguageData,
    NativeBridge,
    License,
    Notice,
    Fixture,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackArtifactV2 {
    pub id: String,
    pub role: ModelPackArtifactRoleV2,
    pub kind: ArtifactKind,
    pub archive_format: Option<ArchiveFormatV1>,
    pub source_urls: Vec<String>,
    pub size_bytes: u64,
    pub sha256: Sha256Digest,
    pub destination: String,
    pub strip_prefix: Option<String>,
    pub required_paths: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackRuntimeV2 {
    pub runtime: String,
    pub immutable_revision: Option<String>,
    pub abi: String,
    pub entrypoint: Option<String>,
    pub supported_platforms: BTreeSet<Platform>,
    pub supported_architectures: BTreeSet<Architecture>,
    pub backends: BTreeSet<String>,
    pub network_access_after_install: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackHardwareV2 {
    pub minimum_ram_bytes: u64,
    pub recommended_ram_bytes: u64,
    pub minimum_vram_bytes: Option<u64>,
    pub recommended_vram_bytes: Option<u64>,
    pub minimum_cpu_threads: u16,
    pub accelerators: BTreeSet<Accelerator>,
    pub required_cpu_features: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackResourcesV2 {
    pub storage_bytes: u64,
    pub peak_install_bytes: u64,
    /// Planning disclosure only. Never accepted by the resource governor.
    pub planning_resident_ram_bytes: Option<u64>,
    /// Planning disclosure only. Never accepted by the resource governor.
    pub planning_resident_vram_bytes: Option<u64>,
    /// Planning disclosure only. Never accepted by the resource governor.
    pub planning_load_millis: Option<u64>,
    pub planning_hardware: Option<String>,
    pub quality_tier: QualityTier,
    pub languages: BTreeSet<String>,
    pub measurement: ResourceMeasurementRequirementV2,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceMeasurementRequirementV2 {
    pub required_schema: String,
    pub minimum_samples: u32,
    pub current_device_fingerprint_required: bool,
    pub signed_evidence_required: bool,
    pub p99_reload_required: bool,
    pub manifest_values_not_for_admission: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackLicenseV2 {
    pub spdx_expression: Option<String>,
    pub license_name: String,
    pub license_url: String,
    pub attribution: String,
    pub redistributable: bool,
    pub commercial_use: crate::LicensePermission,
    pub derivative_use: crate::LicensePermission,
    pub acceptance_required: bool,
    pub notices: Vec<String>,
    pub components: Vec<ModelPackLicenseComponentV2>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackLicenseComponentV2 {
    pub id: String,
    pub component: String,
    pub spdx_expression: Option<String>,
    pub license_url: String,
    pub immutable_source_revision: String,
    pub redistributable: Option<bool>,
    pub notice_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackSelfTestV2 {
    pub kind: String,
    pub suite_revision: String,
    pub allowed_runtime_backends: BTreeSet<String>,
    pub input_fixture: String,
    pub input_fixture_sha256: Option<Sha256Digest>,
    pub expected_output_sha256: Option<Sha256Digest>,
    pub timeout_millis: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackVoiceV2 {
    pub voice_id: String,
    pub display_name: String,
    pub locale: String,
    pub stock_voice: bool,
    pub voice_cloning: bool,
    pub license_component_id: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackRoleExtensionsV2 {
    pub language_model: Option<LanguageModelExtensionV2>,
    pub speech_recognition: Option<SpeechRecognitionExtensionV2>,
    pub speech_synthesis: Option<SpeechSynthesisExtensionV2>,
    pub embedding: Option<EmbeddingExtensionV2>,
    pub vision: Option<VisionExtensionV2>,
    pub lip_sync: Option<LipSyncExtensionV2>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageModelExtensionV2 {
    pub context_tokens: Option<u32>,
    pub structured_output: Option<bool>,
    pub tool_calling: Option<bool>,
    pub thinking_mode: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechRecognitionExtensionV2 {
    pub streaming: Option<bool>,
    pub input_sample_formats: Option<BTreeSet<String>>,
    pub accepted_sample_rates_hz: Option<BTreeSet<u32>>,
    pub working_sample_rate_hz: Option<u32>,
    pub channels: Option<u8>,
    pub partial_transcripts: Option<bool>,
    pub word_timestamps: Option<bool>,
    pub diarization: Option<bool>,
    pub maximum_concurrent_sessions: Option<u16>,
    pub maximum_buffered_audio_millis: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechSynthesisExtensionV2 {
    pub output_sample_rate_hz: Option<u32>,
    pub channels: Option<u8>,
    pub sample_format: Option<String>,
    pub incremental_pcm: Option<bool>,
    pub voice_cloning: Option<bool>,
    pub maximum_concurrent_sessions: Option<u16>,
    pub maximum_text_utf8_bytes: Option<u64>,
    pub word_timing: Option<bool>,
    pub phoneme_timing: Option<bool>,
    pub viseme_timing: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingExtensionV2 {
    pub dimensions: Option<u32>,
    pub maximum_tokens: Option<u32>,
    pub pooling: Option<String>,
    pub normalized: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisionExtensionV2 {
    pub input_pixel_formats: Option<BTreeSet<String>>,
    pub output_signals: Option<BTreeSet<String>>,
    pub maximum_signal_rate_hz: Option<u16>,
    pub identity_recognition: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LipSyncExtensionV2 {
    pub audio_sample_formats: Option<BTreeSet<String>>,
    pub output_signals: Option<BTreeSet<String>>,
    pub causal: Option<bool>,
    pub maximum_queue_depth: Option<u16>,
    pub stale_generation_drop_required: Option<bool>,
    pub stale_frame_drop_required: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackLifecycleV2 {
    pub explicit_download_required: bool,
    pub automatic_download_allowed: bool,
    pub install_strategy: InstallStrategyV2,
    pub repair_strategy: RepairStrategyV2,
    pub remove_requires_unreferenced: bool,
    pub activation_gates: BTreeSet<ActivationGateV2>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStrategyV2 {
    VerifyThenAtomicActivate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairStrategyV2 {
    VerifyQuarantineReinstall,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationGateV2 {
    ExplicitUserApproval,
    CatalogTrust,
    ArtifactIntegrity,
    LicenseAcceptance,
    RuntimeCompatibility,
    SelfTestAttestation,
    CurrentDeviceMeasurement,
    WholeLoadoutAdmission,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackTrustV2 {
    pub release_catalog_required: bool,
    pub immutable_revision_required: bool,
    pub artifact_sha256_required: bool,
    pub self_test_attestation_required: bool,
    pub measured_resource_envelope_required: bool,
    pub unsigned_activation_allowed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelPackAdmissionStateV2 {
    CandidateUnqualified,
    BlockedPendingMeasurement,
    EligibleAfterExternalAdmission,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackAdmissionV2 {
    pub state: ModelPackAdmissionStateV2,
    pub reason: String,
    pub allowed_residencies: BTreeSet<crate::ResidencyModeV1>,
    pub unknowns_fail_closed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelPackNormalizationOriginV2 {
    CanonicalV2,
    DeterministicGenericV1Adapter,
}

#[derive(Clone, Debug)]
pub struct NormalizedModelPackManifestV2 {
    pub document: ModelPackManifestV2,
    pub core_manifest: ModelPackManifestV1,
    pub origin: ModelPackNormalizationOriginV2,
    pub canonical_document_sha256: Sha256Digest,
}

pub fn parse_and_normalize_model_pack_manifest(
    bytes: &[u8],
) -> Result<NormalizedModelPackManifestV2, ModelPackManifestV2Error> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let schema = value
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .get("schemaVersion")
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            value
                .get("schema_version")
                .and_then(serde_json::Value::as_str)
        })
        .ok_or(ModelPackManifestV2Error::MissingSchema)?;
    match schema {
        MODEL_PACK_MANIFEST_SCHEMA_V2 => {
            validate_required_v2_shape(&value)?;
            let document: ModelPackManifestV2 = serde_json::from_value(value)?;
            normalize_v2(document, ModelPackNormalizationOriginV2::CanonicalV2)
        }
        MODEL_PACK_MANIFEST_SCHEMA_V1 => {
            let legacy: LegacyGenericManifestV1 = serde_json::from_value(value)?;
            let document = adapt_generic_v1(legacy)?;
            normalize_v2(
                document,
                ModelPackNormalizationOriginV2::DeterministicGenericV1Adapter,
            )
        }
        "npc.local-model-pack/v2" => Err(ModelPackManifestV2Error::LegacyRoleSchema {
            schema: schema.to_owned(),
            adapter: "moonshine_local_model_v2_to_npc_model_pack_v2",
        }),
        "npc.local-tts-pack/v1" => Err(ModelPackManifestV2Error::LegacyRoleSchema {
            schema: schema.to_owned(),
            adapter: "local_tts_v1_to_npc_model_pack_v2",
        }),
        other => Err(ModelPackManifestV2Error::UnsupportedSchema(
            other.to_owned(),
        )),
    }
}

fn validate_required_v2_shape(value: &serde_json::Value) -> Result<(), ModelPackManifestV2Error> {
    let require = |object: &serde_json::Value,
                   object_path: &str,
                   fields: &[&str]|
     -> Result<(), ModelPackManifestV2Error> {
        let object = object.as_object().ok_or_else(|| {
            ModelPackManifestV2Error::MissingRequiredField(object_path.to_owned())
        })?;
        for field in fields {
            if !object.contains_key(*field) {
                return Err(ModelPackManifestV2Error::MissingRequiredField(format!(
                    "{object_path}.{field}"
                )));
            }
        }
        Ok(())
    };
    require(&value["source"], "source", &["model_card_url"])?;
    for (index, artifact) in value["artifacts"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        require(
            artifact,
            &format!("artifacts[{index}]"),
            &["archive_format", "strip_prefix"],
        )?;
    }
    require(
        &value["runtime"],
        "runtime",
        &[
            "immutable_revision",
            "entrypoint",
            "network_access_after_install",
        ],
    )?;
    require(
        &value["hardware"],
        "hardware",
        &["minimum_vram_bytes", "recommended_vram_bytes"],
    )?;
    require(
        &value["resources"],
        "resources",
        &[
            "planning_resident_ram_bytes",
            "planning_resident_vram_bytes",
            "planning_load_millis",
            "planning_hardware",
        ],
    )?;
    require(&value["license"], "license", &["spdx_expression"])?;
    for (index, component) in value["license"]["components"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        require(
            component,
            &format!("license.components[{index}]"),
            &["spdx_expression", "redistributable"],
        )?;
    }
    require(
        &value["self_test"],
        "self_test",
        &["expected_output_sha256"],
    )?;
    let kind = value["capability"]["kind"].as_str().unwrap_or_default();
    let (extension, fields): (&str, &[&str]) = match kind {
        "language_model" => (
            "language_model",
            &[
                "context_tokens",
                "structured_output",
                "tool_calling",
                "thinking_mode",
            ],
        ),
        "speech_recognition" => (
            "speech_recognition",
            &[
                "streaming",
                "input_sample_formats",
                "accepted_sample_rates_hz",
                "working_sample_rate_hz",
                "channels",
                "partial_transcripts",
                "word_timestamps",
                "diarization",
                "maximum_concurrent_sessions",
                "maximum_buffered_audio_millis",
            ],
        ),
        "speech_synthesis" => (
            "speech_synthesis",
            &[
                "output_sample_rate_hz",
                "channels",
                "sample_format",
                "incremental_pcm",
                "voice_cloning",
                "maximum_concurrent_sessions",
                "maximum_text_utf8_bytes",
                "word_timing",
                "phoneme_timing",
                "viseme_timing",
            ],
        ),
        "embedding" => (
            "embedding",
            &["dimensions", "maximum_tokens", "pooling", "normalized"],
        ),
        "vision" => (
            "vision",
            &[
                "input_pixel_formats",
                "output_signals",
                "maximum_signal_rate_hz",
                "identity_recognition",
            ],
        ),
        "lip_sync" => (
            "lip_sync",
            &[
                "audio_sample_formats",
                "output_signals",
                "causal",
                "maximum_queue_depth",
                "stale_generation_drop_required",
                "stale_frame_drop_required",
            ],
        ),
        _ => return Ok(()),
    };
    let extension_value = &value["extensions"][extension];
    if !extension_value.is_object() {
        // The semantic validator reports the more useful role mismatch. This
        // preflight only distinguishes an explicitly present nullable fact from
        // an accidentally omitted one inside the selected extension.
        return Ok(());
    }
    require(extension_value, &format!("extensions.{extension}"), fields)
}

pub fn normalize_model_pack_manifest_v2(
    document: ModelPackManifestV2,
) -> Result<NormalizedModelPackManifestV2, ModelPackManifestV2Error> {
    normalize_v2(document, ModelPackNormalizationOriginV2::CanonicalV2)
}

fn normalize_v2(
    document: ModelPackManifestV2,
    origin: ModelPackNormalizationOriginV2,
) -> Result<NormalizedModelPackManifestV2, ModelPackManifestV2Error> {
    validate_v2(&document)?;
    let core_manifest = to_core_manifest(&document)?;
    core_manifest
        .validate()
        .map_err(ModelPackManifestV2Error::CoreManifest)?;
    let canonical = serde_json::to_vec(&document)?;
    Ok(NormalizedModelPackManifestV2 {
        document,
        core_manifest,
        origin,
        canonical_document_sha256: Sha256Digest::of_bytes(&canonical),
    })
}

fn validate_v2(document: &ModelPackManifestV2) -> Result<(), ModelPackManifestV2Error> {
    if document.schema != MODEL_PACK_MANIFEST_SCHEMA_V2 {
        return Err(ModelPackManifestV2Error::UnsupportedSchema(
            document.schema.clone(),
        ));
    }
    if document.schema_uri != MODEL_PACK_MANIFEST_JSON_SCHEMA_V2 {
        return Err(ModelPackManifestV2Error::UnexpectedSchemaUri(
            document.schema_uri.clone(),
        ));
    }
    if document.source.immutable_revision.trim().is_empty()
        || is_moving_revision(&document.source.immutable_revision)
        || document
            .runtime
            .immutable_revision
            .as_ref()
            .is_some_and(|revision| revision.trim().is_empty() || is_moving_revision(revision))
    {
        return Err(ModelPackManifestV2Error::MutableRevision);
    }
    if document.artifacts.is_empty() || document.runtime.backends.is_empty() {
        return Err(ModelPackManifestV2Error::IncompleteDocument);
    }
    for artifact in &document.artifacts {
        match (&artifact.kind, &artifact.archive_format) {
            (ArtifactKind::Archive, None) => {
                return Err(ModelPackManifestV2Error::ArchiveFormatRequired(
                    artifact.id.clone(),
                ))
            }
            (ArtifactKind::File, Some(_)) => {
                return Err(ModelPackManifestV2Error::ArchiveFormatForbidden(
                    artifact.id.clone(),
                ))
            }
            _ => {}
        }
        if artifact.kind == ArtifactKind::File
            && (artifact.strip_prefix.is_some() || !artifact.required_paths.is_empty())
        {
            return Err(ModelPackManifestV2Error::FileExtractionMetadata(
                artifact.id.clone(),
            ));
        }
    }
    let measurement = &document.resources.measurement;
    if measurement.required_schema != MEASURED_RESOURCE_ENVELOPE_SCHEMA_V1
        || measurement.minimum_samples < 20
        || !measurement.current_device_fingerprint_required
        || !measurement.signed_evidence_required
        || !measurement.p99_reload_required
        || !measurement.manifest_values_not_for_admission
    {
        return Err(ModelPackManifestV2Error::UnsafeMeasurementPolicy);
    }
    if !document.trust.release_catalog_required
        || !document.trust.immutable_revision_required
        || !document.trust.artifact_sha256_required
        || !document.trust.self_test_attestation_required
        || !document.trust.measured_resource_envelope_required
        || document.trust.unsigned_activation_allowed
        || !document.admission.unknowns_fail_closed
        || document.admission.allowed_residencies.is_empty()
    {
        return Err(ModelPackManifestV2Error::UnsafeTrustOrAdmissionPolicy);
    }
    validate_extensions(document)?;
    let license_ids: BTreeSet<_> = document
        .license
        .components
        .iter()
        .map(|component| component.id.as_str())
        .collect();
    if license_ids.len() != document.license.components.len()
        || document
            .voices
            .iter()
            .any(|voice| !license_ids.contains(voice.license_component_id.as_str()))
    {
        return Err(ModelPackManifestV2Error::InvalidLicenseComponentBinding);
    }
    if !document.voices.is_empty() && document.capability.kind != ModelPackKindV1::SpeechSynthesis {
        return Err(ModelPackManifestV2Error::VoicesOutsideSpeechSynthesis);
    }
    Ok(())
}

fn validate_extensions(document: &ModelPackManifestV2) -> Result<(), ModelPackManifestV2Error> {
    let extensions = &document.extensions;
    let present = [
        extensions.language_model.is_some(),
        extensions.speech_recognition.is_some(),
        extensions.speech_synthesis.is_some(),
        extensions.embedding.is_some(),
        extensions.vision.is_some(),
        extensions.lip_sync.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if present > 1 {
        return Err(ModelPackManifestV2Error::MultipleRoleExtensions);
    }
    let matches = match document.capability.kind {
        ModelPackKindV1::LanguageModel => extensions.language_model.is_some(),
        ModelPackKindV1::SpeechRecognition => extensions.speech_recognition.is_some(),
        ModelPackKindV1::SpeechSynthesis => extensions.speech_synthesis.is_some(),
        ModelPackKindV1::Embedding => extensions.embedding.is_some(),
        ModelPackKindV1::Vision => extensions.vision.is_some(),
        ModelPackKindV1::LipSync => extensions.lip_sync.is_some(),
        ModelPackKindV1::Animation | ModelPackKindV1::Other => present == 0,
    };
    if !matches {
        return Err(ModelPackManifestV2Error::RoleExtensionMismatch);
    }
    Ok(())
}

fn to_core_manifest(
    document: &ModelPackManifestV2,
) -> Result<ModelPackManifestV1, ModelPackManifestV2Error> {
    Ok(ModelPackManifestV1 {
        schema: MODEL_PACK_MANIFEST_SCHEMA_V1.to_owned(),
        pack_id: document.pack_id.clone(),
        revision: document.revision.clone(),
        display_name: document.display_name.clone(),
        description: document.description.clone(),
        source_project: document.source.project_url.clone(),
        source_revision: document.source.immutable_revision.clone(),
        capability: document.capability.clone(),
        artifacts: document
            .artifacts
            .iter()
            .map(|artifact| ArtifactV1 {
                id: artifact.id.clone(),
                kind: artifact.kind.clone(),
                archive_format: artifact.archive_format.clone(),
                source_urls: artifact.source_urls.clone(),
                size_bytes: artifact.size_bytes,
                sha256: artifact.sha256.clone(),
                destination: artifact.destination.clone(),
                strip_prefix: artifact.strip_prefix.clone(),
                required_paths: artifact.required_paths.clone(),
            })
            .collect(),
        runtime: RuntimeCompatibilityV1 {
            runtime: document.runtime.runtime.clone(),
            abi: document.runtime.abi.clone(),
            minimum_runtime_revision: document.runtime.immutable_revision.clone(),
            supported_platforms: document.runtime.supported_platforms.clone(),
            supported_architectures: document.runtime.supported_architectures.clone(),
        },
        hardware: HardwareRequirementsV1 {
            minimum_ram_bytes: document.hardware.minimum_ram_bytes,
            recommended_ram_bytes: document.hardware.recommended_ram_bytes,
            minimum_vram_bytes: document.hardware.minimum_vram_bytes,
            recommended_vram_bytes: document.hardware.recommended_vram_bytes,
            minimum_cpu_threads: document.hardware.minimum_cpu_threads,
            accelerators: document.hardware.accelerators.clone(),
            required_cpu_features: document.hardware.required_cpu_features.clone(),
        },
        resources: ResourceEnvelopeV1 {
            storage_bytes: document.resources.storage_bytes,
            peak_install_bytes: document.resources.peak_install_bytes,
            measured_ram_bytes: document.resources.planning_resident_ram_bytes,
            measured_vram_bytes: document.resources.planning_resident_vram_bytes,
            measured_load_millis: document.resources.planning_load_millis,
            benchmark_hardware: document.resources.planning_hardware.clone(),
            quality_tier: document.resources.quality_tier.clone(),
            languages: document.resources.languages.clone(),
        },
        license: LicenseMetadataV1 {
            spdx_expression: document.license.spdx_expression.clone(),
            license_name: document.license.license_name.clone(),
            license_url: document.license.license_url.clone(),
            attribution: document.license.attribution.clone(),
            redistributable: document.license.redistributable,
            commercial_use: document.license.commercial_use.clone(),
            derivative_use: document.license.derivative_use.clone(),
            acceptance_required: document.license.acceptance_required,
            notices: document.license.notices.clone(),
        },
        self_test: SelfTestV1 {
            kind: document.self_test.kind.clone(),
            suite_revision: document.self_test.suite_revision.clone(),
            allowed_runtime_backends: document.self_test.allowed_runtime_backends.clone(),
            input_fixture: document.self_test.input_fixture.clone(),
            expected_output_sha256: document.self_test.expected_output_sha256.clone(),
            timeout_millis: document.self_test.timeout_millis,
        },
    })
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyGenericManifestV1 {
    #[serde(rename = "$schema")]
    schema_uri: Option<String>,
    schema: String,
    pack_id: PackId,
    revision: Revision,
    display_name: String,
    description: String,
    source_project: String,
    source_revision: String,
    capability: ModelPackCapabilityV1,
    artifacts: Vec<ArtifactV1>,
    runtime: RuntimeCompatibilityV1,
    hardware: HardwareRequirementsV1,
    resources: ResourceEnvelopeV1,
    license: LicenseMetadataV1,
    self_test: SelfTestV1,
}

fn adapt_generic_v1(
    legacy: LegacyGenericManifestV1,
) -> Result<ModelPackManifestV2, ModelPackManifestV2Error> {
    let _ = legacy.schema_uri;
    if legacy.schema != MODEL_PACK_MANIFEST_SCHEMA_V1 {
        return Err(ModelPackManifestV2Error::UnsupportedSchema(legacy.schema));
    }
    let allowed_residencies = allowed_residencies(&legacy.hardware.accelerators);
    let extension = conservative_extension(&legacy.capability.kind);
    Ok(ModelPackManifestV2 {
        schema_uri: MODEL_PACK_MANIFEST_JSON_SCHEMA_V2.to_owned(),
        schema: MODEL_PACK_MANIFEST_SCHEMA_V2.to_owned(),
        pack_id: legacy.pack_id,
        revision: legacy.revision,
        display_name: legacy.display_name,
        description: legacy.description,
        source: ImmutableSourceV2 {
            project_url: legacy.source_project,
            immutable_revision: legacy.source_revision,
            model_card_url: None,
        },
        artifacts: legacy
            .artifacts
            .into_iter()
            .map(|artifact| ModelPackArtifactV2 {
                id: artifact.id,
                role: ModelPackArtifactRoleV2::Other,
                kind: artifact.kind,
                archive_format: artifact.archive_format,
                source_urls: artifact.source_urls,
                size_bytes: artifact.size_bytes,
                sha256: artifact.sha256,
                destination: artifact.destination,
                strip_prefix: None,
                required_paths: Vec::new(),
            })
            .collect(),
        runtime: ModelPackRuntimeV2 {
            runtime: legacy.runtime.runtime,
            immutable_revision: legacy.runtime.minimum_runtime_revision,
            abi: legacy.runtime.abi,
            entrypoint: None,
            supported_platforms: legacy.runtime.supported_platforms,
            supported_architectures: legacy.runtime.supported_architectures,
            backends: legacy.self_test.allowed_runtime_backends.clone(),
            network_access_after_install: None,
        },
        hardware: ModelPackHardwareV2 {
            minimum_ram_bytes: legacy.hardware.minimum_ram_bytes,
            recommended_ram_bytes: legacy.hardware.recommended_ram_bytes,
            minimum_vram_bytes: legacy.hardware.minimum_vram_bytes,
            recommended_vram_bytes: legacy.hardware.recommended_vram_bytes,
            minimum_cpu_threads: legacy.hardware.minimum_cpu_threads,
            accelerators: legacy.hardware.accelerators,
            required_cpu_features: legacy.hardware.required_cpu_features,
        },
        resources: ModelPackResourcesV2 {
            storage_bytes: legacy.resources.storage_bytes,
            peak_install_bytes: legacy.resources.peak_install_bytes,
            planning_resident_ram_bytes: legacy.resources.measured_ram_bytes,
            planning_resident_vram_bytes: legacy.resources.measured_vram_bytes,
            planning_load_millis: legacy.resources.measured_load_millis,
            planning_hardware: legacy.resources.benchmark_hardware,
            quality_tier: legacy.resources.quality_tier,
            languages: legacy.resources.languages,
            measurement: strict_measurement_requirement(),
        },
        license: ModelPackLicenseV2 {
            spdx_expression: legacy.license.spdx_expression,
            license_name: legacy.license.license_name,
            license_url: legacy.license.license_url,
            attribution: legacy.license.attribution,
            redistributable: legacy.license.redistributable,
            commercial_use: legacy.license.commercial_use,
            derivative_use: legacy.license.derivative_use,
            acceptance_required: legacy.license.acceptance_required,
            notices: legacy.license.notices,
            components: Vec::new(),
        },
        self_test: ModelPackSelfTestV2 {
            kind: legacy.self_test.kind,
            suite_revision: legacy.self_test.suite_revision,
            allowed_runtime_backends: legacy.self_test.allowed_runtime_backends,
            input_fixture: legacy.self_test.input_fixture,
            input_fixture_sha256: None,
            expected_output_sha256: legacy.self_test.expected_output_sha256,
            timeout_millis: legacy.self_test.timeout_millis,
        },
        voices: Vec::new(),
        extensions: extension,
        lifecycle: conservative_lifecycle(),
        trust: strict_trust(),
        admission: ModelPackAdmissionV2 {
            state: ModelPackAdmissionStateV2::BlockedPendingMeasurement,
            reason: "deterministic v1 migration remains blocked until signed current-device evidence and whole-loadout admission".to_owned(),
            allowed_residencies,
            unknowns_fail_closed: true,
        },
        capability: legacy.capability,
    })
}

fn conservative_extension(kind: &ModelPackKindV1) -> ModelPackRoleExtensionsV2 {
    let mut extensions = ModelPackRoleExtensionsV2::default();
    match kind {
        ModelPackKindV1::LanguageModel => {
            extensions.language_model = Some(LanguageModelExtensionV2 {
                context_tokens: None,
                structured_output: None,
                tool_calling: None,
                thinking_mode: None,
            });
        }
        ModelPackKindV1::SpeechRecognition => {
            extensions.speech_recognition = Some(SpeechRecognitionExtensionV2 {
                streaming: None,
                input_sample_formats: None,
                accepted_sample_rates_hz: None,
                working_sample_rate_hz: None,
                channels: None,
                partial_transcripts: None,
                word_timestamps: None,
                diarization: None,
                maximum_concurrent_sessions: None,
                maximum_buffered_audio_millis: None,
            });
        }
        ModelPackKindV1::SpeechSynthesis => {
            extensions.speech_synthesis = Some(SpeechSynthesisExtensionV2 {
                output_sample_rate_hz: None,
                channels: None,
                sample_format: None,
                incremental_pcm: None,
                voice_cloning: None,
                maximum_concurrent_sessions: None,
                maximum_text_utf8_bytes: None,
                word_timing: None,
                phoneme_timing: None,
                viseme_timing: None,
            });
        }
        ModelPackKindV1::Embedding => {
            extensions.embedding = Some(EmbeddingExtensionV2 {
                dimensions: None,
                maximum_tokens: None,
                pooling: None,
                normalized: None,
            });
        }
        ModelPackKindV1::Vision => {
            extensions.vision = Some(VisionExtensionV2 {
                input_pixel_formats: None,
                output_signals: None,
                maximum_signal_rate_hz: None,
                identity_recognition: None,
            });
        }
        ModelPackKindV1::LipSync => {
            extensions.lip_sync = Some(LipSyncExtensionV2 {
                audio_sample_formats: None,
                output_signals: None,
                causal: None,
                maximum_queue_depth: None,
                stale_generation_drop_required: None,
                stale_frame_drop_required: None,
            });
        }
        ModelPackKindV1::Animation | ModelPackKindV1::Other => {}
    }
    extensions
}

fn allowed_residencies(accelerators: &BTreeSet<Accelerator>) -> BTreeSet<crate::ResidencyModeV1> {
    let mut modes = BTreeSet::new();
    if accelerators.contains(&Accelerator::Cpu) {
        modes.insert(crate::ResidencyModeV1::CpuResident);
    }
    if accelerators.iter().any(|item| item != &Accelerator::Cpu) {
        modes.insert(crate::ResidencyModeV1::GpuResident);
    }
    if modes.is_empty() {
        modes.insert(crate::ResidencyModeV1::CpuResident);
    }
    modes
}

fn strict_measurement_requirement() -> ResourceMeasurementRequirementV2 {
    ResourceMeasurementRequirementV2 {
        required_schema: MEASURED_RESOURCE_ENVELOPE_SCHEMA_V1.to_owned(),
        minimum_samples: 20,
        current_device_fingerprint_required: true,
        signed_evidence_required: true,
        p99_reload_required: true,
        manifest_values_not_for_admission: true,
    }
}

fn strict_trust() -> ModelPackTrustV2 {
    ModelPackTrustV2 {
        release_catalog_required: true,
        immutable_revision_required: true,
        artifact_sha256_required: true,
        self_test_attestation_required: true,
        measured_resource_envelope_required: true,
        unsigned_activation_allowed: false,
    }
}

fn conservative_lifecycle() -> ModelPackLifecycleV2 {
    ModelPackLifecycleV2 {
        explicit_download_required: true,
        automatic_download_allowed: false,
        install_strategy: InstallStrategyV2::VerifyThenAtomicActivate,
        repair_strategy: RepairStrategyV2::VerifyQuarantineReinstall,
        remove_requires_unreferenced: true,
        activation_gates: BTreeSet::from([
            ActivationGateV2::ExplicitUserApproval,
            ActivationGateV2::CatalogTrust,
            ActivationGateV2::ArtifactIntegrity,
            ActivationGateV2::LicenseAcceptance,
            ActivationGateV2::RuntimeCompatibility,
            ActivationGateV2::SelfTestAttestation,
            ActivationGateV2::CurrentDeviceMeasurement,
            ActivationGateV2::WholeLoadoutAdmission,
        ]),
    }
}

fn is_moving_revision(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "main" | "master" | "head" | "latest"
    )
}

#[derive(Debug, Error)]
pub enum ModelPackManifestV2Error {
    #[error("model-pack document is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("model-pack document has no recognized schema discriminator")]
    MissingSchema,
    #[error("unsupported model-pack schema: {0}")]
    UnsupportedSchema(String),
    #[error("legacy role schema {schema} requires deterministic adapter {adapter}")]
    LegacyRoleSchema {
        schema: String,
        adapter: &'static str,
    },
    #[error("canonical v2 document must use schema URI {MODEL_PACK_MANIFEST_JSON_SCHEMA_V2}: {0}")]
    UnexpectedSchemaUri(String),
    #[error("canonical v2 document is missing required field {0}")]
    MissingRequiredField(String),
    #[error("source and runtime revisions must be immutable")]
    MutableRevision,
    #[error("model-pack document is incomplete")]
    IncompleteDocument,
    #[error("archive artifact requires archive_format: {0}")]
    ArchiveFormatRequired(String),
    #[error("file artifact must not declare archive_format: {0}")]
    ArchiveFormatForbidden(String),
    #[error("file artifact must not declare extraction metadata: {0}")]
    FileExtractionMetadata(String),
    #[error("resource measurement policy is unsafe or incomplete")]
    UnsafeMeasurementPolicy,
    #[error("trust/admission policy is unsafe or incomplete")]
    UnsafeTrustOrAdmissionPolicy,
    #[error("more than one role extension is present")]
    MultipleRoleExtensions,
    #[error("role extension does not match capability")]
    RoleExtensionMismatch,
    #[error("voice entries are only valid for speech_synthesis")]
    VoicesOutsideSpeechSynthesis,
    #[error("voice/license component binding is invalid")]
    InvalidLicenseComponentBinding,
    #[error("normalized core manifest is invalid: {0}")]
    CoreManifest(crate::ManifestError),
}
