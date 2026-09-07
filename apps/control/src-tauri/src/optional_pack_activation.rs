//! App-owned activation seam for trusted optional provider packs.
//!
//! Installation deliberately ends at an immutable inactive version. This
//! module completes the remaining native-only sequence without letting a
//! WebView manufacture an attestation, a pack path, or an active pointer:
//!
//! 1. bind the explicit selection to a governor-minted whole-loadout receipt;
//! 2. issue the model-manager challenge for the exact inactive tree;
//! 3. ask the authenticated mouth worker to load that exact tree;
//! 4. authenticate a challenge-bound provider-load-only outcome with an
//!    in-process, OS-random key;
//! 5. let model-manager re-hash the staged tree, consume replay state, and
//!    atomically activate it.
//!
//! This is a provider-load check. It does not claim inference correctness,
//! latency, tracking quality, or lip-sync quality.

use async_trait::async_trait;
use npc_model_manager::{
    canonical_attestation_payload_bytes, AttestationProofV1, AttestationVerificationError,
    AttestedSelfTestOutcomeV1, CatalogTrustDomainV1, InstallState, InstalledPackInventoryV1,
    LoadoutAdmissionV1, OptionalPackLifecycleError, PackRevision, SelfTestAttestationPayloadV1,
    SelfTestAttestationV1, SelfTestAttestationVerifier, SelfTestChallengeV1, Sha256Digest,
    TrustedOptionalPackLifecycleV1, VerifiedRunnerIdentity, SELF_TEST_ATTESTATION_SCHEMA_V1,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use zeroize::Zeroizing;

pub(crate) const YUNET_PROVIDER_LOAD_PACK_ID: &str = "openseeface-yunet640-lm1-mouth-signal";
pub(crate) const YUNET_PROVIDER_LOAD_REVISION: &str = "85aa70fc67582d046e771ea73625182a0d8f7475";
pub(crate) const YUNET_PROVIDER_LOAD_RUNTIME_ABI: &str = "npc-yunet640-lm1-mouth-signal-v1";
pub(crate) const YUNET_PROVIDER_LOAD_BACKEND: &str = "cpu-execution-provider-one-thread";
pub(crate) const YUNET_PROVIDER_LOAD_SUITE: &str = "yunet-lm1-provider-load-only-v1";
const PROVIDER_READY_DETAIL: &str = "admitted_landmark_provider_load_self_test_passed";
const PROVIDER_LOAD_CHALLENGE_LIFETIME_SECONDS: u64 = 5 * 60;
const APP_ATTESTATION_ALGORITHM: &str = "hmac-sha256-app-ephemeral-v1";
const APP_RUNNER_PREFIX: &str = "interactive-npcs-control-mouth-worker";
const APP_RUNNER_TRUST_REVISION: &str = "provider-load-only-v1";
const PROVIDER_LOAD_OUTPUT_DOMAIN: &[u8] = b"npc.provider-load-self-test-output/v1\0";
const HMAC_BLOCK_BYTES: usize = 64;

/// Native clock authority. Production uses the operating-system wall clock;
/// focused tests can supply a deterministic value without weakening the
/// attestation verifier.
pub(crate) trait ProviderLoadSelfTestClockV1: Send + Sync {
    fn unix_seconds(&self) -> Result<u64, ProviderLoadActivationError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SystemProviderLoadSelfTestClockV1;

impl ProviderLoadSelfTestClockV1 for SystemProviderLoadSelfTestClockV1 {
    fn unix_seconds(&self) -> Result<u64, ProviderLoadActivationError> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .map_err(|_| ProviderLoadActivationError::Clock)
    }
}

/// A one-shot, non-serializable request created only after model-manager has
/// issued a challenge and the inactive version has been completely re-hashed.
pub(crate) struct ProviderLoadSelfTestRequestV1 {
    challenge: SelfTestChallengeV1,
    inventory: InstalledPackInventoryV1,
    measured_envelope_sha256: Sha256Digest,
}

impl ProviderLoadSelfTestRequestV1 {
    fn new(
        challenge: SelfTestChallengeV1,
        inventory: InstalledPackInventoryV1,
        measured_envelope_sha256: Sha256Digest,
    ) -> Result<Self, ProviderLoadActivationError> {
        validate_yunet_challenge(&challenge)?;
        validate_inventory_binding(&challenge, &inventory)?;
        Ok(Self {
            challenge,
            inventory,
            measured_envelope_sha256,
        })
    }

    pub(crate) fn challenge(&self) -> &SelfTestChallengeV1 {
        &self.challenge
    }

    pub(crate) fn inventory(&self) -> &InstalledPackInventoryV1 {
        &self.inventory
    }

    pub(crate) fn measured_envelope_sha256(&self) -> &Sha256Digest {
        &self.measured_envelope_sha256
    }

    /// The visual runtime may call this only after `WorkerClient` has verified
    /// the HMAC-authenticated response and its exact ready detail. The supplied
    /// session digest binds the app observation to that fresh worker session
    /// without exposing the session secret or pipe nonce.
    pub(crate) fn authenticated_mouth_worker_ready(
        self,
        worker_process_id: u32,
        worker_process_creation_time: u64,
        authenticated_session_binding_sha256: Sha256Digest,
        response_detail: &str,
    ) -> Result<AuthenticatedProviderLoadObservationV1, ProviderLoadActivationError> {
        if worker_process_id == 0 || worker_process_creation_time == 0 {
            return Err(ProviderLoadActivationError::UnauthenticatedWorkerContext);
        }
        if response_detail != PROVIDER_READY_DETAIL {
            return Err(ProviderLoadActivationError::UnexpectedWorkerResponse);
        }
        Ok(AuthenticatedProviderLoadObservationV1 {
            challenge: self.challenge,
            inventory: self.inventory,
            worker_process_id,
            worker_process_creation_time,
            authenticated_session_binding_sha256,
        })
    }
}

/// Native-only observation returned after the authenticated child loaded the
/// exact detector, landmark model, and runtime from the inactive inventory.
pub(crate) struct AuthenticatedProviderLoadObservationV1 {
    challenge: SelfTestChallengeV1,
    inventory: InstalledPackInventoryV1,
    worker_process_id: u32,
    worker_process_creation_time: u64,
    authenticated_session_binding_sha256: Sha256Digest,
}

#[async_trait]
pub(crate) trait TrustedProviderLoadSelfTestProbeV1: Send + Sync {
    async fn run_provider_load_self_test(
        &self,
        request: ProviderLoadSelfTestRequestV1,
    ) -> Result<AuthenticatedProviderLoadObservationV1, ProviderLoadActivationError>;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedProviderLoadActivationReceiptV1 {
    pub schema_version: u32,
    pub identity: PackRevision,
    pub manifest_sha256: Sha256Digest,
    pub installed_content_tree_sha256: Sha256Digest,
    pub attestation_sha256: Sha256Digest,
    pub provider_load_duration_millis: u64,
    pub trust_domain: CatalogTrustDomainV1,
    pub detail: String,
}

/// Completes activation under one native operation. `admission` is a sealed
/// model-manager receipt, not a deserialized UI assertion. The self-test reads
/// only the immutable inactive inventory selected by the lifecycle challenge.
pub(crate) async fn activate_trusted_yunet_provider_pack_v1(
    lifecycle: &mut TrustedOptionalPackLifecycleV1,
    identity: &PackRevision,
    selection_id: String,
    admission: &LoadoutAdmissionV1,
    probe: &impl TrustedProviderLoadSelfTestProbeV1,
    clock: &impl ProviderLoadSelfTestClockV1,
) -> Result<TrustedProviderLoadActivationReceiptV1, ProviderLoadActivationError> {
    validate_yunet_identity(identity)?;
    let requested_unix_seconds = clock.unix_seconds()?;
    let record = lifecycle
        .manager()
        .record(identity)
        .ok_or(ProviderLoadActivationError::UnknownPack)?;
    if record.state != InstallState::AwaitingSelfTest {
        return Err(ProviderLoadActivationError::InvalidState);
    }
    validate_yunet_manifest_contract(record)?;
    // Re-authorize on every attempt. A failed probe abandons only its nonce;
    // the next explicit activation must still present a fresh governor-minted
    // admission rather than inheriting an older fit decision.
    let mut admitted_models = admission
        .models()
        .iter()
        .filter(|model| model.identity == *identity);
    let admitted_model = admitted_models
        .next()
        .ok_or(ProviderLoadActivationError::MissingNativeAdmissionContext)?;
    if admitted_models.next().is_some() {
        return Err(ProviderLoadActivationError::MissingNativeAdmissionContext);
    }
    let measured_envelope_sha256 = admitted_model.measured_envelope_sha256.clone();
    lifecycle.authorize_activation(identity, selection_id, requested_unix_seconds, admission)?;
    let challenge = lifecycle.manager_mut().issue_self_test_challenge(
        identity,
        YUNET_PROVIDER_LOAD_BACKEND,
        requested_unix_seconds,
        PROVIDER_LOAD_CHALLENGE_LIFETIME_SECONDS,
    )?;
    validate_yunet_challenge(&challenge)?;

    let inventory = match lifecycle.manager().storage().installed_inventory(identity) {
        Ok(Some(inventory)) => inventory,
        Ok(None) => {
            return Err(abandon_failed_challenge(
                lifecycle,
                identity,
                &challenge.nonce,
                ProviderLoadActivationError::InactiveInventoryMissing,
            ));
        }
        Err(error) => {
            return Err(abandon_failed_challenge(
                lifecycle,
                identity,
                &challenge.nonce,
                ProviderLoadActivationError::Storage(error),
            ));
        }
    };
    let request = match ProviderLoadSelfTestRequestV1::new(
        challenge.clone(),
        inventory,
        measured_envelope_sha256,
    ) {
        Ok(request) => request,
        Err(error) => {
            return Err(abandon_failed_challenge(
                lifecycle,
                identity,
                &challenge.nonce,
                error,
            ));
        }
    };
    let authority = match AppPrivateEphemeralSelfTestAuthorityV1::new() {
        Ok(authority) => authority,
        Err(error) => {
            return Err(abandon_failed_challenge(
                lifecycle,
                identity,
                &challenge.nonce,
                error,
            ));
        }
    };
    let started = Instant::now();
    let observation = match probe.run_provider_load_self_test(request).await {
        Ok(observation) => observation,
        Err(error) => {
            return Err(abandon_failed_challenge(
                lifecycle,
                identity,
                &challenge.nonce,
                error,
            ));
        }
    };
    let duration_millis = elapsed_millis(started.elapsed());
    if let Err(error) = validate_observation(&challenge, &observation) {
        return Err(abandon_failed_challenge(
            lifecycle,
            identity,
            &challenge.nonce,
            error,
        ));
    }
    let completed_unix_seconds = match clock.unix_seconds() {
        Ok(completed_unix_seconds) => completed_unix_seconds,
        Err(error) => {
            return Err(abandon_failed_challenge(
                lifecycle,
                identity,
                &challenge.nonce,
                error,
            ));
        }
    };
    if completed_unix_seconds < requested_unix_seconds
        || completed_unix_seconds > challenge.expires_unix_seconds
    {
        return Err(abandon_failed_challenge(
            lifecycle,
            identity,
            &challenge.nonce,
            ProviderLoadActivationError::ChallengeExpired,
        ));
    }

    let output_sha256 = provider_load_output_digest(&observation);
    let attestation = match authority.attest(challenge.clone(), duration_millis, output_sha256) {
        Ok(attestation) => attestation,
        Err(error) => {
            return Err(abandon_failed_challenge(
                lifecycle,
                identity,
                &challenge.nonce,
                error,
            ));
        }
    };
    let attestation_sha256 = match npc_model_manager::attestation_digest(&attestation) {
        Ok(digest) => digest,
        Err(error) => {
            return Err(abandon_failed_challenge(
                lifecycle,
                identity,
                &challenge.nonce,
                ProviderLoadActivationError::AttestationSerialization(error),
            ));
        }
    };
    if let Err(error) = lifecycle.manager_mut().record_self_test_attestation(
        identity,
        attestation,
        &authority,
        completed_unix_seconds,
    ) {
        let primary = ProviderLoadActivationError::Manager(error);
        return Err(abandon_record_failure_if_retryable(
            lifecycle,
            identity,
            &challenge.nonce,
            primary,
        ));
    }
    lifecycle.manager_mut().activate(identity)?;

    let active = lifecycle
        .manager()
        .storage()
        .active_installed_inventory(&identity.pack_id)?
        .filter(|inventory| inventory.identity == *identity)
        .ok_or(ProviderLoadActivationError::ActiveInventoryMismatch)?;
    if active.manifest_sha256 != observation.inventory.manifest_sha256
        || active.content_tree_sha256 != observation.inventory.content_tree_sha256
        || active.total_file_bytes != observation.inventory.total_file_bytes
        || active.files != observation.inventory.files
        || active.artifact_evidence != observation.inventory.artifact_evidence
    {
        return Err(ProviderLoadActivationError::ActiveInventoryMismatch);
    }

    Ok(TrustedProviderLoadActivationReceiptV1 {
        schema_version: 1,
        identity: active.identity,
        manifest_sha256: active.manifest_sha256,
        installed_content_tree_sha256: active.content_tree_sha256,
        attestation_sha256,
        provider_load_duration_millis: duration_millis,
        trust_domain: observation.challenge.catalog.trust_domain,
        detail: "The authenticated mouth worker loaded the exact inactive YuNet and LM1 provider tree. This receipt proves provider load only; inference and visual quality remain separate evidence.".into(),
    })
}

fn abandon_failed_challenge(
    lifecycle: &mut TrustedOptionalPackLifecycleV1,
    identity: &PackRevision,
    nonce: &str,
    primary: ProviderLoadActivationError,
) -> ProviderLoadActivationError {
    match lifecycle.abandon_self_test_challenge(identity, nonce) {
        Ok(()) => primary,
        Err(cleanup) => ProviderLoadActivationError::ChallengeCleanup {
            primary: primary.to_string(),
            cleanup: cleanup.to_string(),
        },
    }
}

fn abandon_record_failure_if_retryable(
    lifecycle: &mut TrustedOptionalPackLifecycleV1,
    identity: &PackRevision,
    nonce: &str,
    primary: ProviderLoadActivationError,
) -> ProviderLoadActivationError {
    if lifecycle.state(identity) == Some(&InstallState::AwaitingSelfTest) {
        abandon_failed_challenge(lifecycle, identity, nonce, primary)
    } else {
        // Attestation validation can quarantine or consume the challenge. In
        // those terminal states, cleanup must not mask the primary failure.
        primary
    }
}

fn validate_yunet_identity(identity: &PackRevision) -> Result<(), ProviderLoadActivationError> {
    if identity.pack_id.as_str() != YUNET_PROVIDER_LOAD_PACK_ID
        || identity.revision.as_str() != YUNET_PROVIDER_LOAD_REVISION
    {
        return Err(ProviderLoadActivationError::UnsupportedPack);
    }
    Ok(())
}

fn validate_yunet_manifest_contract(
    record: &npc_model_manager::PackRecord,
) -> Result<(), ProviderLoadActivationError> {
    validate_yunet_identity(&record.manifest.identity())?;
    if record.manifest.runtime.abi != YUNET_PROVIDER_LOAD_RUNTIME_ABI
        || record.manifest.self_test.kind != YUNET_PROVIDER_LOAD_SUITE
        || !record
            .manifest
            .self_test
            .allowed_runtime_backends
            .contains(YUNET_PROVIDER_LOAD_BACKEND)
        || record.manifest.self_test.expected_output_sha256.is_some()
    {
        return Err(ProviderLoadActivationError::UntruthfulSelfTestContract);
    }
    Ok(())
}

fn validate_yunet_challenge(
    challenge: &SelfTestChallengeV1,
) -> Result<(), ProviderLoadActivationError> {
    validate_yunet_identity(&challenge.identity)?;
    if challenge.runtime_abi != YUNET_PROVIDER_LOAD_RUNTIME_ABI
        || challenge.runtime_backend != YUNET_PROVIDER_LOAD_BACKEND
        || challenge.test_suite_id != YUNET_PROVIDER_LOAD_SUITE
    {
        return Err(ProviderLoadActivationError::UntruthfulSelfTestContract);
    }
    Ok(())
}

fn validate_inventory_binding(
    challenge: &SelfTestChallengeV1,
    inventory: &InstalledPackInventoryV1,
) -> Result<(), ProviderLoadActivationError> {
    let file_count = u64::try_from(inventory.files.len())
        .map_err(|_| ProviderLoadActivationError::InactiveInventoryMismatch)?;
    if inventory.identity != challenge.identity
        || inventory.manifest_sha256 != challenge.manifest_sha256
        || inventory.manifest_sha256 != challenge.staged.manifest_sha256
        || inventory.content_tree_sha256 != challenge.staged.content_tree_sha256
        || inventory.total_file_bytes != challenge.staged.total_file_bytes
        || file_count != challenge.staged.file_count
    {
        return Err(ProviderLoadActivationError::InactiveInventoryMismatch);
    }
    Ok(())
}

fn validate_observation(
    challenge: &SelfTestChallengeV1,
    observation: &AuthenticatedProviderLoadObservationV1,
) -> Result<(), ProviderLoadActivationError> {
    if &observation.challenge != challenge {
        return Err(ProviderLoadActivationError::ChallengeMismatch);
    }
    validate_inventory_binding(challenge, &observation.inventory)
}

fn elapsed_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn provider_load_output_digest(
    observation: &AuthenticatedProviderLoadObservationV1,
) -> Sha256Digest {
    let mut binding = Vec::with_capacity(512);
    binding.extend_from_slice(PROVIDER_LOAD_OUTPUT_DOMAIN);
    binding.extend_from_slice(observation.challenge.nonce.as_bytes());
    binding.push(0);
    binding.extend_from_slice(observation.challenge.identity.pack_id.as_str().as_bytes());
    binding.push(0);
    binding.extend_from_slice(observation.challenge.identity.revision.as_str().as_bytes());
    binding.push(0);
    binding.extend_from_slice(observation.inventory.manifest_sha256.as_str().as_bytes());
    binding.push(0);
    binding.extend_from_slice(
        observation
            .inventory
            .content_tree_sha256
            .as_str()
            .as_bytes(),
    );
    binding.push(0);
    binding.extend_from_slice(&observation.worker_process_id.to_le_bytes());
    binding.extend_from_slice(&observation.worker_process_creation_time.to_le_bytes());
    binding.extend_from_slice(
        observation
            .authenticated_session_binding_sha256
            .as_str()
            .as_bytes(),
    );
    binding.extend_from_slice(PROVIDER_READY_DETAIL.as_bytes());
    Sha256Digest::of_bytes(&binding)
}

struct AppPrivateEphemeralSelfTestAuthorityV1 {
    secret: Zeroizing<[u8; 32]>,
    runner_id: String,
}

impl AppPrivateEphemeralSelfTestAuthorityV1 {
    fn new() -> Result<Self, ProviderLoadActivationError> {
        let mut secret = Zeroizing::new([0_u8; 32]);
        getrandom::fill(secret.as_mut()).map_err(|_| ProviderLoadActivationError::Random)?;
        let public_instance = Sha256::digest(secret.as_ref());
        let runner_id = format!("{APP_RUNNER_PREFIX}-{}", hex::encode(&public_instance[..8]));
        Ok(Self { secret, runner_id })
    }

    fn attest(
        &self,
        challenge: SelfTestChallengeV1,
        duration_millis: u64,
        output_sha256: Sha256Digest,
    ) -> Result<SelfTestAttestationV1, ProviderLoadActivationError> {
        let signed = SelfTestAttestationPayloadV1 {
            schema: SELF_TEST_ATTESTATION_SCHEMA_V1.to_owned(),
            challenge,
            runner_id: self.runner_id.clone(),
            outcome: AttestedSelfTestOutcomeV1 {
                passed: true,
                duration_millis,
                output_sha256: Some(output_sha256),
                diagnostic_code: None,
            },
        };
        let payload = canonical_attestation_payload_bytes(&signed)?;
        Ok(SelfTestAttestationV1 {
            signed,
            proof: AttestationProofV1 {
                algorithm: APP_ATTESTATION_ALGORITHM.to_owned(),
                value: hex::encode(hmac_sha256(self.secret.as_ref(), &payload)),
            },
        })
    }
}

impl SelfTestAttestationVerifier for AppPrivateEphemeralSelfTestAuthorityV1 {
    fn verify(
        &self,
        claimed_runner_id: &str,
        proof: &AttestationProofV1,
        canonical_payload: &[u8],
    ) -> Result<VerifiedRunnerIdentity, AttestationVerificationError> {
        if claimed_runner_id != self.runner_id {
            return Err(AttestationVerificationError::UnknownRunner);
        }
        if proof.algorithm != APP_ATTESTATION_ALGORITHM {
            return Err(AttestationVerificationError::UnsupportedAlgorithm);
        }
        let presented =
            hex::decode(&proof.value).map_err(|_| AttestationVerificationError::InvalidProof)?;
        let expected = hmac_sha256(self.secret.as_ref(), canonical_payload);
        if presented.len() != expected.len() || !constant_time_equal(&presented, &expected) {
            return Err(AttestationVerificationError::InvalidProof);
        }
        Ok(VerifiedRunnerIdentity {
            runner_id: self.runner_id.clone(),
            trust_revision: APP_RUNNER_TRUST_REVISION.to_owned(),
        })
    }
}

fn hmac_sha256(key: &[u8], payload: &[u8]) -> [u8; 32] {
    let mut key_block = [0_u8; HMAC_BLOCK_BYTES];
    if key.len() > HMAC_BLOCK_BYTES {
        key_block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36_u8; HMAC_BLOCK_BYTES];
    let mut outer_pad = [0x5c_u8; HMAC_BLOCK_BYTES];
    for index in 0..HMAC_BLOCK_BYTES {
        inner_pad[index] ^= key_block[index];
        outer_pad[index] ^= key_block[index];
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(payload);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner);
    outer.finalize().into()
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[derive(Debug, Error)]
pub(crate) enum ProviderLoadActivationError {
    #[error("the exact optional provider pack is unknown to the active lifecycle")]
    UnknownPack,
    #[error("the optional provider pack is not awaiting its first self-test")]
    InvalidState,
    #[error("this activation seam supports only the exact admitted YuNet and LM1 revision")]
    UnsupportedPack,
    #[error("the signed manifest does not declare the provider-load-only contract")]
    UntruthfulSelfTestContract,
    #[error("the immutable inactive inventory is unavailable")]
    InactiveInventoryMissing,
    #[error("the inactive inventory differs from the lifecycle challenge")]
    InactiveInventoryMismatch,
    #[error("the provider-load observation belongs to another challenge")]
    ChallengeMismatch,
    #[error("the authenticated provider-load challenge expired")]
    ChallengeExpired,
    #[error("the mouth worker did not return its exact admitted provider-ready response")]
    UnexpectedWorkerResponse,
    #[error("the mouth worker process/session context was not authenticated")]
    UnauthenticatedWorkerContext,
    #[error("activation completed, but the active installed inventory differs from the attested inactive tree; refresh lifecycle state before retrying")]
    ActiveInventoryMismatch,
    #[error("the current native whole-loadout setup admission does not uniquely bind this pack")]
    MissingNativeAdmissionContext,
    #[error("authenticated mouth-worker provider-load self-test failed: {0}")]
    Worker(String),
    #[error("the provider loaded, but the fresh mouth-worker could not be fully shut down: {0}")]
    WorkerCleanup(String),
    #[error("provider-load activation failed and its nonce could not be abandoned: {primary}; cleanup: {cleanup}")]
    ChallengeCleanup { primary: String, cleanup: String },
    #[error("operating-system randomness is unavailable")]
    Random,
    #[error("the trusted wall clock is unavailable")]
    Clock,
    #[error("optional-pack lifecycle rejected activation: {0}")]
    Lifecycle(#[from] OptionalPackLifecycleError),
    #[error("model-manager rejected activation: {0}")]
    Manager(#[from] npc_model_manager::ManagerError),
    #[error("optional-pack storage rejected activation: {0}")]
    Storage(#[from] npc_model_manager::StorageError),
    #[error("attestation serialization failed: {0}")]
    AttestationSerialization(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use npc_model_manager::{
        CatalogInstallBindingV1, InstalledFileV1, PackId, Revision, StagedContentBindingV1,
    };
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn identity() -> PackRevision {
        PackRevision {
            pack_id: PackId::parse(YUNET_PROVIDER_LOAD_PACK_ID).expect("pack id"),
            revision: Revision::parse(YUNET_PROVIDER_LOAD_REVISION).expect("revision"),
        }
    }

    fn challenge() -> SelfTestChallengeV1 {
        let identity = identity();
        SelfTestChallengeV1 {
            schema: npc_model_manager::SELF_TEST_CHALLENGE_SCHEMA_V1.to_owned(),
            identity: identity.clone(),
            manifest_sha256: Sha256Digest::of_bytes(b"manifest"),
            catalog: CatalogInstallBindingV1 {
                catalog_payload_sha256: Sha256Digest::of_bytes(b"catalog"),
                catalog_version: 7,
                trust_domain: CatalogTrustDomainV1::LocalReviewDevOnly,
            },
            staged: StagedContentBindingV1 {
                identity,
                transaction_id: "provider-load-test-transaction".into(),
                manifest_sha256: Sha256Digest::of_bytes(b"manifest"),
                content_tree_sha256: Sha256Digest::of_bytes(b"tree"),
                file_count: 1,
                total_file_bytes: 4,
            },
            runtime_abi: YUNET_PROVIDER_LOAD_RUNTIME_ABI.into(),
            runtime_backend: YUNET_PROVIDER_LOAD_BACKEND.into(),
            test_suite_id: YUNET_PROVIDER_LOAD_SUITE.into(),
            test_suite_revision: "2026-09-05".into(),
            nonce: "ab".repeat(32),
            issued_unix_seconds: 100,
            expires_unix_seconds: 400,
        }
    }

    fn inventory() -> InstalledPackInventoryV1 {
        InstalledPackInventoryV1 {
            identity: identity(),
            root: PathBuf::from("C:/private/inactive-pack"),
            manifest_sha256: Sha256Digest::of_bytes(b"manifest"),
            content_tree_sha256: Sha256Digest::of_bytes(b"tree"),
            total_file_bytes: 4,
            artifact_evidence: BTreeMap::new(),
            files: vec![InstalledFileV1 {
                relative_path: "models/model.onnx".into(),
                size_bytes: 4,
                sha256: Sha256Digest::of_bytes(b"data"),
            }],
        }
    }

    fn observation() -> AuthenticatedProviderLoadObservationV1 {
        ProviderLoadSelfTestRequestV1::new(
            challenge(),
            inventory(),
            Sha256Digest::of_bytes(b"measurement"),
        )
        .expect("request")
        .authenticated_mouth_worker_ready(
            42,
            9_001,
            Sha256Digest::of_bytes(b"authenticated worker session"),
            PROVIDER_READY_DETAIL,
        )
        .expect("authenticated observation")
    }

    #[test]
    fn app_private_attestation_accepts_exact_payload_and_rejects_tampering() {
        let authority = AppPrivateEphemeralSelfTestAuthorityV1::new().expect("authority");
        let observation = observation();
        let mut attestation = authority
            .attest(
                observation.challenge.clone(),
                17,
                provider_load_output_digest(&observation),
            )
            .expect("attestation");
        let exact = canonical_attestation_payload_bytes(&attestation.signed).expect("payload");
        assert!(authority
            .verify(&attestation.signed.runner_id, &attestation.proof, &exact)
            .is_ok());

        attestation.signed.outcome.duration_millis = 18;
        let changed =
            canonical_attestation_payload_bytes(&attestation.signed).expect("changed payload");
        assert_eq!(
            authority.verify(&attestation.signed.runner_id, &attestation.proof, &changed),
            Err(AttestationVerificationError::InvalidProof)
        );
    }

    #[test]
    fn inventory_tree_must_match_every_staged_challenge_bound() {
        let mut changed = inventory();
        changed.total_file_bytes += 1;
        assert!(matches!(
            ProviderLoadSelfTestRequestV1::new(
                challenge(),
                changed,
                Sha256Digest::of_bytes(b"measurement")
            ),
            Err(ProviderLoadActivationError::InactiveInventoryMismatch)
        ));
    }

    #[test]
    fn worker_ready_observation_rejects_wrong_response_and_missing_process_identity() {
        assert!(matches!(
            ProviderLoadSelfTestRequestV1::new(
                challenge(),
                inventory(),
                Sha256Digest::of_bytes(b"measurement")
            )
            .expect("request")
            .authenticated_mouth_worker_ready(
                42,
                9_001,
                Sha256Digest::of_bytes(b"session"),
                "ready"
            ),
            Err(ProviderLoadActivationError::UnexpectedWorkerResponse)
        ));
        assert!(matches!(
            ProviderLoadSelfTestRequestV1::new(
                challenge(),
                inventory(),
                Sha256Digest::of_bytes(b"measurement")
            )
            .expect("request")
            .authenticated_mouth_worker_ready(
                0,
                9_001,
                Sha256Digest::of_bytes(b"session"),
                PROVIDER_READY_DETAIL
            ),
            Err(ProviderLoadActivationError::UnauthenticatedWorkerContext)
        ));
    }

    #[test]
    fn challenge_cannot_swap_provider_or_runtime_contract() {
        let mut changed = challenge();
        changed.runtime_backend = "cuda".into();
        assert!(matches!(
            ProviderLoadSelfTestRequestV1::new(
                changed,
                inventory(),
                Sha256Digest::of_bytes(b"measurement")
            ),
            Err(ProviderLoadActivationError::UntruthfulSelfTestContract)
        ));
    }

    #[test]
    fn ephemeral_authority_rejects_another_operation_runner_and_algorithm() {
        let first = AppPrivateEphemeralSelfTestAuthorityV1::new().expect("first");
        let second = AppPrivateEphemeralSelfTestAuthorityV1::new().expect("second");
        let observation = observation();
        let attestation = first
            .attest(
                observation.challenge.clone(),
                1,
                provider_load_output_digest(&observation),
            )
            .expect("attestation");
        let payload = canonical_attestation_payload_bytes(&attestation.signed).expect("payload");
        assert_eq!(
            second.verify(&attestation.signed.runner_id, &attestation.proof, &payload),
            Err(AttestationVerificationError::UnknownRunner)
        );
        let mut wrong_algorithm = attestation.proof;
        wrong_algorithm.algorithm = "sha256".into();
        assert_eq!(
            first.verify(&attestation.signed.runner_id, &wrong_algorithm, &payload),
            Err(AttestationVerificationError::UnsupportedAlgorithm)
        );
    }
}
