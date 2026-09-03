use crate::{
    canonical_catalog_payload_bytes, verify_catalog, CatalogEntryV1, CatalogError,
    CatalogPayloadV1, CatalogSignatureV1, CatalogTrustPolicy, CatalogTrustState,
    Ed25519CatalogVerifier, MeasurementTrustPolicyV1, MeasurementTrustStateV1,
    ModelPackAdmissionStateV2, NormalizedModelPackManifestV2, PackRevision, ResidencyModeV1,
    Sha256Digest, SignedCatalogV1, TrustedCatalog, ED25519_CATALOG_ALGORITHM,
    MODEL_PACK_MANIFEST_SCHEMA_V2, SIGNED_CATALOG_SCHEMA_V1,
};
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use thiserror::Error;

pub const RELEASE_CATALOG_ROOT_SCHEMA_V1: &str = "npc.model-catalog-root/v1";
pub const MODEL_PACK_SOURCE_INVENTORY_SCHEMA_V1: &str = "npc.model-pack-source-inventory/v1";
pub const RELEASE_CATALOG_ROOT_FILE_V1: &str = "model-catalog-root-v1.json";
pub const RELEASE_CATALOG_FILE_V1: &str = "model-catalog-v1.json";
pub const OPTIONAL_LOCAL_CATALOG_CHANNEL_V1: &str = "optional-local";
pub const MAX_RELEASE_CATALOG_METADATA_BYTES_V1: u64 = 8 * 1024 * 1024;
pub const NON_QUALIFYING_REVIEW_EVIDENCE_INDEX_SCHEMA_V1: &str =
    "npc.non-qualifying-model-review-evidence-index/v1";
pub const NON_QUALIFYING_REVIEW_EVIDENCE_INDEX_FILE_V1: &str = "review-evidence-index-v1.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseCatalogRootV1 {
    pub schema: String,
    pub trust_scope: ReleaseCatalogTrustScopeV1,
    pub production_trust: bool,
    pub rotation_required_before_release: bool,
    pub promotion_supported: bool,
    pub publication_supported: bool,
    pub signature_threshold: usize,
    pub keys: Vec<ReleaseCatalogRootKeyV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseCatalogTrustScopeV1 {
    AutomatedLocalReviewBootstrap,
    ProductionRelease,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseCatalogRootKeyV1 {
    pub key_id: String,
    pub public_key_hex: String,
}

/// The v1 catalog remains the planner's compatibility payload. The second
/// threshold-signed payload binds that normalized catalog to the richer v2
/// source documents without teaching older readers to trust ignored fields.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedReleaseCatalogBundleV1 {
    pub signed: CatalogPayloadV1,
    pub signatures: Vec<CatalogSignatureV1>,
    pub source_inventory: ModelPackSourceInventoryPayloadV1,
    pub source_inventory_signatures: Vec<CatalogSignatureV1>,
}

