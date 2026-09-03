use crate::{
    CatalogSignatureV1, CatalogSignatureVerifier, ModelPackKindV1, PackRevision, Sha256Digest,
};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const MEASURED_RESOURCE_ENVELOPE_SCHEMA_V1: &str = "npc.measured-resource-envelope/v1";
pub const RESOURCE_GOVERNOR_POLICY_SCHEMA_V1: &str = "npc.resource-governor-policy/v1";
const BASIS_POINTS: u64 = 10_000;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidencyModeV1 {
    CpuResident,
    GpuResident,
    CpuResidentGpuCold,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlacementMeasurementV1 {
    pub resident_ram_bytes: u64,
    pub p99_total_ram_bytes: u64,
    pub resident_vram_bytes: u64,
    /// Additional transient device memory beyond resident VRAM.
    pub p99_workspace_vram_bytes: u64,
    pub p99_load_millis: u64,
    /// Measured p99 cost of restoring this placement after it was released.
    /// This is deliberately distinct from first-load time: residency policy
    /// must not pretend a planning estimate or a cold-install benchmark is a
    /// measured reload cost.
    pub p99_reload_millis: u64,
    pub p99_operation_millis: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MeasuredResourceEnvelopePayloadV1 {
    pub schema: String,
    pub report_id: String,
    /// Monotonic per model/device/runtime measurement sequence.
    pub sequence: u64,
    pub measured_unix_seconds: u64,
    pub expires_unix_seconds: u64,
    pub device_fingerprint_sha256: Sha256Digest,
    pub identity: PackRevision,
    pub manifest_sha256: Sha256Digest,
    /// Signed capability binding prevents a caller from relabeling, for
    /// example, two LLM envelopes as different roles to bypass loadout rules.
    pub capability: ModelPackKindV1,
    pub benchmark_suite_revision: String,
    pub runtime: String,
    pub runtime_revision: String,
    pub backend: String,
    pub sample_count: u32,
    pub placements: BTreeMap<ResidencyModeV1, PlacementMeasurementV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SignedMeasuredResourceEnvelopeV1 {
    pub signed: MeasuredResourceEnvelopePayloadV1,
    pub signatures: Vec<CatalogSignatureV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeasurementTrustPolicyV1 {
    pub signature_threshold: usize,
    pub minimum_samples: u32,
    pub maximum_lifetime_seconds: u64,
    pub maximum_clock_skew_seconds: u64,
}

impl Default for MeasurementTrustPolicyV1 {
    fn default() -> Self {
        Self {
            signature_threshold: 1,
            minimum_samples: 20,
            maximum_lifetime_seconds: 30 * 24 * 60 * 60,
            maximum_clock_skew_seconds: 10 * 60,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MeasurementTrustStateV1 {
    pub accepted: BTreeMap<MeasurementStreamKeyV1, MeasurementVersionV1>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct MeasurementStreamKeyV1 {
    pub identity: PackRevision,
    pub device_fingerprint_sha256: Sha256Digest,
    pub runtime: String,
    pub backend: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MeasurementVersionV1 {
    pub sequence: u64,
    pub payload_sha256: Sha256Digest,
}

/// Private-field verified evidence. Resource admission accepts this type, never
/// a manifest estimate or caller-supplied byte totals.
#[derive(Clone, Debug)]
pub struct QualifiedResourceEnvelopeV1 {
    payload: MeasuredResourceEnvelopePayloadV1,
    payload_sha256: Sha256Digest,
}

impl QualifiedResourceEnvelopeV1 {
    pub fn payload(&self) -> &MeasuredResourceEnvelopePayloadV1 {
        &self.payload
    }

    pub fn payload_sha256(&self) -> &Sha256Digest {
        &self.payload_sha256
    }

    pub fn identity(&self) -> &PackRevision {
        &self.payload.identity
    }

    pub fn manifest_sha256(&self) -> &Sha256Digest {
        &self.payload.manifest_sha256
    }

    pub fn placement(&self, mode: &ResidencyModeV1) -> Option<&PlacementMeasurementV1> {
        self.payload.placements.get(mode)
    }
}

pub fn canonical_measured_resource_envelope_bytes(
    payload: &MeasuredResourceEnvelopePayloadV1,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(payload)
}

pub fn verify_measured_resource_envelope(
    envelope: &SignedMeasuredResourceEnvelopeV1,
    verifier: &impl CatalogSignatureVerifier,
    policy: &MeasurementTrustPolicyV1,
    prior: &MeasurementTrustStateV1,
    now_unix_seconds: u64,
) -> Result<(QualifiedResourceEnvelopeV1, MeasurementTrustStateV1), ResourceEnvelopeError> {
    validate_measurement_payload(&envelope.signed, policy, now_unix_seconds)?;
    if policy.signature_threshold == 0 {
        return Err(ResourceEnvelopeError::ZeroSignatureThreshold);
    }
    let bytes = canonical_measured_resource_envelope_bytes(&envelope.signed)
        .map_err(ResourceEnvelopeError::Serialization)?;
    let digest = Sha256Digest::of_bytes(&bytes);
    let stream = MeasurementStreamKeyV1 {
        identity: envelope.signed.identity.clone(),
        device_fingerprint_sha256: envelope.signed.device_fingerprint_sha256.clone(),
        runtime: envelope.signed.runtime.clone(),
        backend: envelope.signed.backend.clone(),
    };
    if let Some(version) = prior.accepted.get(&stream) {
        if envelope.signed.sequence < version.sequence {
            return Err(ResourceEnvelopeError::Rollback {
                trusted: version.sequence,
                received: envelope.signed.sequence,
            });
        }
        if envelope.signed.sequence == version.sequence && digest != version.payload_sha256 {
            return Err(ResourceEnvelopeError::SequenceEquivocation(
                envelope.signed.sequence,
            ));
        }
    }
    let mut accepted_keys = BTreeSet::new();
    for signature in &envelope.signatures {
        if !accepted_keys.contains(&signature.key_id)
            && verifier.is_trusted_key(&signature.key_id)
            && verifier.verify(
                &signature.key_id,
                &signature.algorithm,
                &bytes,
                &signature.signature,
            )
        {
            accepted_keys.insert(signature.key_id.clone());
        }
    }
    if accepted_keys.len() < policy.signature_threshold {
        return Err(ResourceEnvelopeError::SignatureThreshold {
            required: policy.signature_threshold,
            valid: accepted_keys.len(),
        });
    }
    let mut next = prior.clone();
    next.accepted.insert(
        stream,
        MeasurementVersionV1 {
            sequence: envelope.signed.sequence,
            payload_sha256: digest.clone(),
        },
    );
    Ok((
        QualifiedResourceEnvelopeV1 {
            payload: envelope.signed.clone(),
            payload_sha256: digest,
        },
        next,
    ))
}

fn validate_measurement_payload(
    payload: &MeasuredResourceEnvelopePayloadV1,
    policy: &MeasurementTrustPolicyV1,
    now_unix_seconds: u64,
) -> Result<(), ResourceEnvelopeError> {
    if payload.schema != MEASURED_RESOURCE_ENVELOPE_SCHEMA_V1 {
        return Err(ResourceEnvelopeError::UnsupportedSchema(
            payload.schema.clone(),
        ));
    }
    validate_token("report_id", &payload.report_id, 8, 128)?;
    validate_token(
        "benchmark_suite_revision",
        &payload.benchmark_suite_revision,
        1,
        128,
    )?;
    validate_token("runtime", &payload.runtime, 1, 128)?;
    validate_token("runtime_revision", &payload.runtime_revision, 1, 128)?;
    validate_token("backend", &payload.backend, 1, 128)?;
    if payload.sequence == 0
        || payload.measured_unix_seconds == 0
        || payload.sample_count < policy.minimum_samples
        || payload.placements.is_empty()
    {
        return Err(ResourceEnvelopeError::IncompleteMeasurement);
    }
    if payload.measured_unix_seconds
        > now_unix_seconds.saturating_add(policy.maximum_clock_skew_seconds)
    {
        return Err(ResourceEnvelopeError::MeasuredInFuture);
    }
    if payload.expires_unix_seconds <= now_unix_seconds {
        return Err(ResourceEnvelopeError::Expired);
    }
    let lifetime = payload
        .expires_unix_seconds
        .checked_sub(payload.measured_unix_seconds)
        .ok_or(ResourceEnvelopeError::InvalidLifetime)?;
    if lifetime > policy.maximum_lifetime_seconds {
        return Err(ResourceEnvelopeError::InvalidLifetime);
    }
    for (mode, measurement) in &payload.placements {
        if measurement.resident_ram_bytes == 0
            || measurement.p99_total_ram_bytes < measurement.resident_ram_bytes
            || measurement.p99_load_millis == 0
            || measurement.p99_reload_millis == 0
            || measurement.p99_operation_millis == 0
        {
            return Err(ResourceEnvelopeError::InvalidPlacementMeasurement(
                mode.clone(),
            ));
        }
        match mode {
            ResidencyModeV1::GpuResident if measurement.resident_vram_bytes == 0 => {
                return Err(ResourceEnvelopeError::InvalidPlacementMeasurement(
                    mode.clone(),
                ));
            }
            ResidencyModeV1::CpuResident | ResidencyModeV1::CpuResidentGpuCold
                if measurement.resident_vram_bytes != 0
                    || measurement.p99_workspace_vram_bytes != 0 =>
            {
                return Err(ResourceEnvelopeError::InvalidPlacementMeasurement(
                    mode.clone(),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResourceGovernorPolicyV1 {
    pub schema: String,
    /// Soft ceilings are fractions of the OS budget/physical RAM in basis points.
    pub vram_soft_ceiling_basis_points: u16,
    pub ram_soft_ceiling_basis_points: u16,
    pub minimum_vram_safety_bytes: u64,
    pub proportional_vram_safety_basis_points: u16,
    pub minimum_ram_safety_bytes: u64,
    pub maximum_snapshot_age_millis: u64,
    pub keep_warm_millis: u64,
    pub unload_ttl_millis: u64,
}

impl Default for ResourceGovernorPolicyV1 {
    fn default() -> Self {
        Self {
            schema: RESOURCE_GOVERNOR_POLICY_SCHEMA_V1.to_owned(),
            vram_soft_ceiling_basis_points: 9_000,
            ram_soft_ceiling_basis_points: 8_500,
            minimum_vram_safety_bytes: 1_500_000_000,
            proportional_vram_safety_basis_points: 1_500,
            minimum_ram_safety_bytes: 2_000_000_000,
            maximum_snapshot_age_millis: 2_000,
            keep_warm_millis: 30_000,
            unload_ttl_millis: 120_000,
        }
    }
}

impl ResourceGovernorPolicyV1 {
    pub fn validate(&self) -> Result<(), ResourceAdmissionError> {
        if self.schema != RESOURCE_GOVERNOR_POLICY_SCHEMA_V1 {
            return Err(ResourceAdmissionError::UnsupportedPolicySchema(
                self.schema.clone(),
            ));
        }
        if self.vram_soft_ceiling_basis_points == 0
            || u64::from(self.vram_soft_ceiling_basis_points) > BASIS_POINTS
            || self.ram_soft_ceiling_basis_points == 0
            || u64::from(self.ram_soft_ceiling_basis_points) > BASIS_POINTS
            || u64::from(self.proportional_vram_safety_basis_points) > BASIS_POINTS
            || self.maximum_snapshot_age_millis == 0
            || self.keep_warm_millis > self.unload_ttl_millis
        {
            return Err(ResourceAdmissionError::InvalidPolicy);
        }
        Ok(())
    }
}

/// Snapshot captured before loading the requested local loadout. Desktop and
/// game usage are separate live inputs; `game_reserve_vram_bytes` is the user
/// or profile floor and may exceed the game's current usage.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LiveResourceSnapshotV1 {
    pub captured_monotonic_millis: u64,
    pub device_fingerprint_sha256: Sha256Digest,
    pub physical_vram_bytes: u64,
    pub os_vram_budget_bytes: u64,
    pub desktop_resident_vram_bytes: u64,
    pub game_resident_vram_bytes: u64,
    pub game_reserve_vram_bytes: u64,
    pub physical_ram_bytes: u64,
    pub available_ram_bytes: u64,
    pub game_additional_reserve_ram_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct LoadoutModelRequestV1<'a> {
    pub role: ModelPackKindV1,
    pub mode: ResidencyModeV1,
    pub envelope: &'a QualifiedResourceEnvelopeV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdmittedModelV1 {
    pub role: ModelPackKindV1,
    pub identity: PackRevision,
    pub manifest_sha256: Sha256Digest,
    pub measured_envelope_sha256: Sha256Digest,
    pub mode: ResidencyModeV1,
    pub measurement: PlacementMeasurementV1,
}

// Deliberately not `Deserialize`: only `ResourceGovernorV1::admit` may mint an
// admission receipt.  Callers must not be able to fabricate one from JSON and
// use it as measured-local activation evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LoadoutAdmissionV1 {
    snapshot_monotonic_millis: u64,
    device_fingerprint_sha256: Sha256Digest,
    models: Vec<AdmittedModelV1>,
    vram_soft_ceiling_bytes: u64,
    protected_desktop_and_game_vram_bytes: u64,
    selected_peak_vram_bytes: u64,
    vram_safety_bytes: u64,
    projected_total_vram_bytes: u64,
    ram_soft_ceiling_bytes: u64,
    selected_peak_ram_bytes: u64,
    ram_safety_bytes: u64,
}

impl LoadoutAdmissionV1 {
    pub fn snapshot_monotonic_millis(&self) -> u64 {
        self.snapshot_monotonic_millis
    }

    pub fn device_fingerprint_sha256(&self) -> &Sha256Digest {
        &self.device_fingerprint_sha256
    }

    pub fn models(&self) -> &[AdmittedModelV1] {
        &self.models
    }

    pub fn vram_soft_ceiling_bytes(&self) -> u64 {
        self.vram_soft_ceiling_bytes
    }

    pub fn protected_desktop_and_game_vram_bytes(&self) -> u64 {
        self.protected_desktop_and_game_vram_bytes
    }

    pub fn selected_peak_vram_bytes(&self) -> u64 {
        self.selected_peak_vram_bytes
    }

    pub fn vram_safety_bytes(&self) -> u64 {
        self.vram_safety_bytes
    }

    pub fn projected_total_vram_bytes(&self) -> u64 {
        self.projected_total_vram_bytes
    }

    pub fn ram_soft_ceiling_bytes(&self) -> u64 {
        self.ram_soft_ceiling_bytes
    }

    pub fn selected_peak_ram_bytes(&self) -> u64 {
        self.selected_peak_ram_bytes
    }

    pub fn ram_safety_bytes(&self) -> u64 {
        self.ram_safety_bytes
    }

    pub fn digest(&self) -> Result<Sha256Digest, ResourceAdmissionError> {
        serde_json::to_vec(self)
            .map(|bytes| Sha256Digest::of_bytes(&bytes))
            .map_err(|_| ResourceAdmissionError::AdmissionSerialization)
    }

    pub fn validates_manifest(
        &self,
        identity: &PackRevision,
        manifest_sha256: &Sha256Digest,
    ) -> bool {
        self.models
            .iter()
            .any(|model| &model.identity == identity && &model.manifest_sha256 == manifest_sha256)
    }
}

pub struct ResourceGovernorV1 {
    policy: ResourceGovernorPolicyV1,
}

impl ResourceGovernorV1 {
    pub fn new(policy: ResourceGovernorPolicyV1) -> Result<Self, ResourceAdmissionError> {
        policy.validate()?;
        Ok(Self { policy })
    }

    pub fn policy(&self) -> &ResourceGovernorPolicyV1 {
        &self.policy
    }

    pub fn admit(
        &self,
        snapshot: &LiveResourceSnapshotV1,
        now_monotonic_millis: u64,
        requests: &[LoadoutModelRequestV1<'_>],
    ) -> Result<LoadoutAdmissionV1, ResourceAdmissionError> {
        validate_snapshot(snapshot, now_monotonic_millis, &self.policy)?;
        if requests.is_empty() {
            return Err(ResourceAdmissionError::EmptyLoadout);
        }
        let mut roles = BTreeSet::new();
        let mut models = Vec::with_capacity(requests.len());
        let mut selected_peak_vram = 0_u64;
        let mut selected_peak_ram = 0_u64;
        for request in requests {
            if !roles.insert(request.role.clone()) {
                return Err(ResourceAdmissionError::DuplicateRole(request.role.clone()));
            }
            if request.envelope.payload.device_fingerprint_sha256
                != snapshot.device_fingerprint_sha256
            {
                return Err(ResourceAdmissionError::DeviceFingerprintMismatch);
            }
            if request.envelope.payload.capability != request.role {
                return Err(ResourceAdmissionError::CapabilityMismatch {
                    measured: request.envelope.payload.capability.clone(),
                    requested: request.role.clone(),
                });
            }
            let measurement = request.envelope.placement(&request.mode).ok_or_else(|| {
                ResourceAdmissionError::UnknownEnvelope {
                    identity: request.envelope.identity().clone(),
                    mode: request.mode.clone(),
                }
            })?;
            selected_peak_vram = checked_sum(
                selected_peak_vram,
                checked_sum(
                    measurement.resident_vram_bytes,
                    measurement.p99_workspace_vram_bytes,
                )?,
            )?;
            selected_peak_ram = checked_sum(selected_peak_ram, measurement.p99_total_ram_bytes)?;
            models.push(AdmittedModelV1 {
                role: request.role.clone(),
                identity: request.envelope.identity().clone(),
                manifest_sha256: request.envelope.manifest_sha256().clone(),
                measured_envelope_sha256: request.envelope.payload_sha256().clone(),
                mode: request.mode.clone(),
                measurement: measurement.clone(),
            });
        }
        let vram_hard_budget = snapshot
            .physical_vram_bytes
            .min(snapshot.os_vram_budget_bytes);
        let vram_soft_ceiling =
            basis_points(vram_hard_budget, self.policy.vram_soft_ceiling_basis_points)?;
        let protected_game = snapshot
            .game_resident_vram_bytes
            .max(snapshot.game_reserve_vram_bytes);
        let protected_desktop_and_game =
            checked_sum(snapshot.desktop_resident_vram_bytes, protected_game)?;
        let proportional_safety = basis_points(
            vram_hard_budget,
            self.policy.proportional_vram_safety_basis_points,
        )?;
        let vram_safety = self
            .policy
            .minimum_vram_safety_bytes
            .max(proportional_safety);
        let projected_total_vram = checked_sum(
            checked_sum(protected_desktop_and_game, selected_peak_vram)?,
            vram_safety,
        )?;
        if projected_total_vram > vram_soft_ceiling {
            return Err(ResourceAdmissionError::VramSoftCeilingExceeded {
                projected_bytes: projected_total_vram,
                ceiling_bytes: vram_soft_ceiling,
            });
        }
        let ram_soft_ceiling = basis_points(
            snapshot.physical_ram_bytes,
            self.policy.ram_soft_ceiling_basis_points,
        )?;
        let incremental_ram_budget = snapshot.available_ram_bytes.min(ram_soft_ceiling);
        let required_incremental_ram = checked_sum(
            checked_sum(
                selected_peak_ram,
                snapshot.game_additional_reserve_ram_bytes,
            )?,
            self.policy.minimum_ram_safety_bytes,
        )?;
        if required_incremental_ram > incremental_ram_budget {
            return Err(ResourceAdmissionError::RamSoftCeilingExceeded {
                projected_bytes: required_incremental_ram,
                ceiling_bytes: incremental_ram_budget,
            });
        }
        models.sort_by(|left, right| left.role.cmp(&right.role));
        Ok(LoadoutAdmissionV1 {
            snapshot_monotonic_millis: snapshot.captured_monotonic_millis,
            device_fingerprint_sha256: snapshot.device_fingerprint_sha256.clone(),
            models,
            vram_soft_ceiling_bytes: vram_soft_ceiling,
            protected_desktop_and_game_vram_bytes: protected_desktop_and_game,
            selected_peak_vram_bytes: selected_peak_vram,
            vram_safety_bytes: vram_safety,
            projected_total_vram_bytes: projected_total_vram,
            ram_soft_ceiling_bytes: incremental_ram_budget,
            selected_peak_ram_bytes: selected_peak_ram,
            ram_safety_bytes: self.policy.minimum_ram_safety_bytes,
        })
    }
}

fn validate_snapshot(
    snapshot: &LiveResourceSnapshotV1,
    now: u64,
    policy: &ResourceGovernorPolicyV1,
) -> Result<(), ResourceAdmissionError> {
    if snapshot.captured_monotonic_millis == 0
        || snapshot.physical_vram_bytes == 0
        || snapshot.os_vram_budget_bytes == 0
        || snapshot.physical_ram_bytes == 0
        || snapshot.available_ram_bytes == 0
        || snapshot.os_vram_budget_bytes > snapshot.physical_vram_bytes
        || snapshot.available_ram_bytes > snapshot.physical_ram_bytes
        || snapshot.desktop_resident_vram_bytes > snapshot.os_vram_budget_bytes
        || snapshot.game_resident_vram_bytes > snapshot.os_vram_budget_bytes
    {
        return Err(ResourceAdmissionError::InvalidSnapshot);
    }
    let age = now
        .checked_sub(snapshot.captured_monotonic_millis)
        .ok_or(ResourceAdmissionError::SnapshotFromFuture)?;
    if age > policy.maximum_snapshot_age_millis {
        return Err(ResourceAdmissionError::StaleSnapshot { age_millis: age });
    }
    Ok(())
}

fn basis_points(value: u64, bps: u16) -> Result<u64, ResourceAdmissionError> {
    value
        .checked_mul(u64::from(bps))
        .map(|scaled| scaled / BASIS_POINTS)
        .ok_or(ResourceAdmissionError::ArithmeticOverflow)
}

fn checked_sum(left: u64, right: u64) -> Result<u64, ResourceAdmissionError> {
    left.checked_add(right)
        .ok_or(ResourceAdmissionError::ArithmeticOverflow)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum RuntimeResidencyStateV1 {
    GpuWarm,
    CpuResidentGpuCold,
    CpuWarm,
    Unloaded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResidencyRecordV1 {
    pub identity: PackRevision,
    pub state: RuntimeResidencyStateV1,
    pub cpu_fallback_supported: bool,
    pub last_used_monotonic_millis: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum ResidencyActionV1 {
    DropGpuKeepCpu { identity: PackRevision },
    Unload { identity: PackRevision },
}

#[derive(Clone, Debug)]
pub struct ResidencyTrackerV1 {
    keep_warm_millis: u64,
    unload_ttl_millis: u64,
    records: BTreeMap<PackRevision, ResidencyRecordV1>,
}

impl ResidencyTrackerV1 {
    pub fn new(policy: &ResourceGovernorPolicyV1) -> Result<Self, ResourceAdmissionError> {
        policy.validate()?;
        Ok(Self {
            keep_warm_millis: policy.keep_warm_millis,
            unload_ttl_millis: policy.unload_ttl_millis,
            records: BTreeMap::new(),
        })
    }

    pub fn record_loaded(
        &mut self,
        identity: PackRevision,
        state: RuntimeResidencyStateV1,
        cpu_fallback_supported: bool,
        now: u64,
    ) -> Result<(), ResourceAdmissionError> {
        if now == 0 || state == RuntimeResidencyStateV1::Unloaded {
            return Err(ResourceAdmissionError::InvalidResidencyUpdate);
        }
        self.records.insert(
            identity.clone(),
            ResidencyRecordV1 {
                identity,
                state,
                cpu_fallback_supported,
                last_used_monotonic_millis: now,
            },
        );
        Ok(())
    }

    pub fn touch(
        &mut self,
        identity: &PackRevision,
        now: u64,
    ) -> Result<(), ResourceAdmissionError> {
        let record = self
            .records
            .get_mut(identity)
            .ok_or(ResourceAdmissionError::UnknownResidency)?;
        if now < record.last_used_monotonic_millis {
            return Err(ResourceAdmissionError::ResidencyClockWentBackward);
        }
        record.last_used_monotonic_millis = now;
        Ok(())
    }

    pub fn record(&self, identity: &PackRevision) -> Option<&ResidencyRecordV1> {
        self.records.get(identity)
    }

    /// Returns deterministic actions and updates the tracker immediately so
    /// repeated maintenance calls cannot emit the same action twice.
    pub fn maintenance_actions(
        &mut self,
        now: u64,
    ) -> Result<Vec<ResidencyActionV1>, ResourceAdmissionError> {
        let identities: Vec<_> = self.records.keys().cloned().collect();
        let mut actions = Vec::new();
        for identity in identities {
            let record = self
                .records
                .get_mut(&identity)
                .expect("identity came from record map");
            let idle = now
                .checked_sub(record.last_used_monotonic_millis)
                .ok_or(ResourceAdmissionError::ResidencyClockWentBackward)?;
            if idle >= self.unload_ttl_millis {
                record.state = RuntimeResidencyStateV1::Unloaded;
                actions.push(ResidencyActionV1::Unload {
                    identity: identity.clone(),
                });
            } else if idle >= self.keep_warm_millis
                && record.state == RuntimeResidencyStateV1::GpuWarm
                && record.cpu_fallback_supported
            {
                record.state = RuntimeResidencyStateV1::CpuResidentGpuCold;
                actions.push(ResidencyActionV1::DropGpuKeepCpu {
                    identity: identity.clone(),
                });
            }
        }
        self.records
            .retain(|_, record| record.state != RuntimeResidencyStateV1::Unloaded);
        Ok(actions)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKindV1 {
    LipSync,
    SpeechRecognition,
    SpeechSynthesis,
    LanguageModel,
    Vision,
    Embedding,
}

impl WorkKindV1 {
    fn priority(&self) -> u8 {
        match self {
            Self::LipSync => 6,
            Self::SpeechRecognition => 5,
            Self::SpeechSynthesis => 4,
            Self::LanguageModel => 3,
            Self::Vision => 2,
            Self::Embedding => 1,
        }
    }

    fn is_interactive(&self) -> bool {
        matches!(
            self,
            Self::LipSync | Self::SpeechRecognition | Self::SpeechSynthesis | Self::LanguageModel
        )
    }

    fn is_visual(&self) -> bool {
        matches!(self, Self::LipSync | Self::Vision)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VisualWorkAddressV1 {
    pub generation_id: u64,
    pub frame_id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkItemV1 {
    pub work_id: String,
    pub kind: WorkKindV1,
    pub submitted_monotonic_millis: u64,
    pub deadline_monotonic_millis: u64,
    pub visual: Option<VisualWorkAddressV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum CancellationReasonV1 {
    PreemptedByInteractive,
    SupersededVisualWork,
    StaleGeneration,
    StaleFrame,
    DeadlineExpired,
    QueuePressure,
    QueueCapacity,
    CancelledByNativeRuntime,
    AdmissionRevoked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkCancellationV1 {
    pub work_id: String,
    pub reason: CancellationReasonV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkSubmitResultV1 {
    pub accepted: bool,
    pub cancellations: Vec<WorkCancellationV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkQueuePolicyV1 {
    pub maximum_pending: usize,
}

impl Default for WorkQueuePolicyV1 {
    fn default() -> Self {
        Self {
            maximum_pending: 256,
        }
    }
}

impl WorkQueuePolicyV1 {
    fn validate(&self) -> Result<(), WorkSchedulerError> {
        if self.maximum_pending == 0 || self.maximum_pending > 65_536 {
            return Err(WorkSchedulerError::InvalidQueuePolicy);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePressureLevelV1 {
    Normal,
    Elevated,
    Critical,
}

#[derive(Clone, Debug, Default)]
pub struct WorkSchedulerV1 {
    policy: WorkQueuePolicyV1,
    sequence: u64,
    pending: BTreeMap<String, ScheduledWorkV1>,
    visual_generation_id: u64,
    visual_frame_id: u64,
}

#[derive(Clone, Debug)]
struct ScheduledWorkV1 {
    sequence: u64,
    item: WorkItemV1,
}

impl WorkSchedulerV1 {
    pub fn with_policy(policy: WorkQueuePolicyV1) -> Result<Self, WorkSchedulerError> {
        policy.validate()?;
        Ok(Self {
            policy,
            ..Self::default()
        })
    }

    pub fn policy(&self) -> &WorkQueuePolicyV1 {
        &self.policy
    }

    pub fn submit(
        &mut self,
        item: WorkItemV1,
        now: u64,
    ) -> Result<WorkSubmitResultV1, WorkSchedulerError> {
        validate_work_item(&item, now)?;
        if self.pending.contains_key(&item.work_id) {
            return Err(WorkSchedulerError::DuplicateWorkId);
        }
        if let Some(visual) = &item.visual {
            if visual.generation_id < self.visual_generation_id {
                return Ok(WorkSubmitResultV1 {
                    accepted: false,
                    cancellations: vec![WorkCancellationV1 {
                        work_id: item.work_id,
                        reason: CancellationReasonV1::StaleGeneration,
                    }],
                });
            }
            if visual.generation_id == self.visual_generation_id
                && visual.frame_id < self.visual_frame_id
            {
                return Ok(WorkSubmitResultV1 {
                    accepted: false,
                    cancellations: vec![WorkCancellationV1 {
                        work_id: item.work_id,
                        reason: CancellationReasonV1::StaleFrame,
                    }],
                });
            }
            self.visual_generation_id = visual.generation_id;
            self.visual_frame_id = visual.frame_id;
        }
        let mut cancellations = Vec::new();
        let pending_ids: Vec<_> = self.pending.keys().cloned().collect();
        for pending_id in pending_ids {
            let existing = self.pending.get(&pending_id).expect("known pending work");
            let reason = if let Some(existing_visual) = &existing.item.visual {
                if existing_visual.generation_id < self.visual_generation_id {
                    Some(CancellationReasonV1::StaleGeneration)
                } else if existing_visual.generation_id == self.visual_generation_id
                    && existing_visual.frame_id < self.visual_frame_id
                {
                    Some(CancellationReasonV1::StaleFrame)
                } else {
                    None
                }
            } else {
                None
            }
            .or_else(|| {
                (item.kind.is_interactive()
                    && matches!(
                        existing.item.kind,
                        WorkKindV1::Embedding | WorkKindV1::Vision
                    ))
                .then_some(CancellationReasonV1::PreemptedByInteractive)
            })
            .or_else(|| {
                (item.kind.is_visual()
                    && existing.item.kind.is_visual()
                    && existing.item.kind == item.kind)
                    .then_some(CancellationReasonV1::SupersededVisualWork)
            });
            if let Some(reason) = reason {
                self.pending.remove(&pending_id);
                cancellations.push(WorkCancellationV1 {
                    work_id: pending_id,
                    reason,
                });
            }
        }
        if self.pending.len() >= self.policy.maximum_pending {
            let lowest = self
                .pending
                .iter()
                .min_by_key(|(_, scheduled)| (scheduled.item.kind.priority(), scheduled.sequence))
                .map(|(id, scheduled)| (id.clone(), scheduled.item.kind.priority()))
                .expect("a full non-zero-capacity queue has a lowest item");
            if item.kind.priority() > lowest.1 {
                self.pending.remove(&lowest.0);
                cancellations.push(WorkCancellationV1 {
                    work_id: lowest.0,
                    reason: CancellationReasonV1::QueueCapacity,
                });
            } else {
                cancellations.push(WorkCancellationV1 {
                    work_id: item.work_id,
                    reason: CancellationReasonV1::QueueCapacity,
                });
                return Ok(WorkSubmitResultV1 {
                    accepted: false,
                    cancellations,
                });
            }
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(WorkSchedulerError::SequenceOverflow)?;
        self.pending.insert(
            item.work_id.clone(),
            ScheduledWorkV1 {
                sequence: self.sequence,
                item,
            },
        );
        Ok(WorkSubmitResultV1 {
            accepted: true,
            cancellations,
        })
    }

    /// Advances the authoritative capture address and cancels queued visual
    /// work that can no longer be presented safely.
    pub fn advance_visual_clock(
        &mut self,
        generation_id: u64,
        frame_id: u64,
        now: u64,
    ) -> Result<Vec<WorkCancellationV1>, WorkSchedulerError> {
        if generation_id < self.visual_generation_id
            || (generation_id == self.visual_generation_id && frame_id < self.visual_frame_id)
        {
            return Err(WorkSchedulerError::VisualClockWentBackward);
        }
        self.visual_generation_id = generation_id;
        self.visual_frame_id = frame_id;
        let ids: Vec<_> = self.pending.keys().cloned().collect();
        let mut cancellations = Vec::new();
        for id in ids {
            let scheduled = self.pending.get(&id).expect("known pending work");
            let reason = if scheduled.item.deadline_monotonic_millis <= now {
                Some(CancellationReasonV1::DeadlineExpired)
            } else if let Some(visual) = &scheduled.item.visual {
                if visual.generation_id < generation_id {
                    Some(CancellationReasonV1::StaleGeneration)
                } else if visual.generation_id == generation_id && visual.frame_id < frame_id {
                    Some(CancellationReasonV1::StaleFrame)
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(reason) = reason {
                self.pending.remove(&id);
                cancellations.push(WorkCancellationV1 {
                    work_id: id,
                    reason,
                });
            }
        }
        Ok(cancellations)
    }

    pub fn pop_next(&mut self, now: u64) -> Option<WorkItemV1> {
        self.drop_expired(now);
        let next_id = self
            .pending
            .iter()
            .max_by_key(|(_, scheduled)| {
                (scheduled.item.kind.priority(), Reverse(scheduled.sequence))
            })
            .map(|(id, _)| id.clone())?;
        self.pending
            .remove(&next_id)
            .map(|scheduled| scheduled.item)
    }

    /// Pops only the requested native work class. This prevents an optional
    /// visual consumer from accidentally dispatching or cancelling speech,
    /// language-model, or memory work owned by a different runtime.
    pub fn pop_next_kind(&mut self, kind: &WorkKindV1, now: u64) -> Option<WorkItemV1> {
        self.drop_expired(now);
        let next_id = self
            .pending
            .iter()
            .filter(|(_, scheduled)| &scheduled.item.kind == kind)
            .min_by_key(|(_, scheduled)| scheduled.sequence)
            .map(|(id, _)| id.clone())?;
        self.pending
            .remove(&next_id)
            .map(|scheduled| scheduled.item)
    }

    pub fn cancel_kind(
        &mut self,
        work_id: &str,
        kind: &WorkKindV1,
        reason: CancellationReasonV1,
    ) -> Option<WorkCancellationV1> {
        if self
            .pending
            .get(work_id)
            .is_none_or(|scheduled| &scheduled.item.kind != kind)
        {
            return None;
        }
        self.pending.remove(work_id);
        Some(WorkCancellationV1 {
            work_id: work_id.to_owned(),
            reason,
        })
    }

    pub fn cancel_all_kind(
        &mut self,
        kind: &WorkKindV1,
        reason: CancellationReasonV1,
    ) -> Vec<WorkCancellationV1> {
        let ids = self
            .pending
            .iter()
            .filter(|(_, scheduled)| &scheduled.item.kind == kind)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| self.cancel_kind(&id, kind, reason.clone()))
            .collect()
    }

    pub fn cancel_all(&mut self, reason: CancellationReasonV1) -> Vec<WorkCancellationV1> {
        let ids = self.pending.keys().cloned().collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| {
                self.pending.remove(&id).map(|_| WorkCancellationV1 {
                    work_id: id,
                    reason: reason.clone(),
                })
            })
            .collect()
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn pending_by_kind(&self) -> BTreeMap<WorkKindV1, usize> {
        let mut counts = BTreeMap::new();
        for scheduled in self.pending.values() {
            *counts.entry(scheduled.item.kind.clone()).or_insert(0) += 1;
        }
        counts
    }

    /// Sheds only queued work. The runtime remains responsible for cancelling
    /// already-dispatched work using its generation/cancellation token.
    pub fn apply_pressure(
        &mut self,
        level: ResourcePressureLevelV1,
        now: u64,
    ) -> Vec<WorkCancellationV1> {
        let ids: Vec<_> = self.pending.keys().cloned().collect();
        let mut cancellations = Vec::new();
        for id in ids {
            let scheduled = self.pending.get(&id).expect("known pending work");
            let stale_visual = scheduled.item.visual.as_ref().is_some_and(|visual| {
                visual.generation_id < self.visual_generation_id
                    || (visual.generation_id == self.visual_generation_id
                        && visual.frame_id < self.visual_frame_id)
            });
            let shed_for_pressure = match level {
                ResourcePressureLevelV1::Normal => false,
                ResourcePressureLevelV1::Elevated => matches!(
                    scheduled.item.kind,
                    WorkKindV1::Embedding | WorkKindV1::Vision
                ),
                ResourcePressureLevelV1::Critical => matches!(
                    scheduled.item.kind,
                    WorkKindV1::Embedding | WorkKindV1::Vision | WorkKindV1::SpeechSynthesis
                ),
            };
            let reason = if scheduled.item.deadline_monotonic_millis <= now {
                Some(CancellationReasonV1::DeadlineExpired)
            } else if stale_visual {
                scheduled.item.visual.as_ref().map(|visual| {
                    if visual.generation_id < self.visual_generation_id {
                        CancellationReasonV1::StaleGeneration
                    } else {
                        CancellationReasonV1::StaleFrame
                    }
                })
            } else if shed_for_pressure {
                Some(CancellationReasonV1::QueuePressure)
            } else {
                None
            };
            if let Some(reason) = reason {
                self.pending.remove(&id);
                cancellations.push(WorkCancellationV1 {
                    work_id: id,
                    reason,
                });
            }
        }
        cancellations
    }

    fn drop_expired(&mut self, now: u64) {
        let expired = self
            .pending
            .iter()
            .filter(|(_, scheduled)| scheduled.item.deadline_monotonic_millis <= now)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in expired {
            self.pending.remove(&id);
        }
    }
}

fn validate_work_item(item: &WorkItemV1, now: u64) -> Result<(), WorkSchedulerError> {
    validate_token("work_id", &item.work_id, 8, 128)
        .map_err(|_| WorkSchedulerError::InvalidWorkId)?;
    if item.submitted_monotonic_millis == 0
        || item.submitted_monotonic_millis > now
        || item.deadline_monotonic_millis <= now
        || item.deadline_monotonic_millis < item.submitted_monotonic_millis
    {
        return Err(WorkSchedulerError::InvalidDeadline);
    }
    if item.kind.is_visual() != item.visual.is_some() {
        return Err(WorkSchedulerError::InvalidVisualAddress);
    }
    if item
        .visual
        .as_ref()
        .is_some_and(|visual| visual.generation_id == 0 || visual.frame_id == 0)
    {
        return Err(WorkSchedulerError::InvalidVisualAddress);
    }
    Ok(())
}

fn validate_token(
    field: &'static str,
    value: &str,
    min: usize,
    max: usize,
) -> Result<(), ResourceEnvelopeError> {
    if !(min..=max).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(ResourceEnvelopeError::InvalidField(field));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ResourceEnvelopeError {
    #[error("unsupported measured resource envelope schema: {0}")]
    UnsupportedSchema(String),
    #[error("invalid measured resource envelope field: {0}")]
    InvalidField(&'static str),
    #[error("measured resource envelope is incomplete")]
    IncompleteMeasurement,
    #[error("resource measurement is unreasonably far in the future")]
    MeasuredInFuture,
    #[error("resource measurement has expired")]
    Expired,
    #[error("resource measurement lifetime violates policy")]
    InvalidLifetime,
    #[error("invalid measurement for placement {0:?}")]
    InvalidPlacementMeasurement(ResidencyModeV1),
    #[error("measurement signature threshold cannot be zero")]
    ZeroSignatureThreshold,
    #[error("valid measurement signature threshold not met: required {required}, valid {valid}")]
    SignatureThreshold { required: usize, valid: usize },
    #[error("measurement rollback: trusted sequence {trusted}, received {received}")]
    Rollback { trusted: u64, received: u64 },
    #[error("measurement sequence {0} has conflicting signed payloads")]
    SequenceEquivocation(u64),
    #[error("resource envelope serialization failed: {0}")]
    Serialization(serde_json::Error),
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ResourceAdmissionError {
    #[error("unsupported resource governor policy schema: {0}")]
    UnsupportedPolicySchema(String),
    #[error("resource governor policy is invalid")]
    InvalidPolicy,
    #[error("live resource snapshot is invalid or incomplete")]
    InvalidSnapshot,
    #[error("live resource snapshot is from the future")]
    SnapshotFromFuture,
    #[error("live resource snapshot is stale by {age_millis} ms")]
    StaleSnapshot { age_millis: u64 },
    #[error("local loadout is empty")]
    EmptyLoadout,
    #[error("loadout contains more than one active model for role {0:?}")]
    DuplicateRole(ModelPackKindV1),
    #[error("measured envelope belongs to a different device")]
    DeviceFingerprintMismatch,
    #[error("measured capability {measured:?} does not match requested role {requested:?}")]
    CapabilityMismatch {
        measured: ModelPackKindV1,
        requested: ModelPackKindV1,
    },
    #[error("no measured envelope for {identity:?} in placement {mode:?}")]
    UnknownEnvelope {
        identity: PackRevision,
        mode: ResidencyModeV1,
    },
    #[error("VRAM soft ceiling exceeded: projected {projected_bytes}, ceiling {ceiling_bytes}")]
    VramSoftCeilingExceeded {
        projected_bytes: u64,
        ceiling_bytes: u64,
    },
    #[error("RAM soft ceiling exceeded: projected {projected_bytes}, ceiling {ceiling_bytes}")]
    RamSoftCeilingExceeded {
        projected_bytes: u64,
        ceiling_bytes: u64,
    },
    #[error("resource arithmetic overflow")]
    ArithmeticOverflow,
    #[error("loadout admission serialization failed")]
    AdmissionSerialization,
    #[error("invalid residency update")]
    InvalidResidencyUpdate,
    #[error("unknown residency record")]
    UnknownResidency,
    #[error("residency clock moved backward")]
    ResidencyClockWentBackward,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum WorkSchedulerError {
    #[error("work queue policy is invalid")]
    InvalidQueuePolicy,
    #[error("work ID is invalid")]
    InvalidWorkId,
    #[error("work deadline/timestamp is invalid")]
    InvalidDeadline,
    #[error(
        "visual work must contain a non-zero generation/frame address and non-visual work must not"
    )]
    InvalidVisualAddress,
    #[error("work ID is already pending")]
    DuplicateWorkId,
    #[error("visual clock cannot move backward")]
    VisualClockWentBackward,
    #[error("work submission sequence overflow")]
    SequenceOverflow,
}
