use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelPackKind {
    LanguageModel,
    SpeechToText,
    TextToSpeech,
    Embedding,
    VoiceActivityDetection,
    LipSync,
    Vision,
    Runtime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputeBackend {
    Cpu,
    Cuda,
    DirectMl,
    Vulkan,
    WindowsMl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuArchitecture {
    X86_64,
    Aarch64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImmutableSourceV1 {
    pub project_url: String,
    pub revision: String,
    pub download_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackArtifactV1 {
    /// Safe path relative to the pack root.
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub executable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAbiV1 {
    pub runtime_id: String,
    pub abi_version: String,
    pub minimum_runtime_version: String,
    pub maximum_runtime_version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareConstraintsV1 {
    pub architectures: Vec<CpuArchitecture>,
    pub backends: Vec<ComputeBackend>,
    pub minimum_ram_mib: u64,
    pub minimum_vram_mib: u64,
    pub minimum_free_disk_mib: u64,
    pub required_cpu_features: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeasuredResourceEnvelopeV1 {
    pub hardware_class: String,
    pub backend: ComputeBackend,
    pub peak_ram_mib: u64,
    pub peak_vram_mib: u64,
    pub cold_load_ms: u64,
    pub throughput_units_per_second: f64,
    pub measurement_date: String,
    pub benchmark_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseDescriptorV1 {
    pub spdx_expression: Option<String>,
    pub license_name: String,
    pub license_url: String,
    pub redistributable: bool,
    pub commercial_use_allowed: bool,
    pub derivative_use_allowed: bool,
    pub gated_access: bool,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionV1 {
    pub title: String,
    pub creator: String,
    pub source_url: String,
    pub notice: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackSelfTestV1 {
    /// Runtime-owned test ID. This is never a shell command.
    pub test_id: String,
    pub expected_output_sha256: Option<String>,
    pub timeout_ms: u64,
}

/// Human-reviewable, immutable catalog entry for a downloadable model/runtime pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelPackManifestV1 {
    pub schema_version: u32,
    pub pack_id: String,
    pub display_name: String,
    pub version: String,
    pub kind: ModelPackKind,
    pub source: ImmutableSourceV1,
    pub artifacts: Vec<PackArtifactV1>,
    pub runtime_abi: RuntimeAbiV1,
    pub hardware: HardwareConstraintsV1,
    pub measured_resources: Vec<MeasuredResourceEnvelopeV1>,
    pub license: LicenseDescriptorV1,
    pub attributions: Vec<AttributionV1>,
    pub self_tests: Vec<PackSelfTestV1>,
    pub dependency_pack_ids: Vec<String>,
    pub languages: Vec<String>,
    pub total_installed_bytes: u64,
}

impl ModelPackManifestV1 {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn validate(&self) -> Result<(), ModelManifestError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(ModelManifestError::UnsupportedSchema(self.schema_version));
        }
        validate_id(&self.pack_id)?;
        if self.display_name.trim().is_empty() || self.display_name.len() > 128 {
            return Err(ModelManifestError::InvalidDisplayName);
        }
        validate_version(&self.version)?;
        validate_url(&self.source.project_url)?;
        validate_url(&self.source.download_url)?;
        if self.source.revision.trim().is_empty() || self.source.revision.len() > 256 {
            return Err(ModelManifestError::MutableOrMissingRevision);
        }
        if self.artifacts.is_empty() || self.artifacts.len() > 10_000 {
            return Err(ModelManifestError::InvalidArtifactCount);
        }
        let mut paths = BTreeSet::new();
        let mut computed_size = 0_u64;
        for artifact in &self.artifacts {
            validate_relative_path(&artifact.path)?;
            if !paths.insert(artifact.path.to_ascii_lowercase()) {
                return Err(ModelManifestError::DuplicateArtifact(artifact.path.clone()));
            }
            if artifact.size_bytes == 0 || !is_sha256(&artifact.sha256) {
                return Err(ModelManifestError::InvalidArtifact(artifact.path.clone()));
            }
            computed_size = computed_size
                .checked_add(artifact.size_bytes)
                .ok_or(ModelManifestError::SizeOverflow)?;
        }
        if computed_size != self.total_installed_bytes {
            return Err(ModelManifestError::InstalledSizeMismatch {
                declared: self.total_installed_bytes,
                computed: computed_size,
            });
        }
        validate_id(&self.runtime_abi.runtime_id)?;
        validate_version(&self.runtime_abi.abi_version)?;
        validate_version(&self.runtime_abi.minimum_runtime_version)?;
        if let Some(maximum) = &self.runtime_abi.maximum_runtime_version {
            validate_version(maximum)?;
        }
        if self.hardware.architectures.is_empty()
            || self.hardware.backends.is_empty()
            || self.hardware.minimum_free_disk_mib == 0
        {
            return Err(ModelManifestError::InvalidHardwareConstraints);
        }
        if self.measured_resources.len() > 128 {
            return Err(ModelManifestError::TooManyMeasurements);
        }
        for measurement in &self.measured_resources {
            if measurement.hardware_class.trim().is_empty()
                || measurement.hardware_class.len() > 256
                || !measurement.throughput_units_per_second.is_finite()
                || measurement.throughput_units_per_second <= 0.0
                || measurement.measurement_date.len() > 32
                || measurement.benchmark_revision.is_empty()
            {
                return Err(ModelManifestError::InvalidMeasurement);
            }
        }
        if self.license.license_name.trim().is_empty() {
            return Err(ModelManifestError::MissingLicense);
        }
        validate_url(&self.license.license_url)?;
        if self.attributions.is_empty() {
            return Err(ModelManifestError::MissingAttribution);
        }
        for attribution in &self.attributions {
            if attribution.title.trim().is_empty() || attribution.creator.trim().is_empty() {
                return Err(ModelManifestError::MissingAttribution);
            }
            validate_url(&attribution.source_url)?;
        }
        if self.self_tests.is_empty() || self.self_tests.len() > 64 {
            return Err(ModelManifestError::InvalidSelfTests);
        }
        for test in &self.self_tests {
            validate_id(&test.test_id)?;
            if test.timeout_ms == 0 || test.timeout_ms > 600_000 {
                return Err(ModelManifestError::InvalidSelfTests);
            }
            if test
                .expected_output_sha256
                .as_ref()
                .is_some_and(|hash| !is_sha256(hash))
            {
                return Err(ModelManifestError::InvalidSelfTests);
            }
        }
        for dependency in &self.dependency_pack_ids {
            validate_id(dependency)?;
            if dependency == &self.pack_id {
                return Err(ModelManifestError::SelfDependency);
            }
        }
        if self.languages.len() > 1_024 || self.languages.iter().any(|value| value.len() > 64) {
            return Err(ModelManifestError::InvalidLanguages);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Error)]
pub enum ModelManifestError {
    #[error("unsupported model manifest schema {0}")]
    UnsupportedSchema(u32),
    #[error("stable identifier is malformed")]
    InvalidId,
    #[error("display name is missing or too large")]
    InvalidDisplayName,
    #[error("version is malformed")]
    InvalidVersion,
    #[error("URL must use HTTPS")]
    InvalidUrl,
    #[error("source revision must be immutable and non-empty")]
    MutableOrMissingRevision,
    #[error("artifact count is invalid")]
    InvalidArtifactCount,
    #[error("artifact path is unsafe: {0}")]
    UnsafeArtifactPath(String),
    #[error("artifact is invalid: {0}")]
    InvalidArtifact(String),
    #[error("artifact path appears more than once: {0}")]
    DuplicateArtifact(String),
    #[error("artifact sizes overflow u64")]
    SizeOverflow,
    #[error("installed size mismatch: declared {declared}, computed {computed}")]
    InstalledSizeMismatch { declared: u64, computed: u64 },
    #[error("hardware constraints are incomplete")]
    InvalidHardwareConstraints,
    #[error("too many measured resource records")]
    TooManyMeasurements,
    #[error("measured resource record is invalid")]
    InvalidMeasurement,
    #[error("license metadata is missing")]
    MissingLicense,
    #[error("attribution metadata is missing")]
    MissingAttribution,
    #[error("self-test metadata is invalid")]
    InvalidSelfTests,
    #[error("model pack cannot depend on itself")]
    SelfDependency,
    #[error("language metadata is invalid")]
    InvalidLanguages,
}

fn validate_id(value: &str) -> Result<(), ModelManifestError> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.' | b'_')
        })
    {
        Err(ModelManifestError::InvalidId)
    } else {
        Ok(())
    }
}

fn validate_version(value: &str) -> Result<(), ModelManifestError> {
    let mut parts = value.split('.');
    let valid = (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) && parts.next().is_none();
    if valid {
        Ok(())
    } else {
        Err(ModelManifestError::InvalidVersion)
    }
}

fn validate_url(value: &str) -> Result<(), ModelManifestError> {
    if value.starts_with("https://") && value.len() <= 2_048 && !value.contains(char::is_whitespace)
    {
        Ok(())
    } else {
        Err(ModelManifestError::InvalidUrl)
    }
}

fn validate_relative_path(value: &str) -> Result<(), ModelManifestError> {
    let normalized = value.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || normalized.contains('\0')
    {
        Err(ModelManifestError::UnsafeArtifactPath(value.to_owned()))
    } else {
        Ok(())
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn valid_manifest() -> ModelPackManifestV1 {
        ModelPackManifestV1 {
            schema_version: 1,
            pack_id: "kokoro-en-cpu".into(),
            display_name: "Kokoro English CPU".into(),
            version: "1.0.0".into(),
            kind: ModelPackKind::TextToSpeech,
            source: ImmutableSourceV1 {
                project_url: "https://example.invalid/project".into(),
                revision: "0123456789abcdef".into(),
                download_url: "https://example.invalid/pack.zip".into(),
            },
            artifacts: vec![PackArtifactV1 {
                path: "models/kokoro.onnx".into(),
                size_bytes: 42,
                sha256: "a".repeat(64),
                executable: false,
            }],
            runtime_abi: RuntimeAbiV1 {
                runtime_id: "onnx-runtime".into(),
                abi_version: "1.0.0".into(),
                minimum_runtime_version: "1.20.0".into(),
                maximum_runtime_version: None,
            },
            hardware: HardwareConstraintsV1 {
                architectures: vec![CpuArchitecture::X86_64],
                backends: vec![ComputeBackend::Cpu],
                minimum_ram_mib: 512,
                minimum_vram_mib: 0,
                minimum_free_disk_mib: 128,
                required_cpu_features: vec![],
            },
            measured_resources: vec![MeasuredResourceEnvelopeV1 {
                hardware_class: "12th-gen mobile i7".into(),
                backend: ComputeBackend::Cpu,
                peak_ram_mib: 400,
                peak_vram_mib: 0,
                cold_load_ms: 150,
                throughput_units_per_second: 20.0,
                measurement_date: "2026-08-28".into(),
                benchmark_revision: "bench-v1".into(),
            }],
            license: LicenseDescriptorV1 {
                spdx_expression: Some("Apache-2.0".into()),
                license_name: "Apache License 2.0".into(),
                license_url: "https://example.invalid/license".into(),
                redistributable: true,
                commercial_use_allowed: true,
                derivative_use_allowed: true,
                gated_access: false,
                notes: String::new(),
            },
            attributions: vec![AttributionV1 {
                title: "Kokoro".into(),
                creator: "Upstream authors".into(),
                source_url: "https://example.invalid/project".into(),
                notice: "See upstream.".into(),
            }],
            self_tests: vec![PackSelfTestV1 {
                test_id: "synthesize-fixture".into(),
                expected_output_sha256: None,
                timeout_ms: 30_000,
            }],
            dependency_pack_ids: vec![],
            languages: vec!["en".into()],
            total_installed_bytes: 42,
        }
    }

    #[test]
    fn valid_manifest_passes() {
        assert_eq!(valid_manifest().validate(), Ok(()));
    }

    #[test]
    fn traversal_and_absolute_artifact_paths_are_rejected() {
        for path in ["../escape", "models/../../escape", "C:\\escape", "/escape"] {
            let mut manifest = valid_manifest();
            manifest.artifacts[0].path = path.into();
            assert!(matches!(
                manifest.validate(),
                Err(ModelManifestError::UnsafeArtifactPath(_))
            ));
        }
    }

    proptest! {
        #[test]
        fn sha256_requires_exact_hex(value in "[A-Za-z0-9]{0,100}") {
            let mut manifest = valid_manifest();
            manifest.artifacts[0].sha256 = value.clone();
            let expected = value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
            prop_assert_eq!(manifest.validate().is_ok(), expected);
        }
    }
}
