use crate::secure_path::{secure_create_dir_all, with_protected_root};
use crate::{
    atomic_write_json, extract_artifact, read_json, verify_artifact, ActivationAuthorization,
    ArchivePolicy, ArtifactEvidence, FileDownloadJournalStore, ModelPackManifestV1, PackId,
    PackRevision, PackStorage, Revision, Sha256Digest, StagedContentBindingV1, StagedPack,
    StorageError, StorageInspection,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use walkdir::WalkDir;

const ACTIVE_POINTER_SCHEMA_V1: &str = "npc.active-pack/v1";
const INSTALL_INDEX_SCHEMA_V1: &str = "npc.install-index/v1";
const REFERENCES_SCHEMA_V1: &str = "npc.pack-references/v1";
const ATTESTATION_REPLAY_SCHEMA_V1: &str = "npc.attestation-replay/v1";
const ACTIVATION_RECEIPT_SCHEMA_V1: &str = "npc.activation-receipt/v1";
static TRANSACTION_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct FilesystemPackStorage {
    root: PathBuf,
    archive_policy: ArchivePolicy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActivePackPointerV1 {
    pub schema: String,
    pub pack_id: PackId,
    pub active_revision: Revision,
    pub previous_revisions: Vec<Revision>,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct InstallIndexV1 {
    schema: String,
    identity: PackRevision,
    manifest_sha256: Sha256Digest,
    artifacts: BTreeMap<String, ArtifactEvidence>,
    files: Vec<InstalledFileV1>,
    content_tree_sha256: Sha256Digest,
    total_file_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InstalledFileV1 {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: Sha256Digest,
}

/// Native-only authority over an immutable, fully re-hashed installed version.
/// The path is never deserialized from a caller and the inventory is accepted
/// only after its on-disk tree matches the atomically installed index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledPackInventoryV1 {
    pub identity: PackRevision,
    pub root: PathBuf,
    pub manifest_sha256: Sha256Digest,
    pub content_tree_sha256: Sha256Digest,
    pub total_file_bytes: u64,
    pub artifact_evidence: BTreeMap<String, ArtifactEvidence>,
    pub files: Vec<InstalledFileV1>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct ReferenceFileV1 {
    schema: String,
    references: BTreeMap<PackId, BTreeSet<String>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct AttestationReplayStateV1 {
    schema: String,
    consumed: BTreeSet<Sha256Digest>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct ActivationReceiptV1 {
    schema: String,
    attestation_sha256: Sha256Digest,
    replay_key: Sha256Digest,
    staged: StagedContentBindingV1,
}

impl FilesystemPackStorage {
    pub fn new(
        root: impl Into<PathBuf>,
        archive_policy: ArchivePolicy,
    ) -> Result<Self, StorageError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(storage_error)?;
        reject_link(&root)?;
        let root = fs::canonicalize(root).map_err(storage_error)?;
        reject_link(&root)?;
        for child in ["downloads", ".staging", "packs", "state"] {
            let path = root.join(child);
            fs::create_dir_all(&path).map_err(storage_error)?;
            reject_link(&path)?;
        }
        Ok(Self {
            root,
            archive_policy,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn journal_store(&self) -> Result<FileDownloadJournalStore, StorageError> {
        FileDownloadJournalStore::new(self.root.join("state").join("download-journals"))
            .map_err(|error| StorageError::new(error.to_string()))
    }

    pub fn download_path(
        &self,
        identity: &PackRevision,
        artifact_id: &str,
    ) -> Result<PathBuf, StorageError> {
        validate_token(artifact_id)?;
        let path = self
            .root
            .join("downloads")
            .join(identity.pack_id.as_str())
            .join(identity.revision.as_str())
            .join(format!("{artifact_id}.verified"));
        let parent = path
            .parent()
            .ok_or_else(|| StorageError::new("download path has no parent"))?;
        secure_create_dir_all(&self.root.join("downloads"), parent).map_err(storage_error)?;
        Ok(path)
    }

    pub fn active_pointer(
        &self,
        pack_id: &PackId,
    ) -> Result<Option<ActivePackPointerV1>, StorageError> {
        let lock = self.pack_root(pack_id).join("active.lock");
        self.with_lock(&lock, || self.read_active_pointer_unlocked(pack_id))
    }

    /// Re-hashes the complete immutable version tree and returns its exact
    /// native paths only when its persisted install index still matches.
    pub fn installed_inventory(
        &self,
        identity: &PackRevision,
    ) -> Result<Option<InstalledPackInventoryV1>, StorageError> {
        let root = self.version_root(identity);
        if !root.is_dir() {
            return Ok(None);
        }
        reject_link(&root)?;
        let index: InstallIndexV1 = read_json(&root.join(".npc").join("index.json"))
            .map_err(|error| StorageError::new(error.to_string()))?;
        if index.schema != INSTALL_INDEX_SCHEMA_V1 || &index.identity != identity {
            return Err(StorageError::new("installed version metadata mismatch"));
        }
        if !verify_index(&root, &index)? {
            return Err(StorageError::new(
                "installed version content does not match its immutable index",
            ));
        }
        Ok(Some(InstalledPackInventoryV1 {
            identity: index.identity,
            root,
            manifest_sha256: index.manifest_sha256,
            content_tree_sha256: index.content_tree_sha256,
            total_file_bytes: index.total_file_bytes,
            artifact_evidence: index.artifacts,
            files: index.files,
        }))
    }

    /// Resolves only the version selected by the protected active pointer.
    pub fn active_installed_inventory(
        &self,
        pack_id: &PackId,
    ) -> Result<Option<InstalledPackInventoryV1>, StorageError> {
        let Some(pointer) = self.active_pointer(pack_id)? else {
            return Ok(None);
        };
        self.installed_inventory(&PackRevision {
            pack_id: pointer.pack_id,
            revision: pointer.active_revision,
        })
    }

    fn read_active_pointer_unlocked(
        &self,
        pack_id: &PackId,
    ) -> Result<Option<ActivePackPointerV1>, StorageError> {
        let path = self.pack_root(pack_id).join("active.json");
        match read_json::<ActivePackPointerV1>(&path) {
            Ok(pointer) => {
                if pointer.schema != ACTIVE_POINTER_SCHEMA_V1 || &pointer.pack_id != pack_id {
                    return Err(StorageError::new(
                        "active pointer identity or schema mismatch",
                    ));
                }
                Ok(Some(pointer))
            }
            Err(crate::JournalError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(error) => Err(StorageError::new(error.to_string())),
        }
    }

    /// Performs a persisted rollback without requiring an in-memory manager snapshot.
    pub fn rollback_persisted(&mut self, pack_id: &PackId) -> Result<PackRevision, StorageError> {
        if self.reference_count(pack_id)? > 0 {
            return Err(StorageError::new("pack has active consumers"));
        }
        let lock = self.pack_root(pack_id).join("active.lock");
        self.with_lock(&lock, || {
            let mut pointer = self
                .read_active_pointer_unlocked(pack_id)?
                .ok_or_else(|| StorageError::new("pack has no active revision"))?;
            let target = pointer
                .previous_revisions
                .pop()
                .ok_or_else(|| StorageError::new("pack has no persisted rollback target"))?;
            let identity = PackRevision {
                pack_id: pack_id.clone(),
                revision: target.clone(),
            };
            if !self.version_root(&identity).is_dir() {
                return Err(StorageError::new("persisted rollback target is missing"));
            }
            let current = pointer.active_revision;
            pointer.active_revision = target;
            pointer.previous_revisions.push(current);
            pointer.generation = pointer.generation.saturating_add(1);
            self.write_active_pointer(&pointer)?;
            Ok(identity)
        })
    }

    fn pack_root(&self, pack_id: &PackId) -> PathBuf {
        self.root.join("packs").join(pack_id.as_str())
    }

    fn version_root(&self, identity: &PackRevision) -> PathBuf {
        self.pack_root(&identity.pack_id)
            .join("versions")
            .join(identity.revision.as_str())
    }

    fn staging_root(&self, transaction_id: &str) -> PathBuf {
        self.root.join(".staging").join(transaction_id)
    }

    fn staged_content_root(&self, staged: &StagedPack) -> Result<PathBuf, StorageError> {
        let staging = self.staging_root(&staged.transaction_id);
        if staging.is_dir() {
            return Ok(staging);
        }
        let installed = self.version_root(&staged.identity);
        if installed.is_dir() {
            return Ok(installed);
        }
        Err(StorageError::new(
            "staged or atomically committed inactive content is missing",
        ))
    }

    fn write_active_pointer(&self, pointer: &ActivePackPointerV1) -> Result<(), StorageError> {
        let root = self.pack_root(&pointer.pack_id);
        secure_create_dir_all(&self.root.join("packs"), &root).map_err(storage_error)?;
        reject_link(&root)?;
        atomic_write_json(&root.join("active.json"), pointer)
            .map_err(|error| StorageError::new(error.to_string()))
    }

    fn references_path(&self) -> PathBuf {
        self.root.join("state").join("references.json")
    }

    fn read_references(&self) -> Result<ReferenceFileV1, StorageError> {
        match read_json::<ReferenceFileV1>(&self.references_path()) {
            Ok(file) if file.schema == REFERENCES_SCHEMA_V1 => Ok(file),
            Ok(_) => Err(StorageError::new("unsupported references schema")),
            Err(crate::JournalError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                Ok(ReferenceFileV1 {
                    schema: REFERENCES_SCHEMA_V1.to_owned(),
                    references: BTreeMap::new(),
                })
            }
            Err(error) => Err(StorageError::new(error.to_string())),
        }
    }

    fn write_references(&self, references: &ReferenceFileV1) -> Result<(), StorageError> {
        atomic_write_json(&self.references_path(), references)
            .map_err(|error| StorageError::new(error.to_string()))
    }

    fn replay_state_path(&self) -> PathBuf {
        self.root.join("state").join("attestation-replay.json")
    }

    fn read_replay_state(&self) -> Result<AttestationReplayStateV1, StorageError> {
        match read_json::<AttestationReplayStateV1>(&self.replay_state_path()) {
            Ok(state) if state.schema == ATTESTATION_REPLAY_SCHEMA_V1 => Ok(state),
            Ok(_) => Err(StorageError::new("unsupported attestation replay schema")),
            Err(crate::JournalError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                Ok(AttestationReplayStateV1 {
                    schema: ATTESTATION_REPLAY_SCHEMA_V1.to_owned(),
                    consumed: BTreeSet::new(),
                })
            }
            Err(error) => Err(StorageError::new(error.to_string())),
        }
    }

    fn with_lock<T>(
        &self,
        lock_path: &Path,
        operation: impl FnOnce() -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        let parent = lock_path
            .parent()
            .ok_or_else(|| StorageError::new("lock path has no parent"))?;
        fs::create_dir_all(parent).map_err(storage_error)?;
        reject_link(parent)?;
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(storage_error)?;
        lock_file.lock_exclusive().map_err(storage_error)?;
        let result = operation();
        FileExt::unlock(&lock_file).map_err(storage_error)?;
        result
    }
}

impl PackStorage for FilesystemPackStorage {
    fn stage(
        &mut self,
        manifest: &ModelPackManifestV1,
        evidence: &[ArtifactEvidence],
    ) -> Result<StagedPack, StorageError> {
        manifest
            .validate()
            .map_err(|error| StorageError::new(error.to_string()))?;
        let identity = manifest.identity();
        let supplied: BTreeMap<_, _> = evidence
            .iter()
            .cloned()
            .map(|item| (item.artifact_id.clone(), item))
            .collect();
        if supplied.len() != manifest.artifacts.len() {
            return Err(StorageError::new(
                "artifact evidence is incomplete or duplicated",
            ));
        }
        let transaction_id = format!(
            "{}-{}-{}-{}",
            identity.pack_id,
            identity.revision,
            std::process::id(),
            TRANSACTION_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        validate_token(&transaction_id)?;
        let staging = self.staging_root(&transaction_id);
        secure_create_dir_all(&self.root.join(".staging"), &staging).map_err(storage_error)?;
        reject_link(&staging)?;

        let result = (|| {
            for artifact in &manifest.artifacts {
                let declared = supplied
                    .get(&artifact.id)
                    .ok_or_else(|| StorageError::new("artifact evidence is missing"))?;
                if declared.size_bytes != artifact.size_bytes || declared.sha256 != artifact.sha256
                {
                    return Err(StorageError::new("artifact evidence differs from manifest"));
                }
                let source = self.download_path(&identity, &artifact.id)?;
                let source_file = File::open(&source).map_err(storage_error)?;
                let actual = verify_artifact(
                    artifact.id.clone(),
                    artifact.size_bytes,
                    &artifact.sha256,
                    source_file,
                )
                .map_err(|error| StorageError::new(error.to_string()))?;
                if &actual != declared {
                    return Err(StorageError::new(
                        "verified artifact changed before staging",
                    ));
                }
                extract_artifact(artifact, &source, &staging, &self.archive_policy)
                    .map_err(|error| StorageError::new(error.to_string()))?;
                let source_file = File::open(&source).map_err(storage_error)?;
                let after_extraction = verify_artifact(
                    artifact.id.clone(),
                    artifact.size_bytes,
                    &artifact.sha256,
                    source_file,
                )
                .map_err(|error| StorageError::new(error.to_string()))?;
                if after_extraction != actual {
                    return Err(StorageError::new(
                        "verified artifact changed during extraction",
                    ));
                }
            }
            let files = index_files(&staging)?;
            let content_tree_sha256 = content_tree_digest(&files)?;
            let total_file_bytes = files.iter().try_fold(0_u64, |total, file| {
                total
                    .checked_add(file.size_bytes)
                    .ok_or_else(|| StorageError::new("staged content byte count overflow"))
            })?;
            let metadata = staging.join(".npc");
            secure_create_dir_all(&staging, &metadata).map_err(storage_error)?;
            let index = InstallIndexV1 {
                schema: INSTALL_INDEX_SCHEMA_V1.to_owned(),
                identity: identity.clone(),
                manifest_sha256: manifest
                    .digest()
                    .map_err(|error| StorageError::new(error.to_string()))?,
                artifacts: supplied,
                files,
                content_tree_sha256,
                total_file_bytes,
            };
            atomic_write_json(&metadata.join("manifest.json"), manifest)
                .map_err(|error| StorageError::new(error.to_string()))?;
            atomic_write_json(&metadata.join("index.json"), &index)
                .map_err(|error| StorageError::new(error.to_string()))?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        Ok(StagedPack {
            identity,
            transaction_id,
        })
    }

    fn staged_content_binding(
        &self,
        staged: &StagedPack,
    ) -> Result<StagedContentBindingV1, StorageError> {
        validate_token(&staged.transaction_id)?;
        let content_root = self.staged_content_root(staged)?;
        reject_link(&content_root)?;
        let index: InstallIndexV1 = read_json(&content_root.join(".npc").join("index.json"))
            .map_err(|error| StorageError::new(error.to_string()))?;
        if index.schema != INSTALL_INDEX_SCHEMA_V1 || index.identity != staged.identity {
            return Err(StorageError::new("staged transaction identity mismatch"));
        }
        if !verify_index(&content_root, &index)? {
            return Err(StorageError::new(
                "staged content tree does not match its index",
            ));
        }
        Ok(StagedContentBindingV1 {
            identity: index.identity,
            transaction_id: staged.transaction_id.clone(),
            manifest_sha256: index.manifest_sha256,
            content_tree_sha256: index.content_tree_sha256,
            file_count: index.files.len() as u64,
            total_file_bytes: index.total_file_bytes,
        })
    }

    fn commit_inactive(&mut self, staged: &StagedPack) -> Result<(), StorageError> {
        validate_token(&staged.transaction_id)?;
        let staging = self.staging_root(&staged.transaction_id);
        reject_link(&staging)?;
        let index: InstallIndexV1 = read_json(&staging.join(".npc").join("index.json"))
            .map_err(|error| StorageError::new(error.to_string()))?;
        if index.schema != INSTALL_INDEX_SCHEMA_V1 || index.identity != staged.identity {
            return Err(StorageError::new("staged transaction identity mismatch"));
        }
        if !verify_index(&staging, &index)? {
            return Err(StorageError::new(
                "staged content tree does not match its immutable index",
            ));
        }
        let destination = self.version_root(&staged.identity);
        let versions = destination
            .parent()
            .ok_or_else(|| StorageError::new("version path has no parent"))?;
        secure_create_dir_all(&self.root.join("packs"), versions).map_err(storage_error)?;
        reject_link(versions)?;
        if destination.exists() {
            reject_link(&destination)?;
            let existing: InstallIndexV1 = read_json(&destination.join(".npc").join("index.json"))
                .map_err(|error| StorageError::new(error.to_string()))?;
            if existing != index || !verify_index(&destination, &existing)? {
                return Err(StorageError::new(
                    "immutable inactive version already contains different content",
                ));
            }
            fs::remove_dir_all(&staging).map_err(storage_error)?;
        } else {
            let staging_parent = staging
                .parent()
                .ok_or_else(|| StorageError::new("staging path has no parent"))?;
            with_protected_root(staging_parent, || {
                with_protected_root(versions, || fs::rename(&staging, &destination))
            })
            .map_err(storage_error)?;
            reject_link(&destination)?;
            let committed: InstallIndexV1 = read_json(&destination.join(".npc").join("index.json"))
                .map_err(|error| StorageError::new(error.to_string()))?;
            if committed != index || !verify_index(&destination, &committed)? {
                return Err(StorageError::new(
                    "atomically committed inactive version changed during publication",
                ));
            }
        }
        Ok(())
    }

    fn consume_attestation_replay_key(
        &mut self,
        replay_key: &Sha256Digest,
    ) -> Result<bool, StorageError> {
        let lock = self.root.join("state").join("attestation-replay.lock");
        self.with_lock(&lock, || {
            let mut state = self.read_replay_state()?;
            if !state.consumed.insert(replay_key.clone()) {
                return Ok(false);
            }
            atomic_write_json(&self.replay_state_path(), &state)
                .map_err(|error| StorageError::new(error.to_string()))?;
            Ok(true)
        })
    }

    fn activate(
        &mut self,
        staged: StagedPack,
        authorization: &ActivationAuthorization,
    ) -> Result<(), StorageError> {
        validate_token(&staged.transaction_id)?;
        let content_root = self.staged_content_root(&staged)?;
        reject_link(&content_root)?;
        let binding = self.staged_content_binding(&staged)?;
        if &binding != authorization.staged() {
            return Err(StorageError::new(
                "attested staged-content binding changed before activation",
            ));
        }
        let replay_state = self.read_replay_state()?;
        if !replay_state.consumed.contains(authorization.replay_key()) {
            return Err(StorageError::new(
                "activation authorization replay key was not durably consumed",
            ));
        }
        let index: InstallIndexV1 = read_json(&content_root.join(".npc").join("index.json"))
            .map_err(|error| StorageError::new(error.to_string()))?;
        if index.schema != INSTALL_INDEX_SCHEMA_V1 || index.identity != staged.identity {
            return Err(StorageError::new("staged transaction identity mismatch"));
        }
        atomic_write_json(
            &content_root.join(".npc").join("activation.json"),
            &ActivationReceiptV1 {
                schema: ACTIVATION_RECEIPT_SCHEMA_V1.to_owned(),
                attestation_sha256: authorization.attestation_sha256().clone(),
                replay_key: authorization.replay_key().clone(),
                staged: binding,
            },
        )
        .map_err(|error| StorageError::new(error.to_string()))?;
        let destination = self.version_root(&staged.identity);
        let versions = destination
            .parent()
            .ok_or_else(|| StorageError::new("version path has no parent"))?;
        secure_create_dir_all(&self.root.join("packs"), versions).map_err(storage_error)?;
        reject_link(versions)?;
        if content_root == destination {
            let installed: InstallIndexV1 = read_json(&destination.join(".npc").join("index.json"))
                .map_err(|error| StorageError::new(error.to_string()))?;
            if installed != index || !verify_index(&destination, &installed)? {
                return Err(StorageError::new(
                    "inactive installed version changed before activation",
                ));
            }
        } else if destination.exists() {
            let existing: InstallIndexV1 = read_json(&destination.join(".npc").join("index.json"))
                .map_err(|error| StorageError::new(error.to_string()))?;
            if existing != index || !verify_index(&destination, &existing)? {
                return Err(StorageError::new(
                    "immutable version directory already contains different content",
                ));
            }
            fs::remove_dir_all(&content_root).map_err(storage_error)?;
        } else {
            let staging_parent = content_root
                .parent()
                .ok_or_else(|| StorageError::new("staging path has no parent"))?;
            with_protected_root(staging_parent, || {
                with_protected_root(versions, || fs::rename(&content_root, &destination))
            })
            .map_err(storage_error)?;
            reject_link(&destination)?;
            let activated_index: InstallIndexV1 =
                read_json(&destination.join(".npc").join("index.json"))
                    .map_err(|error| StorageError::new(error.to_string()))?;
            if !verify_index(&destination, &activated_index)?
                || activated_index.content_tree_sha256 != authorization.staged().content_tree_sha256
                || activated_index.manifest_sha256 != authorization.staged().manifest_sha256
            {
                return Err(StorageError::new(
                    "atomically moved version does not match its attested staged tree",
                ));
            }
        }

        let pack_id = staged.identity.pack_id;
        let active_revision = staged.identity.revision;
        let lock = self.pack_root(&pack_id).join("active.lock");
        self.with_lock(&lock, || {
            let prior = self.read_active_pointer_unlocked(&pack_id)?;
            let mut previous = prior
                .as_ref()
                .map(|pointer| pointer.previous_revisions.clone())
                .unwrap_or_default();
            if let Some(pointer) = &prior {
                if pointer.active_revision != active_revision {
                    previous.push(pointer.active_revision.clone());
                }
            }
            previous.dedup();
            let pointer = ActivePackPointerV1 {
                schema: ACTIVE_POINTER_SCHEMA_V1.to_owned(),
                pack_id,
                active_revision,
                previous_revisions: previous,
                generation: prior.map_or(1, |pointer| pointer.generation.saturating_add(1)),
            };
            self.write_active_pointer(&pointer)
        })
    }

    fn activate_existing(&mut self, target: &PackRevision) -> Result<(), StorageError> {
        if !self.version_root(target).is_dir() {
            return Err(StorageError::new("activation target is not installed"));
        }
        let lock = self.pack_root(&target.pack_id).join("active.lock");
        self.with_lock(&lock, || {
            let prior = self.read_active_pointer_unlocked(&target.pack_id)?;
            let mut history = prior
                .as_ref()
                .map(|pointer| pointer.previous_revisions.clone())
                .unwrap_or_default();
            if let Some(pointer) = &prior {
                if pointer.active_revision != target.revision {
                    history.push(pointer.active_revision.clone());
                }
            }
            let pointer = ActivePackPointerV1 {
                schema: ACTIVE_POINTER_SCHEMA_V1.to_owned(),
                pack_id: target.pack_id.clone(),
                active_revision: target.revision.clone(),
                previous_revisions: history,
                generation: prior.map_or(1, |pointer| pointer.generation.saturating_add(1)),
            };
            self.write_active_pointer(&pointer)
        })
    }

    fn inspect(&self, target: &PackRevision) -> Result<StorageInspection, StorageError> {
        let root = self.version_root(target);
        if !root.is_dir() {
            return Ok(StorageInspection {
                present: false,
                content_healthy: false,
                manifest_sha256: None,
                content_tree_sha256: None,
                artifact_evidence: BTreeMap::new(),
            });
        }
        let index: InstallIndexV1 = read_json(&root.join(".npc").join("index.json"))
            .map_err(|error| StorageError::new(error.to_string()))?;
        if index.schema != INSTALL_INDEX_SCHEMA_V1 || &index.identity != target {
            return Err(StorageError::new("installed version metadata mismatch"));
        }
        let healthy = verify_index(&root, &index).unwrap_or(false);
        Ok(StorageInspection {
            present: true,
            content_healthy: healthy,
            manifest_sha256: Some(index.manifest_sha256),
            content_tree_sha256: Some(index.content_tree_sha256),
            artifact_evidence: index.artifacts,
        })
    }

    fn remove_revision(&mut self, target: &PackRevision) -> Result<(), StorageError> {
        if self.reference_count(&target.pack_id)? > 0 {
            return Err(StorageError::new("cannot remove referenced pack"));
        }
        if self
            .active_pointer(&target.pack_id)?
            .as_ref()
            .is_some_and(|pointer| pointer.active_revision == target.revision)
        {
            match fs::remove_file(self.pack_root(&target.pack_id).join("active.json")) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(storage_error(error)),
            }
        }
        let version = self.version_root(target);
        if version.exists() {
            reject_link(&version)?;
            fs::remove_dir_all(version).map_err(storage_error)?;
        }
        let downloaded = self
            .root
            .join("downloads")
            .join(target.pack_id.as_str())
            .join(target.revision.as_str());
        if downloaded.exists() {
            reject_link(&downloaded)?;
            fs::remove_dir_all(downloaded).map_err(storage_error)?;
        }
        Ok(())
    }

    fn add_reference(&mut self, pack_id: &PackId, consumer: &str) -> Result<(), StorageError> {
        let lock = self.root.join("state").join("references.lock");
        self.with_lock(&lock, || {
            let mut file = self.read_references()?;
            file.references
                .entry(pack_id.clone())
                .or_default()
                .insert(consumer.to_owned());
            self.write_references(&file)
        })
    }

    fn remove_reference(&mut self, pack_id: &PackId, consumer: &str) -> Result<bool, StorageError> {
        let lock = self.root.join("state").join("references.lock");
        self.with_lock(&lock, || {
            let mut file = self.read_references()?;
            let removed = file
                .references
                .get_mut(pack_id)
                .is_some_and(|references| references.remove(consumer));
            if file.references.get(pack_id).is_some_and(BTreeSet::is_empty) {
                file.references.remove(pack_id);
            }
            self.write_references(&file)?;
            Ok(removed)
        })
    }

    fn reference_count(&self, pack_id: &PackId) -> Result<usize, StorageError> {
        let lock = self.root.join("state").join("references.lock");
        self.with_lock(&lock, || {
            Ok(self
                .read_references()?
                .references
                .get(pack_id)
                .map_or(0, BTreeSet::len))
        })
    }
}

fn index_files(root: &Path) -> Result<Vec<InstalledFileV1>, StorageError> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| StorageError::new(error.to_string()))?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(storage_error)?;
        if is_link_or_reparse(&metadata) {
            return Err(StorageError::new(
                "staging tree contains a symlink or reparse link",
            ));
        }
        if !metadata.is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|_| StorageError::new("indexed file escaped staging root"))?;
        if relative.starts_with(".npc") {
            continue;
        }
        let (size_bytes, sha256) = hash_file(entry.path())?;
        let relative_path = relative
            .to_str()
            .ok_or_else(|| StorageError::new("installed path is not UTF-8"))?
            .replace('\\', "/");
        files.push(InstalledFileV1 {
            relative_path,
            size_bytes,
            sha256,
        });
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

fn verify_index(root: &Path, index: &InstallIndexV1) -> Result<bool, StorageError> {
    let actual = index_files(root)?;
    if actual != index.files {
        return Ok(false);
    }
    if content_tree_digest(&actual)? != index.content_tree_sha256 {
        return Ok(false);
    }
    let total = actual.iter().try_fold(0_u64, |sum, file| {
        sum.checked_add(file.size_bytes)
            .ok_or_else(|| StorageError::new("installed content byte count overflow"))
    })?;
    if total != index.total_file_bytes {
        return Ok(false);
    }
    for file in &index.files {
        let path = root.join(&file.relative_path);
        if !path.starts_with(root) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn content_tree_digest(files: &[InstalledFileV1]) -> Result<Sha256Digest, StorageError> {
    let encoded =
        serde_json::to_vec(files).map_err(|error| StorageError::new(error.to_string()))?;
    let mut domain = b"npc-staged-content-tree-v1\0".to_vec();
    domain.extend_from_slice(&encoded);
    Ok(Sha256Digest::of_bytes(&domain))
}

fn hash_file(path: &Path) -> Result<(u64, Sha256Digest), StorageError> {
    let mut file = File::open(path).map_err(storage_error)?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(storage_error)?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .ok_or_else(|| StorageError::new("installed file size overflow"))?;
        hasher.update(&buffer[..read]);
    }
    let digest = Sha256Digest::parse(hex::encode(hasher.finalize()))
        .map_err(|error| StorageError::new(error.to_string()))?;
    Ok((size, digest))
}

fn reject_link(path: &Path) -> Result<(), StorageError> {
    let metadata = fs::symlink_metadata(path).map_err(storage_error)?;
    if is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(StorageError::new(format!(
            "path is a reparse link or non-directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn validate_token(value: &str) -> Result<(), StorageError> {
    if value.is_empty()
        || value.len() > 512
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(StorageError::new("invalid filesystem token"));
    }
    Ok(())
}

fn storage_error(error: impl std::fmt::Display) -> StorageError {
    StorageError::new(error.to_string())
}
