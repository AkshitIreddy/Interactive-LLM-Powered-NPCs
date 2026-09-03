use crate::{
    CheckResult, DiagnosticEvent, PrivacyEgressDeclaration, RecoveryMetadata, RedactionFinding,
    Redactor, MAX_CHECK_RESULTS,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use thiserror::Error;

pub const DIAGNOSTICS_EXPORT_SCHEMA_JSON: &str =
    include_str!("../schemas/diagnostics-export-v2.schema.json");
pub const MAX_EXPORT_EVENTS: usize = 4_096;
pub const MAX_EXPORT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsExport {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub application_version: String,
    pub privacy_egress: PrivacyEgressDeclaration,
    pub check_results: Vec<CheckResult>,
    pub events: Vec<DiagnosticEvent>,
    pub recovery: Option<RecoveryMetadata>,
    pub redaction_summary: Vec<RedactionFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsExportPreview {
    pub schema_version: String,
    pub check_count: usize,
    pub event_count: usize,
    pub includes_recovery_metadata: bool,
    pub serialized_bytes: usize,
    pub sha256: String,
    pub redactions: Vec<RedactionFinding>,
    pub remote_telemetry: bool,
    pub automatic_upload: bool,
}

#[derive(Debug, Error)]
pub enum DiagnosticsExportError {
    #[error("diagnostics export identity is invalid")]
    InvalidIdentity,
    #[error("diagnostics export privacy declaration is invalid: {0}")]
    InvalidPrivacy(&'static str),
    #[error("diagnostics export contains too many checks")]
    TooManyChecks,
    #[error("diagnostics export contains duplicate check id {0}")]
    DuplicateCheck(String),
    #[error("diagnostics check is invalid: {0}")]
    InvalidCheck(String),
    #[error("diagnostics export contains too many events")]
    TooManyEvents,
    #[error("diagnostics event is invalid: {0}")]
    InvalidEvent(&'static str),
    #[error("diagnostics recovery metadata is invalid: {0}")]
    InvalidRecovery(String),
    #[error("diagnostics export exceeds its hard size bound")]
    TooLarge,
    #[error("diagnostics export serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub struct DiagnosticsExportBuilder {
    generated_at_utc: String,
    application_version: String,
    privacy_egress: PrivacyEgressDeclaration,
    check_results: Vec<CheckResult>,
    events: Vec<DiagnosticEvent>,
    recovery: Option<RecoveryMetadata>,
    redactor: Redactor,
}

impl DiagnosticsExportBuilder {
    pub fn new(
        generated_at_utc: impl Into<String>,
        application_version: impl Into<String>,
    ) -> Self {
        Self {
            generated_at_utc: generated_at_utc.into(),
            application_version: application_version.into(),
            privacy_egress: PrivacyEgressDeclaration::local_diagnostics_v1(),
            check_results: Vec::new(),
            events: Vec::new(),
            recovery: None,
            redactor: Redactor,
        }
    }

    pub fn set_privacy_declaration(
        &mut self,
        declaration: PrivacyEgressDeclaration,
    ) -> Result<(), DiagnosticsExportError> {
        declaration
            .validate()
            .map_err(DiagnosticsExportError::InvalidPrivacy)?;
        self.privacy_egress = declaration;
        Ok(())
    }

    pub fn push_check(&mut self, check: CheckResult) -> Result<(), DiagnosticsExportError> {
        if self.check_results.len() >= MAX_CHECK_RESULTS {
            return Err(DiagnosticsExportError::TooManyChecks);
        }
        check
            .validate()
            .map_err(|error| DiagnosticsExportError::InvalidCheck(error.to_string()))?;
        if self
            .check_results
            .iter()
            .any(|existing| existing.check_id == check.check_id)
        {
            return Err(DiagnosticsExportError::DuplicateCheck(check.check_id));
        }
        self.check_results.push(check);
        Ok(())
    }

    pub fn push_event(&mut self, event: DiagnosticEvent) -> Result<(), DiagnosticsExportError> {
        if self.events.len() >= MAX_EXPORT_EVENTS {
            return Err(DiagnosticsExportError::TooManyEvents);
        }
        event
            .validate()
            .map_err(DiagnosticsExportError::InvalidEvent)?;
        self.events.push(event);
        Ok(())
    }

    pub fn set_recovery(
        &mut self,
        recovery: RecoveryMetadata,
    ) -> Result<(), DiagnosticsExportError> {
        recovery
            .validate()
            .map_err(|error| DiagnosticsExportError::InvalidRecovery(error.to_string()))?;
        self.recovery = Some(recovery);
        Ok(())
    }

    pub fn build(
        self,
    ) -> Result<(DiagnosticsExport, DiagnosticsExportPreview), DiagnosticsExportError> {
        if !valid_identity(&self.generated_at_utc, 64)
            || !valid_identity(&self.application_version, 64)
        {
            return Err(DiagnosticsExportError::InvalidIdentity);
        }
        self.privacy_egress
            .validate()
            .map_err(DiagnosticsExportError::InvalidPrivacy)?;
        let raw = DiagnosticsExport {
            schema_version: "2.0.0".into(),
            generated_at_utc: self.generated_at_utc,
            application_version: self.application_version,
            privacy_egress: self.privacy_egress,
            check_results: self.check_results,
            events: self.events,
            recovery: self.recovery,
            redaction_summary: Vec::new(),
        };
        let raw_value = serde_json::to_value(&raw)?;
        let (redacted_value, redactions) = self.redactor.redact_json_value(&raw_value);
        let mut export: DiagnosticsExport = serde_json::from_value(redacted_value)?;
        export.redaction_summary = redactions;
        let serialized = serde_json::to_vec_pretty(&export)?;
        if serialized.len() > MAX_EXPORT_BYTES {
            return Err(DiagnosticsExportError::TooLarge);
        }
        let preview = DiagnosticsExportPreview {
            schema_version: export.schema_version.clone(),
            check_count: export.check_results.len(),
            event_count: export.events.len(),
            includes_recovery_metadata: export.recovery.is_some(),
            serialized_bytes: serialized.len(),
            sha256: format!("{:x}", Sha256::digest(&serialized)),
            redactions: export.redaction_summary.clone(),
            remote_telemetry: false,
            automatic_upload: false,
        };
        Ok((export, preview))
    }
}

fn valid_identity(value: &str, max_len: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_len && !value.chars().any(char::is_control)
}

/// Checks the invariants which the bundled JSON Schema cannot express alone.
pub fn validate_export(export: &DiagnosticsExport) -> Result<(), DiagnosticsExportError> {
    if export.schema_version != "2.0.0"
        || !valid_identity(&export.generated_at_utc, 64)
        || !valid_identity(&export.application_version, 64)
    {
        return Err(DiagnosticsExportError::InvalidIdentity);
    }
    export
        .privacy_egress
        .validate()
        .map_err(DiagnosticsExportError::InvalidPrivacy)?;
    if export.check_results.len() > MAX_CHECK_RESULTS {
        return Err(DiagnosticsExportError::TooManyChecks);
    }
    let mut ids = BTreeSet::new();
    for check in &export.check_results {
        check
            .validate()
            .map_err(|error| DiagnosticsExportError::InvalidCheck(error.to_string()))?;
        if !ids.insert(check.check_id.clone()) {
            return Err(DiagnosticsExportError::DuplicateCheck(
                check.check_id.clone(),
            ));
        }
    }
    if export.events.len() > MAX_EXPORT_EVENTS {
        return Err(DiagnosticsExportError::TooManyEvents);
    }
    for event in &export.events {
        event
            .validate()
            .map_err(DiagnosticsExportError::InvalidEvent)?;
    }
    if let Some(recovery) = &export.recovery {
        recovery
            .validate()
            .map_err(|error| DiagnosticsExportError::InvalidRecovery(error.to_string()))?;
    }
    if serde_json::to_vec_pretty(export)?.len() > MAX_EXPORT_BYTES {
        return Err(DiagnosticsExportError::TooLarge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CheckCategory, DiagnosticMetric, DiagnosticStatus, MetricUnit, ObservationProvenance,
        SuggestedAction, SuggestedActionKind,
    };
    use std::collections::BTreeMap;

    fn check() -> CheckResult {
        CheckResult {
            check_id: "hardware.gpu_budget".into(),
            category: CheckCategory::Hardware,
            status: DiagnosticStatus::Degraded,
            provenance: ObservationProvenance::Measured,
            observed_at_utc: Some("2026-08-30T10:00:00Z".into()),
            duration_ms: Some(4.0),
            timing: Some(crate::TimingEvidence {
                clock_source: crate::ClockSource::ProcessMonotonic,
                provenance: ObservationProvenance::Measured,
                started_monotonic_ns: 10,
                completed_monotonic_ns: 4_000_010,
                duration_ms: 4.0,
            }),
            summary_code: "hardware.vram_low".into(),
            summary: "VRAM reserve is below the configured target.".into(),
            error_code: None,
            provider_id: None,
            model_id: None,
            metrics: BTreeMap::from([(
                "available_vram".into(),
                DiagnosticMetric {
                    value: 1024.0,
                    unit: MetricUnit::Mebibytes,
                    provenance: ObservationProvenance::Measured,
                },
            )]),
            suggested_actions: vec![SuggestedAction {
                action_id: "models.reduce_load".into(),
                kind: SuggestedActionKind::ReduceLocalModelLoad,
                label: "Unload an optional local model".into(),
                target_id: Some("local_models".into()),
                requires_confirmation: true,
            }],
        }
    }

    #[test]
    fn export_is_redacted_bounded_hashed_and_local_only() {
        let mut builder = DiagnosticsExportBuilder::new("2026-08-30T10:00:00Z", "2.0.0-alpha.1");
        let mut value = check();
        value.summary = "provider token=canary-secret-123 failed for user@example.com".into();
        builder.push_check(value).unwrap();
        let (export, preview) = builder.build().unwrap();
        let serialized = serde_json::to_string(&export).unwrap();
        assert!(!serialized.contains("canary-secret"));
        assert!(!serialized.contains("user@example.com"));
        assert!(serialized.len() <= MAX_EXPORT_BYTES);
        assert_eq!(preview.sha256.len(), 64);
        assert!(!preview.remote_telemetry);
        assert!(!preview.automatic_upload);
        assert!(validate_export(&export).is_ok());
    }

    #[test]
    fn duplicate_check_ids_are_rejected() {
        let mut builder = DiagnosticsExportBuilder::new("2026-08-30T10:00:00Z", "2.0.0");
        builder.push_check(check()).unwrap();
        assert!(matches!(
            builder.push_check(check()),
            Err(DiagnosticsExportError::DuplicateCheck(_))
        ));
    }

    #[test]
    fn redacted_provider_identifier_still_satisfies_export_contract() {
        let mut builder = DiagnosticsExportBuilder::new("2026-08-30T10:00:00Z", "2.0.0");
        let mut value = check();
        value.provider_id = Some(["sk", "-proj-", "canaryabcdefghijklmnop"].concat());
        builder.push_check(value).unwrap();
        let (export, _) = builder.build().unwrap();
        assert_eq!(
            export.check_results[0].provider_id.as_deref(),
            Some("<REDACTED_PROVIDER_CREDENTIAL>")
        );
        assert!(validate_export(&export).is_ok());
    }

    #[test]
    fn bundled_schema_is_valid_json_and_describes_local_privacy_fields() {
        let schema: serde_json::Value =
            serde_json::from_str(DIAGNOSTICS_EXPORT_SCHEMA_JSON).unwrap();
        assert_eq!(schema["$id"], "interactive-npcs/diagnostics-export-v2");
        let text = schema.to_string();
        assert!(text.contains("privacyEgress"));
        assert!(text.contains("redactionSummary"));
    }
}
