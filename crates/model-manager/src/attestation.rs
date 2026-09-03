use crate::{PackRevision, Sha256Digest, StagedContentBindingV1};
use serde::{Deserialize, Serialize};
#[cfg(any(test, feature = "test-support"))]
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const SELF_TEST_ATTESTATION_SCHEMA_V1: &str = "npc.self-test-attestation/v1";
pub const SELF_TEST_CHALLENGE_SCHEMA_V1: &str = "npc.self-test-challenge/v1";
pub const MAX_SELF_TEST_ATTESTATION_LIFETIME_SECONDS: u64 = 15 * 60;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogInstallBindingV1 {
    pub catalog_payload_sha256: Sha256Digest,
    pub catalog_version: u64,
    /// Release and local-review catalogs are deliberately different trust
    /// domains. Dev evidence is cryptographically testable, but can never be
    /// serialized or displayed as release-threshold evidence.
    pub trust_domain: CatalogTrustDomainV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogTrustDomainV1 {
    ReleaseThreshold,
    LocalReviewDevOnly,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SelfTestChallengeV1 {
    pub schema: String,
    pub identity: PackRevision,
    pub manifest_sha256: Sha256Digest,
    pub catalog: CatalogInstallBindingV1,
    pub staged: StagedContentBindingV1,
    pub runtime_abi: String,
    pub runtime_backend: String,
    pub test_suite_id: String,
    pub test_suite_revision: String,
    /// 256-bit OS-random challenge, lowercase hex encoded.
    pub nonce: String,
    pub issued_unix_seconds: u64,
    pub expires_unix_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AttestedSelfTestOutcomeV1 {
    pub passed: bool,
    pub duration_millis: u64,
    pub output_sha256: Option<Sha256Digest>,
    pub diagnostic_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SelfTestAttestationPayloadV1 {
    pub schema: String,
    pub challenge: SelfTestChallengeV1,
    pub runner_id: String,
    pub outcome: AttestedSelfTestOutcomeV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AttestationProofV1 {
    pub algorithm: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SelfTestAttestationV1 {
    pub signed: SelfTestAttestationPayloadV1,
    pub proof: AttestationProofV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedRunnerIdentity {
    pub runner_id: String,
    pub trust_revision: String,
}

pub trait SelfTestAttestationVerifier {
    /// Verifies the proof over `canonical_payload` and returns the trusted runner identity.
    /// Implementations own runner allowlists, key rotation, and signature/MAC policy.
    fn verify(
        &self,
        claimed_runner_id: &str,
        proof: &AttestationProofV1,
        canonical_payload: &[u8],
    ) -> Result<VerifiedRunnerIdentity, AttestationVerificationError>;
}

pub fn canonical_attestation_payload_bytes(
    payload: &SelfTestAttestationPayloadV1,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(payload)
}

pub fn attestation_digest(
    attestation: &SelfTestAttestationV1,
) -> Result<Sha256Digest, serde_json::Error> {
    serde_json::to_vec(attestation).map(|bytes| Sha256Digest::of_bytes(&bytes))
}

pub fn validate_challenge(challenge: &SelfTestChallengeV1) -> Result<(), AttestationError> {
    if challenge.schema != SELF_TEST_CHALLENGE_SCHEMA_V1 {
        return Err(AttestationError::UnsupportedChallengeSchema(
            challenge.schema.clone(),
        ));
    }
    if challenge.nonce.len() != 64
        || !challenge
            .nonce
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(AttestationError::InvalidNonce);
    }
    if challenge.expires_unix_seconds <= challenge.issued_unix_seconds
        || challenge.expires_unix_seconds - challenge.issued_unix_seconds
            > MAX_SELF_TEST_ATTESTATION_LIFETIME_SECONDS
    {
        return Err(AttestationError::InvalidLifetime);
    }
    validate_token("runtime backend", &challenge.runtime_backend, 128)?;
    validate_token("test suite id", &challenge.test_suite_id, 128)?;
    validate_token("test suite revision", &challenge.test_suite_revision, 128)?;
    Ok(())
}

pub(crate) fn generate_attestation_nonce() -> Result<String, AttestationError> {
    let mut nonce = [0_u8; 32];
    getrandom::fill(&mut nonce).map_err(|error| AttestationError::Random(error.to_string()))?;
    Ok(hex::encode(nonce))
}

fn validate_token(
    label: &'static str,
    value: &str,
    maximum: usize,
) -> Result<(), AttestationError> {
    if value.is_empty()
        || value.len() > maximum
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':'))
    {
        return Err(AttestationError::InvalidToken(label));
    }
    Ok(())
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Debug)]
pub struct MockAttestedExecutorVerifier {
    runner_id: String,
    secret: [u8; 32],
}

#[cfg(any(test, feature = "test-support"))]
impl MockAttestedExecutorVerifier {
    pub const ALGORITHM: &'static str = "mock-sha256-test-only";

    pub fn new(runner_id: impl Into<String>, secret: [u8; 32]) -> Self {
        Self {
            runner_id: runner_id.into(),
            secret,
        }
    }

    pub fn attest(
        &self,
        challenge: SelfTestChallengeV1,
        outcome: AttestedSelfTestOutcomeV1,
    ) -> SelfTestAttestationV1 {
        let signed = SelfTestAttestationPayloadV1 {
            schema: SELF_TEST_ATTESTATION_SCHEMA_V1.to_owned(),
            challenge,
            runner_id: self.runner_id.clone(),
            outcome,
        };
        let bytes = canonical_attestation_payload_bytes(&signed).unwrap_or_default();
        SelfTestAttestationV1 {
            proof: AttestationProofV1 {
                algorithm: Self::ALGORITHM.to_owned(),
                value: self.mac(&bytes),
            },
            signed,
        }
    }

    fn mac(&self, bytes: &[u8]) -> String {
        let mut hash = Sha256::new();
        hash.update(b"npc-mock-attestation\0");
        hash.update(self.secret);
        hash.update(bytes);
        hex::encode(hash.finalize())
    }
}

#[cfg(any(test, feature = "test-support"))]
impl SelfTestAttestationVerifier for MockAttestedExecutorVerifier {
    fn verify(
        &self,
        claimed_runner_id: &str,
        proof: &AttestationProofV1,
        canonical_payload: &[u8],
    ) -> Result<VerifiedRunnerIdentity, AttestationVerificationError> {
        if claimed_runner_id != self.runner_id {
            return Err(AttestationVerificationError::UnknownRunner);
        }
        if proof.algorithm != Self::ALGORITHM || proof.value != self.mac(canonical_payload) {
            return Err(AttestationVerificationError::InvalidProof);
        }
        Ok(VerifiedRunnerIdentity {
            runner_id: self.runner_id.clone(),
            trust_revision: "mock-v1".to_owned(),
        })
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AttestationVerificationError {
    #[error("runner is not trusted")]
    UnknownRunner,
    #[error("attestation proof is invalid")]
    InvalidProof,
    #[error("attestation proof algorithm is unsupported")]
    UnsupportedAlgorithm,
    #[error("attestation verifier failed: {0}")]
    VerifierFailure(String),
}

#[derive(Debug, Error)]
pub enum AttestationError {
    #[error("unsupported self-test challenge schema: {0}")]
    UnsupportedChallengeSchema(String),
    #[error("invalid self-test challenge nonce")]
    InvalidNonce,
    #[error("invalid self-test challenge lifetime")]
    InvalidLifetime,
    #[error("invalid {0}")]
    InvalidToken(&'static str),
    #[error("operating-system randomness failed: {0}")]
    Random(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PackId, Revision};

    #[test]
    fn deterministic_mock_is_explicitly_test_only_and_detects_payload_change() {
        let runner = MockAttestedExecutorVerifier::new("mock-runner", [9_u8; 32]);
        let digest = Sha256Digest::of_bytes(b"fixture");
        let identity = PackRevision {
            pack_id: PackId::parse("mock.fixture").expect("pack id"),
            revision: Revision::parse("r1").expect("revision"),
        };
        let challenge = SelfTestChallengeV1 {
            schema: SELF_TEST_CHALLENGE_SCHEMA_V1.to_owned(),
            identity: identity.clone(),
            manifest_sha256: digest.clone(),
            catalog: CatalogInstallBindingV1 {
                catalog_payload_sha256: digest.clone(),
                catalog_version: 1,
                trust_domain: CatalogTrustDomainV1::ReleaseThreshold,
            },
            staged: StagedContentBindingV1 {
                identity,
                transaction_id: "mock-transaction".to_owned(),
                manifest_sha256: digest.clone(),
                content_tree_sha256: digest,
                file_count: 1,
                total_file_bytes: 1,
            },
            runtime_abi: "mock-abi".to_owned(),
            runtime_backend: "mock-backend".to_owned(),
            test_suite_id: "mock-suite".to_owned(),
            test_suite_revision: "mock-suite-v1".to_owned(),
            nonce: "01".repeat(32),
            issued_unix_seconds: 10,
            expires_unix_seconds: 20,
        };
        let mut attestation = runner.attest(
            challenge,
            AttestedSelfTestOutcomeV1 {
                passed: true,
                duration_millis: 1,
                output_sha256: None,
                diagnostic_code: None,
            },
        );
        let bytes = canonical_attestation_payload_bytes(&attestation.signed).expect("serialize");
        assert!(runner
            .verify(&attestation.signed.runner_id, &attestation.proof, &bytes)
            .is_ok());
        attestation.signed.outcome.duration_millis = 2;
        let tampered = canonical_attestation_payload_bytes(&attestation.signed).expect("serialize");
        assert_eq!(
            runner.verify(&attestation.signed.runner_id, &attestation.proof, &tampered),
            Err(AttestationVerificationError::InvalidProof)
        );
    }
}
