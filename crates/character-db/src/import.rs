use crate::{encounter::hex_sha256, CharacterDbError, CHARACTER_DB_SCHEMA_VERSION};
use npc_game_profile::{
    GameProfileV2, KnowledgeAuthority, KnowledgeRecord, ProvenanceKind, ProvenanceRecord,
    ReviewStatus, StyleExample,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

const LEGACY_IMPORT_SCHEMA_VERSION: &str = "legacy-character-import/1.0.0";
const TRANSFORM_VERSION: &str = "legacy-character-import-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacySource {
    pub source_id: String,
    pub revision: String,
    pub root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportLimits {
    pub max_files: usize,
    pub max_depth: usize,
    pub max_text_or_json_bytes: u64,
    pub max_image_bytes: u64,
    pub max_total_read_bytes: u64,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_files: 20_000,
            max_depth: 8,
            max_text_or_json_bytes: 4 * 1024 * 1024,
            max_image_bytes: 16 * 1024 * 1024,
            max_total_read_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportedArtifactKind {
    WorldText,
    PublicLore,
    CharacterBiography,
    CharacterKnowledge,
    StyleExamples,
    IdentityImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionKind {
    UnsafeDerived,
    Executable,
    NotAllowlisted,
    Symlink,
    Oversized,
    InvalidUtf8,
    InvalidJson,
    InvalidImage,
    MutableRuntimeState,
    ExcessiveTree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFingerprintV1 {
    pub source_id: String,
    pub revision: String,
    pub relative_path: String,
    pub raw_sha256: String,
    pub normalized_sha256: String,
    pub byte_length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedArtifactV1 {
    pub logical_path: String,
    pub kind: ImportedArtifactKind,
    pub candidates: Vec<SourceFingerprintV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_source_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicateSourceConflictV1 {
    pub logical_path: String,
    pub candidates: Vec<SourceFingerprintV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectedArtifactV1 {
    pub source_id: String,
    pub relative_path: String,
    pub kind: RejectionKind,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportSourceManifestV1 {
    pub source_id: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyImportManifestV1 {
    pub schema_version: String,
    pub target_profile_id: String,
    pub sources: Vec<ImportSourceManifestV1>,
    pub accepted: Vec<AcceptedArtifactV1>,
    pub conflicts: Vec<DuplicateSourceConflictV1>,
    pub rejected: Vec<RejectedArtifactV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConflictResolutionV1 {
    pub logical_path: String,
    pub selected_source_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityAssetV1 {
    pub schema_version: String,
    pub asset_id: String,
    pub character_id: String,
    pub source_path: String,
    pub source_sha256: String,
    pub byte_length: u64,
    pub local_only: bool,
    pub review_status: ReviewStatus,
}

#[derive(Debug, Clone)]
pub struct LegacyImportMaterialization {
    pub staged_profile: GameProfileV2,
    pub identity_assets: Vec<IdentityAssetV1>,
    pub manifest: LegacyImportManifestV1,
    /// Legacy imports remain inert until every pending provenance/rights record
    /// is reviewed and explicitly activated by a higher-level transaction.
    pub activatable: bool,
}

#[derive(Debug, Error)]
pub enum LegacyImportError {
    #[error("source `{source_id}` is invalid: {reason}")]
    InvalidSource { source_id: String, reason: String },
    #[error("I/O error at `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("legacy import has unresolved duplicate-source conflicts: {0:?}")]
    UnresolvedConflicts(Vec<String>),
    #[error("resolution for `{logical_path}` selects unknown source `{source_id}`")]
    InvalidResolution {
        logical_path: String,
        source_id: String,
    },
    #[error("legacy content references character `{0}` absent from the canonical profile")]
    UnknownCharacter(String),
    #[error(transparent)]
    CharacterDb(#[from] CharacterDbError),
}

#[derive(Debug, Clone)]
pub struct LegacyImportPlan {
    manifest: LegacyImportManifestV1,
    candidates: BTreeMap<String, Vec<ScannedCandidate>>,
}

impl LegacyImportPlan {
    pub fn manifest(&self) -> &LegacyImportManifestV1 {
        &self.manifest
    }

    pub fn materialize(
        &self,
        base_profile: &GameProfileV2,
        resolutions: &[ConflictResolutionV1],
    ) -> Result<LegacyImportMaterialization, LegacyImportError> {
        if base_profile.id != self.manifest.target_profile_id {
            return Err(CharacterDbError::InvalidInput(format!(
                "import targets `{}` but base profile is `{}`",
                self.manifest.target_profile_id, base_profile.id
            ))
            .into());
        }
        let resolution_map = resolutions
            .iter()
            .map(|resolution| {
                (
                    resolution.logical_path.as_str(),
                    resolution.selected_source_id.as_str(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let unresolved = self
            .manifest
            .conflicts
            .iter()
            .filter(|conflict| !resolution_map.contains_key(conflict.logical_path.as_str()))
            .map(|conflict| conflict.logical_path.clone())
            .collect::<Vec<_>>();
        if !unresolved.is_empty() {
            return Err(LegacyImportError::UnresolvedConflicts(unresolved));
        }

        let mut selected = Vec::new();
        let mut manifest = self.manifest.clone();
        for accepted in &mut manifest.accepted {
            let candidates = &self.candidates[&accepted.logical_path];
            let distinct = candidates
                .iter()
                .map(|candidate| candidate.fingerprint.normalized_sha256.as_str())
                .collect::<BTreeSet<_>>();
            let candidate = if distinct.len() == 1 {
                candidates
                    .iter()
                    .min_by(|left, right| {
                        left.fingerprint.source_id.cmp(&right.fingerprint.source_id)
                    })
                    .expect("accepted artifact has a candidate")
            } else {
                let source_id = resolution_map[accepted.logical_path.as_str()];
                candidates
                    .iter()
                    .find(|candidate| candidate.fingerprint.source_id == source_id)
                    .ok_or_else(|| LegacyImportError::InvalidResolution {
                        logical_path: accepted.logical_path.clone(),
                        source_id: source_id.to_owned(),
                    })?
            };
            accepted.selected_source_id = Some(candidate.fingerprint.source_id.clone());
            selected.push(candidate);
        }

        let mut staged_profile = base_profile.clone();
        let spoiler_tier = staged_profile
            .content
            .spoiler_tiers
            .iter()
            .find(|tier| tier.default_enabled)
            .or_else(|| staged_profile.content.spoiler_tiers.first())
            .map(|tier| tier.id.clone())
            .ok_or_else(|| {
                CharacterDbError::InvalidInput("profile has no spoiler tier".to_owned())
            })?;
        let mut identity_assets = Vec::new();
        for candidate in selected {
            let provenance_id = provenance_id(&candidate.logical_path, &candidate.fingerprint);
            if !staged_profile
                .content
                .provenance
                .iter()
                .any(|item| item.id == provenance_id)
            {
                staged_profile.content.provenance.push(ProvenanceRecord {
                    id: provenance_id.clone(),
                    title: format!("Legacy import {}", candidate.logical_path),
                    kind: ProvenanceKind::LegacyImport,
                    source_url: None,
                    license: None,
                    notes: Some("Quarantined pending provenance and rights review.".to_owned()),
                    source_revision: Some(candidate.fingerprint.revision.clone()),
                    source_path: Some(candidate.logical_path.clone()),
                    source_sha256: Some(candidate.fingerprint.raw_sha256.clone()),
                    review_status: Some(ReviewStatus::Pending),
                    transform_version: Some(TRANSFORM_VERSION.to_owned()),
                });
            }
            match (&candidate.kind, &candidate.payload) {
                (ImportedArtifactKind::WorldText, LegacyPayload::Text(text)) => {
                    add_knowledge_chunks(
                        &mut staged_profile,
                        "legacy-core",
                        KnowledgeAuthority::CoreCanon,
                        None,
                        text,
                        &spoiler_tier,
                        &provenance_id,
                    );
                }
                (ImportedArtifactKind::PublicLore, LegacyPayload::Text(text)) => {
                    add_knowledge_chunks(
                        &mut staged_profile,
                        "legacy-public",
                        KnowledgeAuthority::GamePublic,
                        None,
                        text,
                        &spoiler_tier,
                        &provenance_id,
                    );
                }
                (
                    ImportedArtifactKind::CharacterBiography
                    | ImportedArtifactKind::CharacterKnowledge,
                    LegacyPayload::Text(text),
                ) => {
                    let character_id =
                        character_id_for_path(&candidate.logical_path, &staged_profile)?;
                    add_knowledge_chunks(
                        &mut staged_profile,
                        "legacy-character",
                        KnowledgeAuthority::CharacterAuthored,
                        Some(character_id.clone()),
                        text,
                        &spoiler_tier,
                        &provenance_id,
                    );
                }
                (ImportedArtifactKind::StyleExamples, LegacyPayload::Style(lines)) => {
                    let character_id =
                        character_id_for_path(&candidate.logical_path, &staged_profile)?;
                    let character = staged_profile
                        .characters
                        .iter_mut()
                        .find(|character| character.id == character_id)
                        .ok_or_else(|| LegacyImportError::UnknownCharacter(character_id.clone()))?;
                    for (index, line) in lines.iter().enumerate() {
                        let (speaker, text) = split_speaker(line, &character.display_name);
                        let example_identity =
                            format!("{}\0{}\0{}\0{}", character.id, provenance_id, index, line);
                        character.style_examples.push(StyleExample {
                            id: format!(
                                "legacy-style-{}",
                                &hex_sha256(example_identity.as_bytes())[..20]
                            ),
                            speaker,
                            text,
                            situation_tags: vec!["legacy-unclassified".to_owned()],
                            tone_tags: vec![],
                            weight_millis: 1_000,
                            provenance_id: provenance_id.clone(),
                        });
                    }
                    character
                        .style_examples
                        .sort_by(|left, right| left.id.cmp(&right.id));
                    character
                        .style_examples
                        .dedup_by(|left, right| left.id == right.id);
                }
                (ImportedArtifactKind::IdentityImage, LegacyPayload::Image) => {
                    let character_id =
                        character_id_for_path(&candidate.logical_path, &staged_profile)?;
                    identity_assets.push(IdentityAssetV1 {
                        schema_version: CHARACTER_DB_SCHEMA_VERSION.to_owned(),
                        asset_id: format!(
                            "legacy-image-{}",
                            &candidate.fingerprint.raw_sha256[..20]
                        ),
                        character_id,
                        source_path: candidate.logical_path.clone(),
                        source_sha256: candidate.fingerprint.raw_sha256.clone(),
                        byte_length: candidate.fingerprint.byte_length,
                        local_only: true,
                        review_status: ReviewStatus::Pending,
                    });
                }
                _ => {
                    return Err(CharacterDbError::InvalidInput(format!(
                        "legacy payload kind mismatch for {}",
                        candidate.logical_path
                    ))
                    .into())
                }
            }
        }

        staged_profile
            .content
            .knowledge
            .sort_by(|left, right| left.id.cmp(&right.id));
        staged_profile
            .content
            .knowledge
            .dedup_by(|left, right| left.id == right.id);
        staged_profile
            .content
            .provenance
            .sort_by(|left, right| left.id.cmp(&right.id));
        identity_assets.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
        identity_assets.dedup_by(|left, right| left.asset_id == right.asset_id);
        let report = staged_profile.validate();
        if !report.is_valid() {
            return Err(CharacterDbError::InvalidProfile(report).into());
        }
        Ok(LegacyImportMaterialization {
            staged_profile,
            identity_assets,
            manifest,
            activatable: false,
        })
    }
}

#[derive(Debug, Clone)]
struct ScannedCandidate {
    logical_path: String,
    kind: ImportedArtifactKind,
    fingerprint: SourceFingerprintV1,
    payload: LegacyPayload,
}

#[derive(Debug, Clone)]
enum LegacyPayload {
    Text(String),
    Style(Vec<String>),
    Image,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyStyleFile {
    pre_conversation: Vec<LegacyStyleLine>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyStyleLine {
    line: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyConversationFile {
    conversation: Vec<LegacyConversationLine>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyConversationLine {
    sender: String,
    message: String,
}

pub fn scan_legacy_sources(
    target_profile_id: &str,
    sources: &[LegacySource],
    limits: ImportLimits,
) -> Result<LegacyImportPlan, LegacyImportError> {
    if target_profile_id.trim().is_empty() || sources.is_empty() {
        return Err(LegacyImportError::InvalidSource {
            source_id: "<request>".to_owned(),
            reason: "target profile and at least one source are required".to_owned(),
        });
    }
    let mut source_ids = BTreeSet::new();
    let mut candidates: BTreeMap<String, Vec<ScannedCandidate>> = BTreeMap::new();
    let mut rejected = Vec::new();
    let mut total_read = 0_u64;
    let mut file_count = 0_usize;
    for source in sources {
        if source.source_id.trim().is_empty()
            || source.revision.trim().is_empty()
            || !source_ids.insert(source.source_id.clone())
        {
            return Err(LegacyImportError::InvalidSource {
                source_id: source.source_id.clone(),
                reason: "source IDs/revisions must be non-blank and source IDs unique".to_owned(),
            });
        }
        let metadata =
            fs::symlink_metadata(&source.root).map_err(|error| LegacyImportError::Io {
                path: source.root.clone(),
                source: error,
            })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(LegacyImportError::InvalidSource {
                source_id: source.source_id.clone(),
                reason: "source root must be a real directory, not a symlink".to_owned(),
            });
        }
        scan_directory(
            source,
            &source.root,
            0,
            limits,
            &mut total_read,
            &mut file_count,
            &mut candidates,
            &mut rejected,
        )?;
    }

    let mut accepted = Vec::new();
    let mut conflicts = Vec::new();
    for (logical_path, records) in &candidates {
        let mut fingerprints = records
            .iter()
            .map(|record| record.fingerprint.clone())
            .collect::<Vec<_>>();
        fingerprints.sort_by(|left, right| left.source_id.cmp(&right.source_id));
        let hashes = fingerprints
            .iter()
            .map(|candidate| candidate.normalized_sha256.as_str())
            .collect::<BTreeSet<_>>();
        accepted.push(AcceptedArtifactV1 {
            logical_path: logical_path.clone(),
            kind: records[0].kind,
            candidates: fingerprints.clone(),
            selected_source_id: None,
        });
        if hashes.len() > 1 {
            conflicts.push(DuplicateSourceConflictV1 {
                logical_path: logical_path.clone(),
                candidates: fingerprints,
            });
        }
    }
    rejected.sort_by(|left, right| {
        left.source_id
            .cmp(&right.source_id)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    let mut source_manifest = sources
        .iter()
        .map(|source| ImportSourceManifestV1 {
            source_id: source.source_id.clone(),
            revision: source.revision.clone(),
        })
        .collect::<Vec<_>>();
    source_manifest.sort_by(|left, right| left.source_id.cmp(&right.source_id));
    Ok(LegacyImportPlan {
        manifest: LegacyImportManifestV1 {
            schema_version: LEGACY_IMPORT_SCHEMA_VERSION.to_owned(),
            target_profile_id: target_profile_id.to_owned(),
            sources: source_manifest,
            accepted,
            conflicts,
            rejected,
        },
        candidates,
    })
}

#[allow(clippy::too_many_arguments)]
fn scan_directory(
    source: &LegacySource,
    directory: &Path,
    depth: usize,
    limits: ImportLimits,
    total_read: &mut u64,
    file_count: &mut usize,
    candidates: &mut BTreeMap<String, Vec<ScannedCandidate>>,
    rejected: &mut Vec<RejectedArtifactV1>,
) -> Result<(), LegacyImportError> {
    if depth > limits.max_depth {
        rejected.push(RejectedArtifactV1 {
            source_id: source.source_id.clone(),
            relative_path: safe_relative(&source.root, directory),
            kind: RejectionKind::ExcessiveTree,
            reason: "directory exceeds import depth limit".to_owned(),
        });
        return Ok(());
    }
    let entries = fs::read_dir(directory).map_err(|error| LegacyImportError::Io {
        path: directory.to_owned(),
        source: error,
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| LegacyImportError::Io {
            path: directory.to_owned(),
            source: error,
        })?;
        let path = entry.path();
        let relative = safe_relative(&source.root, &path);
        let metadata = fs::symlink_metadata(&path).map_err(|error| LegacyImportError::Io {
            path: path.clone(),
            source: error,
        })?;
        if metadata.file_type().is_symlink() {
            rejected.push(RejectedArtifactV1 {
                source_id: source.source_id.clone(),
                relative_path: relative,
                kind: RejectionKind::Symlink,
                reason: "symlinks are never followed".to_owned(),
            });
            continue;
        }
        if metadata.is_dir() {
            scan_directory(
                source,
                &path,
                depth + 1,
                limits,
                total_read,
                file_count,
                candidates,
                rejected,
            )?;
            continue;
        }
        if !metadata.is_file() {
            rejected.push(RejectedArtifactV1 {
                source_id: source.source_id.clone(),
                relative_path: relative,
                kind: RejectionKind::NotAllowlisted,
                reason: "only regular files are eligible".to_owned(),
            });
            continue;
        }
        *file_count += 1;
        if *file_count > limits.max_files {
            return Err(LegacyImportError::InvalidSource {
                source_id: source.source_id.clone(),
                reason: format!("tree exceeds {} files", limits.max_files),
            });
        }
        match classify_path(Path::new(&relative)) {
            Classification::Reject(kind, reason) => rejected.push(RejectedArtifactV1 {
                source_id: source.source_id.clone(),
                relative_path: relative,
                kind,
                reason: reason.to_owned(),
            }),
            Classification::InspectConversation => {
                let bytes = read_bounded(
                    &path,
                    metadata.len(),
                    limits.max_text_or_json_bytes,
                    total_read,
                    limits.max_total_read_bytes,
                )?;
                let parsed = serde_json::from_slice::<LegacyConversationFile>(&bytes);
                let (kind, reason) = match parsed {
                    Ok(value)
                        if value.conversation.len() <= 100_000
                            && value.conversation.iter().all(|line| {
                                !line.sender.trim().is_empty()
                                    && !line.message.trim().is_empty()
                                    && line.sender.len() <= 1_024
                                    && line.message.len() <= 32_768
                            }) =>
                    {
                        (
                            RejectionKind::MutableRuntimeState,
                            "conversation JSON is valid but excluded from automatic import",
                        )
                    }
                    Ok(_) => (
                        RejectionKind::InvalidJson,
                        "conversation JSON exceeds validated limits",
                    ),
                    Err(_) => (RejectionKind::InvalidJson, "conversation JSON is invalid"),
                };
                rejected.push(RejectedArtifactV1 {
                    source_id: source.source_id.clone(),
                    relative_path: relative,
                    kind,
                    reason: reason.to_owned(),
                });
            }
            Classification::Accept(kind) => {
                let max_bytes = if kind == ImportedArtifactKind::IdentityImage {
                    limits.max_image_bytes
                } else {
                    limits.max_text_or_json_bytes
                };
                if metadata.len() > max_bytes {
                    rejected.push(RejectedArtifactV1 {
                        source_id: source.source_id.clone(),
                        relative_path: relative,
                        kind: RejectionKind::Oversized,
                        reason: format!("file exceeds {max_bytes} byte limit"),
                    });
                    continue;
                }
                let bytes = read_bounded(
                    &path,
                    metadata.len(),
                    max_bytes,
                    total_read,
                    limits.max_total_read_bytes,
                )?;
                match normalize_payload(kind, &relative, &bytes) {
                    Ok((payload, normalized)) => {
                        let fingerprint = SourceFingerprintV1 {
                            source_id: source.source_id.clone(),
                            revision: source.revision.clone(),
                            relative_path: relative.clone(),
                            raw_sha256: hex_sha256(&bytes),
                            normalized_sha256: hex_sha256(&normalized),
                            byte_length: bytes.len() as u64,
                        };
                        candidates
                            .entry(relative.clone())
                            .or_default()
                            .push(ScannedCandidate {
                                logical_path: relative,
                                kind,
                                fingerprint,
                                payload,
                            });
                    }
                    Err((rejection_kind, reason)) => rejected.push(RejectedArtifactV1 {
                        source_id: source.source_id.clone(),
                        relative_path: relative,
                        kind: rejection_kind,
                        reason,
                    }),
                }
            }
        }
    }
    Ok(())
}

fn read_bounded(
    path: &Path,
    length: u64,
    file_limit: u64,
    total_read: &mut u64,
    total_limit: u64,
) -> Result<Vec<u8>, LegacyImportError> {
    if length > file_limit || total_read.saturating_add(length) > total_limit {
        return Err(LegacyImportError::InvalidSource {
            source_id: path.display().to_string(),
            reason: "accepted input exceeds configured byte limits".to_owned(),
        });
    }
    let file = fs::File::open(path).map_err(|error| LegacyImportError::Io {
        path: path.to_owned(),
        source: error,
    })?;
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(file_limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| LegacyImportError::Io {
            path: path.to_owned(),
            source: error,
        })?;
    if bytes.len() as u64 != length || bytes.len() as u64 > file_limit {
        return Err(LegacyImportError::InvalidSource {
            source_id: path.display().to_string(),
            reason: "file changed during bounded import read".to_owned(),
        });
    }
    *total_read += bytes.len() as u64;
    Ok(bytes)
}

enum Classification {
    Accept(ImportedArtifactKind),
    InspectConversation,
    Reject(RejectionKind, &'static str),
}

fn classify_path(path: &Path) -> Classification {
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    let lower_extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(
        lower_extension.as_str(),
        "pkl" | "pickle" | "parquet" | "bin"
    ) {
        return Classification::Reject(
            RejectionKind::UnsafeDerived,
            "derived pickle/parquet/bin state is rejected without opening",
        );
    }
    if matches!(
        lower_extension.as_str(),
        "py" | "pyc" | "exe" | "dll" | "bat" | "cmd" | "ps1" | "sh" | "com" | "msi"
    ) {
        return Classification::Reject(
            RejectionKind::Executable,
            "executable or script input is never imported",
        );
    }
    match components.as_slice() {
        ["world.txt"] => Classification::Accept(ImportedArtifactKind::WorldText),
        ["public_info.txt"] => Classification::Accept(ImportedArtifactKind::PublicLore),
        ["characters", character, "bio.txt"] if *character != "default" => {
            Classification::Accept(ImportedArtifactKind::CharacterBiography)
        }
        ["characters", character, "character_knowledge.txt"] if *character != "default" => {
            Classification::Accept(ImportedArtifactKind::CharacterKnowledge)
        }
        ["characters", _, "pre_conversation.json"] => {
            Classification::Accept(ImportedArtifactKind::StyleExamples)
        }
        ["characters", _, "conversation.json"] => Classification::InspectConversation,
        ["characters", character, "images", file]
            if *character != "default"
                && matches!(
                    Path::new(file)
                        .extension()
                        .and_then(|value| value.to_str())
                        .map(str::to_ascii_lowercase)
                        .as_deref(),
                    Some("jpg" | "jpeg" | "png")
                ) =>
        {
            Classification::Accept(ImportedArtifactKind::IdentityImage)
        }
        ["characters", "default", "name.txt" | "bio.txt" | "voice.txt" | "timestamp.txt" | "face.jpg"] => {
            Classification::Reject(
                RejectionKind::MutableRuntimeState,
                "mutable default encounter state is excluded from automatic import",
            )
        }
        _ => Classification::Reject(
            RejectionKind::NotAllowlisted,
            "path is outside the legacy text/JSON/image allowlist",
        ),
    }
}

fn normalize_payload(
    kind: ImportedArtifactKind,
    relative_path: &str,
    bytes: &[u8],
) -> Result<(LegacyPayload, Vec<u8>), (RejectionKind, String)> {
    match kind {
        ImportedArtifactKind::IdentityImage => {
            let lower = relative_path.to_ascii_lowercase();
            let valid = if lower.ends_with(".png") {
                bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
            } else {
                bytes.starts_with(&[0xff, 0xd8, 0xff]) && bytes.ends_with(&[0xff, 0xd9])
            };
            if !valid {
                return Err((
                    RejectionKind::InvalidImage,
                    "image extension does not match a bounded PNG/JPEG payload".to_owned(),
                ));
            }
            Ok((LegacyPayload::Image, bytes.to_vec()))
        }
        ImportedArtifactKind::StyleExamples => {
            let style: LegacyStyleFile = serde_json::from_slice(bytes).map_err(|error| {
                (
                    RejectionKind::InvalidJson,
                    format!("invalid style JSON: {error}"),
                )
            })?;
            if style.pre_conversation.len() > 4_096
                || style
                    .pre_conversation
                    .iter()
                    .any(|line| line.line.trim().is_empty() || line.line.len() > 32_768)
            {
                return Err((
                    RejectionKind::InvalidJson,
                    "style JSON exceeds line count or line length limits".to_owned(),
                ));
            }
            let lines = style
                .pre_conversation
                .into_iter()
                .map(|line| normalize_text(&line.line))
                .collect::<Vec<_>>();
            let normalized = serde_json::to_vec(&lines).map_err(|error| {
                (
                    RejectionKind::InvalidJson,
                    format!("cannot normalize style JSON: {error}"),
                )
            })?;
            Ok((LegacyPayload::Style(lines), normalized))
        }
        _ => {
            let text = std::str::from_utf8(bytes).map_err(|_| {
                (
                    RejectionKind::InvalidUtf8,
                    "text must be strict UTF-8".to_owned(),
                )
            })?;
            let text = normalize_text(text);
            if text.trim().is_empty() {
                return Err((
                    RejectionKind::InvalidUtf8,
                    "text cannot be blank".to_owned(),
                ));
            }
            Ok((LegacyPayload::Text(text.clone()), text.into_bytes()))
        }
    }
}

fn normalize_text(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn safe_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn provenance_id(logical_path: &str, source: &SourceFingerprintV1) -> String {
    let identity = format!(
        "{}\0{}\0{}\0{}",
        logical_path, source.source_id, source.revision, source.raw_sha256
    );
    format!("legacy-source-{}", &hex_sha256(identity.as_bytes())[..20])
}

fn character_id_for_path(
    logical_path: &str,
    profile: &GameProfileV2,
) -> Result<String, LegacyImportError> {
    let folder = logical_path
        .split('/')
        .nth(1)
        .ok_or_else(|| LegacyImportError::UnknownCharacter(logical_path.to_owned()))?;
    if folder == "default" {
        return profile
            .characters
            .iter()
            .filter(|character| character.background_npc)
            .map(|character| character.id.clone())
            .min()
            .ok_or_else(|| LegacyImportError::UnknownCharacter("default".to_owned()));
    }
    let candidate = slugify(folder);
    profile
        .characters
        .iter()
        .find(|character| character.id == candidate)
        .map(|character| character.id.clone())
        .ok_or(LegacyImportError::UnknownCharacter(candidate))
}

fn slugify(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn split_speaker(line: &str, fallback: &str) -> (String, String) {
    line.split_once(": ")
        .filter(|(speaker, text)| !speaker.trim().is_empty() && !text.trim().is_empty())
        .map(|(speaker, text)| (speaker.trim().to_owned(), text.trim().to_owned()))
        .unwrap_or_else(|| (fallback.to_owned(), line.trim().to_owned()))
}

fn add_knowledge_chunks(
    profile: &mut GameProfileV2,
    id_prefix: &str,
    authority: KnowledgeAuthority,
    owner_character_id: Option<String>,
    text: &str,
    spoiler_tier: &str,
    provenance_id: &str,
) {
    for (index, chunk) in split_chunks(text, 16_000).into_iter().enumerate() {
        let identity =
            format!("{authority:?}\0{owner_character_id:?}\0{provenance_id}\0{index}\0{chunk}");
        let id = format!("{id_prefix}-{}", &hex_sha256(identity.as_bytes())[..20]);
        profile.content.knowledge.push(KnowledgeRecord {
            id: id.clone(),
            authority,
            owner_character_id: owner_character_id.clone(),
            text: chunk,
            topic_tags: vec!["legacy-unclassified".to_owned()],
            spoiler_tier: spoiler_tier.to_owned(),
            provenance_id: provenance_id.to_owned(),
        });
        if let Some(owner) = owner_character_id.as_deref() {
            if let Some(character) = profile.characters.iter_mut().find(|item| item.id == owner) {
                character.prompt.knowledge_refs.push(id);
                character.prompt.knowledge_refs.sort();
                character.prompt.knowledge_refs.dedup();
            }
        }
    }
}

fn split_chunks(text: &str, max_bytes: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for paragraph in text.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        if paragraph.len() > max_bytes {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            let mut fragment = String::new();
            for character in paragraph.chars() {
                if fragment.len() + character.len_utf8() > max_bytes {
                    chunks.push(std::mem::take(&mut fragment));
                }
                fragment.push(character);
            }
            if !fragment.is_empty() {
                chunks.push(fragment);
            }
            continue;
        }
        let separator = usize::from(!current.is_empty()) * 2;
        if current.len() + separator + paragraph.len() > max_bytes {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(paragraph);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}
