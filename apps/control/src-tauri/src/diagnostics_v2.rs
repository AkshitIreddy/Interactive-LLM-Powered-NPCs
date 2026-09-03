use interactive_npcs_diagnostics::{
    CrashMarker, CrashMarkerStore, DiagnosticEvent, DiagnosticMatrixResult, DiagnosticStatus,
    DiagnosticVerbosity, DiagnosticsExportBuilder, DiagnosticsExportPreview, EventWriteReceipt,
    LocalEventLog, LocalEventLogConfig, ObservationProvenance, PrivacyEgressDeclaration,
    RecoveryMetadata, RecoveryPhase, Severity, StoredDiagnosticEvent,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[cfg(test)]
use interactive_npcs_diagnostics::DiagnosticMatrixBuilder;

const MAX_COMMAND_EVENTS: usize = 1_000;

#[derive(Debug)]
pub struct DiagnosticsV2Manager {
    event_log: LocalEventLog,
    crash_store: CrashMarkerStore,
    session_id: String,
    recovery: Mutex<RecoveryMetadata>,
    latest_matrix: Mutex<Option<DiagnosticMatrixResult>>,
    exports_directory: PathBuf,
    settings_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum DiagnosticsV2Error {
    #[error("diagnostics request is invalid: {0}")]
    Invalid(String),
    #[error("diagnostics state is temporarily unavailable")]
    State,
    #[error("diagnostics operation failed: {0}")]
    Operation(String),
    #[error("diagnostics export requires explicit user confirmation")]
    ConfirmationRequired,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsV2Snapshot {
    pub schema_version: u32,
    pub maximum_disk_bytes: u64,
    pub requested_event_limit: usize,
    pub events: Vec<StoredDiagnosticEvent>,
    pub skipped_corrupt_records: usize,
    pub privacy: PrivacyEgressDeclaration,
    pub recovery: RecoveryMetadata,
    pub verbosity: DiagnosticVerbosity,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticWriteResult {
    pub sequence: u64,
    pub serialized_bytes: usize,
    pub redaction_count: usize,
    pub rotated: bool,
    pub discarded_oversized_files: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticsExportRequest {
    pub max_events: usize,
    pub explicit_user_confirmation: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsExportResult {
    pub file_name: String,
    pub preview: DiagnosticsExportPreview,
    pub uploaded: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticsSettingsV1 {
    pub schema_version: u32,
    pub verbosity: DiagnosticVerbosity,
}

impl Default for DiagnosticsSettingsV1 {
    fn default() -> Self {
        Self {
            schema_version: 1,
            verbosity: DiagnosticVerbosity::Standard,
        }
    }
}

impl DiagnosticsV2Manager {
    pub fn new(config_directory: &Path) -> Result<Self, DiagnosticsV2Error> {
        let session_id = format!("desktop-{}", uuid::Uuid::new_v4().simple());
        let diagnostics_directory = config_directory.join("diagnostics-v2");
        let settings_path = diagnostics_directory.join("settings-v1.json");
        let settings = load_settings(&settings_path);
        let mut event_config = LocalEventLogConfig::conservative(
            diagnostics_directory.join("events"),
            session_id.clone(),
        );
        event_config.verbosity = settings.verbosity;
        let event_log = LocalEventLog::new(event_config)
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        let crash_store = CrashMarkerStore::new(diagnostics_directory.join("recovery"));
        let now = utc_now();
        let recovery = crash_store
            .begin_session(CrashMarker {
                schema_version: "1.0.0".into(),
                session_id: session_id.clone(),
                application_version: env!("CARGO_PKG_VERSION").into(),
                started_at_utc: now.clone(),
                updated_at_utc: now,
                phase: RecoveryPhase::Starting,
                last_event_monotonic_ns: None,
                recovery_attempt: 0,
            })
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        let manager = Self {
            event_log,
            crash_store,
            session_id,
            recovery: Mutex::new(recovery),
            latest_matrix: Mutex::new(None),
            exports_directory: diagnostics_directory.join("exports"),
            settings_path,
        };
        // This is native-owned lifecycle evidence. The WebView has no command
        // that can mint measured provenance or rewrite crash phases.
        manager.append(native_lifecycle_event("application.session_started"))?;
        Ok(manager)
    }

    pub fn snapshot(&self, max_events: usize) -> Result<DiagnosticsV2Snapshot, DiagnosticsV2Error> {
        if max_events > MAX_COMMAND_EVENTS {
            return Err(DiagnosticsV2Error::Invalid(format!(
                "maxEvents cannot exceed {MAX_COMMAND_EVENTS}"
            )));
        }
        let read = self
            .event_log
            .read_recent(max_events)
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        Ok(DiagnosticsV2Snapshot {
            schema_version: 2,
            maximum_disk_bytes: self.event_log.config().maximum_disk_bytes(),
            requested_event_limit: max_events,
            events: read.events,
            skipped_corrupt_records: read.skipped_corrupt_records,
            privacy: PrivacyEgressDeclaration::local_diagnostics_v1(),
            recovery: self
                .recovery
                .lock()
                .map_err(|_| DiagnosticsV2Error::State)?
                .clone(),
            verbosity: self.event_log.verbosity(),
        })
    }

    pub fn settings(&self) -> Result<DiagnosticsSettingsV1, DiagnosticsV2Error> {
        Ok(DiagnosticsSettingsV1 {
            schema_version: 1,
            verbosity: self.event_log.verbosity(),
        })
    }

    pub fn save_settings(
        &self,
        settings: DiagnosticsSettingsV1,
    ) -> Result<DiagnosticsSettingsV1, DiagnosticsV2Error> {
        if settings.schema_version != 1 {
            return Err(DiagnosticsV2Error::Invalid(
                "unsupported diagnostics settings schema".into(),
            ));
        }
        atomic_write_json(&self.settings_path, &settings)?;
        self.event_log.set_verbosity(settings.verbosity);
        Ok(settings)
    }

    pub fn record_matrix(&self, matrix: &DiagnosticMatrixResult) -> Result<(), DiagnosticsV2Error> {
        matrix
            .validate()
            .map_err(|error| DiagnosticsV2Error::Invalid(error.to_string()))?;
        *self
            .latest_matrix
            .lock()
            .map_err(|_| DiagnosticsV2Error::State)? = Some(matrix.clone());
        Ok(())
    }

    fn append(&self, event: DiagnosticEvent) -> Result<DiagnosticWriteResult, DiagnosticsV2Error> {
        let receipt = self
            .event_log
            .append(&event)
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        self.crash_store
            .update_phase(
                &self.session_id,
                RecoveryPhase::Running,
                utc_now(),
                (!receipt.filtered_by_verbosity).then_some(event.monotonic_ns),
            )
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        Ok(write_result(receipt))
    }

    /// Native-only structured subsystem evidence. This is deliberately not a
    /// Tauri command: WebView callers cannot choose provenance, status,
    /// provider/model identity, timestamps, or numeric evidence.
    pub(crate) fn record_native_event(
        &self,
        component: &'static str,
        event_name: &'static str,
        severity: Severity,
        status: DiagnosticStatus,
        error_code: Option<&'static str>,
    ) -> Result<DiagnosticWriteResult, DiagnosticsV2Error> {
        self.append(native_subsystem_event(
            component, event_name, severity, status, error_code,
        ))
    }

    pub(crate) fn record_native_turn_event(
        &self,
        component: &'static str,
        event_name: &'static str,
        severity: Severity,
        status: DiagnosticStatus,
        error_code: Option<&'static str>,
        turn_id: &str,
    ) -> Result<DiagnosticWriteResult, DiagnosticsV2Error> {
        let event = native_subsystem_event(component, event_name, severity, status, error_code)
            .with_turn_id(turn_id)
            .map_err(|error| DiagnosticsV2Error::Invalid(error.into()))?;
        self.append(event)
    }

    pub fn export(
        &self,
        request: DiagnosticsExportRequest,
    ) -> Result<DiagnosticsExportResult, DiagnosticsV2Error> {
        if !request.explicit_user_confirmation {
            return Err(DiagnosticsV2Error::ConfirmationRequired);
        }
        if request.max_events > MAX_COMMAND_EVENTS {
            return Err(DiagnosticsV2Error::Invalid(format!(
                "maxEvents cannot exceed {MAX_COMMAND_EVENTS}"
            )));
        }
        let recent = self
            .event_log
            .read_recent(request.max_events)
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        let mut builder = DiagnosticsExportBuilder::new(utc_now(), env!("CARGO_PKG_VERSION"));
        if let Some(matrix) = self
            .latest_matrix
            .lock()
            .map_err(|_| DiagnosticsV2Error::State)?
            .clone()
        {
            for check in matrix.checks {
                builder
                    .push_check(check)
                    .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
            }
        }
        for stored in recent.events {
            builder
                .push_event(stored.event)
                .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        }
        builder
            .set_recovery(
                self.recovery
                    .lock()
                    .map_err(|_| DiagnosticsV2Error::State)?
                    .clone(),
            )
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        let (export, preview) = builder
            .build()
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        fs::create_dir_all(&self.exports_directory)
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        let file_name = format!("diagnostics-{}.json", preview.sha256);
        let path = self.exports_directory.join(&file_name);
        let bytes = serde_json::to_vec_pretty(&export)
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.exports_directory)
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        temporary
            .write_all(&bytes)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        temporary
            .persist(&path)
            .map_err(|error| DiagnosticsV2Error::Operation(error.error.to_string()))?;
        Ok(DiagnosticsExportResult {
            file_name,
            preview,
            uploaded: false,
        })
    }

    pub fn mark_clean_exit(&self) -> Result<(), DiagnosticsV2Error> {
        self.crash_store
            .update_phase(
                &self.session_id,
                RecoveryPhase::ShuttingDown,
                utc_now(),
                None,
            )
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
        self.crash_store
            .mark_clean_exit(&self.session_id)
            .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))
    }
}

fn native_subsystem_event(
    component: &'static str,
    event_name: &'static str,
    severity: Severity,
    status: DiagnosticStatus,
    error_code: Option<&'static str>,
) -> DiagnosticEvent {
    DiagnosticEvent {
        monotonic_ns: monotonic_ns(),
        component: component.into(),
        event_name: event_name.into(),
        severity,
        status,
        provenance: ObservationProvenance::Measured,
        trace_id: None,
        span_id: None,
        duration_ms: None,
        timing: None,
        error_code: error_code.map(str::to_owned),
        provider_id: None,
        model_id: None,
        numeric: BTreeMap::new(),
        labels: BTreeMap::from([("source".into(), "native_subsystem".into())]),
    }
}

fn load_settings(path: &Path) -> DiagnosticsSettingsV1 {
    fs::read(path)
        .ok()
        .filter(|bytes| bytes.len() <= 16 * 1024)
        .and_then(|bytes| serde_json::from_slice::<DiagnosticsSettingsV1>(&bytes).ok())
        .filter(|settings| settings.schema_version == 1)
        .unwrap_or_default()
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), DiagnosticsV2Error> {
    let directory = path
        .parent()
        .ok_or_else(|| DiagnosticsV2Error::Operation("settings directory unavailable".into()))?;
    fs::create_dir_all(directory)
        .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)
        .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| DiagnosticsV2Error::Operation(error.to_string()))?;
    temporary
        .persist(path)
        .map_err(|error| DiagnosticsV2Error::Operation(error.error.to_string()))?;
    Ok(())
}

fn native_lifecycle_event(event_name: &str) -> DiagnosticEvent {
    DiagnosticEvent {
        monotonic_ns: monotonic_ns(),
        component: "control-shell".into(),
        event_name: event_name.into(),
        severity: Severity::Info,
        status: DiagnosticStatus::Ok,
        provenance: ObservationProvenance::Measured,
        trace_id: None,
        span_id: None,
        duration_ms: None,
        timing: None,
        error_code: None,
        provider_id: None,
        model_id: None,
        numeric: BTreeMap::new(),
        labels: BTreeMap::from([("source".into(), "native_lifecycle".into())]),
    }
}

fn monotonic_ns() -> u64 {
    static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    ORIGIN
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_nanos()
        .min(u128::from(u64::MAX)) as u64
        + 1
}

fn write_result(receipt: EventWriteReceipt) -> DiagnosticWriteResult {
    DiagnosticWriteResult {
        sequence: receipt.sequence,
        serialized_bytes: receipt.serialized_bytes,
        redaction_count: receipt.redaction_count,
        rotated: receipt.rotated,
        discarded_oversized_files: receipt.discarded_oversized_files,
    }
}

fn utc_now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> DiagnosticEvent {
        DiagnosticEvent {
            monotonic_ns: 42,
            component: "control".into(),
            event_name: "target.selected".into(),
            severity: Severity::Info,
            status: DiagnosticStatus::Ok,
            provenance: ObservationProvenance::Measured,
            trace_id: None,
            span_id: None,
            duration_ms: None,
            timing: None,
            error_code: None,
            provider_id: None,
            model_id: None,
            numeric: BTreeMap::new(),
            labels: BTreeMap::from([("backend".into(), "native".into())]),
        }
    }

    #[test]
    fn bounded_store_export_and_clean_exit_are_local_only() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager = DiagnosticsV2Manager::new(directory.path()).expect("manager");
        manager.append(event()).expect("append private test event");
        manager
            .record_native_event(
                "runtime",
                "simulation.completed",
                Severity::Info,
                DiagnosticStatus::Ok,
                None,
            )
            .expect("append native runtime event");
        manager
            .record_native_event(
                "audio-broker",
                "playback.failed",
                Severity::Error,
                DiagnosticStatus::Failed,
                Some("playback_broker_unavailable"),
            )
            .expect("append native broker failure");
        let snapshot = manager.snapshot(10).expect("snapshot");
        assert_eq!(snapshot.events.len(), 4);
        assert_eq!(
            snapshot.events[0].event.event_name,
            "application.session_started"
        );
        assert_eq!(snapshot.events[1].event.event_name, "target.selected");
        assert_eq!(snapshot.events[2].event.component, "runtime");
        assert_eq!(
            snapshot.events[2].event.provenance,
            ObservationProvenance::Measured
        );
        assert_eq!(snapshot.events[3].event.component, "audio-broker");
        assert_eq!(
            snapshot.events[3].event.error_code.as_deref(),
            Some("playback_broker_unavailable")
        );
        assert!(snapshot.maximum_disk_bytes <= 2 * 1024 * 1024);
        let result = manager
            .export(DiagnosticsExportRequest {
                max_events: 10,
                explicit_user_confirmation: true,
            })
            .expect("export");
        assert!(!result.uploaded);
        assert!(!result.preview.remote_telemetry);
        assert!(manager.exports_directory.join(&result.file_name).is_file());
        assert!(!result.file_name.contains(['/', '\\']));
        manager.mark_clean_exit().expect("clean exit");
        assert!(!manager.crash_store.marker_path().exists());
    }

    #[test]
    fn export_and_read_limits_fail_closed() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager = DiagnosticsV2Manager::new(directory.path()).expect("manager");
        assert!(manager.snapshot(MAX_COMMAND_EVENTS + 1).is_err());
        assert!(matches!(
            manager.export(DiagnosticsExportRequest {
                max_events: 1,
                explicit_user_confirmation: false,
            }),
            Err(DiagnosticsV2Error::ConfirmationRequired)
        ));
        manager.mark_clean_exit().expect("clean exit");
    }

    #[test]
    fn verbosity_persists_and_turn_correlation_stays_content_free() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager = DiagnosticsV2Manager::new(directory.path()).expect("manager");
        manager
            .save_settings(DiagnosticsSettingsV1 {
                schema_version: 1,
                verbosity: DiagnosticVerbosity::Essential,
            })
            .expect("save essential verbosity");
        manager
            .record_native_turn_event(
                "runtime",
                "turn.failed",
                Severity::Error,
                DiagnosticStatus::Failed,
                Some("provider_timeout"),
                "runtime-simulation-000042",
            )
            .expect("record correlated failure");
        let snapshot = manager.snapshot(20).expect("snapshot");
        let event = snapshot.events.last().expect("correlated event");
        assert_eq!(event.event.turn_id(), Some("runtime-simulation-000042"));
        assert_eq!(snapshot.verbosity, DiagnosticVerbosity::Essential);
        assert!(!event.event.labels.keys().any(|key| matches!(
            key.as_str(),
            "prompt" | "transcript" | "audio" | "frame" | "file_path"
        )));
        manager.mark_clean_exit().expect("clean exit");
        drop(manager);

        let reopened = DiagnosticsV2Manager::new(directory.path()).expect("reopen manager");
        assert_eq!(
            reopened.settings().expect("settings").verbosity,
            DiagnosticVerbosity::Essential
        );
        reopened.mark_clean_exit().expect("clean reopen exit");
    }

    #[test]
    fn export_includes_the_last_complete_validated_matrix() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager = DiagnosticsV2Manager::new(directory.path()).expect("manager");
        let mut builder = DiagnosticMatrixBuilder::new();
        builder.fill_unmeasured();
        let matrix = builder.build().expect("complete matrix");
        manager.record_matrix(&matrix).expect("cache matrix");

        let result = manager
            .export(DiagnosticsExportRequest {
                max_events: 10,
                explicit_user_confirmation: true,
            })
            .expect("export with matrix");
        assert_eq!(result.preview.check_count, matrix.checks.len());
        manager.mark_clean_exit().expect("clean exit");
    }
}
