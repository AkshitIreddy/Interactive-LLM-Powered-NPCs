use crate::{ModelPackManifestV1, PackRevision, Sha256Digest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const SIGNED_CATALOG_SCHEMA_V1: &str = "npc.model-catalog/v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogEntryV1 {
    pub manifest: ModelPackManifestV1,
    pub manifest_sha256: Sha256Digest,
    pub channels: BTreeSet<String>,
    pub published_unix_seconds: u64,
    pub revoked: bool,
    pub revocation_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogPayloadV1 {
    pub schema: String,
    /// Monotonic TUF-like snapshot version. Older versions are rejected.
    pub version: u64,
    pub generated_unix_seconds: u64,
    pub expires_unix_seconds: u64,
    pub entries: Vec<CatalogEntryV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogSignatureV1 {
    pub key_id: String,
    pub algorithm: String,
    /// Encoding is defined by the verifier implementation (normally base64url).
    pub signature: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SignedCatalogV1 {
    pub signed: CatalogPayloadV1,
    pub signatures: Vec<CatalogSignatureV1>,
}

/// Deterministic signed bytes for `CatalogPayloadV1`. All maps/sets nested in the payload
/// are ordered collections, and catalog entry ordering is validated as strictly increasing.
pub fn canonical_catalog_payload_bytes(
    payload: &CatalogPayloadV1,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(payload)
}

/// Crypto boundary for platform- or TUF-owned key implementations.
pub trait CatalogSignatureVerifier {
    fn is_trusted_key(&self, key_id: &str) -> bool;
    fn verify(&self, key_id: &str, algorithm: &str, message: &[u8], signature: &str) -> bool;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogTrustPolicy {
    pub signature_threshold: usize,
    pub maximum_lifetime_seconds: u64,
    pub maximum_clock_skew_seconds: u64,
}

impl Default for CatalogTrustPolicy {
    fn default() -> Self {
        Self {
            signature_threshold: 2,
            maximum_lifetime_seconds: 31 * 24 * 60 * 60,
            maximum_clock_skew_seconds: 10 * 60,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogTrustState {
    pub highest_version: u64,
    pub accepted_payload_sha256: Option<Sha256Digest>,
    /// Once a revision is published its manifest digest can never be replaced.
    pub immutable_revisions: BTreeMap<PackRevision, Sha256Digest>,
}

#[derive(Clone, Debug)]
pub struct TrustedCatalog {
    payload: CatalogPayloadV1,
    entries: BTreeMap<PackRevision, CatalogEntryV1>,
    payload_digest: Sha256Digest,
}

impl TrustedCatalog {
    pub fn payload(&self) -> &CatalogPayloadV1 {
        &self.payload
    }

    pub fn payload_digest(&self) -> &Sha256Digest {
        &self.payload_digest
    }

    pub fn entry(&self, identity: &PackRevision) -> Option<&CatalogEntryV1> {
        self.entries.get(identity)
    }

    pub fn installable_entry(
        &self,
        identity: &PackRevision,
    ) -> Result<&CatalogEntryV1, CatalogError> {
        let entry = self
            .entries
            .get(identity)
            .ok_or(CatalogError::UnknownRevision)?;
        if entry.revoked {
            return Err(CatalogError::Revoked(
                entry
                    .revocation_reason
                    .clone()
                    .unwrap_or_else(|| "no reason supplied".to_owned()),
            ));
        }
        Ok(entry)
    }
}

pub fn verify_catalog(
    catalog: &SignedCatalogV1,
    verifier: &impl CatalogSignatureVerifier,
    policy: &CatalogTrustPolicy,
    prior: &CatalogTrustState,
    now_unix_seconds: u64,
) -> Result<(TrustedCatalog, CatalogTrustState), CatalogError> {
    if catalog.signed.schema != SIGNED_CATALOG_SCHEMA_V1 {
        return Err(CatalogError::UnsupportedSchema(
            catalog.signed.schema.clone(),
        ));
    }
    if policy.signature_threshold == 0 {
        return Err(CatalogError::ZeroThreshold);
    }
    if catalog.signed.version < prior.highest_version {
        return Err(CatalogError::Rollback {
            trusted: prior.highest_version,
            received: catalog.signed.version,
        });
    }
    if catalog.signed.generated_unix_seconds
        > now_unix_seconds.saturating_add(policy.maximum_clock_skew_seconds)
    {
        return Err(CatalogError::GeneratedInFuture);
    }
    if catalog.signed.expires_unix_seconds <= now_unix_seconds {
        return Err(CatalogError::Expired);
    }
    let lifetime = catalog
        .signed
        .expires_unix_seconds
        .checked_sub(catalog.signed.generated_unix_seconds)
        .ok_or(CatalogError::InvalidLifetime)?;
    if lifetime > policy.maximum_lifetime_seconds {
        return Err(CatalogError::InvalidLifetime);
    }

    // The payload is a struct containing ordered collections; entry ordering is enforced below.
    let message =
        canonical_catalog_payload_bytes(&catalog.signed).map_err(CatalogError::Serialization)?;
    let payload_digest = Sha256Digest::of_bytes(&message);
    if catalog.signed.version == prior.highest_version
        && prior.highest_version != 0
        && prior.accepted_payload_sha256.as_ref() != Some(&payload_digest)
    {
        return Err(CatalogError::VersionEquivocation(catalog.signed.version));
    }

    let mut accepted_keys = BTreeSet::new();
    for signature in &catalog.signatures {
        if !accepted_keys.contains(&signature.key_id)
            && verifier.is_trusted_key(&signature.key_id)
            && verifier.verify(
                &signature.key_id,
                &signature.algorithm,
                &message,
                &signature.signature,
            )
        {
            accepted_keys.insert(signature.key_id.clone());
        }
    }
    if accepted_keys.len() < policy.signature_threshold {
        return Err(CatalogError::SignatureThreshold {
            required: policy.signature_threshold,
            valid: accepted_keys.len(),
        });
    }

    let mut entries = BTreeMap::new();
    let mut immutable = prior.immutable_revisions.clone();
    let mut previous: Option<PackRevision> = None;
    for entry in &catalog.signed.entries {
        entry.manifest.validate().map_err(CatalogError::Manifest)?;
        let identity = entry.manifest.identity();
        if previous.as_ref().is_some_and(|p| p >= &identity) {
            return Err(CatalogError::EntriesNotStrictlySorted);
        }
        previous = Some(identity.clone());
        let actual = entry.manifest.digest().map_err(CatalogError::Manifest)?;
        if actual != entry.manifest_sha256 {
            return Err(CatalogError::ManifestDigestMismatch(identity));
        }
        if let Some(trusted) = immutable.get(&identity) {
            if trusted != &actual {
                return Err(CatalogError::ImmutableRevisionChanged(identity));
            }
        } else {
            immutable.insert(identity.clone(), actual);
        }
        if entry.revoked
            && entry
                .revocation_reason
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
        {
            return Err(CatalogError::RevocationWithoutReason(identity));
        }
        entries.insert(identity, entry.clone());
    }

    let state = CatalogTrustState {
        highest_version: catalog.signed.version,
        accepted_payload_sha256: Some(payload_digest.clone()),
        immutable_revisions: immutable,
    };
    Ok((
        TrustedCatalog {
            payload: catalog.signed.clone(),
            entries,
            payload_digest,
        },
        state,
    ))
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("unsupported catalog schema: {0}")]
    UnsupportedSchema(String),
    #[error("catalog signature threshold cannot be zero")]
    ZeroThreshold,
    #[error("catalog rollback: trusted version {trusted}, received {received}")]
    Rollback { trusted: u64, received: u64 },
    #[error("catalog generation time is unreasonably far in the future")]
    GeneratedInFuture,
    #[error("catalog metadata is expired")]
    Expired,
    #[error("catalog metadata lifetime violates policy")]
    InvalidLifetime,
    #[error("catalog version {0} has conflicting signed payloads")]
    VersionEquivocation(u64),
    #[error("valid signature threshold not met: required {required}, valid {valid}")]
    SignatureThreshold { required: usize, valid: usize },
    #[error("catalog entries are not unique and strictly sorted")]
    EntriesNotStrictlySorted,
    #[error("invalid manifest: {0}")]
    Manifest(crate::ManifestError),
    #[error("catalog manifest digest mismatch: {0:?}")]
    ManifestDigestMismatch(PackRevision),
    #[error("immutable catalog revision changed: {0:?}")]
    ImmutableRevisionChanged(PackRevision),
    #[error("revoked catalog revision lacks a reason: {0:?}")]
    RevocationWithoutReason(PackRevision),
    #[error("unknown catalog revision")]
    UnknownRevision,
    #[error("catalog revision is revoked: {0}")]
    Revoked(String),
    #[error("catalog serialization failed: {0}")]
    Serialization(serde_json::Error),
}
