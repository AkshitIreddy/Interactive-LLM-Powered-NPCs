use crate::{RedactionFinding, Redactor};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanarySurface {
    Source,
    Ipc,
    StructuredLog,
    DiagnosticReport,
    DiagnosticExport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretPatternScan {
    pub findings: Vec<RedactionFinding>,
    pub total_findings: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanaryScanResult {
    pub surface: CanarySurface,
    pub canary_occurrences: usize,
    pub secret_patterns: SecretPatternScan,
    pub passed: bool,
}

/// Synthetic sentinels only. Callers must never construct this suite from a
/// credential vault or environment variable.
#[derive(Debug, Clone)]
pub struct SecretCanarySuite {
    sentinels: Vec<String>,
    redactor: Redactor,
}

impl Default for SecretCanarySuite {
    fn default() -> Self {
        Self::synthetic()
    }
}

impl SecretCanarySuite {
    pub fn synthetic() -> Self {
        Self {
            sentinels: vec![
                ["sk", "-canary-", "DO_NOT_LOG_7H2K9M4Q"].concat(),
                ["Bearer ", "canary-do-not-log-9Q7M2K4H"].concat(),
                ["credential=", "canary_never_serialize_4K7M9Q2H"].concat(),
            ],
            redactor: Redactor,
        }
    }

    /// Scans bytes in memory and reports counts only. Neither matching bytes,
    /// context, fingerprints, nor file contents are returned.
    pub fn scan(&self, surface: CanarySurface, bytes: &[u8]) -> CanaryScanResult {
        let text = String::from_utf8_lossy(bytes);
        let canary_occurrences = self
            .sentinels
            .iter()
            .map(|sentinel| text.matches(sentinel).count())
            .sum();
        let findings = self.redactor.inspect(&text);
        let total_findings = findings.iter().map(|finding| finding.count).sum();
        CanaryScanResult {
            surface,
            canary_occurrences,
            secret_patterns: SecretPatternScan {
                findings,
                total_findings,
            },
            passed: canary_occurrences == 0 && total_findings == 0,
        }
    }

    pub fn verify_all<'a>(
        &self,
        surfaces: impl IntoIterator<Item = (CanarySurface, &'a [u8])>,
    ) -> Vec<CanaryScanResult> {
        surfaces
            .into_iter()
            .map(|(surface, bytes)| self.scan(surface, bytes))
            .collect()
    }

    #[cfg(test)]
    fn seeded_payload(&self) -> Vec<u8> {
        self.sentinels.join(" ").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DiagnosticEvent, DiagnosticMatrixBuilder, DiagnosticReportBuilder, DiagnosticStatus,
        DiagnosticsExportBuilder, LocalEventLog, LocalEventLogConfig, ObservationProvenance,
        Severity,
    };
    use std::collections::BTreeMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn every_required_surface_detects_canaries_without_reporting_material() {
        let suite = SecretCanarySuite::synthetic();
        let payload = suite.seeded_payload();
        for surface in [
            CanarySurface::Source,
            CanarySurface::Ipc,
            CanarySurface::StructuredLog,
            CanarySurface::DiagnosticReport,
            CanarySurface::DiagnosticExport,
        ] {
            let result = suite.scan(surface, &payload);
            assert!(!result.passed);
            assert!(result.canary_occurrences >= 3);
            let metadata = format!("{result:?}");
            assert!(!metadata.contains("DO_NOT_LOG"));
            assert!(!metadata.contains("never_serialize"));
        }
    }

    #[test]
    fn redacted_outputs_pass_all_surface_scans() {
        let suite = SecretCanarySuite::synthetic();
        let raw = String::from_utf8(suite.seeded_payload()).unwrap();
        let (redacted, _) = Redactor.redact(&raw);
        let surfaces = [
            CanarySurface::Source,
            CanarySurface::Ipc,
            CanarySurface::StructuredLog,
            CanarySurface::DiagnosticReport,
            CanarySurface::DiagnosticExport,
        ];
        assert!(surfaces
            .into_iter()
            .all(|surface| suite.scan(surface, redacted.as_bytes()).passed));
    }

    #[test]
    fn synthetic_canary_is_absent_from_real_ipc_log_report_and_export_outputs() {
        let suite = SecretCanarySuite::synthetic();
        let seeded = String::from_utf8(suite.seeded_payload()).unwrap();
        let redactor = Redactor;

        let (source, _) = redactor.redact(&seeded);
        let (ipc, _) = redactor.redact_json_value(&serde_json::json!({
            "authorization": seeded,
            "operation": "diagnostics"
        }));
        let ipc = serde_json::to_vec(&ipc).unwrap();

        let directory = std::env::temp_dir().join(format!(
            "interactive-npcs-canary-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let log = LocalEventLog::new(LocalEventLogConfig::conservative(
            &directory,
            "canary-surface-test",
        ))
        .unwrap();
        let mut log_event = DiagnosticEvent {
            monotonic_ns: 1,
            component: "runtime".into(),
            event_name: "provider.failed".into(),
            severity: Severity::Error,
            status: DiagnosticStatus::Failed,
            provenance: ObservationProvenance::Measured,
            trace_id: Some("trace-canary-test".into()),
            span_id: None,
            duration_ms: None,
            timing: None,
            error_code: Some("provider_auth_failed".into()),
            provider_id: Some(suite.sentinels[0].clone()),
            model_id: None,
            numeric: BTreeMap::new(),
            labels: BTreeMap::new(),
        };
        log_event = log_event.with_turn_id("turn-canary-test").unwrap();
        log.append(&log_event).unwrap();
        let log_bytes = fs::read(log.active_path()).unwrap();

        let mut report_builder = DiagnosticReportBuilder::new("2026-08-30T10:00:00Z", "2.0.0");
        report_builder
            .push(crate::DiagnosticFact {
                check_id: "provider.auth".into(),
                label: "Provider authentication".into(),
                status: DiagnosticStatus::Failed,
                summary: seeded.clone(),
                fix: None,
                metadata: BTreeMap::new(),
            })
            .unwrap();
        let (report, _) = report_builder.build();
        let report_bytes = serde_json::to_vec(&report).unwrap();

        let mut matrix = DiagnosticMatrixBuilder::new();
        matrix.fill_unmeasured();
        let mut check = matrix.build().unwrap().checks.remove(0);
        check.summary = seeded;
        let mut export_builder = DiagnosticsExportBuilder::new("2026-08-30T10:00:00Z", "2.0.0");
        export_builder.push_check(check).unwrap();
        let (export, _) = export_builder.build().unwrap();
        let export_bytes = serde_json::to_vec(&export).unwrap();

        for result in suite.verify_all([
            (CanarySurface::Source, source.as_bytes()),
            (CanarySurface::Ipc, ipc.as_slice()),
            (CanarySurface::StructuredLog, log_bytes.as_slice()),
            (CanarySurface::DiagnosticReport, report_bytes.as_slice()),
            (CanarySurface::DiagnosticExport, export_bytes.as_slice()),
        ]) {
            assert!(result.passed, "surface metadata: {result:?}");
        }
        fs::remove_dir_all(directory).unwrap();
    }
}
