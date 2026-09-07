use crate::{
    CatalogError, CatalogInstallBindingV1, CatalogTrustDomainV1, DownloadError, DownloadPolicy,
    FileDownloadJournalStore, FilesystemPackStorage, HttpsArtifactDownloader, InstallState,
    ManagerError, MeasuredLocalPackSelectionPolicyV1, ModelPackManager, PackRevision,
    PackSelectionActionV1, PackSelectionOriginV1, PackSelectionRequestV1, RemovalPolicy,
    StorageError, TrustedCatalog,
};
use std::path::PathBuf;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// Native-only coordinator for the catalog-to-install half of an optional
/// model-pack lifecycle. It never selects a pack automatically and deliberately
/// stops at `AwaitingSelfTest`; activation still requires a runtime attestation
/// and, separately, a whole-loadout measured admission receipt.
pub struct TrustedOptionalPackLifecycleV1 {
    catalog: TrustedCatalog,
    catalog_binding: CatalogInstallBindingV1,
    manager: ModelPackManager<FilesystemPackStorage>,
    downloader: HttpsArtifactDownloader,
    journals: FileDownloadJournalStore,
}

impl TrustedOptionalPackLifecycleV1 {
    pub fn new(
        root: PathBuf,
        catalog: TrustedCatalog,
        trust_domain: CatalogTrustDomainV1,
        download_policy: DownloadPolicy,
    ) -> Result<Self, OptionalPackLifecycleError> {
        let storage = FilesystemPackStorage::new(root, crate::ArchivePolicy::default())?;
        let journals = storage.journal_store()?;
        let catalog_binding = CatalogInstallBindingV1 {
            catalog_payload_sha256: catalog.payload_digest().clone(),
            catalog_version: catalog.payload().version,
            trust_domain,
        };
        Ok(Self {
            catalog,
            catalog_binding,
            manager: ModelPackManager::new(storage),
            downloader: HttpsArtifactDownloader::new(download_policy)?,
            journals,
        })
    }

    pub fn state(&self, identity: &PackRevision) -> Option<&InstallState> {
        self.manager.record(identity).map(|record| &record.state)
    }

    pub fn manager(&self) -> &ModelPackManager<FilesystemPackStorage> {
        &self.manager
    }

    pub fn manager_mut(&mut self) -> &mut ModelPackManager<FilesystemPackStorage> {
        &mut self.manager
    }

    /// Downloads only the exact artifacts authorized by the already-verified
    /// catalog and explicit user selection. The downloader's durable journal
    /// makes an interrupted call resumable. Verified bytes are extracted in
    /// same-volume staging, indexed, and atomically renamed into an immutable
    /// inactive version. No active pointer exists until attested activation.
    pub async fn install_or_resume(
        &mut self,
        identity: &PackRevision,
        selection_id: String,
        selected_unix_seconds: u64,
        explicit_user_confirmation: bool,
        license_accepted: bool,
        cancellation: &CancellationToken,
    ) -> Result<InstallState, OptionalPackLifecycleError> {
        if !explicit_user_confirmation {
            return Err(OptionalPackLifecycleError::ExplicitConfirmationRequired);
        }
        let entry = self.catalog.installable_entry(identity)?.clone();
        let manifest = entry.manifest;
        let selection = MeasuredLocalPackSelectionPolicyV1.authorize(
            &manifest,
            PackSelectionRequestV1 {
                selection_id,
                origin: PackSelectionOriginV1::ExplicitUser,
                action: PackSelectionActionV1::InstallOnly,
                selected_unix_seconds,
            },
            None,
        )?;
        if self.manager.record(identity).is_none() {
            self.manager.begin_install(
                manifest.clone(),
                self.catalog_binding.clone(),
                selection.clone(),
                license_accepted,
            )?;
        } else {
            let record = self
                .manager
                .record(identity)
                .ok_or(OptionalPackLifecycleError::State)?;
            if record.manifest_sha256 != entry.manifest_sha256
                || record.selection.identity() != identity
                || !matches!(
                    record.state,
                    InstallState::Downloading | InstallState::Verifying
                )
            {
                return Err(OptionalPackLifecycleError::State);
            }
        }

        for artifact in &manifest.artifacts {
            if self
                .manager
                .record(identity)
                .is_some_and(|record| record.evidence.contains_key(&artifact.id))
            {
                continue;
            }
            let destination = self
                .manager
                .storage()
                .download_path(identity, &artifact.id)?;
            let evidence = self
                .downloader
                .download(
                    identity,
                    artifact,
                    &selection,
                    destination,
                    &self.journals,
                    cancellation,
                )
                .await?;
            let journal = self.manager.download_journal_mut(identity, &artifact.id)?;
            if journal.received_bytes == 0 {
                journal.record_bytes(evidence.size_bytes)?;
            }
            self.manager.accept_verified_artifact(identity, evidence)?;
        }
        self.manager.stage(identity)?;
        self.manager
            .record(identity)
            .map(|record| record.state.clone())
            .ok_or(OptionalPackLifecycleError::State)
    }

