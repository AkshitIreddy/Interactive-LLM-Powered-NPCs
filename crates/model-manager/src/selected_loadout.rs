use crate::{
    verify_measured_resource_envelope, CancellationReasonV1, CatalogError,
    CatalogSignatureVerifier, LoadoutAdmissionV1, LoadoutModelRequestV1, MeasurementTrustPolicyV1,
    MeasurementTrustStateV1, ModelPackKindV1, ModelPackScopeV1, PackRevision,
    QualifiedResourceEnvelopeV1, ResidencyModeV1, ResourceAdmissionError, ResourceEnvelopeError,
    ResourceGovernorPolicyV1, ResourceGovernorV1, ResourcePressureLevelV1, RuntimeResidencyStateV1,
    Sha256Digest, SignedMeasuredResourceEnvelopeV1, TrustedCatalog, TrustedPackMeasurementStatusV1,
    TrustedReleasePackSnapshotV1, WorkCancellationV1, WorkItemV1, WorkQueuePolicyV1,
    WorkSchedulerError, WorkSchedulerV1, WorkSubmitResultV1,
};
use npc_system_telemetry::{AdmissionViewError, ResourceTelemetrySnapshotV1};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const SELECTED_LOADOUT_API_SCHEMA_V1: &str = "npc.selected-loadout-admission/v1";

/// Web-facing selection fields. Target PID, clocks, resource policy, reserves,
/// telemetry and evidence are intentionally absent: a native authority must
/// construct [`NativeAdmissionContextV1`] and the manager owns all trust inputs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedLoadoutSelectionV1 {
    pub selection_id: String,
    pub roles: Vec<SelectedPackV1>,
    /// A bounded scheduling prediction, not a resource measurement.
    pub expected_idle_millis: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedPackV1 {
    pub role: ModelPackKindV1,
    pub identity: PackRevision,
    pub preferred_residency: ResidencyModeV1,
}

/// Constructed only by native code after resolving the selected game target
/// and persisted settings. It is deliberately not deserializable.
#[derive(Clone, Copy, Debug)]
pub struct NativeAdmissionContextV1<'a> {
    pub exact_target_pid: Option<u32>,
    pub now_unix_seconds: u64,
    pub now_monotonic_millis: u64,
    pub configured_game_reserve_vram_bytes: u64,
    pub game_additional_reserve_ram_bytes: u64,
    pub resource_pressure: ResourcePressureLevelV1,
    pub telemetry: Option<&'a ResourceTelemetrySnapshotV1>,
}

/// Native evidence source owned by [`SelectedLoadoutManagerV1`]. Implementors
/// may read a protected local store, but must never download or benchmark from
/// this method. Absence is a normal fail-closed admission result.
pub trait SignedResourceEnvelopeSourceV1 {
    type Error: fmt::Display;

