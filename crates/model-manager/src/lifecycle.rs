use crate::{
    attestation_digest, canonical_attestation_payload_bytes, generate_attestation_nonce,
    validate_challenge, AttestationError, AttestationVerificationError, CatalogInstallBindingV1,
    ModelPackManifestV1, PackId, PackRevision, PackSelectionAuthorizationV1, PackSelectionError,
    Revision, SelfTestAttestationV1, SelfTestAttestationVerifier, SelfTestChallengeV1,
    Sha256Digest, SELF_TEST_ATTESTATION_SCHEMA_V1, SELF_TEST_CHALLENGE_SCHEMA_V1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
use thiserror::Error;

pub const DOWNLOAD_BLOCK_SIZE: usize = 128 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArtifactEvidence {
    pub artifact_id: String,
    pub size_bytes: u64,
    pub sha256: Sha256Digest,
}

/// Stream-validates an artifact without retaining it in memory.
pub fn verify_artifact<R: Read>(
    artifact_id: impl Into<String>,
    expected_size: u64,
    expected_sha256: &Sha256Digest,
    mut reader: R,
) -> Result<ArtifactEvidence, VerificationError> {
    let artifact_id = artifact_id.into();
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut block = vec![0_u8; DOWNLOAD_BLOCK_SIZE];
    loop {
        let read = reader.read(&mut block).map_err(VerificationError::Io)?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .ok_or(VerificationError::SizeOverflow)?;
        if size > expected_size {
            return Err(VerificationError::SizeMismatch {
                expected: expected_size,
                actual: size,
            });
        }
        hasher.update(&block[..read]);
    }
    if size != expected_size {
        return Err(VerificationError::SizeMismatch {
            expected: expected_size,
            actual: size,
        });
    }
    let actual = Sha256Digest::parse(hex::encode(hasher.finalize()))
        .map_err(|_| VerificationError::InternalDigestEncoding)?;
    if &actual != expected_sha256 {
        return Err(VerificationError::DigestMismatch {
            expected: expected_sha256.clone(),
            actual,
        });
    }
    Ok(ArtifactEvidence {
        artifact_id,
        size_bytes: size,
        sha256: actual,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadJournal {
    pub artifact_id: String,
    pub expected_size: u64,
    pub expected_sha256: Sha256Digest,
    pub received_bytes: u64,
    pub validator: Option<String>,
    pub complete: bool,
}

impl DownloadJournal {
    pub fn new(artifact_id: String, expected_size: u64, expected_sha256: Sha256Digest) -> Self {
        Self {
            artifact_id,
            expected_size,
            expected_sha256,
            received_bytes: 0,
            validator: None,
            complete: false,
        }
    }

    pub fn resume_request(&self) -> ResumeRequest {
        ResumeRequest {
            offset: self.received_bytes,
            if_range: self.validator.clone(),
        }
    }

    /// Records a server response before bytes are accepted. A resumed response must be a
    /// validated 206 beginning at the exact journal offset; otherwise the caller must restart.
    pub fn accept_response(&mut self, response: ResumeResponse) -> Result<(), ResumeError> {
        if self.complete {
            return Err(ResumeError::AlreadyComplete);
        }
        if response.total_size != self.expected_size {
            return Err(ResumeError::RemoteSizeChanged {
                expected: self.expected_size,
                actual: response.total_size,
            });
        }
        if self.received_bytes > 0 {
            if response.status != 206 || response.range_start != Some(self.received_bytes) {
                return Err(ResumeError::RangeNotHonored);
            }
            if self.validator.is_none() || response.validator != self.validator {
                return Err(ResumeError::ValidatorChanged);
            }
        } else if response.status != 200 && response.status != 206 {
            return Err(ResumeError::UnexpectedStatus(response.status));
        } else if response.status == 206 && response.range_start != Some(0) {
            return Err(ResumeError::RangeNotHonored);
        }
        if response.validator.as_deref().is_some_and(str::is_empty) {
            return Err(ResumeError::InvalidValidator);
        }
        self.validator = response.validator;
        Ok(())
    }

    pub fn record_bytes(&mut self, count: u64) -> Result<(), ResumeError> {
        let next = self
            .received_bytes
            .checked_add(count)
            .ok_or(ResumeError::SizeOverflow)?;
        if next > self.expected_size {
            return Err(ResumeError::ExceedsExpectedSize);
        }
        self.received_bytes = next;
        Ok(())
    }

    pub fn mark_verified(&mut self, evidence: &ArtifactEvidence) -> Result<(), ResumeError> {
        if evidence.artifact_id != self.artifact_id
            || evidence.size_bytes != self.expected_size
            || evidence.sha256 != self.expected_sha256
            || self.received_bytes != self.expected_size
        {
            return Err(ResumeError::EvidenceMismatch);
        }
        self.complete = true;
        Ok(())
    }

    pub fn restart(&mut self) {
        self.received_bytes = 0;
        self.validator = None;
        self.complete = false;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResumeRequest {
    pub offset: u64,
    pub if_range: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResumeResponse {
    pub status: u16,
    pub range_start: Option<u64>,
    pub total_size: u64,
    pub validator: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum InstallState {
    Downloading,
    Verifying,
    Staging,
    AwaitingSelfTest,
    Activating,
    Active,
    Repairing,
    Removing,
    RolledBack,
    Quarantined {
        phase: InstallPhase,
        reason: String,
        retryable: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallPhase {
    Download,
    Verification,
    Extraction,
    SelfTest,
    Activation,
    Repair,
    Removal,
    Rollback,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackRecord {
    pub manifest: ModelPackManifestV1,
    pub manifest_sha256: Sha256Digest,
    pub state: InstallState,
    pub downloads: BTreeMap<String, DownloadJournal>,
    pub evidence: BTreeMap<String, ArtifactEvidence>,
    pub catalog: CatalogInstallBindingV1,
    pub selection: PackSelectionAuthorizationV1,
    pub self_test_authorization: Option<SelfTestAuthorizationReceiptV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SelfTestAuthorizationReceiptV1 {
    pub attestation_sha256: Sha256Digest,
    pub runner_id: String,
    pub runner_trust_revision: String,
    pub nonce: String,
    pub replay_key: Sha256Digest,
    pub staged: StagedContentBindingV1,
    pub verified_unix_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageInspection {
    pub present: bool,
    pub content_healthy: bool,
    pub manifest_sha256: Option<Sha256Digest>,
    pub content_tree_sha256: Option<Sha256Digest>,
    pub artifact_evidence: BTreeMap<String, ArtifactEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedPack {
    pub identity: PackRevision,
    pub transaction_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StagedContentBindingV1 {
    pub identity: PackRevision,
    pub transaction_id: String,
    pub manifest_sha256: Sha256Digest,
    pub content_tree_sha256: Sha256Digest,
    pub file_count: u64,
    pub total_file_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivationAuthorization {
    staged: StagedContentBindingV1,
    attestation_sha256: Sha256Digest,
    replay_key: Sha256Digest,
}

impl ActivationAuthorization {
    pub(crate) fn staged(&self) -> &StagedContentBindingV1 {
        &self.staged
    }

    pub(crate) fn attestation_sha256(&self) -> &Sha256Digest {
        &self.attestation_sha256
    }

    pub(crate) fn replay_key(&self) -> &Sha256Digest {
        &self.replay_key
    }
}

/// The implementation must stage on the same volume as the active directory and make
/// activation atomic (for example, rename/swap). It must not follow reparse points.
pub trait PackStorage {
    fn stage(
        &mut self,
        manifest: &ModelPackManifestV1,
        evidence: &[ArtifactEvidence],
    ) -> Result<StagedPack, StorageError>;
    fn staged_content_binding(
        &self,
        staged: &StagedPack,
    ) -> Result<StagedContentBindingV1, StorageError>;
    /// Atomically publishes verified extracted bytes as an immutable inactive
    /// version. It must not create or change the active pointer. Backends that
    /// cannot persist inactive versions may retain their staging transaction.
    fn commit_inactive(&mut self, _staged: &StagedPack) -> Result<(), StorageError> {
        Ok(())
    }
    fn consume_attestation_replay_key(
        &mut self,
        replay_key: &Sha256Digest,
    ) -> Result<bool, StorageError>;
    fn activate(
        &mut self,
        staged: StagedPack,
        authorization: &ActivationAuthorization,
    ) -> Result<(), StorageError>;
    fn activate_existing(&mut self, target: &PackRevision) -> Result<(), StorageError>;
    fn inspect(&self, target: &PackRevision) -> Result<StorageInspection, StorageError>;
    fn remove_revision(&mut self, target: &PackRevision) -> Result<(), StorageError>;
    fn add_reference(&mut self, pack_id: &PackId, consumer: &str) -> Result<(), StorageError>;
    fn remove_reference(&mut self, pack_id: &PackId, consumer: &str) -> Result<bool, StorageError>;
    fn reference_count(&self, pack_id: &PackId) -> Result<usize, StorageError>;
}

#[derive(Clone, Debug, Default)]
pub struct InMemoryPackStorage {
    staged: BTreeMap<PackRevision, StorageInspection>,
    installed: BTreeMap<PackRevision, StorageInspection>,
    active: BTreeMap<PackId, Revision>,
    staged_bindings: BTreeMap<PackRevision, StagedContentBindingV1>,
    consumed_attestations: BTreeSet<Sha256Digest>,
    transaction_counter: u64,
}

impl InMemoryPackStorage {
    pub fn active_revision(&self, pack_id: &PackId) -> Option<&Revision> {
        self.active.get(pack_id)
    }
}

impl PackStorage for InMemoryPackStorage {
    fn stage(
        &mut self,
        manifest: &ModelPackManifestV1,
        evidence: &[ArtifactEvidence],
    ) -> Result<StagedPack, StorageError> {
        self.transaction_counter += 1;
        let identity = manifest.identity();
        self.staged.insert(
            identity.clone(),
            StorageInspection {
                present: true,
                content_healthy: true,
                manifest_sha256: Some(
                    manifest
                        .digest()
                        .map_err(|e| StorageError::new(e.to_string()))?,
                ),
                content_tree_sha256: Some(in_memory_tree_digest(evidence)),
                artifact_evidence: evidence
                    .iter()
                    .cloned()
                    .map(|e| (e.artifact_id.clone(), e))
                    .collect(),
            },
        );
        let staged = StagedPack {
            identity,
            transaction_id: format!("memory-{}", self.transaction_counter),
        };
        self.staged_bindings.insert(
            staged.identity.clone(),
            StagedContentBindingV1 {
                identity: staged.identity.clone(),
                transaction_id: staged.transaction_id.clone(),
                manifest_sha256: manifest
                    .digest()
                    .map_err(|error| StorageError::new(error.to_string()))?,
                content_tree_sha256: in_memory_tree_digest(evidence),
                file_count: evidence.len() as u64,
                total_file_bytes: evidence.iter().map(|item| item.size_bytes).sum(),
            },
        );
        Ok(staged)
    }

    fn staged_content_binding(
        &self,
        staged: &StagedPack,
    ) -> Result<StagedContentBindingV1, StorageError> {
        self.staged_bindings
            .get(&staged.identity)
            .filter(|binding| binding.transaction_id == staged.transaction_id)
            .cloned()
            .ok_or_else(|| StorageError::new("staged content binding is missing"))
    }

    fn commit_inactive(&mut self, staged: &StagedPack) -> Result<(), StorageError> {
        let inspection = self
            .staged
            .get(&staged.identity)
            .cloned()
            .ok_or_else(|| StorageError::new("staged transaction is missing"))?;
        self.installed.insert(staged.identity.clone(), inspection);
        Ok(())
    }

    fn consume_attestation_replay_key(
        &mut self,
        replay_key: &Sha256Digest,
    ) -> Result<bool, StorageError> {
        Ok(self.consumed_attestations.insert(replay_key.clone()))
    }

    fn activate(
        &mut self,
        staged: StagedPack,
        authorization: &ActivationAuthorization,
    ) -> Result<(), StorageError> {
        let actual = self.staged_content_binding(&staged)?;
        if &actual != authorization.staged() {
            return Err(StorageError::new(
                "activation authorization does not bind staged content",
            ));
        }
        let inspection = self
            .staged
            .remove(&staged.identity)
            .ok_or_else(|| StorageError::new("staged transaction is missing"))?;
        self.active.insert(
            staged.identity.pack_id.clone(),
            staged.identity.revision.clone(),
        );
        self.installed.insert(staged.identity, inspection);
        self.staged_bindings.remove(&actual.identity);
        Ok(())
    }

    fn activate_existing(&mut self, target: &PackRevision) -> Result<(), StorageError> {
        if !self.installed.contains_key(target) {
            return Err(StorageError::new("rollback target is not installed"));
        }
        self.active
            .insert(target.pack_id.clone(), target.revision.clone());
        Ok(())
    }

    fn inspect(&self, target: &PackRevision) -> Result<StorageInspection, StorageError> {
        Ok(self
            .installed
            .get(target)
            .cloned()
            .unwrap_or(StorageInspection {
                present: false,
                content_healthy: false,
                manifest_sha256: None,
                content_tree_sha256: None,
                artifact_evidence: BTreeMap::new(),
            }))
    }

    fn remove_revision(&mut self, target: &PackRevision) -> Result<(), StorageError> {
        if self.active.get(&target.pack_id) == Some(&target.revision) {
            self.active.remove(&target.pack_id);
        }
        self.staged.remove(target);
        self.staged_bindings.remove(target);
        self.installed.remove(target);
        Ok(())
    }

    fn add_reference(&mut self, _pack_id: &PackId, _consumer: &str) -> Result<(), StorageError> {
        Ok(())
    }

    fn remove_reference(
        &mut self,
        _pack_id: &PackId,
        _consumer: &str,
    ) -> Result<bool, StorageError> {
        Ok(true)
    }

    fn reference_count(&self, _pack_id: &PackId) -> Result<usize, StorageError> {
        Ok(0)
    }
}

fn in_memory_tree_digest(evidence: &[ArtifactEvidence]) -> Sha256Digest {
    let mut ordered = evidence.to_vec();
    ordered.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    let bytes = serde_json::to_vec(&ordered).unwrap_or_default();
    let mut domain = b"npc-in-memory-staged-tree-v1\0".to_vec();
    domain.extend_from_slice(&bytes);
    Sha256Digest::of_bytes(&domain)
}

fn attestation_replay_key(identity: &PackRevision, runner_id: &str, nonce: &str) -> Sha256Digest {
    let encoded = serde_json::to_vec(&(identity, runner_id, nonce)).unwrap_or_default();
    let mut domain = b"npc-self-test-replay-key-v1\0".to_vec();
    domain.extend_from_slice(&encoded);
    Sha256Digest::of_bytes(&domain)
}

#[derive(Debug)]
pub struct ModelPackManager<S: PackStorage> {
    storage: S,
    records: BTreeMap<PackRevision, PackRecord>,
    staged: BTreeMap<PackRevision, StagedPack>,
    pending_self_tests: BTreeMap<PackRevision, SelfTestChallengeV1>,
    immutable_revisions: BTreeMap<PackRevision, Sha256Digest>,
    active: BTreeMap<PackId, Revision>,
    rollback: BTreeMap<PackId, Vec<Revision>>,
    references: BTreeMap<PackId, BTreeSet<String>>,
}

impl<S: PackStorage> ModelPackManager<S> {
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            records: BTreeMap::new(),
            staged: BTreeMap::new(),
            pending_self_tests: BTreeMap::new(),
            immutable_revisions: BTreeMap::new(),
            active: BTreeMap::new(),
            rollback: BTreeMap::new(),
            references: BTreeMap::new(),
        }
    }

    pub fn storage(&self) -> &S {
        &self.storage
    }
    pub fn into_storage(self) -> S {
        self.storage
    }
    pub fn record(&self, identity: &PackRevision) -> Option<&PackRecord> {
        self.records.get(identity)
    }
    pub fn active_revision(&self, pack_id: &PackId) -> Option<&Revision> {
        self.active.get(pack_id)
    }

    pub fn begin_install(
        &mut self,
        manifest: ModelPackManifestV1,
        catalog: CatalogInstallBindingV1,
        selection: PackSelectionAuthorizationV1,
        license_accepted: bool,
    ) -> Result<PackRevision, ManagerError> {
        manifest.validate()?;
        if catalog.catalog_version == 0 {
            return Err(ManagerError::InvalidCatalogBinding);
        }
        selection.validate_for_manifest(&manifest)?;
        if manifest.license.acceptance_required && !license_accepted {
            return Err(ManagerError::LicenseAcceptanceRequired);
        }
        let identity = manifest.identity();
        let digest = manifest.digest()?;
        if let Some(trusted_digest) = self.immutable_revisions.get(&identity) {
            if trusted_digest != &digest {
                return Err(ManagerError::ImmutableRevisionChanged(identity));
            }
        }
        if let Some(record) = self.records.get(&identity) {
            if record.manifest_sha256 != digest {
                return Err(ManagerError::ImmutableRevisionChanged(identity));
            }
            return Err(ManagerError::AlreadyManaged(identity));
        }
        let downloads = manifest
            .artifacts
            .iter()
            .map(|a| {
                (
                    a.id.clone(),
                    DownloadJournal::new(a.id.clone(), a.size_bytes, a.sha256.clone()),
                )
            })
            .collect();
        self.records.insert(
            identity.clone(),
            PackRecord {
                manifest,
                manifest_sha256: digest.clone(),
                state: InstallState::Downloading,
                downloads,
                evidence: BTreeMap::new(),
                catalog,
                selection,
                self_test_authorization: None,
            },
        );
        self.immutable_revisions.insert(identity.clone(), digest);
        Ok(identity)
    }

    /// Starts an update while preserving the active revision as a rollback target.
    /// Activation performs the actual switch only after full verification and self-test.
    pub fn begin_update(
        &mut self,
        manifest: ModelPackManifestV1,
        catalog: CatalogInstallBindingV1,
        selection: PackSelectionAuthorizationV1,
        license_accepted: bool,
    ) -> Result<PackRevision, ManagerError> {
        let active = self
            .active
            .get(&manifest.pack_id)
            .cloned()
            .ok_or(ManagerError::NotActive)?;
        if active == manifest.revision {
            return Err(ManagerError::AlreadyActive);
        }
        self.begin_install(manifest, catalog, selection, license_accepted)
    }

    pub fn download_journal_mut(
        &mut self,
        identity: &PackRevision,
        artifact_id: &str,
    ) -> Result<&mut DownloadJournal, ManagerError> {
        let record = self
            .records
            .get_mut(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        if !matches!(
            record.state,
            InstallState::Downloading | InstallState::Verifying
        ) {
            return Err(ManagerError::InvalidState(record.state.clone()));
        }
        record
            .downloads
            .get_mut(artifact_id)
            .ok_or(ManagerError::UnknownArtifact)
    }

    pub fn accept_verified_artifact(
        &mut self,
        identity: &PackRevision,
        evidence: ArtifactEvidence,
    ) -> Result<(), ManagerError> {
        let record = self
            .records
            .get_mut(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        if !matches!(
            record.state,
            InstallState::Downloading | InstallState::Verifying
        ) {
            return Err(ManagerError::InvalidState(record.state.clone()));
        }
        let journal = record
            .downloads
            .get_mut(&evidence.artifact_id)
            .ok_or(ManagerError::UnknownArtifact)?;
        journal.mark_verified(&evidence)?;
        record
            .evidence
            .insert(evidence.artifact_id.clone(), evidence);
        record.state = if record.evidence.len() == record.downloads.len() {
            InstallState::Staging
        } else {
            InstallState::Verifying
        };
        Ok(())
    }

    pub fn stage(&mut self, identity: &PackRevision) -> Result<(), ManagerError> {
        let record = self
            .records
            .get_mut(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        if record.state != InstallState::Staging {
            return Err(ManagerError::InvalidState(record.state.clone()));
        }
        if record.evidence.len() != record.manifest.artifacts.len() {
            return Err(ManagerError::IncompleteEvidence);
        }
        let evidence: Vec<_> = record.evidence.values().cloned().collect();
        let staged = self
            .storage
            .stage(&record.manifest, &evidence)
            .map_err(ManagerError::Storage)?;
        if &staged.identity != identity {
            record.state = InstallState::Quarantined {
                phase: InstallPhase::Extraction,
                reason: "storage returned the wrong staged identity".to_owned(),
                retryable: false,
            };
            return Err(ManagerError::StorageIdentityMismatch);
        }
        self.staged.insert(identity.clone(), staged);
        let staged = self
            .staged
            .get(identity)
            .cloned()
            .ok_or(ManagerError::MissingStagedTransaction)?;
        if let Err(error) = self.storage.commit_inactive(&staged) {
            record.state = InstallState::Quarantined {
                phase: InstallPhase::Extraction,
                reason: "verified staging could not be atomically committed inactive".to_owned(),
                retryable: true,
            };
            return Err(ManagerError::Storage(error));
        }
        record.state = InstallState::AwaitingSelfTest;
        Ok(())
    }

    /// Upgrades an already installed-inactive explicit selection to an exact
    /// loadout-bound activation authorization. This does not run a self-test or
    /// activate anything; it only replaces the earlier install-only capability
    /// after the resource governor has minted a covering admission receipt.
    pub fn authorize_activation(
        &mut self,
        identity: &PackRevision,
        selection: PackSelectionAuthorizationV1,
    ) -> Result<(), ManagerError> {
        let record = self
            .records
            .get_mut(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        if record.state != InstallState::AwaitingSelfTest {
            return Err(ManagerError::InvalidState(record.state.clone()));
        }
        if self.pending_self_tests.contains_key(identity) {
            return Err(ManagerError::InvalidState(record.state.clone()));
        }
        selection.validate_for_manifest(&record.manifest)?;
        if !selection.activation_allowed() {
            return Err(ManagerError::Selection(
                PackSelectionError::ActivationNotAuthorized,
            ));
        }
        record.selection = selection;
        Ok(())
    }

    pub fn issue_self_test_challenge(
        &mut self,
        identity: &PackRevision,
        runtime_backend: impl Into<String>,
        now_unix_seconds: u64,
        lifetime_seconds: u64,
    ) -> Result<SelfTestChallengeV1, ManagerError> {
        let runtime_backend = runtime_backend.into();
        let record = self
            .records
            .get(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        if record.state != InstallState::AwaitingSelfTest {
            return Err(ManagerError::InvalidState(record.state.clone()));
        }
        if !record
            .manifest
            .self_test
            .allowed_runtime_backends
            .contains(&runtime_backend)
        {
            return Err(ManagerError::UnapprovedSelfTestBackend(runtime_backend));
        }
        if lifetime_seconds == 0
            || lifetime_seconds > crate::MAX_SELF_TEST_ATTESTATION_LIFETIME_SECONDS
        {
            return Err(ManagerError::InvalidSelfTestLifetime);
        }
        let staged = self
            .staged
            .get(identity)
            .ok_or(ManagerError::MissingStagedTransaction)?;
        let binding = self
            .storage
            .staged_content_binding(staged)
            .map_err(ManagerError::Storage)?;
        if binding.identity != *identity
            || binding.transaction_id != staged.transaction_id
            || binding.manifest_sha256 != record.manifest_sha256
        {
            return Err(ManagerError::StagedContentBindingMismatch);
        }
        let challenge = SelfTestChallengeV1 {
            schema: SELF_TEST_CHALLENGE_SCHEMA_V1.to_owned(),
            identity: identity.clone(),
            manifest_sha256: record.manifest_sha256.clone(),
            catalog: record.catalog.clone(),
            staged: binding,
            runtime_abi: record.manifest.runtime.abi.clone(),
            runtime_backend,
            test_suite_id: record.manifest.self_test.kind.clone(),
            test_suite_revision: record.manifest.self_test.suite_revision.clone(),
            nonce: generate_attestation_nonce()?,
            issued_unix_seconds: now_unix_seconds,
            expires_unix_seconds: now_unix_seconds
                .checked_add(lifetime_seconds)
                .ok_or(ManagerError::InvalidSelfTestLifetime)?,
        };
        validate_challenge(&challenge)?;
        self.pending_self_tests
            .insert(identity.clone(), challenge.clone());
        Ok(challenge)
    }

    /// Abandons only the caller-owned outstanding challenge after a runtime
    /// probe fails or times out. The exact nonce prevents one operation from
    /// clearing another operation's challenge. Installed bytes, verified
    /// evidence, activation selection, and quarantine state are untouched.
    pub fn abandon_self_test_challenge(
        &mut self,
        identity: &PackRevision,
        expected_nonce: &str,
    ) -> Result<(), ManagerError> {
        let record = self
            .records
            .get(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        if record.state != InstallState::AwaitingSelfTest {
            return Err(ManagerError::InvalidState(record.state.clone()));
        }
        let challenge = self
            .pending_self_tests
            .get(identity)
            .ok_or(ManagerError::MissingSelfTestChallenge)?;
        if challenge.nonce != expected_nonce {
            return Err(ManagerError::AttestationChallengeMismatch);
        }
        self.pending_self_tests.remove(identity);
        Ok(())
    }

    pub fn record_self_test_attestation(
        &mut self,
        identity: &PackRevision,
        attestation: SelfTestAttestationV1,
        verifier: &impl SelfTestAttestationVerifier,
        now_unix_seconds: u64,
    ) -> Result<(), ManagerError> {
        let record = self
            .records
            .get(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        if record.state != InstallState::AwaitingSelfTest {
            if record
                .self_test_authorization
                .as_ref()
                .is_some_and(|receipt| receipt.nonce == attestation.signed.challenge.nonce)
            {
                return Err(ManagerError::AttestationReplay);
            }
            return Err(ManagerError::InvalidState(record.state.clone()));
        }
        if attestation.signed.schema != SELF_TEST_ATTESTATION_SCHEMA_V1 {
            return Err(ManagerError::UnsupportedAttestationSchema(
                attestation.signed.schema.clone(),
            ));
        }
        validate_challenge(&attestation.signed.challenge)?;
        let expected_challenge = self
            .pending_self_tests
            .get(identity)
            .ok_or(ManagerError::MissingSelfTestChallenge)?;
        if &attestation.signed.challenge != expected_challenge
            || &attestation.signed.challenge.identity != identity
        {
            return Err(ManagerError::AttestationChallengeMismatch);
        }
        if now_unix_seconds < expected_challenge.issued_unix_seconds {
            return Err(ManagerError::AttestationNotYetValid);
        }
        if now_unix_seconds > expected_challenge.expires_unix_seconds {
            return Err(ManagerError::AttestationExpired);
        }
        let payload = canonical_attestation_payload_bytes(&attestation.signed)
            .map_err(ManagerError::AttestationSerialization)?;
        let verified_runner = verifier
            .verify(&attestation.signed.runner_id, &attestation.proof, &payload)
            .map_err(ManagerError::AttestationVerification)?;
        if verified_runner.runner_id != attestation.signed.runner_id {
            return Err(ManagerError::RunnerIdentityMismatch);
        }
        let staged = self
            .staged
            .get(identity)
            .ok_or(ManagerError::MissingStagedTransaction)?;
        let current_binding = match self.storage.staged_content_binding(staged) {
            Ok(binding) => binding,
            Err(error) => {
                let record = self
                    .records
                    .get_mut(identity)
                    .ok_or(ManagerError::UnknownRevision)?;
                record.state = InstallState::Quarantined {
                    phase: InstallPhase::SelfTest,
                    reason: "staged content could not be revalidated after self-test".to_owned(),
                    retryable: false,
                };
                return Err(ManagerError::Storage(error));
            }
        };
        if current_binding != expected_challenge.staged {
            let record = self
                .records
                .get_mut(identity)
                .ok_or(ManagerError::UnknownRevision)?;
            record.state = InstallState::Quarantined {
                phase: InstallPhase::SelfTest,
                reason: "staged content changed after self-test challenge".to_owned(),
                retryable: false,
            };
            return Err(ManagerError::StagedContentChangedAfterTest);
        }
        let outcome = &attestation.signed.outcome;
        if outcome.duration_millis > record.manifest.self_test.timeout_millis {
            return self.quarantine_attested_failure(
                identity,
                "attested self-test exceeded its declared timeout",
                true,
            );
        }
        if !outcome.passed {
            return self.quarantine_attested_failure(
                identity,
                outcome
                    .diagnostic_code
                    .as_deref()
                    .unwrap_or("attested self-test failed"),
                true,
            );
        }
        if let Some(expected) = &record.manifest.self_test.expected_output_sha256 {
            if outcome.output_sha256.as_ref() != Some(expected) {
                return self.quarantine_attested_failure(
                    identity,
                    "attested self-test output digest mismatch",
                    false,
                );
            }
        }
        let digest =
            attestation_digest(&attestation).map_err(ManagerError::AttestationSerialization)?;
        let replay_key = attestation_replay_key(
            identity,
            &attestation.signed.runner_id,
            &attestation.signed.challenge.nonce,
        );
        if !self
            .storage
            .consume_attestation_replay_key(&replay_key)
            .map_err(ManagerError::Storage)?
        {
            return Err(ManagerError::AttestationReplay);
        }
        let receipt = SelfTestAuthorizationReceiptV1 {
            attestation_sha256: digest,
            runner_id: verified_runner.runner_id,
            runner_trust_revision: verified_runner.trust_revision,
            nonce: attestation.signed.challenge.nonce,
            replay_key,
            staged: current_binding,
            verified_unix_seconds: now_unix_seconds,
        };
        let record = self
            .records
            .get_mut(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        record.self_test_authorization = Some(receipt);
        record.state = InstallState::Activating;
        self.pending_self_tests.remove(identity);
        Ok(())
    }

    fn quarantine_attested_failure(
        &mut self,
        identity: &PackRevision,
        reason: &str,
        retryable: bool,
    ) -> Result<(), ManagerError> {
        let record = self
            .records
            .get_mut(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        record.state = InstallState::Quarantined {
            phase: InstallPhase::SelfTest,
            reason: reason.to_owned(),
            retryable,
        };
        Err(ManagerError::SelfTestFailed)
    }

    pub fn activate(&mut self, identity: &PackRevision) -> Result<(), ManagerError> {
        let record = self
            .records
            .get(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        if record.state != InstallState::Activating {
            return Err(ManagerError::InvalidState(record.state.clone()));
        }
        let receipt = record
            .self_test_authorization
            .clone()
            .ok_or(ManagerError::MissingSelfTestAuthorization)?;
        if !record.selection.activation_allowed() {
            return Err(ManagerError::Selection(
                PackSelectionError::ActivationNotAuthorized,
            ));
        }
        let staged = self
            .staged
            .get(identity)
            .cloned()
            .ok_or(ManagerError::MissingStagedTransaction)?;
        let current_binding = match self.storage.staged_content_binding(&staged) {
            Ok(binding) => binding,
            Err(error) => {
                let record = self
                    .records
                    .get_mut(identity)
                    .ok_or(ManagerError::UnknownRevision)?;
                record.state = InstallState::Quarantined {
                    phase: InstallPhase::Activation,
                    reason: "attested staged content could not be revalidated".to_owned(),
                    retryable: false,
                };
                return Err(ManagerError::Storage(error));
            }
        };
        if current_binding != receipt.staged {
            let record = self
                .records
                .get_mut(identity)
                .ok_or(ManagerError::UnknownRevision)?;
            record.state = InstallState::Quarantined {
                phase: InstallPhase::Activation,
                reason: "staged content changed after its attested self-test".to_owned(),
                retryable: false,
            };
            return Err(ManagerError::StagedContentChangedAfterTest);
        }
        let authorization = ActivationAuthorization {
            staged: receipt.staged,
            attestation_sha256: receipt.attestation_sha256,
            replay_key: receipt.replay_key,
        };
        if let Err(error) = self.storage.activate(staged, &authorization) {
            let record = self
                .records
                .get_mut(identity)
                .ok_or(ManagerError::UnknownRevision)?;
            record.state = InstallState::Quarantined {
                phase: InstallPhase::Activation,
                reason: error.to_string(),
                // The storage boundary may have failed before or after its atomic swap.
                // Audit/repair must resolve that ambiguity instead of blind retry.
                retryable: false,
            };
            return Err(ManagerError::Storage(error));
        }
        self.staged.remove(identity);
        if let Some(old) = self
            .active
            .insert(identity.pack_id.clone(), identity.revision.clone())
        {
            if old != identity.revision {
                self.rollback
                    .entry(identity.pack_id.clone())
                    .or_default()
                    .push(old);
            }
        }
        let record = self
            .records
            .get_mut(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        record.state = InstallState::Active;
        Ok(())
    }

    pub fn add_reference(
        &mut self,
        pack_id: &PackId,
        consumer: impl Into<String>,
    ) -> Result<(), ManagerError> {
        if !self.active.contains_key(pack_id) {
            return Err(ManagerError::NotActive);
        }
        let consumer = consumer.into();
        if consumer.trim().is_empty() || consumer.len() > 256 {
            return Err(ManagerError::InvalidConsumer);
        }
        self.storage
            .add_reference(pack_id, &consumer)
            .map_err(ManagerError::Storage)?;
        self.references
            .entry(pack_id.clone())
            .or_default()
            .insert(consumer);
        Ok(())
    }

    pub fn release_reference(
        &mut self,
        pack_id: &PackId,
        consumer: &str,
    ) -> Result<bool, ManagerError> {
        let persisted = self
            .storage
            .remove_reference(pack_id, consumer)
            .map_err(ManagerError::Storage)?;
        let removed = self
            .references
            .get_mut(pack_id)
            .is_some_and(|refs| refs.remove(consumer));
        if self.references.get(pack_id).is_some_and(BTreeSet::is_empty) {
            self.references.remove(pack_id);
        }
        Ok(removed || persisted)
    }

    pub fn reference_count(&self, pack_id: &PackId) -> Result<usize, ManagerError> {
        let in_memory = self.references.get(pack_id).map_or(0, BTreeSet::len);
        let persisted = self
            .storage
            .reference_count(pack_id)
            .map_err(ManagerError::Storage)?;
        Ok(in_memory.max(persisted))
    }

    pub fn remove(
        &mut self,
        pack_id: &PackId,
        policy: RemovalPolicy,
    ) -> Result<usize, ManagerError> {
        let refs = self.reference_count(pack_id)?;
        if refs > 0 && policy == RemovalPolicy::RequireUnreferenced {
            return Err(ManagerError::PackInUse(refs));
        }
        let identities: Vec<_> = self
            .records
            .keys()
            .filter(|id| &id.pack_id == pack_id)
            .cloned()
            .collect();
        for identity in &identities {
            if let Some(record) = self.records.get_mut(identity) {
                record.state = InstallState::Removing;
            }
            self.storage
                .remove_revision(identity)
                .map_err(ManagerError::Storage)?;
            self.staged.remove(identity);
            self.pending_self_tests.remove(identity);
            self.records.remove(identity);
        }
        self.active.remove(pack_id);
        self.rollback.remove(pack_id);
        self.references.remove(pack_id);
        Ok(identities.len())
    }

    pub fn rollback(&mut self, pack_id: &PackId) -> Result<PackRevision, ManagerError> {
        let references = self.reference_count(pack_id)?;
        if references > 0 {
            return Err(ManagerError::PackInUse(references));
        }
        let target_revision = self
            .rollback
            .get_mut(pack_id)
            .and_then(Vec::pop)
            .ok_or(ManagerError::NoRollback)?;
        let target = PackRevision {
            pack_id: pack_id.clone(),
            revision: target_revision.clone(),
        };
        self.storage
            .activate_existing(&target)
            .map_err(ManagerError::Storage)?;
        let current = self
            .active
            .insert(pack_id.clone(), target_revision)
            .ok_or(ManagerError::NotActive)?;
        let current_id = PackRevision {
            pack_id: pack_id.clone(),
            revision: current,
        };
        if let Some(record) = self.records.get_mut(&current_id) {
            record.state = InstallState::RolledBack;
        }
        if let Some(record) = self.records.get_mut(&target) {
            record.state = InstallState::Active;
        }
        Ok(target)
    }

    pub fn audit(&self, identity: &PackRevision) -> Result<RepairAssessment, ManagerError> {
        let record = self
            .records
            .get(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        let inspection = self
            .storage
            .inspect(identity)
            .map_err(ManagerError::Storage)?;
        let mut issues = Vec::new();
        if !inspection.present {
            issues.push(RepairIssue::MissingInstallation);
        }
        if !inspection.content_healthy {
            issues.push(RepairIssue::InstalledContentMismatch);
        }
        if inspection.manifest_sha256.as_ref() != Some(&record.manifest_sha256) {
            issues.push(RepairIssue::ManifestMismatch);
        }
        for artifact in &record.manifest.artifacts {
            match inspection.artifact_evidence.get(&artifact.id) {
                None => issues.push(RepairIssue::MissingArtifact(artifact.id.clone())),
                Some(actual) if actual.size_bytes != artifact.size_bytes => {
                    issues.push(RepairIssue::ArtifactSizeMismatch(artifact.id.clone()))
                }
                Some(actual) if actual.sha256 != artifact.sha256 => {
                    issues.push(RepairIssue::ArtifactDigestMismatch(artifact.id.clone()))
                }
                Some(_) => {}
            }
        }
        Ok(RepairAssessment {
            healthy: issues.is_empty(),
            issues,
        })
    }

    pub fn begin_repair(
        &mut self,
        identity: &PackRevision,
    ) -> Result<RepairAssessment, ManagerError> {
        let assessment = self.audit(identity)?;
        if assessment.healthy {
            return Ok(assessment);
        }
        let record = self
            .records
            .get_mut(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        self.staged.remove(identity);
        self.pending_self_tests.remove(identity);
        record.state = InstallState::Repairing;
        Ok(assessment)
    }

    /// Discards untrusted local progress and returns a damaged pack to the same verified
    /// artifact pipeline used by first install. This is deliberately a separate transition
    /// so diagnostics can observe `Repairing` before potentially expensive downloads begin.
    pub fn restart_repair_downloads(
        &mut self,
        identity: &PackRevision,
    ) -> Result<(), ManagerError> {
        let record = self
            .records
            .get_mut(identity)
            .ok_or(ManagerError::UnknownRevision)?;
        if record.state != InstallState::Repairing {
            return Err(ManagerError::InvalidState(record.state.clone()));
        }
        record.evidence.clear();
        record.self_test_authorization = None;
        for journal in record.downloads.values_mut() {
            journal.restart();
        }
        // Repair intentionally returns to the verified download path. It never blesses
        // files merely because they exist in the activation directory.
        record.state = InstallState::Downloading;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemovalPolicy {
    RequireUnreferenced,
    Force,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepairAssessment {
    pub healthy: bool,
    pub issues: Vec<RepairIssue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepairIssue {
    MissingInstallation,
    InstalledContentMismatch,
    ManifestMismatch,
    MissingArtifact(String),
    ArtifactSizeMismatch(String),
    ArtifactDigestMismatch(String),
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("storage operation failed: {message}")]
pub struct StorageError {
    message: String,
}

impl StorageError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("artifact read failed: {0}")]
    Io(io::Error),
    #[error("artifact size overflow")]
    SizeOverflow,
    #[error("artifact size mismatch: expected {expected}, actual {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("artifact digest mismatch: expected {expected}, actual {actual}")]
    DigestMismatch {
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    #[error("SHA-256 implementation returned an invalid encoded digest")]
    InternalDigestEncoding,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ResumeError {
    #[error("artifact is already complete")]
    AlreadyComplete,
    #[error("remote size changed from {expected} to {actual}")]
    RemoteSizeChanged { expected: u64, actual: u64 },
    #[error("server did not honor the exact resume range")]
    RangeNotHonored,
    #[error("strong resume validator changed or is missing")]
    ValidatorChanged,
    #[error("unexpected HTTP status {0}")]
    UnexpectedStatus(u16),
    #[error("invalid resume validator")]
    InvalidValidator,
    #[error("download size overflow")]
    SizeOverflow,
    #[error("received bytes exceed expected size")]
    ExceedsExpectedSize,
    #[error("verified evidence does not match the journal")]
    EvidenceMismatch,
}

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("invalid manifest: {0}")]
    Manifest(#[from] crate::ManifestError),
    #[error("license acceptance is required")]
    LicenseAcceptanceRequired,
    #[error("catalog install binding is invalid")]
    InvalidCatalogBinding,
    #[error("model pack selection rejected: {0}")]
    Selection(#[from] PackSelectionError),
    #[error("immutable revision changed: {0:?}")]
    ImmutableRevisionChanged(PackRevision),
    #[error("pack revision is already managed: {0:?}")]
    AlreadyManaged(PackRevision),
    #[error("unknown pack revision")]
    UnknownRevision,
    #[error("unknown artifact")]
    UnknownArtifact,
    #[error("operation is invalid in state {0:?}")]
    InvalidState(InstallState),
    #[error("resume validation failed: {0}")]
    Resume(#[from] ResumeError),
    #[error("not all artifacts have verified evidence")]
    IncompleteEvidence,
    #[error("storage returned a different pack identity")]
    StorageIdentityMismatch,
    #[error("staged transaction is missing")]
    MissingStagedTransaction,
    #[error("self-test failed")]
    SelfTestFailed,
    #[error("self-test attestation error: {0}")]
    Attestation(#[from] AttestationError),
    #[error("self-test attestation serialization failed: {0}")]
    AttestationSerialization(serde_json::Error),
    #[error("self-test attestation verification failed: {0}")]
    AttestationVerification(AttestationVerificationError),
    #[error("unsupported self-test attestation schema: {0}")]
    UnsupportedAttestationSchema(String),
    #[error("self-test challenge is missing")]
    MissingSelfTestChallenge,
    #[error("self-test attestation does not match the outstanding challenge")]
    AttestationChallengeMismatch,
    #[error("self-test attestation is not yet valid")]
    AttestationNotYetValid,
    #[error("self-test attestation is expired")]
    AttestationExpired,
    #[error("attestation verifier returned a different runner identity")]
    RunnerIdentityMismatch,
    #[error("self-test attestation replay was rejected")]
    AttestationReplay,
    #[error("self-test runtime backend is not approved: {0}")]
    UnapprovedSelfTestBackend(String),
    #[error("self-test challenge lifetime is invalid")]
    InvalidSelfTestLifetime,
    #[error("staged content binding is inconsistent")]
    StagedContentBindingMismatch,
    #[error("staged content changed after its self-test")]
    StagedContentChangedAfterTest,
    #[error("activation has no verified self-test authorization")]
    MissingSelfTestAuthorization,
    #[error("storage error: {0}")]
    Storage(StorageError),
    #[error("pack is not active")]
    NotActive,
    #[error("requested revision is already active")]
    AlreadyActive,
    #[error("invalid reference consumer")]
    InvalidConsumer,
    #[error("pack is in use by {0} consumers")]
    PackInUse(usize),
    #[error("no rollback revision is available")]
    NoRollback,
}