    /// A repair never blesses existing files. It re-enters the same exact,
    /// journaled download and transactional staging path used by installation.
    pub fn begin_repair(
        &mut self,
        identity: &PackRevision,
    ) -> Result<crate::RepairAssessment, OptionalPackLifecycleError> {
        let assessment = self.manager.begin_repair(identity)?;
        if !assessment.healthy {
            self.manager.restart_repair_downloads(identity)?;
        }
        Ok(assessment)
    }

    /// Binds the installed-inactive transaction to a governor-minted complete
    /// loadout admission before any native self-test challenge may be issued.
    pub fn authorize_activation(
        &mut self,
        identity: &PackRevision,
        selection_id: String,
        selected_unix_seconds: u64,
        admission: &crate::LoadoutAdmissionV1,
    ) -> Result<(), OptionalPackLifecycleError> {
        let entry = self.catalog.installable_entry(identity)?;
        let selection = MeasuredLocalPackSelectionPolicyV1.authorize(
            &entry.manifest,
            PackSelectionRequestV1 {
                selection_id,
                origin: PackSelectionOriginV1::ExplicitUser,
                action: PackSelectionActionV1::InstallAndActivate,
                selected_unix_seconds,
            },
            Some(admission),
        )?;
        self.manager.authorize_activation(identity, selection)?;
        Ok(())
    }

    /// Clears only the exact outstanding self-test challenge owned by a
    /// failed provider probe. The immutable inactive version remains verified
    /// and can be retried under a freshly minted whole-loadout admission.
    pub fn abandon_self_test_challenge(
        &mut self,
        identity: &PackRevision,
        expected_nonce: &str,
    ) -> Result<(), OptionalPackLifecycleError> {
        self.manager
            .abandon_self_test_challenge(identity, expected_nonce)?;
        Ok(())
    }

    pub fn remove(
        &mut self,
        identity: &PackRevision,
        explicit_user_confirmation: bool,
    ) -> Result<usize, OptionalPackLifecycleError> {
        if !explicit_user_confirmation {
            return Err(OptionalPackLifecycleError::ExplicitConfirmationRequired);
        }
        Ok(self
            .manager
            .remove(&identity.pack_id, RemovalPolicy::RequireUnreferenced)?)
    }

    /// Completes the same transactional inactive-install boundary from artifacts that a
    /// trusted native transport already placed in the coordinator's download
    /// paths. `FilesystemPackStorage::stage` re-hashes every source, so caller
    /// evidence cannot bless changed bytes. This is also the deterministic
    /// offline/synthetic-fixture seam used by lifecycle tests.
    pub fn import_verified_downloads(
        &mut self,
        identity: &PackRevision,
        selection_id: String,
        selected_unix_seconds: u64,
        explicit_user_confirmation: bool,
        license_accepted: bool,
        evidence: Vec<crate::ArtifactEvidence>,
    ) -> Result<InstallState, OptionalPackLifecycleError> {
        if !explicit_user_confirmation {
            return Err(OptionalPackLifecycleError::ExplicitConfirmationRequired);
        }
        let entry = self.catalog.installable_entry(identity)?.clone();
        let selection = MeasuredLocalPackSelectionPolicyV1.authorize(
            &entry.manifest,
            PackSelectionRequestV1 {
                selection_id,
                origin: PackSelectionOriginV1::ExplicitUser,
                action: PackSelectionActionV1::InstallOnly,
                selected_unix_seconds,
            },
            None,
        )?;
        self.manager.begin_install(
            entry.manifest,
            self.catalog_binding.clone(),
            selection,
            license_accepted,
        )?;
        for artifact in evidence {
            let journal = self
                .manager
                .download_journal_mut(identity, &artifact.artifact_id)?;
            journal.record_bytes(artifact.size_bytes)?;
            self.manager.accept_verified_artifact(identity, artifact)?;
        }
        self.manager.stage(identity)?;
        self.manager
            .record(identity)
            .map(|record| record.state.clone())
            .ok_or(OptionalPackLifecycleError::State)
    }
}

#[derive(Debug, Error)]
pub enum OptionalPackLifecycleError {
    #[error("explicit user confirmation is required")]
    ExplicitConfirmationRequired,
    #[error("optional pack lifecycle state is incompatible with this operation")]
    State,
    #[error("trusted catalog rejected the pack: {0}")]
    Catalog(#[from] CatalogError),
    #[error("optional pack selection failed: {0}")]
    Selection(#[from] crate::PackSelectionError),
    #[error("optional pack manager failed: {0}")]
    Manager(#[from] ManagerError),
    #[error("optional pack storage failed: {0}")]
    Storage(#[from] StorageError),
    #[error("optional pack download failed: {0}")]
    Download(#[from] DownloadError),
    #[error("optional pack download journal failed: {0}")]
    Resume(#[from] crate::ResumeError),
}