    fn load_signed_envelope(
        &self,
        identity: &PackRevision,
        device_fingerprint_sha256: &Sha256Digest,
        placement: &ResidencyModeV1,
    ) -> Result<Option<SignedMeasuredResourceEnvelopeV1>, Self::Error>;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectedLoadoutStatusV1 {
    Admitted,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectedLoadoutBlockCodeV1 {
    InvalidSelection,
    CatalogUnavailable,
    MissingTargetPid,
    MissingTelemetry,
    TargetPidMismatch,
    CatalogExpired,
    UnknownPack,
    RevokedPack,
    ManifestInvalid,
    RoleManifestMismatch,
    NonGenericPack,
    MissingMeasuredEnvelope,
    MeasuredEnvelopeLoadFailed,
    MeasuredEnvelopeUntrusted,
    MeasuredEnvelopeExpired,
    MeasuredEnvelopeWrongDevice,
    MeasuredEnvelopeManifestMismatch,
    MeasuredEnvelopeCapabilityMismatch,
    UnknownPlacementMeasurement,
    InvalidTelemetry,
    RequiredTelemetryUnavailable,
    StaleTelemetry,
    VramContention,
    RamContention,
    ResourceArithmetic,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidencyDispositionV1 {
    KeepWarm,
    CpuResidentGpuCold,
    Unload,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MeasuredResidencyDecisionV1 {
    pub role: ModelPackKindV1,
    pub identity: PackRevision,
    pub current: RuntimeResidencyStateV1,
    pub disposition: ResidencyDispositionV1,
    pub expected_idle_millis: u64,
    pub compared_p99_reload_millis: u64,
    pub evidence_placement: ResidencyModeV1,
    pub measured_envelope_sha256: Sha256Digest,
}

/// Owned, serde-ready native response. It is Serialize-only so neither the
/// receipt nor qualified evidence can be forged by deserializing WebView JSON.
#[derive(Clone, Debug, Serialize)]
pub struct SelectedLoadoutDecisionV1 {
    pub schema: String,
    pub selection_id: String,
    pub status: SelectedLoadoutStatusV1,
    pub reason_code: Option<SelectedLoadoutBlockCodeV1>,
    pub detail: String,
    pub exact_target_pid: Option<u32>,
    pub selected_roles: Vec<SelectedPackV1>,
    pub live_snapshot: Option<ResourceTelemetrySnapshotV1>,
    pub admission_receipt: Option<LoadoutAdmissionV1>,
    pub residency_decisions: Vec<MeasuredResidencyDecisionV1>,
    pub pressure_cancellations: Vec<WorkCancellationV1>,
}

impl SelectedLoadoutDecisionV1 {
    pub fn admitted(&self) -> bool {
        self.status == SelectedLoadoutStatusV1::Admitted
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SelectedLoadoutPlannerSnapshotV1 {
    pub schema: String,
    pub trusted_measurement_streams: usize,
    pub active_evidence: Vec<PackRevision>,
    pub pending_work: usize,
    pub pending_by_kind: BTreeMap<crate::WorkKindV1, usize>,
}

/// Aggregate trust/admission/scheduling boundary handed to the native control
/// layer. Public callers select exact catalog identities only; the manager
/// resolves immutable manifests and signed measurements itself.
pub struct SelectedLoadoutManagerV1<V, E> {
    catalog: TrustedCatalog,
    envelope_verifier: V,
    envelope_source: E,
    measurement_policy: MeasurementTrustPolicyV1,
    measurement_state: MeasurementTrustStateV1,
    governor: ResourceGovernorV1,
    scheduler: WorkSchedulerV1,
    active_evidence: BTreeMap<PackRevision, QualifiedResourceEnvelopeV1>,
}

impl<V, E> SelectedLoadoutManagerV1<V, E>
where
    V: CatalogSignatureVerifier,
    E: SignedResourceEnvelopeSourceV1,
{
    pub fn new(
        catalog: TrustedCatalog,
        envelope_verifier: V,
        envelope_source: E,
        measurement_policy: MeasurementTrustPolicyV1,
        governor_policy: ResourceGovernorPolicyV1,
        queue_policy: WorkQueuePolicyV1,
    ) -> Result<Self, SelectedLoadoutManagerBuildErrorV1> {
        let governor = ResourceGovernorV1::new(governor_policy)
            .map_err(SelectedLoadoutManagerBuildErrorV1::Governor)?;
        let scheduler = WorkSchedulerV1::with_policy(queue_policy)
            .map_err(SelectedLoadoutManagerBuildErrorV1::Scheduler)?;
        if measurement_policy.signature_threshold == 0 {
            return Err(SelectedLoadoutManagerBuildErrorV1::InvalidMeasurementPolicy);
        }
        Ok(Self {
            catalog,
            envelope_verifier,
            envelope_source,
            measurement_policy,
            measurement_state: MeasurementTrustStateV1::default(),
            governor,
            scheduler,
            active_evidence: BTreeMap::new(),
        })
    }

    pub fn planner_snapshot(&self) -> SelectedLoadoutPlannerSnapshotV1 {
        SelectedLoadoutPlannerSnapshotV1 {
            schema: SELECTED_LOADOUT_API_SCHEMA_V1.to_owned(),
            trusted_measurement_streams: self.measurement_state.accepted.len(),
            active_evidence: self.active_evidence.keys().cloned().collect(),
            pending_work: self.scheduler.pending_len(),
            pending_by_kind: self.scheduler.pending_by_kind(),
        }
    }

    /// Native lifecycle code may resolve installable entries from the same
    /// verified catalog used for admission. WebView DTOs receive only the
    /// serialize-only snapshots, never this authority object.
    pub fn trusted_catalog(&self) -> &TrustedCatalog {
        &self.catalog
    }

    pub fn catalog_payload_sha256(&self) -> &Sha256Digest {
        self.catalog.payload_digest()
    }

    /// Verifies an additive native-only authority payload with the same pinned
    /// key set and threshold used for measured envelopes. The signed payload
    /// must separately bind the catalog, manifest, measurement, and receipt.
    pub fn verifies_native_authority_payload(
        &self,
        message: &[u8],
        signatures: &[crate::CatalogSignatureV1],
    ) -> bool {
        let mut accepted = BTreeSet::new();
        for signature in signatures {
            if accepted.contains(&signature.key_id)
                || !self.envelope_verifier.is_trusted_key(&signature.key_id)
                || !self.envelope_verifier.verify(
                    &signature.key_id,
                    &signature.algorithm,
                    message,
                    &signature.signature,
                )
            {
                continue;
            }
            accepted.insert(signature.key_id.clone());
        }
        accepted.len() >= self.measurement_policy.signature_threshold
    }

    /// Populates measurement UI fields only from a signature-verified envelope
    /// for the exact native device. Catalog planning values and unsigned review
    /// evidence cannot enter this path.
    pub fn refresh_trusted_pack_measurement(
        &mut self,
        pack: &mut TrustedReleasePackSnapshotV1,
        device_fingerprint_sha256: &Sha256Digest,
        now_unix_seconds: u64,
    ) {
        pack.measurement_status = TrustedPackMeasurementStatusV1::Unavailable;
        pack.qualified_measurement = None;
        let Some(entry) = self.catalog.entry(&pack.identity) else {
            pack.measurement_detail = "Pack identity is absent from the verified catalog.".into();
            return;
        };
        if entry.revoked {
            pack.measurement_detail = "Pack revision is revoked in the verified catalog.".into();
            return;
        }
        let Ok(manifest_sha256) = entry.manifest.digest() else {
            pack.measurement_detail = "Catalog manifest digest is unavailable.".into();
            return;
        };
        let capability = entry.manifest.capability.kind.clone();
        let placements = pack.allowed_residencies.iter().cloned().collect::<Vec<_>>();
        let mut last_detail =
            "No signed current-device envelope exists for an allowed placement.".to_owned();
        for placement in placements {
            let signed = match self.envelope_source.load_signed_envelope(
                &pack.identity,
                device_fingerprint_sha256,
                &placement,
            ) {
                Ok(Some(signed)) => signed,
                Ok(None) => continue,
                Err(error) => {
                    pack.measurement_detail =
                        format!("Signed envelope storage failed closed: {error}");
                    return;
                }
            };
            let (qualified, next_state) = match verify_measured_resource_envelope(
                &signed,
                &self.envelope_verifier,
                &self.measurement_policy,
                &self.measurement_state,
                now_unix_seconds,
            ) {
                Ok(value) => value,
                Err(error) => {
                    last_detail = format!("Signed envelope failed trust validation: {error}");
                    continue;
                }
            };
            let payload = qualified.payload();
            if payload.identity != pack.identity
                || payload.device_fingerprint_sha256 != *device_fingerprint_sha256
                || payload.manifest_sha256 != manifest_sha256
                || payload.capability != capability
                || !payload.placements.contains_key(&placement)
            {
                last_detail = "Signed envelope does not match the exact pack, device, capability, manifest, and placement.".into();
                continue;
            }
            self.measurement_state = next_state;
            pack.measurement_status = TrustedPackMeasurementStatusV1::Qualified;
            pack.measurement_detail = format!(
                "Verified {} signed samples for the exact native device and manifest.",
                payload.sample_count
            );
            pack.qualified_measurement = Some(payload.clone());
            return;
        }
        pack.measurement_detail = last_detail;
    }

    pub fn admit(
        &mut self,
        selection: &SelectedLoadoutSelectionV1,
        context: NativeAdmissionContextV1<'_>,
    ) -> SelectedLoadoutDecisionV1 {
        self.admit_with_target_policy(selection, context, true)
    }

    /// Mints a setup-only receipt for an explicitly selected complete loadout
    /// before any game process exists. It uses the same signed current-device
    /// envelopes and configured game RAM/VRAM reserves as runtime admission,
    /// but carries no target PID and therefore cannot become runtime launch
    /// authority. The caller must keep this receipt scoped to install/self-test
    /// authorization and run normal target-bound admission before gameplay.
    pub fn admit_for_setup(
        &mut self,
        selection: &SelectedLoadoutSelectionV1,
        context: NativeAdmissionContextV1<'_>,
    ) -> SelectedLoadoutDecisionV1 {
        self.admit_with_target_policy(selection, context, false)
    }

    fn admit_with_target_policy(
        &mut self,
        selection: &SelectedLoadoutSelectionV1,
        context: NativeAdmissionContextV1<'_>,
        require_target_pid: bool,
    ) -> SelectedLoadoutDecisionV1 {
        let mut base = DecisionBuilderV1::new(selection, &context);
        if require_target_pid {
            base.pressure_cancellations = self
                .scheduler
                .apply_pressure(context.resource_pressure, context.now_monotonic_millis);
        }
        if let Err(detail) = validate_selection(selection) {
            return base.block(SelectedLoadoutBlockCodeV1::InvalidSelection, detail);
        }
        let target_pid = match (require_target_pid, context.exact_target_pid) {
            (true, Some(pid)) if pid != 0 => Some(pid),
            (true, _) => {
                return base.block(
                    SelectedLoadoutBlockCodeV1::MissingTargetPid,
                    "a native-selected non-zero target PID is required".to_owned(),
                )
            }
            (false, None) => None,
            (false, Some(_)) => {
                return base.block(
                    SelectedLoadoutBlockCodeV1::InvalidSelection,
                    "setup-only admission must not carry runtime target authority".to_owned(),
                )
            }
        };
        let telemetry = match context.telemetry {
            Some(snapshot) => snapshot,
            None => {
                return base.block(
                    SelectedLoadoutBlockCodeV1::MissingTelemetry,
                    "no native resource snapshot was supplied".to_owned(),
                )
            }
        };
        base.live_snapshot = Some(telemetry.clone());
        if telemetry.selected_game_pid != target_pid {
            return base.block(
                SelectedLoadoutBlockCodeV1::TargetPidMismatch,
                format!(
                    "telemetry PID {:?} does not match admission target PID {target_pid:?}",
                    telemetry.selected_game_pid
                ),
            );
        }
        if self.catalog.payload().expires_unix_seconds <= context.now_unix_seconds {
            return base.block(
                SelectedLoadoutBlockCodeV1::CatalogExpired,
                "the trusted catalog is no longer current".to_owned(),
            );
        }
        let admission_view = match telemetry.admission_view(
            context.configured_game_reserve_vram_bytes,
            context.game_additional_reserve_ram_bytes,
        ) {
            Ok(view) => view,
            Err(error) => {
                let (code, detail) = map_admission_view_error(error);
                return base.block(code, detail);
            }
        };
        let live = match crate::LiveResourceSnapshotV1::try_from(admission_view) {
            Ok(live) => live,
            Err(error) => {
                return base.block(
                    SelectedLoadoutBlockCodeV1::InvalidTelemetry,
                    error.to_string(),
                )
            }
        };

        let mut pending_trust = self.measurement_state.clone();
        let mut qualified = Vec::with_capacity(selection.roles.len());
        for selected in &selection.roles {
            let entry = match self.catalog.installable_entry(&selected.identity) {
                Ok(entry) => entry,
                Err(CatalogError::UnknownRevision) => {
                    return base.block(
                        SelectedLoadoutBlockCodeV1::UnknownPack,
                        format!("unknown catalog revision: {:?}", selected.identity),
                    )
                }
                Err(CatalogError::Revoked(reason)) => {
                    return base.block(SelectedLoadoutBlockCodeV1::RevokedPack, reason)
                }
                Err(error) => {
                    return base.block(
                        SelectedLoadoutBlockCodeV1::ManifestInvalid,
                        error.to_string(),
                    )
                }
            };
            if let Err(error) = entry.manifest.validate() {
                return base.block(
                    SelectedLoadoutBlockCodeV1::ManifestInvalid,
                    error.to_string(),
                );
            }
            let manifest_digest = match entry.manifest.digest() {
                Ok(digest) if digest == entry.manifest_sha256 => digest,
                Ok(_) => {
                    return base.block(
                        SelectedLoadoutBlockCodeV1::ManifestInvalid,
                        "trusted entry manifest digest changed".to_owned(),
                    )
                }
                Err(error) => {
                    return base.block(
                        SelectedLoadoutBlockCodeV1::ManifestInvalid,
                        error.to_string(),
                    )
                }
            };
            if entry.manifest.capability.kind != selected.role {
                return base.block(
                    SelectedLoadoutBlockCodeV1::RoleManifestMismatch,
                    format!(
                        "selected role {:?} does not match manifest capability {:?}",
                        selected.role, entry.manifest.capability.kind
                    ),
                );
            }
            if entry.manifest.capability.scope != ModelPackScopeV1::Generic {
                return base.block(
                    SelectedLoadoutBlockCodeV1::NonGenericPack,
                    "game-specific packs are not eligible for the generic local loadout".to_owned(),
                );
            }
            let signed = match self.envelope_source.load_signed_envelope(
                &selected.identity,
                &live.device_fingerprint_sha256,
                &selected.preferred_residency,
            ) {
                Ok(Some(envelope)) => envelope,
                Ok(None) => {
                    return base.block(
                        SelectedLoadoutBlockCodeV1::MissingMeasuredEnvelope,
                        format!(
                            "no signed current-device {:?} measurement exists for {:?}",
                            selected.preferred_residency, selected.identity
                        ),
                    )
                }
                Err(error) => {
                    return base.block(
                        SelectedLoadoutBlockCodeV1::MeasuredEnvelopeLoadFailed,
                        error.to_string(),
                    )
                }
            };
            let (evidence, next_trust) = match verify_measured_resource_envelope(
                &signed,
                &self.envelope_verifier,
                &self.measurement_policy,
                &pending_trust,
                context.now_unix_seconds,
            ) {
                Ok(result) => result,
                Err(error) => {
                    let (code, detail) = map_envelope_error(error);
                    return base.block(code, detail);
                }
            };
            pending_trust = next_trust;
            if evidence.identity() != &selected.identity
                || evidence.manifest_sha256() != &manifest_digest
            {
                return base.block(
                    SelectedLoadoutBlockCodeV1::MeasuredEnvelopeManifestMismatch,
                    "measurement is not bound to the exact selected manifest".to_owned(),
                );
            }
            if evidence.payload().capability != selected.role {
                return base.block(
                    SelectedLoadoutBlockCodeV1::MeasuredEnvelopeCapabilityMismatch,
                    "measurement capability does not match the selected role".to_owned(),
                );
            }
            if evidence.payload().device_fingerprint_sha256 != live.device_fingerprint_sha256 {
                return base.block(
                    SelectedLoadoutBlockCodeV1::MeasuredEnvelopeWrongDevice,
                    "measurement is for a different device".to_owned(),
                );
            }
            if evidence.placement(&selected.preferred_residency).is_none() {
                return base.block(
                    SelectedLoadoutBlockCodeV1::UnknownPlacementMeasurement,
                    "the selected placement has no measured envelope".to_owned(),
                );
            }
            qualified.push(evidence);
        }

        let requests: Vec<_> = selection
            .roles
            .iter()
            .zip(&qualified)
            .map(|(selected, evidence)| LoadoutModelRequestV1 {
                role: selected.role.clone(),
                mode: selected.preferred_residency.clone(),
                envelope: evidence,
            })
            .collect();
        let receipt = match self
            .governor
            .admit(&live, context.now_monotonic_millis, &requests)
        {
            Ok(receipt) => receipt,
            Err(error) => {
                let (code, detail) = map_governor_error(error);
                return base.block(code, detail);
            }
        };
        let residency_decisions = plan_residency_from_evidence(
            selection.expected_idle_millis,
            &selection.roles,
            &qualified,
        );
        if require_target_pid {
            self.measurement_state = pending_trust;
            self.active_evidence = qualified
                .into_iter()
                .map(|evidence| (evidence.identity().clone(), evidence))
                .collect();
        }
        base.admit(receipt, residency_decisions)
    }

    pub fn plan_maintenance(
        &self,
        admission: &LoadoutAdmissionV1,
        expected_idle_millis: u64,
    ) -> Vec<MeasuredResidencyDecisionV1> {
        admission
            .models()
            .iter()
            .filter_map(|model| {
                self.active_evidence.get(&model.identity).map(|evidence| {
                    plan_one_residency(
                        model.role.clone(),
                        model.identity.clone(),
                        model.mode.clone(),
                        expected_idle_millis,
                        evidence,
                    )
                })
            })
            .collect()
    }

    pub fn submit_work(
        &mut self,
        item: WorkItemV1,
        now_monotonic_millis: u64,
    ) -> Result<WorkSubmitResultV1, WorkSchedulerError> {
        self.scheduler.submit(item, now_monotonic_millis)
    }

    pub fn advance_visual_clock(
        &mut self,
        generation_id: u64,
        frame_id: u64,
        now_monotonic_millis: u64,
    ) -> Result<Vec<WorkCancellationV1>, WorkSchedulerError> {
        self.scheduler
            .advance_visual_clock(generation_id, frame_id, now_monotonic_millis)
    }

    pub fn apply_pressure(
        &mut self,
        level: ResourcePressureLevelV1,
        now_monotonic_millis: u64,
    ) -> Vec<WorkCancellationV1> {
        self.scheduler.apply_pressure(level, now_monotonic_millis)
    }

    pub fn pop_next_work(&mut self, now_monotonic_millis: u64) -> Option<WorkItemV1> {
        self.scheduler.pop_next(now_monotonic_millis)
    }

    pub fn pop_next_work_kind(
        &mut self,
        kind: &crate::WorkKindV1,
        now_monotonic_millis: u64,
    ) -> Option<WorkItemV1> {
        self.scheduler.pop_next_kind(kind, now_monotonic_millis)
    }

    pub fn cancel_work_kind(
        &mut self,
        work_id: &str,
        kind: &crate::WorkKindV1,
        reason: CancellationReasonV1,
    ) -> Option<WorkCancellationV1> {
        self.scheduler.cancel_kind(work_id, kind, reason)
    }

    pub fn cancel_all_work_kind(
        &mut self,
        kind: &crate::WorkKindV1,
        reason: CancellationReasonV1,
    ) -> Vec<WorkCancellationV1> {
        self.scheduler.cancel_all_kind(kind, reason)
    }

    pub fn cancel_all_work(&mut self, reason: CancellationReasonV1) -> Vec<WorkCancellationV1> {
        self.scheduler.cancel_all(reason)
    }

    pub fn active_measurement(
        &self,
        identity: &PackRevision,
    ) -> Option<&crate::MeasuredResourceEnvelopePayloadV1> {
        self.active_evidence
            .get(identity)
            .map(|value| value.payload())
    }
}

fn plan_residency_from_evidence(
    expected_idle_millis: u64,
    roles: &[SelectedPackV1],
    evidence: &[QualifiedResourceEnvelopeV1],
) -> Vec<MeasuredResidencyDecisionV1> {
    roles
        .iter()
        .zip(evidence)
        .map(|(selected, evidence)| {
            plan_one_residency(
                selected.role.clone(),
                selected.identity.clone(),
                selected.preferred_residency.clone(),
                expected_idle_millis,
                evidence,
            )
        })
        .collect()
}

fn plan_one_residency(
    role: ModelPackKindV1,
    identity: PackRevision,
    current_mode: ResidencyModeV1,
    expected_idle_millis: u64,
    evidence: &QualifiedResourceEnvelopeV1,
) -> MeasuredResidencyDecisionV1 {
    let current_measurement = evidence
        .placement(&current_mode)
        .expect("an admitted placement remains present in its qualified envelope");
    let (current, disposition, compared, placement) = match current_mode {
        ResidencyModeV1::GpuResident
            if expected_idle_millis <= current_measurement.p99_reload_millis =>
        {
            (
                RuntimeResidencyStateV1::GpuWarm,
                ResidencyDispositionV1::KeepWarm,
                current_measurement.p99_reload_millis,
                ResidencyModeV1::GpuResident,
            )
        }
        ResidencyModeV1::GpuResident => {
            if let Some(cold) = evidence.placement(&ResidencyModeV1::CpuResidentGpuCold) {
                if expected_idle_millis <= cold.p99_reload_millis {
                    (
                        RuntimeResidencyStateV1::GpuWarm,
                        ResidencyDispositionV1::CpuResidentGpuCold,
                        cold.p99_reload_millis,
                        ResidencyModeV1::CpuResidentGpuCold,
                    )
                } else {
                    (
                        RuntimeResidencyStateV1::GpuWarm,
                        ResidencyDispositionV1::Unload,
                        cold.p99_reload_millis,
                        ResidencyModeV1::CpuResidentGpuCold,
                    )
                }
            } else {
                (
                    RuntimeResidencyStateV1::GpuWarm,
                    ResidencyDispositionV1::Unload,
                    current_measurement.p99_reload_millis,
                    ResidencyModeV1::GpuResident,
                )
            }
        }
        ResidencyModeV1::CpuResidentGpuCold => (
            RuntimeResidencyStateV1::CpuResidentGpuCold,
            if expected_idle_millis <= current_measurement.p99_reload_millis {
                ResidencyDispositionV1::CpuResidentGpuCold
            } else {
                ResidencyDispositionV1::Unload
            },
            current_measurement.p99_reload_millis,
            ResidencyModeV1::CpuResidentGpuCold,
        ),
        ResidencyModeV1::CpuResident => (
            RuntimeResidencyStateV1::CpuWarm,
            if expected_idle_millis <= current_measurement.p99_reload_millis {
                ResidencyDispositionV1::KeepWarm
            } else {
                ResidencyDispositionV1::Unload
            },
            current_measurement.p99_reload_millis,
            ResidencyModeV1::CpuResident,
        ),
    };
    MeasuredResidencyDecisionV1 {
        role,
        identity,
        current,
        disposition,
        expected_idle_millis,
        compared_p99_reload_millis: compared,
        evidence_placement: placement,
        measured_envelope_sha256: evidence.payload_sha256().clone(),
    }
}

fn validate_selection(selection: &SelectedLoadoutSelectionV1) -> Result<(), String> {
    if !(8..=128).contains(&selection.selection_id.len())
        || !selection
            .selection_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err("selection ID is invalid".to_owned());
    }
    if selection.roles.is_empty() || selection.roles.len() > 8 {
        return Err("selected loadout must contain between one and eight roles".to_owned());
    }
    if selection.expected_idle_millis > 24 * 60 * 60 * 1_000 {
        return Err("expected idle scheduling target exceeds 24 hours".to_owned());
    }
    let mut roles = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for selected in &selection.roles {
        if !roles.insert(selected.role.clone()) {
            return Err(format!("duplicate selected role: {:?}", selected.role));
        }
        if !identities.insert(selected.identity.clone()) {
            return Err(format!(
                "one pack revision cannot fill multiple roles: {:?}",
                selected.identity
            ));
        }
    }
    Ok(())
}

struct DecisionBuilderV1 {
    schema: String,
    selection_id: String,
    exact_target_pid: Option<u32>,
    selected_roles: Vec<SelectedPackV1>,
    live_snapshot: Option<ResourceTelemetrySnapshotV1>,
    pressure_cancellations: Vec<WorkCancellationV1>,
}

impl DecisionBuilderV1 {
    fn new(selection: &SelectedLoadoutSelectionV1, context: &NativeAdmissionContextV1<'_>) -> Self {
        Self {
            schema: SELECTED_LOADOUT_API_SCHEMA_V1.to_owned(),
            selection_id: selection.selection_id.clone(),
            exact_target_pid: context.exact_target_pid,
            selected_roles: selection.roles.clone(),
            live_snapshot: context.telemetry.cloned(),
            pressure_cancellations: Vec::new(),
        }
    }

    fn block(
        self,
        reason_code: SelectedLoadoutBlockCodeV1,
        detail: String,
    ) -> SelectedLoadoutDecisionV1 {
        SelectedLoadoutDecisionV1 {
            schema: self.schema,
            selection_id: self.selection_id,
            status: SelectedLoadoutStatusV1::Blocked,
            reason_code: Some(reason_code),
            detail,
            exact_target_pid: self.exact_target_pid,
            selected_roles: self.selected_roles,
            live_snapshot: self.live_snapshot,
            admission_receipt: None,
            residency_decisions: Vec::new(),
            pressure_cancellations: self.pressure_cancellations,
        }
    }

    fn admit(
        self,
        admission_receipt: LoadoutAdmissionV1,
        residency_decisions: Vec<MeasuredResidencyDecisionV1>,
    ) -> SelectedLoadoutDecisionV1 {
        let detail = if self.exact_target_pid.is_some() {
            "exact selected loadout passed current-device whole-loadout admission"
        } else {
            "exact selected loadout passed setup-only current-device admission with configured game reserves; this receipt is not runtime target authority"
        };
        SelectedLoadoutDecisionV1 {
            schema: self.schema,
            selection_id: self.selection_id,
            status: SelectedLoadoutStatusV1::Admitted,
            reason_code: None,
            detail: detail.to_owned(),
            exact_target_pid: self.exact_target_pid,
            selected_roles: self.selected_roles,
            live_snapshot: self.live_snapshot,
            admission_receipt: Some(admission_receipt),
            residency_decisions,
            pressure_cancellations: self.pressure_cancellations,
        }
    }
}

fn map_admission_view_error(error: AdmissionViewError) -> (SelectedLoadoutBlockCodeV1, String) {
    let code = match error {
        AdmissionViewError::RequiredMetricUnavailable(_) => {
            SelectedLoadoutBlockCodeV1::RequiredTelemetryUnavailable
        }
        AdmissionViewError::InvalidSnapshot(_)
        | AdmissionViewError::GameVramExceedsTotalPressure => {
            SelectedLoadoutBlockCodeV1::InvalidTelemetry
        }
    };
    (code, error.to_string())
}

fn map_envelope_error(error: ResourceEnvelopeError) -> (SelectedLoadoutBlockCodeV1, String) {
    let code = match error {
        ResourceEnvelopeError::Expired => SelectedLoadoutBlockCodeV1::MeasuredEnvelopeExpired,
        _ => SelectedLoadoutBlockCodeV1::MeasuredEnvelopeUntrusted,
    };
    (code, error.to_string())
}

fn map_governor_error(error: ResourceAdmissionError) -> (SelectedLoadoutBlockCodeV1, String) {
    let code = match error {
        ResourceAdmissionError::StaleSnapshot { .. }
        | ResourceAdmissionError::SnapshotFromFuture => SelectedLoadoutBlockCodeV1::StaleTelemetry,
        ResourceAdmissionError::InvalidSnapshot => SelectedLoadoutBlockCodeV1::InvalidTelemetry,
        ResourceAdmissionError::DeviceFingerprintMismatch => {
            SelectedLoadoutBlockCodeV1::MeasuredEnvelopeWrongDevice
        }
        ResourceAdmissionError::CapabilityMismatch { .. } => {
            SelectedLoadoutBlockCodeV1::MeasuredEnvelopeCapabilityMismatch
        }
        ResourceAdmissionError::UnknownEnvelope { .. } => {
            SelectedLoadoutBlockCodeV1::UnknownPlacementMeasurement
        }
        ResourceAdmissionError::VramSoftCeilingExceeded { .. } => {
            SelectedLoadoutBlockCodeV1::VramContention
        }
        ResourceAdmissionError::RamSoftCeilingExceeded { .. } => {
            SelectedLoadoutBlockCodeV1::RamContention
        }
        ResourceAdmissionError::ArithmeticOverflow => {
            SelectedLoadoutBlockCodeV1::ResourceArithmetic
        }
        _ => SelectedLoadoutBlockCodeV1::InvalidSelection,
    };
    (code, error.to_string())
}

#[derive(Debug, thiserror::Error)]
pub enum SelectedLoadoutManagerBuildErrorV1 {
    #[error("resource governor policy is invalid: {0}")]
    Governor(ResourceAdmissionError),
    #[error("work queue policy is invalid: {0}")]
    Scheduler(WorkSchedulerError),
    #[error("measurement policy signature threshold cannot be zero")]
    InvalidMeasurementPolicy,
}