impl SignedReleaseCatalogBundleV1 {
    pub fn planner_catalog(&self) -> SignedCatalogV1 {
        SignedCatalogV1 {
            signed: self.signed.clone(),
            signatures: self.signatures.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackSourceInventoryPayloadV1 {
    pub schema: String,
    pub catalog_payload_sha256: Sha256Digest,
    pub manifests: Vec<ModelPackSourceBindingV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackSourceBindingV1 {
    pub identity: PackRevision,
    pub file_name: String,
    pub source_schema: String,
    /// SHA-256 of the exact checked JSON bytes distributed with the product.
    pub raw_document_sha256: Sha256Digest,
    /// SHA-256 of deterministic serde serialization after strict v2 parsing.
    pub canonical_document_sha256: Sha256Digest,
    /// Digest of the normalized v1 core manifest embedded in the planner catalog.
    pub normalized_manifest_sha256: Sha256Digest,
    pub admission_state: ModelPackAdmissionStateV2,
    pub admission_reason: String,
    pub allowed_residencies: BTreeSet<ResidencyModeV1>,
    pub license: ModelPackLicenseBindingV1,
    pub lifecycle: crate::ModelPackLifecycleV2,
    /// Signed catalog binding for real but non-qualifying benchmark evidence.
    /// This is intentionally a different type from `QualifiedEnvelopeBindingV1`.
    pub review_evidence: Vec<NonQualifyingReviewEvidenceBindingV1>,
    /// Empty is meaningful: no measurement is claimed to exist.
    pub qualified_envelopes: Vec<QualifiedEnvelopeBindingV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackLicenseBindingV1 {
    pub spdx_expression: Option<String>,
    pub license_name: String,
    pub license_url: String,
    pub redistributable: bool,
    pub acceptance_required: bool,
    pub component_ids: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedEnvelopeBindingV1 {
    pub report_id: String,
    pub device_fingerprint_sha256: Sha256Digest,
    pub placement: ResidencyModeV1,
    pub envelope_sha256: Sha256Digest,
    pub source_evidence_sha256: Option<Sha256Digest>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NonQualifyingReviewEvidenceBindingV1 {
    pub evidence_schema: String,
    pub file_name: String,
    pub evidence_sha256: Sha256Digest,
    pub signed_measurement_envelope: bool,
    pub admission_eligible: bool,
    pub blocker: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NonQualifyingReviewEvidenceIndexV1 {
    pub schema: String,
    pub entries: Vec<NonQualifyingReviewEvidenceIndexEntryV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NonQualifyingReviewEvidenceIndexEntryV1 {
    pub identity: PackRevision,
    pub evidence: NonQualifyingReviewEvidenceBindingV1,
}

#[derive(Clone, Debug)]
pub struct ReleaseManifestSourceV1 {
    pub file_name: String,
    pub raw_document_sha256: Sha256Digest,
    pub normalized: NormalizedModelPackManifestV2,
    pub review_evidence: Vec<NonQualifyingReviewEvidenceBindingV1>,
    pub qualified_envelopes: Vec<QualifiedEnvelopeBindingV1>,
}

#[derive(Clone, Debug)]
pub struct ReleaseCatalogSignerV1 {
    pub key_id: String,
    pub signing_key: SigningKey,
}

#[derive(Clone, Debug)]
pub struct VerifiedReleaseCatalogBundleV1 {
    pub catalog: TrustedCatalog,
    pub trust_state: CatalogTrustState,
    pub source_inventory: ModelPackSourceInventoryPayloadV1,
    pub verifier: Ed25519CatalogVerifier,
    pub signature_threshold: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedReleasePackSnapshotV1 {
    pub identity: PackRevision,
    pub manifest_raw_sha256: Sha256Digest,
    pub display_name: String,
    pub description: String,
    /// No recommendation is synthesized from catalog presence. `None` is the
    /// honest value until a signed product policy supplies one.
    pub recommendation_reason: Option<String>,
    pub capability: crate::ModelPackCapabilityV1,
    pub runtime: String,
    pub runtime_revision: Option<String>,
    pub abi: String,
    pub backends: BTreeSet<String>,
    pub exact_artifact_download_bytes: u64,
    pub installed_bytes: u64,
    pub planning_storage_bytes: u64,
    pub planning_peak_install_bytes: u64,
    pub admission_state: ModelPackAdmissionStateV2,
    pub admission_reason: String,
    pub allowed_residencies: BTreeSet<ResidencyModeV1>,
    pub license: ModelPackLicenseBindingV1,
    pub lifecycle: crate::ModelPackLifecycleV2,
    pub non_qualifying_review_evidence_count: usize,
    pub qualified_envelope_count: usize,
    pub measurement_status: TrustedPackMeasurementStatusV1,
    pub measurement_detail: String,
    pub qualified_measurement: Option<crate::MeasuredResourceEnvelopePayloadV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustedPackMeasurementStatusV1 {
    Unavailable,
    Qualified,
}

impl VerifiedReleaseCatalogBundleV1 {
    /// Serialize-only metadata for native product presentation. Resource fit is
    /// deliberately absent: callers must use a live selected-loadout decision.
    pub fn trusted_pack_snapshots(
        &self,
    ) -> Result<Vec<TrustedReleasePackSnapshotV1>, ReleaseCatalogBundleError> {
        let bindings = self
            .source_inventory
            .manifests
            .iter()
            .map(|binding| (&binding.identity, binding))
            .collect::<BTreeMap<_, _>>();
        self.catalog
            .payload()
            .entries
            .iter()
            .map(|entry| {
                let identity = entry.manifest.identity();
                let binding = bindings
                    .get(&identity)
                    .ok_or(ReleaseCatalogBundleError::CatalogInventoryMismatch)?;
                let exact_artifact_download_bytes = entry
                    .manifest
                    .artifacts
                    .iter()
                    .try_fold(0_u64, |total, artifact| {
                        total.checked_add(artifact.size_bytes)
                    })
                    .ok_or(ReleaseCatalogBundleError::ArtifactByteOverflow)?;
                Ok(TrustedReleasePackSnapshotV1 {
                    identity,
                    manifest_raw_sha256: binding.raw_document_sha256.clone(),
                    display_name: entry.manifest.display_name.clone(),
                    description: entry.manifest.description.clone(),
                    recommendation_reason: None,
                    capability: entry.manifest.capability.clone(),
                    runtime: entry.manifest.runtime.runtime.clone(),
                    runtime_revision: entry.manifest.runtime.minimum_runtime_revision.clone(),
                    abi: entry.manifest.runtime.abi.clone(),
                    backends: entry.manifest.self_test.allowed_runtime_backends.clone(),
                    exact_artifact_download_bytes,
                    installed_bytes: entry.manifest.resources.storage_bytes,
                    planning_storage_bytes: entry.manifest.resources.storage_bytes,
                    planning_peak_install_bytes: entry.manifest.resources.peak_install_bytes,
                    admission_state: binding.admission_state.clone(),
                    admission_reason: binding.admission_reason.clone(),
                    allowed_residencies: binding.allowed_residencies.clone(),
                    license: binding.license.clone(),
                    lifecycle: binding.lifecycle.clone(),
                    non_qualifying_review_evidence_count: binding.review_evidence.len(),
                    qualified_envelope_count: binding.qualified_envelopes.len(),
                    measurement_status: TrustedPackMeasurementStatusV1::Unavailable,
                    measurement_detail: "No signed envelope has been verified for the current native device in this snapshot.".to_owned(),
                    qualified_measurement: None,
                })
            })
            .collect()
    }
}

pub fn build_release_catalog_bundle_v1(
    mut sources: Vec<ReleaseManifestSourceV1>,
    version: u64,
    generated_unix_seconds: u64,
    expires_unix_seconds: u64,
    signers: &[ReleaseCatalogSignerV1],
    signature_threshold: usize,
) -> Result<(ReleaseCatalogRootV1, SignedReleaseCatalogBundleV1), ReleaseCatalogBundleError> {
    if version == 0 || generated_unix_seconds >= expires_unix_seconds {
        return Err(ReleaseCatalogBundleError::InvalidValidityWindow);
    }
    if signature_threshold < 2 || signature_threshold > signers.len() {
        return Err(ReleaseCatalogBundleError::InvalidThreshold);
    }
    let unique_keys: BTreeSet<_> = signers
        .iter()
        .map(|signer| signer.key_id.as_str())
        .collect();
    if unique_keys.len() != signers.len() {
        return Err(ReleaseCatalogBundleError::DuplicateKey);
    }
    sources.sort_by(|left, right| {
        left.normalized
            .core_manifest
            .identity()
            .cmp(&right.normalized.core_manifest.identity())
    });
    let mut seen = BTreeSet::new();
    let mut entries = Vec::with_capacity(sources.len());
    let mut bindings = Vec::with_capacity(sources.len());
    for source in sources {
        if source.normalized.document.schema != MODEL_PACK_MANIFEST_SCHEMA_V2 {
            return Err(ReleaseCatalogBundleError::NonCanonicalManifest);
        }
        let identity = source.normalized.core_manifest.identity();
        if !seen.insert(identity.clone()) {
            return Err(ReleaseCatalogBundleError::DuplicateManifest(identity));
        }
        let manifest_sha256 = source
            .normalized
            .core_manifest
            .digest()
            .map_err(ReleaseCatalogBundleError::Manifest)?;
        if source.review_evidence.iter().any(|evidence| {
            evidence.evidence_schema.trim().is_empty()
                || evidence.file_name.trim().is_empty()
                || evidence.signed_measurement_envelope
                || evidence.admission_eligible
                || evidence.blocker.trim().is_empty()
        }) {
            return Err(ReleaseCatalogBundleError::UnsafeReviewEvidence(identity));
        }
        entries.push(CatalogEntryV1 {
            manifest: source.normalized.core_manifest.clone(),
            manifest_sha256: manifest_sha256.clone(),
            channels: BTreeSet::from([OPTIONAL_LOCAL_CATALOG_CHANNEL_V1.to_owned()]),
            published_unix_seconds: generated_unix_seconds,
            revoked: false,
            revocation_reason: None,
        });
        let component_ids = source
            .normalized
            .document
            .license
            .components
            .iter()
            .map(|component| component.id.clone())
            .collect();
        bindings.push(ModelPackSourceBindingV1 {
            identity,
            file_name: source.file_name,
            source_schema: source.normalized.document.schema.clone(),
            raw_document_sha256: source.raw_document_sha256,
            canonical_document_sha256: source.normalized.canonical_document_sha256,
            normalized_manifest_sha256: manifest_sha256,
            admission_state: source.normalized.document.admission.state.clone(),
            admission_reason: source.normalized.document.admission.reason.clone(),
            allowed_residencies: source
                .normalized
                .document
                .admission
                .allowed_residencies
                .clone(),
            license: ModelPackLicenseBindingV1 {
                spdx_expression: source.normalized.document.license.spdx_expression.clone(),
                license_name: source.normalized.document.license.license_name.clone(),
                license_url: source.normalized.document.license.license_url.clone(),
                redistributable: source.normalized.document.license.redistributable,
                acceptance_required: source.normalized.document.license.acceptance_required,
                component_ids,
            },
            lifecycle: source.normalized.document.lifecycle.clone(),
            review_evidence: source.review_evidence,
            qualified_envelopes: source.qualified_envelopes,
        });
    }
    let catalog_payload = CatalogPayloadV1 {
        schema: SIGNED_CATALOG_SCHEMA_V1.to_owned(),
        version,
        generated_unix_seconds,
        expires_unix_seconds,
        entries,
    };
    let catalog_bytes = canonical_catalog_payload_bytes(&catalog_payload)?;
    let source_inventory = ModelPackSourceInventoryPayloadV1 {
        schema: MODEL_PACK_SOURCE_INVENTORY_SCHEMA_V1.to_owned(),
        catalog_payload_sha256: Sha256Digest::of_bytes(&catalog_bytes),
        manifests: bindings,
    };
    let inventory_bytes = canonical_source_inventory_bytes(&source_inventory)?;
    let signatures = sign_payload(signers, &catalog_bytes);
    let source_inventory_signatures = sign_payload(signers, &inventory_bytes);
    let root = ReleaseCatalogRootV1 {
        schema: RELEASE_CATALOG_ROOT_SCHEMA_V1.to_owned(),
        trust_scope: ReleaseCatalogTrustScopeV1::AutomatedLocalReviewBootstrap,
        production_trust: false,
        rotation_required_before_release: true,
        promotion_supported: false,
        publication_supported: false,
        signature_threshold,
        keys: signers
            .iter()
            .map(|signer| ReleaseCatalogRootKeyV1 {
                key_id: signer.key_id.clone(),
                public_key_hex: hex::encode(signer.signing_key.verifying_key().to_bytes()),
            })
            .collect(),
    };
    Ok((
        root,
        SignedReleaseCatalogBundleV1 {
            signed: catalog_payload,
            signatures,
            source_inventory,
            source_inventory_signatures,
        },
    ))
}

pub fn canonical_source_inventory_bytes(
    inventory: &ModelPackSourceInventoryPayloadV1,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(inventory)
}

pub fn verify_release_catalog_bundle_v1(
    root: &ReleaseCatalogRootV1,
    bundle: &SignedReleaseCatalogBundleV1,
    prior: &CatalogTrustState,
    now_unix_seconds: u64,
) -> Result<VerifiedReleaseCatalogBundleV1, ReleaseCatalogBundleError> {
    if root.schema != RELEASE_CATALOG_ROOT_SCHEMA_V1
        || root.signature_threshold < 2
        || root.signature_threshold > root.keys.len()
    {
        return Err(ReleaseCatalogBundleError::InvalidThreshold);
    }
    match &root.trust_scope {
        ReleaseCatalogTrustScopeV1::AutomatedLocalReviewBootstrap
            if !root.production_trust
                && root.rotation_required_before_release
                && !root.promotion_supported
                && !root.publication_supported => {}
        ReleaseCatalogTrustScopeV1::ProductionRelease if root.production_trust => {}
        _ => return Err(ReleaseCatalogBundleError::InvalidTrustScope),
    }
    let mut keys = Vec::with_capacity(root.keys.len());
    for key in &root.keys {
        let bytes = hex::decode(&key.public_key_hex)
            .map_err(|_| ReleaseCatalogBundleError::InvalidPublicKey(key.key_id.clone()))?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| ReleaseCatalogBundleError::InvalidPublicKey(key.key_id.clone()))?;
        keys.push((key.key_id.clone(), bytes));
    }
    let verifier = Ed25519CatalogVerifier::new(keys)?;
    let policy = CatalogTrustPolicy {
        signature_threshold: root.signature_threshold,
        ..CatalogTrustPolicy::default()
    };
    let (catalog, trust_state) = verify_catalog(
        &bundle.planner_catalog(),
        &verifier,
        &policy,
        prior,
        now_unix_seconds,
    )?;
    if bundle.source_inventory.schema != MODEL_PACK_SOURCE_INVENTORY_SCHEMA_V1 {
        return Err(ReleaseCatalogBundleError::InvalidInventorySchema);
    }
    let catalog_bytes = canonical_catalog_payload_bytes(&bundle.signed)?;
    if bundle.source_inventory.catalog_payload_sha256 != Sha256Digest::of_bytes(&catalog_bytes) {
        return Err(ReleaseCatalogBundleError::CatalogInventoryMismatch);
    }
    let inventory_bytes = canonical_source_inventory_bytes(&bundle.source_inventory)?;
    let valid = bundle
        .source_inventory_signatures
        .iter()
        .filter(|signature| {
            crate::CatalogSignatureVerifier::verify(
                &verifier,
                &signature.key_id,
                &signature.algorithm,
                &inventory_bytes,
                &signature.signature,
            )
        })
        .map(|signature| signature.key_id.as_str())
        .collect::<BTreeSet<_>>();
    if valid.len() < root.signature_threshold {
        return Err(ReleaseCatalogBundleError::InventorySignatureThreshold {
            required: root.signature_threshold,
            valid: valid.len(),
        });
    }
    let catalog_digests: BTreeMap<_, _> = bundle
        .signed
        .entries
        .iter()
        .map(|entry| (entry.manifest.identity(), entry.manifest_sha256.clone()))
        .collect();
    if catalog_digests.len() != bundle.source_inventory.manifests.len() {
        return Err(ReleaseCatalogBundleError::CatalogInventoryMismatch);
    }
    let mut prior_identity: Option<&PackRevision> = None;
    for binding in &bundle.source_inventory.manifests {
        if prior_identity.is_some_and(|prior| prior >= &binding.identity)
            || catalog_digests.get(&binding.identity) != Some(&binding.normalized_manifest_sha256)
        {
            return Err(ReleaseCatalogBundleError::CatalogInventoryMismatch);
        }
        prior_identity = Some(&binding.identity);
    }
    Ok(VerifiedReleaseCatalogBundleV1 {
        catalog,
        trust_state,
        source_inventory: bundle.source_inventory.clone(),
        verifier,
        signature_threshold: root.signature_threshold,
    })
}

pub fn load_release_catalog_bundle_v1(
    directory: &Path,
) -> Result<(ReleaseCatalogRootV1, SignedReleaseCatalogBundleV1), ReleaseCatalogBundleError> {
    let root_path = directory.join(RELEASE_CATALOG_ROOT_FILE_V1);
    let catalog_path = directory.join(RELEASE_CATALOG_FILE_V1);
    let root_bytes = read_bounded_regular_file(&root_path)?;
    let catalog_bytes = read_bounded_regular_file(&catalog_path)?;
    Ok((
        serde_json::from_slice(&root_bytes)?,
        serde_json::from_slice(&catalog_bytes)?,
    ))
}

pub fn verify_release_manifest_files_v1(
    directory: &Path,
    verified: &VerifiedReleaseCatalogBundleV1,
) -> Result<(), ReleaseCatalogBundleError> {
    for binding in &verified.source_inventory.manifests {
        let relative = Path::new(&binding.file_name);
        if relative.components().count() != 1
            || relative.extension().and_then(|value| value.to_str()) != Some("json")
        {
            return Err(ReleaseCatalogBundleError::UnsafeManifestFileName(
                binding.file_name.clone(),
            ));
        }
        let bytes = read_bounded_regular_file(&directory.join(relative))?;
        if Sha256Digest::of_bytes(&bytes) != binding.raw_document_sha256 {
            return Err(ReleaseCatalogBundleError::SourceManifestDigestMismatch(
                binding.identity.clone(),
            ));
        }
        let normalized = crate::parse_and_normalize_model_pack_manifest(&bytes)
            .map_err(ReleaseCatalogBundleError::SourceManifest)?;
        let core_digest = normalized
            .core_manifest
            .digest()
            .map_err(ReleaseCatalogBundleError::Manifest)?;
        if normalized.core_manifest.identity() != binding.identity
            || normalized.canonical_document_sha256 != binding.canonical_document_sha256
            || core_digest != binding.normalized_manifest_sha256
        {
            return Err(ReleaseCatalogBundleError::SourceManifestDigestMismatch(
                binding.identity.clone(),
            ));
        }
    }
    Ok(())
}

pub fn verify_release_envelope_files_v1(
    directory: &Path,
    verified: &VerifiedReleaseCatalogBundleV1,
    now_unix_seconds: u64,
) -> Result<MeasurementTrustStateV1, ReleaseCatalogBundleError> {
    let mut state = MeasurementTrustStateV1::default();
    let policy = MeasurementTrustPolicyV1 {
        signature_threshold: verified.signature_threshold,
        ..MeasurementTrustPolicyV1::default()
    };
    for source in &verified.source_inventory.manifests {
        let entry = verified
            .catalog
            .entry(&source.identity)
            .ok_or(ReleaseCatalogBundleError::CatalogInventoryMismatch)?;
        for binding in &source.qualified_envelopes {
            // The signed inventory already binds the exact envelope digest.
            // Use that digest as the distributed filename so untrusted pack,
            // revision, and device identifiers cannot create legacy-length
            // Windows paths or path-shape ambiguity.
            let path = directory
                .join("qual")
                .join(format!("{}.json", binding.envelope_sha256.as_str()));
            let bytes = read_bounded_regular_file(&path)?;
            if Sha256Digest::of_bytes(&bytes) != binding.envelope_sha256 {
                return Err(ReleaseCatalogBundleError::QualifiedEnvelopeDigestMismatch(
                    source.identity.clone(),
                ));
            }
            let signed: crate::SignedMeasuredResourceEnvelopeV1 = serde_json::from_slice(&bytes)?;
            let (qualified, next) = crate::verify_measured_resource_envelope(
                &signed,
                &verified.verifier,
                &policy,
                &state,
                now_unix_seconds,
            )
            .map_err(ReleaseCatalogBundleError::QualifiedEnvelope)?;
            let payload = qualified.payload();
            if payload.report_id != binding.report_id
                || payload.identity != source.identity
                || payload.device_fingerprint_sha256 != binding.device_fingerprint_sha256
                || payload.manifest_sha256 != source.normalized_manifest_sha256
                || payload.capability != entry.manifest.capability.kind
                || payload.runtime != entry.manifest.runtime.runtime
                || entry.manifest.runtime.minimum_runtime_revision.as_deref()
                    != Some(payload.runtime_revision.as_str())
                || !payload.placements.contains_key(&binding.placement)
            {
                return Err(ReleaseCatalogBundleError::QualifiedEnvelopeBindingMismatch(
                    source.identity.clone(),
                ));
            }
            if let Some(evidence_sha256) = &binding.source_evidence_sha256 {
                if !source
                    .review_evidence
                    .iter()
                    .any(|evidence| &evidence.evidence_sha256 == evidence_sha256)
                {
                    return Err(ReleaseCatalogBundleError::QualifiedEnvelopeBindingMismatch(
                        source.identity.clone(),
                    ));
                }
            }
            state = next;
        }
    }
    Ok(state)
}

fn read_bounded_regular_file(path: &Path) -> Result<Vec<u8>, ReleaseCatalogBundleError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|source| ReleaseCatalogBundleError::Read(path.to_owned(), source))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_RELEASE_CATALOG_METADATA_BYTES_V1
    {
        return Err(ReleaseCatalogBundleError::UnsafeFile(path.to_owned()));
    }
    std::fs::read(path).map_err(|source| ReleaseCatalogBundleError::Read(path.to_owned(), source))
}

fn sign_payload(signers: &[ReleaseCatalogSignerV1], bytes: &[u8]) -> Vec<CatalogSignatureV1> {
    signers
        .iter()
        .map(|signer| CatalogSignatureV1 {
            key_id: signer.key_id.clone(),
            algorithm: ED25519_CATALOG_ALGORITHM.to_owned(),
            signature: base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(signer.signing_key.sign(bytes).to_bytes()),
        })
        .collect()
}

#[derive(Debug, Error)]
pub enum ReleaseCatalogBundleError {
    #[error("release catalog signature threshold is invalid")]
    InvalidThreshold,
    #[error("release catalog validity window is invalid")]
    InvalidValidityWindow,
    #[error("release catalog trust scope flags are inconsistent")]
    InvalidTrustScope,
    #[error("release catalog signer key IDs are not unique")]
    DuplicateKey,
    #[error("release catalog contains a non-canonical manifest")]
    NonCanonicalManifest,
    #[error("release catalog contains duplicate manifest identity {0:?}")]
    DuplicateManifest(PackRevision),
    #[error("release catalog manifest is invalid: {0}")]
    Manifest(crate::ManifestError),
    #[error("release catalog JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("release catalog verification failed: {0}")]
    Catalog(#[from] CatalogError),
    #[error("release catalog key is invalid: {0}")]
    Key(#[from] crate::CatalogKeyError),
    #[error("release catalog public key is invalid: {0}")]
    InvalidPublicKey(String),
    #[error("release catalog inventory schema is invalid")]
    InvalidInventorySchema,
    #[error(
        "release catalog inventory signature threshold not met: required {required}, valid {valid}"
    )]
    InventorySignatureThreshold { required: usize, valid: usize },
    #[error("release catalog and source inventory do not match")]
    CatalogInventoryMismatch,
    #[error("release catalog artifact byte total overflowed")]
    ArtifactByteOverflow,
    #[error("release catalog non-qualifying review evidence is unsafe: {0:?}")]
    UnsafeReviewEvidence(PackRevision),
    #[error("release catalog file could not be read at {0}: {1}")]
    Read(std::path::PathBuf, std::io::Error),
    #[error("release catalog file is not a bounded regular file: {0}")]
    UnsafeFile(std::path::PathBuf),
    #[error("release catalog manifest file name is unsafe: {0}")]
    UnsafeManifestFileName(String),
    #[error("release catalog source manifest failed validation: {0}")]
    SourceManifest(crate::ModelPackManifestV2Error),
    #[error("release catalog source manifest digest does not match inventory: {0:?}")]
    SourceManifestDigestMismatch(PackRevision),
    #[error("release catalog qualified envelope failed trust validation: {0}")]
    QualifiedEnvelope(crate::ResourceEnvelopeError),
    #[error("release catalog qualified envelope file digest does not match inventory: {0:?}")]
    QualifiedEnvelopeDigestMismatch(PackRevision),
    #[error("release catalog qualified envelope binding does not match payload: {0:?}")]
    QualifiedEnvelopeBindingMismatch(PackRevision),
}
