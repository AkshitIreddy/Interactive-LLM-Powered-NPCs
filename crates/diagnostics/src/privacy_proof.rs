use crate::{check::is_identifier, ObservationProvenance};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const PRIVACY_PROOF_SCHEMA_JSON: &str = include_str!("../schemas/privacy-proof-v1.schema.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactIdentity {
    pub application_version: String,
    pub release_candidate_id: String,
    pub package_manifest_sha256: String,
    pub package_sha256: String,
    pub installed_distribution_manifest_sha256: String,
    pub executable_sha256: String,
}

impl ArtifactIdentity {
    fn validate(&self) -> Result<(), PrivacyProofError> {
        if self.application_version.trim().is_empty()
            || self.application_version.len() > 64
            || !is_identifier(&self.release_candidate_id, 128)
            || !is_sha256(&self.package_manifest_sha256)
            || !is_sha256(&self.package_sha256)
            || !is_sha256(&self.installed_distribution_manifest_sha256)
            || !is_sha256(&self.executable_sha256)
        {
            return Err(PrivacyProofError::InvalidArtifactIdentity);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofEvidenceSource {
    PackagedExecutableObservation,
    FixtureHarness,
    NotObserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofOutcome {
    Passed,
    Failed,
    NotMeasured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteTelemetryAbsenceProof {
    pub schema_version: String,
    pub artifact: ArtifactIdentity,
    pub provenance: ObservationProvenance,
    pub evidence_source: ProofEvidenceSource,
    pub evidence_run_id: String,
    pub observed_at_utc: Option<String>,
    pub dependency_inventory_scanned: bool,
    pub endpoint_inventory_scanned: bool,
    pub automatic_upload_entry_points: u64,
    pub remote_telemetry_destinations: u64,
    pub outcome: ProofOutcome,
}

impl RemoteTelemetryAbsenceProof {
    pub fn validate(&self) -> Result<(), PrivacyProofError> {
        self.artifact.validate()?;
        validate_header(
            &self.schema_version,
            self.provenance,
            self.evidence_source,
            &self.evidence_run_id,
            self.observed_at_utc.as_deref(),
        )?;
        let passed = self.dependency_inventory_scanned
            && self.endpoint_inventory_scanned
            && self.automatic_upload_entry_points == 0
            && self.remote_telemetry_destinations == 0;
        validate_outcome(self.provenance, self.outcome, passed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyAllScenario {
    OfflineMode,
    LocalLipSync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkEnforcement {
    OsDenyAll,
    TestHarness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DenyAllEgressProof {
    pub schema_version: String,
    pub artifact: ArtifactIdentity,
    pub scenario: DenyAllScenario,
    pub provenance: ObservationProvenance,
    pub evidence_source: ProofEvidenceSource,
    pub evidence_run_id: String,
    pub observed_at_utc: Option<String>,
    pub enforcement: NetworkEnforcement,
    pub monitored_process_count: u32,
    pub observed_connection_attempts: u64,
    pub observed_provider_requests: u64,
    pub external_destination_count: u64,
    pub outcome: ProofOutcome,
}

impl DenyAllEgressProof {
    pub fn validate(&self) -> Result<(), PrivacyProofError> {
        self.artifact.validate()?;
        validate_header(
            &self.schema_version,
            self.provenance,
            self.evidence_source,
            &self.evidence_run_id,
            self.observed_at_utc.as_deref(),
        )?;
        let passed = self.enforcement == NetworkEnforcement::OsDenyAll
            && self.monitored_process_count > 0
            && self.observed_connection_attempts == 0
            && self.observed_provider_requests == 0
            && self.external_destination_count == 0;
        validate_outcome(self.provenance, self.outcome, passed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrivacyProofBundle {
    pub schema_version: String,
    pub capture_evidence_sha256: String,
    pub remote_telemetry_absence: RemoteTelemetryAbsenceProof,
    pub deny_all_egress: Vec<DenyAllEgressProof>,
}

impl PrivacyProofBundle {
    pub fn validate(&self) -> Result<(), PrivacyProofError> {
        if self.schema_version != "1.0.0" || !is_sha256(&self.capture_evidence_sha256) {
            return Err(PrivacyProofError::InvalidSchema);
        }
        self.remote_telemetry_absence.validate()?;
        if self.remote_telemetry_absence.provenance != ObservationProvenance::Measured
            || self.remote_telemetry_absence.outcome != ProofOutcome::Passed
        {
            return Err(PrivacyProofError::InsufficientProvenance);
        }
        let expected_artifact = &self.remote_telemetry_absence.artifact;
        let mut scenarios = BTreeSet::new();
        for proof in &self.deny_all_egress {
            proof.validate()?;
            if proof.provenance != ObservationProvenance::Measured
                || proof.outcome != ProofOutcome::Passed
            {
                return Err(PrivacyProofError::InsufficientProvenance);
            }
            if &proof.artifact != expected_artifact {
                return Err(PrivacyProofError::ArtifactMismatch);
            }
            if !scenarios.insert(proof.scenario) {
                return Err(PrivacyProofError::DuplicateScenario);
            }
        }
        if scenarios
            != BTreeSet::from([DenyAllScenario::OfflineMode, DenyAllScenario::LocalLipSync])
        {
            return Err(PrivacyProofError::MissingScenario);
        }
        Ok(())
    }
}

fn validate_header(
    schema_version: &str,
    provenance: ObservationProvenance,
    evidence_source: ProofEvidenceSource,
    evidence_run_id: &str,
    observed_at_utc: Option<&str>,
) -> Result<(), PrivacyProofError> {
    if schema_version != "1.0.0" {
        return Err(PrivacyProofError::InvalidSchema);
    }
    if provenance == ObservationProvenance::Measured {
        if evidence_source != ProofEvidenceSource::PackagedExecutableObservation
            || !is_identifier(evidence_run_id, 128)
        {
            return Err(PrivacyProofError::InvalidEvidenceBinding);
        }
        if observed_at_utc.is_none_or(|value| !is_utc_timestamp(value)) {
            return Err(PrivacyProofError::MissingMeasurementTime);
        }
    } else {
        let expected_source = match provenance {
            ObservationProvenance::Fixture => ProofEvidenceSource::FixtureHarness,
            ObservationProvenance::Unmeasured => ProofEvidenceSource::NotObserved,
            ObservationProvenance::Measured => unreachable!(),
        };
        if evidence_source != expected_source
            || observed_at_utc.is_some()
            || !is_identifier(evidence_run_id, 128)
        {
            return Err(PrivacyProofError::InvalidEvidenceBinding);
        }
    }
    Ok(())
}

fn validate_outcome(
    provenance: ObservationProvenance,
    outcome: ProofOutcome,
    passed: bool,
) -> Result<(), PrivacyProofError> {
    match (provenance, outcome, passed) {
        (ObservationProvenance::Measured, ProofOutcome::Passed, true)
        | (ObservationProvenance::Measured, ProofOutcome::Failed, false)
        | (ObservationProvenance::Fixture, ProofOutcome::NotMeasured, _)
        | (ObservationProvenance::Unmeasured, ProofOutcome::NotMeasured, _) => Ok(()),
        _ => Err(PrivacyProofError::ContradictoryOutcome),
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_utc_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if !(20..=40).contains(&bytes.len())
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || *bytes.last().unwrap_or(&0) != b'Z'
    {
        return false;
    }
    for index in [0usize, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18] {
        if !bytes[index].is_ascii_digit() {
            return false;
        }
    }
    bytes.len() == 20
        || (bytes[19] == b'.' && bytes[20..bytes.len() - 1].iter().all(u8::is_ascii_digit))
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PrivacyProofError {
    #[error("privacy proof schema is invalid")]
    InvalidSchema,
    #[error("packaged artifact identity is invalid")]
    InvalidArtifactIdentity,
    #[error("measured proof requires an observation timestamp")]
    MissingMeasurementTime,
    #[error("unmeasured proof cannot carry a measurement timestamp")]
    UnmeasuredTimestamp,
    #[error("privacy proof source, run, timestamp, and provenance are inconsistent")]
    InvalidEvidenceBinding,
    #[error("privacy proof outcome contradicts its evidence")]
    ContradictoryOutcome,
    #[error("privacy proof bundle mixes artifact identities")]
    ArtifactMismatch,
    #[error("privacy proof bundle repeats a deny-all scenario")]
    DuplicateScenario,
    #[error("privacy proof bundle omits a required deny-all scenario")]
    MissingScenario,
    #[error("packaged privacy acceptance requires measured passing evidence")]
    InsufficientProvenance,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact() -> ArtifactIdentity {
        ArtifactIdentity {
            application_version: "2.0.0-alpha.1".into(),
            release_candidate_id: "rc-2026-08-30-01".into(),
            package_manifest_sha256: "c".repeat(64),
            package_sha256: "a".repeat(64),
            installed_distribution_manifest_sha256: "d".repeat(64),
            executable_sha256: "b".repeat(64),
        }
    }

    fn deny_all(scenario: DenyAllScenario) -> DenyAllEgressProof {
        DenyAllEgressProof {
            schema_version: "1.0.0".into(),
            artifact: artifact(),
            scenario,
            provenance: ObservationProvenance::Measured,
            evidence_source: ProofEvidenceSource::PackagedExecutableObservation,
            evidence_run_id: match scenario {
                DenyAllScenario::OfflineMode => "privacy-run-offline".into(),
                DenyAllScenario::LocalLipSync => "privacy-run-local-lip-sync".into(),
            },
            observed_at_utc: Some("2026-08-30T10:00:00Z".into()),
            enforcement: NetworkEnforcement::OsDenyAll,
            monitored_process_count: 3,
            observed_connection_attempts: 0,
            observed_provider_requests: 0,
            external_destination_count: 0,
            outcome: ProofOutcome::Passed,
        }
    }

    #[test]
    fn measured_bundle_requires_both_packaged_deny_all_scenarios() {
        let telemetry = RemoteTelemetryAbsenceProof {
            schema_version: "1.0.0".into(),
            artifact: artifact(),
            provenance: ObservationProvenance::Measured,
            evidence_source: ProofEvidenceSource::PackagedExecutableObservation,
            evidence_run_id: "privacy-run-telemetry".into(),
            observed_at_utc: Some("2026-08-30T10:00:00Z".into()),
            dependency_inventory_scanned: true,
            endpoint_inventory_scanned: true,
            automatic_upload_entry_points: 0,
            remote_telemetry_destinations: 0,
            outcome: ProofOutcome::Passed,
        };
        let mut bundle = PrivacyProofBundle {
            schema_version: "1.0.0".into(),
            capture_evidence_sha256: "e".repeat(64),
            remote_telemetry_absence: telemetry,
            deny_all_egress: vec![deny_all(DenyAllScenario::OfflineMode)],
        };
        assert_eq!(bundle.validate(), Err(PrivacyProofError::MissingScenario));
        bundle
            .deny_all_egress
            .push(deny_all(DenyAllScenario::LocalLipSync));
        assert_eq!(bundle.validate(), Ok(()));
        let value = serde_json::to_value(&bundle).unwrap();
        let schema: serde_json::Value = serde_json::from_str(PRIVACY_PROOF_SCHEMA_JSON).unwrap();
        assert_required_fields(&value, &schema);
        assert_required_fields(
            &value["remoteTelemetryAbsence"],
            &schema["$defs"]["telemetryProof"],
        );
        for proof in value["denyAllEgress"].as_array().unwrap() {
            assert_required_fields(proof, &schema["$defs"]["denyAllProof"]);
        }
    }

    #[test]
    fn fixture_or_internal_harness_cannot_masquerade_as_packaged_pass() {
        let mut proof = deny_all(DenyAllScenario::OfflineMode);
        proof.enforcement = NetworkEnforcement::TestHarness;
        assert_eq!(
            proof.validate(),
            Err(PrivacyProofError::ContradictoryOutcome)
        );
        proof.enforcement = NetworkEnforcement::OsDenyAll;
        proof.provenance = ObservationProvenance::Fixture;
        proof.evidence_source = ProofEvidenceSource::FixtureHarness;
        proof.observed_at_utc = None;
        assert_eq!(
            proof.validate(),
            Err(PrivacyProofError::ContradictoryOutcome)
        );
    }

    #[test]
    fn measured_proof_rejects_ambiguous_timestamp_or_unbound_source() {
        let mut proof = deny_all(DenyAllScenario::OfflineMode);
        proof.observed_at_utc = Some("2026-08-30 local time".into());
        assert_eq!(
            proof.validate(),
            Err(PrivacyProofError::MissingMeasurementTime)
        );
        proof.observed_at_utc = Some("2026-08-30T10:00:00Z".into());
        proof.evidence_source = ProofEvidenceSource::FixtureHarness;
        assert_eq!(
            proof.validate(),
            Err(PrivacyProofError::InvalidEvidenceBinding)
        );
    }

    #[test]
    fn bundled_schema_is_machine_readable_and_candidate_bound() {
        let schema: serde_json::Value = serde_json::from_str(PRIVACY_PROOF_SCHEMA_JSON).unwrap();
        assert_eq!(schema["$id"], "interactive-npcs/privacy-proof-v1");
        let text = schema.to_string();
        assert!(text.contains("releaseCandidateId"));
        assert!(text.contains("packaged_executable_observation"));
        assert!(text.contains("local_lip_sync"));
    }

    fn assert_required_fields(value: &serde_json::Value, schema: &serde_json::Value) {
        let object = value.as_object().unwrap();
        for required in schema["required"].as_array().unwrap() {
            assert!(object.contains_key(required.as_str().unwrap()));
        }
        assert_eq!(
            object.len(),
            schema["properties"].as_object().unwrap().len(),
            "deny_unknown_fields DTO and schema properties drifted"
        );
    }
}
