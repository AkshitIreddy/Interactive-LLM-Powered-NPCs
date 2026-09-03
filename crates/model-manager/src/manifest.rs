use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

pub const MODEL_PACK_MANIFEST_SCHEMA_V1: &str = "npc.model-pack/v1";
pub const MAX_PACK_BYTES: u64 = 512 * 1024 * 1024 * 1024;
pub const MAX_ARTIFACTS: usize = 256;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackId(String);

impl PackId {
    pub fn parse(value: impl Into<String>) -> Result<Self, ManifestError> {
        let value = value.into();
        let valid = (3..=96).contains(&value.len())
            && value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.'))
            && value
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && value
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
            && !value.contains("..")
            && !value.contains(".-")
            && !value.contains("-.");
        valid
            .then_some(Self(value.clone()))
            .ok_or(ManifestError::InvalidPackId(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for PackId {
    type Err = ManifestError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(String);

impl Revision {
    pub fn parse(value: impl Into<String>) -> Result<Self, ManifestError> {
        let value = value.into();
        let valid = (1..=128).contains(&value.len())
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
            && value != "."
            && value != "..";
        valid
            .then_some(Self(value.clone()))
            .ok_or(ManifestError::InvalidRevision(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn parse(value: impl Into<String>) -> Result<Self, ManifestError> {
        let value = value.into().to_ascii_lowercase();
        (value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
            .then_some(Self(value.clone()))
            .ok_or(ManifestError::InvalidSha256(value))
    }

    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(hex::encode(Sha256::digest(bytes)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Archive,
    File,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFormatV1 {
    Zip,
    Tar,
    TarGz,
    TarBz2,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArtifactV1 {
    /// Stable identifier used by resume journals; not a URL or filesystem path.
    pub id: String,
    pub kind: ArtifactKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_format: Option<ArchiveFormatV1>,
    pub source_urls: Vec<String>,
    pub size_bytes: u64,
    pub sha256: Sha256Digest,
    /// Relative destination. Archive members are additionally validated while extracting.
    pub destination: String,
    /// Optional immutable archive root removed before destination mapping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip_prefix: Option<String>,
    /// When non-empty, only these exact post-strip files are installed and all
    /// entries must be present. This keeps optional runtime trees closed-world.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_paths: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuntimeCompatibilityV1 {
    pub runtime: String,
    pub abi: String,
    pub minimum_runtime_revision: Option<String>,
    pub supported_platforms: BTreeSet<Platform>,
    pub supported_architectures: BTreeSet<Architecture>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows10,
    Windows11,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X86_64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HardwareRequirementsV1 {
    pub minimum_ram_bytes: u64,
    pub recommended_ram_bytes: u64,
    pub minimum_vram_bytes: Option<u64>,
    pub recommended_vram_bytes: Option<u64>,
    pub minimum_cpu_threads: u16,
    pub accelerators: BTreeSet<Accelerator>,
    pub required_cpu_features: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accelerator {
    Cpu,
    Cuda,
    DirectMl,
    Vulkan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResourceEnvelopeV1 {
    pub storage_bytes: u64,
    pub peak_install_bytes: u64,
    pub measured_ram_bytes: Option<u64>,
    pub measured_vram_bytes: Option<u64>,
    pub measured_load_millis: Option<u64>,
    pub benchmark_hardware: Option<String>,
    pub quality_tier: QualityTier,
    pub languages: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityTier {
    Fast,
    Balanced,
    Quality,
    Experimental,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelPackKindV1 {
    LanguageModel,
    SpeechRecognition,
    SpeechSynthesis,
    Embedding,
    Vision,
    LipSync,
    Animation,
    Other,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelPackScopeV1 {
    Generic,
    GameSpecific,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelPackCapabilityV1 {
    pub kind: ModelPackKindV1,
    pub scope: ModelPackScopeV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LicenseMetadataV1 {
    pub spdx_expression: Option<String>,
    pub license_name: String,
    pub license_url: String,
    pub attribution: String,
    pub redistributable: bool,
    pub commercial_use: LicensePermission,
    pub derivative_use: LicensePermission,
    pub acceptance_required: bool,
    pub notices: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LicensePermission {
    Allowed,
    Prohibited,
    Restricted,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelPackManifestV1 {
    pub schema: String,
    pub pack_id: PackId,
    /// Immutable upstream/model revision, never a moving alias such as `latest`.
    pub revision: Revision,
    pub display_name: String,
    pub description: String,
    pub source_project: String,
    pub source_revision: String,
    pub capability: ModelPackCapabilityV1,
    pub artifacts: Vec<ArtifactV1>,
    pub runtime: RuntimeCompatibilityV1,
    pub hardware: HardwareRequirementsV1,
    pub resources: ResourceEnvelopeV1,
    pub license: LicenseMetadataV1,
    pub self_test: SelfTestV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SelfTestV1 {
    pub kind: String,
    pub suite_revision: String,
    pub allowed_runtime_backends: BTreeSet<String>,
    pub input_fixture: String,
    pub expected_output_sha256: Option<Sha256Digest>,
    pub timeout_millis: u64,
}

impl ModelPackManifestV1 {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema != MODEL_PACK_MANIFEST_SCHEMA_V1 {
            return Err(ManifestError::UnsupportedSchema(self.schema.clone()));
        }
        PackId::parse(self.pack_id.as_str())?;
        Revision::parse(self.revision.as_str())?;
        require_text("display_name", &self.display_name, 1, 160)?;
        require_text("description", &self.description, 1, 4_096)?;
        require_text("source_project", &self.source_project, 1, 512)?;
        require_text("source_revision", &self.source_revision, 1, 256)?;
        if matches!(
            self.source_revision.to_ascii_lowercase().as_str(),
            "main" | "master" | "head" | "latest"
        ) {
            return Err(ManifestError::MutableSourceRevision(
                self.source_revision.clone(),
            ));
        }
        if self.artifacts.is_empty() || self.artifacts.len() > MAX_ARTIFACTS {
            return Err(ManifestError::ArtifactCount(self.artifacts.len()));
        }
        let mut ids = BTreeSet::new();
        let mut destinations = BTreeSet::new();
        let mut total = 0_u64;
        for artifact in &self.artifacts {
            require_token("artifact.id", &artifact.id, 1, 128)?;
            if !ids.insert(artifact.id.to_ascii_lowercase()) {
                return Err(ManifestError::DuplicateArtifactId(artifact.id.clone()));
            }
            if artifact.source_urls.is_empty() || artifact.source_urls.len() > 8 {
                return Err(ManifestError::SourceUrlCount(artifact.id.clone()));
            }
            for url in &artifact.source_urls {
                validate_https_url(url)?;
            }
            if artifact.size_bytes == 0 {
                return Err(ManifestError::EmptyArtifact(artifact.id.clone()));
            }
            match (&artifact.kind, &artifact.archive_format) {
                (ArtifactKind::Archive, None) => {
                    return Err(ManifestError::MissingArchiveFormat(artifact.id.clone()));
                }
                (ArtifactKind::File, Some(_)) => {
                    return Err(ManifestError::UnexpectedArchiveFormat(artifact.id.clone()));
                }
                _ => {}
            }
            total = total
                .checked_add(artifact.size_bytes)
                .ok_or(ManifestError::PackTooLarge)?;
            crate::validate_relative_archive_path(&artifact.destination).map_err(|source| {
                ManifestError::UnsafeDestination {
                    artifact: artifact.id.clone(),
                    source,
                }
            })?;
            if let Some(strip_prefix) = &artifact.strip_prefix {
                if artifact.kind != ArtifactKind::Archive {
                    return Err(ManifestError::UnexpectedArchiveFormat(artifact.id.clone()));
                }
                crate::validate_relative_archive_path(strip_prefix).map_err(|source| {
                    ManifestError::UnsafeDestination {
                        artifact: artifact.id.clone(),
                        source,
                    }
                })?;
            }
            if !artifact.required_paths.is_empty() {
                if artifact.kind != ArtifactKind::Archive || artifact.required_paths.len() > 1_024 {
                    return Err(ManifestError::UnexpectedArchiveFormat(artifact.id.clone()));
                }
                let mut required = BTreeSet::new();
                for path in &artifact.required_paths {
                    let path = crate::validate_relative_archive_path(path).map_err(|source| {
                        ManifestError::UnsafeDestination {
                            artifact: artifact.id.clone(),
                            source,
                        }
                    })?;
                    if !required.insert(path.as_str().to_ascii_lowercase()) {
                        return Err(ManifestError::DuplicateDestination(
                            path.as_str().to_owned(),
                        ));
                    }
                }
            }
            if !destinations.insert(artifact.destination.to_ascii_lowercase().replace('\\', "/")) {
                return Err(ManifestError::DuplicateDestination(
                    artifact.destination.clone(),
                ));
            }
        }
        if total > MAX_PACK_BYTES || self.resources.storage_bytes > MAX_PACK_BYTES {
            return Err(ManifestError::PackTooLarge);
        }
        if self.resources.storage_bytes < total {
            return Err(ManifestError::StorageUnderreported {
                declared: self.resources.storage_bytes,
                artifacts: total,
            });
        }
        if self.resources.peak_install_bytes < self.resources.storage_bytes {
            return Err(ManifestError::PeakInstallUnderreported);
        }
        if self.hardware.minimum_ram_bytes > self.hardware.recommended_ram_bytes {
            return Err(ManifestError::InvertedHardwareRange("ram"));
        }
        if let (Some(minimum), Some(recommended)) = (
            self.hardware.minimum_vram_bytes,
            self.hardware.recommended_vram_bytes,
        ) {
            if minimum > recommended {
                return Err(ManifestError::InvertedHardwareRange("vram"));
            }
        }
        if self.hardware.minimum_cpu_threads == 0 || self.hardware.accelerators.is_empty() {
            return Err(ManifestError::IncompleteHardware);
        }
        if self.runtime.supported_platforms.is_empty()
            || self.runtime.supported_architectures.is_empty()
            || self.resources.languages.is_empty()
        {
            return Err(ManifestError::IncompleteCompatibility);
        }
        require_text("runtime.runtime", &self.runtime.runtime, 1, 128)?;
        require_text("runtime.abi", &self.runtime.abi, 1, 128)?;
        require_text("license.name", &self.license.license_name, 1, 256)?;
        validate_https_url(&self.license.license_url)?;
        require_text("license.attribution", &self.license.attribution, 1, 8_192)?;
        if self.license.redistributable && self.license.commercial_use == LicensePermission::Unknown
        {
            return Err(ManifestError::AmbiguousRedistributionLicense);
        }
        require_token("self_test.kind", &self.self_test.kind, 1, 64)?;
        require_token(
            "self_test.suite_revision",
            &self.self_test.suite_revision,
            1,
            128,
        )?;
        if self.self_test.allowed_runtime_backends.is_empty()
            || self.self_test.allowed_runtime_backends.len() > 16
        {
            return Err(ManifestError::InvalidSelfTestBackends);
        }
        for backend in &self.self_test.allowed_runtime_backends {
            require_token("self_test.runtime_backend", backend, 1, 128)?;
        }
        crate::validate_relative_archive_path(&self.self_test.input_fixture)
            .map_err(|source| ManifestError::UnsafeSelfTestFixture { source })?;
        if !(100..=300_000).contains(&self.self_test.timeout_millis) {
            return Err(ManifestError::InvalidSelfTestTimeout(
                self.self_test.timeout_millis,
            ));
        }
        Ok(())
    }

    /// Canonical digest used to bind an immutable catalog entry to this manifest.
    pub fn digest(&self) -> Result<Sha256Digest, ManifestError> {
        self.validate()?;
        let encoded = serde_json::to_vec(self).map_err(ManifestError::Serialization)?;
        Ok(Sha256Digest::of_bytes(&encoded))
    }

    pub fn identity(&self) -> PackRevision {
        PackRevision {
            pack_id: self.pack_id.clone(),
            revision: self.revision.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct PackRevision {
    pub pack_id: PackId,
    pub revision: Revision,
}

fn require_text(
    field: &'static str,
    value: &str,
    min: usize,
    max: usize,
) -> Result<(), ManifestError> {
    if !(min..=max).contains(&value.trim().len()) || value.contains('\0') {
        return Err(ManifestError::InvalidText(field));
    }
    Ok(())
}

fn require_token(
    field: &'static str,
    value: &str,
    min: usize,
    max: usize,
) -> Result<(), ManifestError> {
    if !(min..=max).contains(&value.len())
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return Err(ManifestError::InvalidText(field));
    }
    Ok(())
}

fn validate_https_url(url: &str) -> Result<(), ManifestError> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| ManifestError::InsecureUrl(url.to_owned()))?;
    if rest.is_empty()
        || rest.starts_with('/')
        || rest.contains('@')
        || rest.chars().any(char::is_whitespace)
    {
        return Err(ManifestError::InvalidUrl(url.to_owned()));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("invalid pack id: {0}")]
    InvalidPackId(String),
    #[error("invalid immutable revision: {0}")]
    InvalidRevision(String),
    #[error("invalid SHA-256 digest: {0}")]
    InvalidSha256(String),
    #[error("unsupported manifest schema: {0}")]
    UnsupportedSchema(String),
    #[error("source revision must be immutable, not {0}")]
    MutableSourceRevision(String),
    #[error("invalid text field: {0}")]
    InvalidText(&'static str),
    #[error("manifest has invalid artifact count {0}")]
    ArtifactCount(usize),
    #[error("duplicate artifact id: {0}")]
    DuplicateArtifactId(String),
    #[error("artifact {0} has an invalid source URL count")]
    SourceUrlCount(String),
    #[error("artifact {0} has zero bytes")]
    EmptyArtifact(String),
    #[error("archive artifact {0} is missing its archive format")]
    MissingArchiveFormat(String),
    #[error("file artifact {0} unexpectedly declares an archive format")]
    UnexpectedArchiveFormat(String),
    #[error("insecure source URL (HTTPS required): {0}")]
    InsecureUrl(String),
    #[error("invalid source URL: {0}")]
    InvalidUrl(String),
    #[error("unsafe destination for artifact {artifact}: {source}")]
    UnsafeDestination {
        artifact: String,
        source: crate::ArchivePathError,
    },
    #[error("unsafe self-test fixture: {source}")]
    UnsafeSelfTestFixture { source: crate::ArchivePathError },
    #[error("duplicate destination: {0}")]
    DuplicateDestination(String),
    #[error("model pack is too large")]
    PackTooLarge,
    #[error("storage is underreported: declared {declared}, artifact bytes {artifacts}")]
    StorageUnderreported { declared: u64, artifacts: u64 },
    #[error("peak install bytes must be at least storage bytes")]
    PeakInstallUnderreported,
    #[error("minimum {0} exceeds recommended value")]
    InvertedHardwareRange(&'static str),
    #[error("hardware requirements are incomplete")]
    IncompleteHardware,
    #[error("runtime compatibility or language metadata is incomplete")]
    IncompleteCompatibility,
    #[error("redistributable packs cannot have unknown commercial-use permission")]
    AmbiguousRedistributionLicense,
    #[error("self-test timeout outside 100ms..300s: {0}")]
    InvalidSelfTestTimeout(u64),
    #[error("self-test must declare 1 to 16 allowed runtime backends")]
    InvalidSelfTestBackends,
    #[error("manifest serialization failed: {0}")]
    Serialization(serde_json::Error),
}
