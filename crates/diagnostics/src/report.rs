use crate::{
    redaction::is_sensitive_field_name, DiagnosticStatus, RedactionFinding, RedactionKind, Redactor,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticFact {
    pub check_id: String,
    pub label: String,
    pub status: DiagnosticStatus,
    pub summary: String,
    pub fix: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub application_version: String,
    pub facts: Vec<DiagnosticFact>,
    pub redaction_summary: Vec<RedactionFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreview {
    pub fact_count: usize,
    pub failed_count: usize,
    pub serialized_bytes: usize,
    pub sha256: String,
    pub redactions: Vec<RedactionFinding>,
    pub excluded_by_design: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("diagnostic fact {0} is missing an identifier or label")]
    MissingIdentity(usize),
    #[error("diagnostic fact {0} exceeds content limits")]
    TooLarge(usize),
    #[error("diagnostic report exceeds its fact count bound")]
    TooManyFacts,
    #[error("failed to serialize diagnostic report: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub struct DiagnosticReportBuilder {
    generated_at_utc: String,
    application_version: String,
    facts: Vec<DiagnosticFact>,
    redactor: Redactor,
}

impl DiagnosticReportBuilder {
    pub fn new(
        generated_at_utc: impl Into<String>,
        application_version: impl Into<String>,
    ) -> Self {
        Self {
            generated_at_utc: generated_at_utc.into(),
            application_version: application_version.into(),
            facts: Vec::new(),
            redactor: Redactor,
        }
    }

    pub fn push(&mut self, fact: DiagnosticFact) -> Result<(), ReportError> {
        let index = self.facts.len();
        if self.facts.len() >= 256 {
            return Err(ReportError::TooManyFacts);
        }
        if fact.check_id.trim().is_empty()
            || fact.label.trim().is_empty()
            || fact.check_id.len() > 128
            || fact.label.len() > 240
        {
            return Err(ReportError::MissingIdentity(index));
        }
        if fact.summary.len() > 2_000
            || fact.fix.as_ref().is_some_and(|value| value.len() > 2_000)
            || fact.metadata.len() > 32
            || fact
                .metadata
                .iter()
                .any(|(key, value)| key.is_empty() || key.len() > 128 || value.len() > 2_000)
        {
            return Err(ReportError::TooLarge(index));
        }
        self.facts.push(fact);
        Ok(())
    }

    pub fn build(self) -> (DiagnosticReport, ExportPreview) {
        let mut report = DiagnosticReport {
            schema_version: "1.0.0".into(),
            generated_at_utc: self.generated_at_utc,
            application_version: self.application_version,
            facts: self.facts,
            redaction_summary: Vec::new(),
        };
        let mut redactions = Vec::new();
        for fact in &mut report.facts {
            redact_field(&self.redactor, &mut fact.summary, &mut redactions);
            if let Some(fix) = &mut fact.fix {
                redact_field(&self.redactor, fix, &mut redactions);
            }
            for (key, value) in &mut fact.metadata {
                if is_sensitive_field_name(key) {
                    *value = "<REDACTED_FIELD>".into();
                    redactions.push(RedactionFinding {
                        kind: RedactionKind::SensitiveField,
                        count: 1,
                    });
                } else {
                    redact_field(&self.redactor, value, &mut redactions);
                }
            }
        }
        report.redaction_summary = consolidate(redactions);
        let serialized =
            serde_json::to_vec_pretty(&report).expect("report serialization is infallible");
        let preview = ExportPreview {
            fact_count: report.facts.len(),
            failed_count: report
                .facts
                .iter()
                .filter(|fact| fact.status == DiagnosticStatus::Failed)
                .count(),
            serialized_bytes: serialized.len(),
            sha256: format!("{:x}", Sha256::digest(&serialized)),
            redactions: report.redaction_summary.clone(),
            excluded_by_design: vec![
                "credentials".into(),
                "prompts and responses".into(),
                "transcripts".into(),
                "audio and screenshots".into(),
                "webcam frames".into(),
                "minidumps".into(),
                "absolute user paths".into(),
            ],
        };
        (report, preview)
    }
}

fn redact_field(redactor: &Redactor, value: &mut String, findings: &mut Vec<RedactionFinding>) {
    let (redacted, found) = redactor.redact(value);
    *value = redacted;
    findings.extend(found);
}

fn consolidate(findings: Vec<RedactionFinding>) -> Vec<RedactionFinding> {
    let mut counts = BTreeMap::new();
    for finding in findings {
        *counts.entry(finding.kind).or_insert(0usize) += finding.count;
    }
    counts
        .into_iter()
        .map(|(kind, count)| RedactionFinding { kind, count })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_redacts_before_hashing_and_previewing() {
        let mut builder = DiagnosticReportBuilder::new("2026-08-28T00:00:00Z", "2.0.0");
        builder
            .push(DiagnosticFact {
                check_id: "provider.auth".into(),
                label: "Provider connection".into(),
                status: DiagnosticStatus::Failed,
                summary: "token=canary-secret-123 was rejected for user@example.com".into(),
                fix: Some("Re-enter the credential".into()),
                metadata: BTreeMap::new(),
            })
            .unwrap();
        let (report, preview) = builder.build();
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("canary-secret"));
        assert!(!serialized.contains("user@example.com"));
        assert_eq!(preview.failed_count, 1);
        assert_eq!(
            preview
                .redactions
                .iter()
                .map(|item| item.count)
                .sum::<usize>(),
            2
        );
        assert_eq!(preview.sha256.len(), 64);
    }

    #[test]
    fn legacy_report_redacts_short_credentials_by_metadata_key() {
        let mut builder = DiagnosticReportBuilder::new("2026-08-28T00:00:00Z", "2.0.0");
        builder
            .push(DiagnosticFact {
                check_id: "provider.auth".into(),
                label: "Provider connection".into(),
                status: DiagnosticStatus::Failed,
                summary: "Authentication failed".into(),
                fix: None,
                metadata: BTreeMap::from([
                    ("apiKey".into(), "abc".into()),
                    ("safe_code".into(), "provider.auth_failed".into()),
                ]),
            })
            .unwrap();
        let (report, _) = builder.build();
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("\"abc\""));
        assert!(serialized.contains("provider.auth_failed"));
    }
}
