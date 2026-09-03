use crate::{model::validate_safe_id, BenchmarkReportV1};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

const MAX_REPORT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct BenchmarkReportStore {
    directory: PathBuf,
}

#[derive(Debug, Error)]
pub enum ReportStoreError {
    #[error("benchmark report directory is invalid")]
    InvalidDirectory,
    #[error("benchmark report identifier is invalid")]
    InvalidReportId,
    #[error("benchmark report claims or fields are invalid")]
    InvalidReport,
    #[error("benchmark report exceeds its hard size bound")]
    ReportTooLarge,
    #[error("benchmark report serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("benchmark report persistence failed")]
    Io(#[from] io::Error),
}

impl BenchmarkReportStore {
    pub fn new(directory: impl Into<PathBuf>) -> Result<Self, ReportStoreError> {
        let directory = directory.into();
        fs::create_dir_all(&directory)?;
        let metadata = fs::symlink_metadata(&directory)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(ReportStoreError::InvalidDirectory);
        }
        Ok(Self { directory })
    }

    pub fn persist(&self, report: &BenchmarkReportV1) -> Result<String, ReportStoreError> {
        report
            .validate()
            .map_err(|_| ReportStoreError::InvalidReport)?;
        validate_safe_id(&report.report_id, 96).map_err(|_| ReportStoreError::InvalidReportId)?;
        let file_name = format!("this-pc-benchmark-{}.json", report.report_id);
        let destination = self.directory.join(&file_name);
        if destination.parent() != Some(self.directory.as_path()) {
            return Err(ReportStoreError::InvalidReportId);
        }
        let mut bytes = serde_json::to_vec_pretty(report)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_REPORT_BYTES {
            return Err(ReportStoreError::ReportTooLarge);
        }
        let temporary = self
            .directory
            .join(format!(".{file_name}.{}.tmp", std::process::id()));
        let result = write_atomic(&temporary, &destination, &bytes);
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        Ok(file_name)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

fn write_atomic(temporary: &Path, destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    set_owner_only_mode(&mut options);
    let mut file = match options.open(temporary) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(temporary)?;
            options.open(temporary)?
        }
        Err(error) => return Err(error),
    };
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temporary, destination)
}

#[cfg(unix)]
fn set_owner_only_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_owner_only_mode(_options: &mut OpenOptions) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BenchmarkClassificationV1, BenchmarkRunState, EvidenceExecutionMode, FrameImpactV1,
        HardwareEvidenceV1, MeasurementKind, PersistenceEvidenceV1, ProbeProvenanceV1,
        ProductBindingV1, RunBoundsV1, SourceCoverageV1, BENCHMARK_REPORT_SCHEMA_V1,
    };

    fn report() -> BenchmarkReportV1 {
        BenchmarkReportV1 {
            schema_version: BENCHMARK_REPORT_SCHEMA_V1.into(),
            report_id: "20260830T120000Z-1-1".into(),
            generated_at_utc: "2026-08-30T12:00:00Z".into(),
            state: BenchmarkRunState::Unavailable,
            classification: BenchmarkClassificationV1 {
                execution_mode: EvidenceExecutionMode::Live,
                measurement_kind: MeasurementKind::Measured,
                acceptance_eligible: false,
                reason: "Unavailable live paths are not acceptance evidence.".into(),
            },
            bounds: RunBoundsV1 {
                requested_iterations: 3,
                completed_iterations: 0,
                timeout_millis: 5_000,
                baseline_window_millis: 500,
                elapsed_millis: 1,
                completed_within_bounds: true,
            },
            binding: ProductBindingV1 {
                selected_game_pid: None,
                game_profile_id: "synthetic-game".into(),
                executable_sha256: "0".repeat(64),
                target_instance_recorded: false,
                loadout_revision: "loadout-1".into(),
                provider_routes: vec![],
            },
            hardware: HardwareEvidenceV1 {
                operating_system: "windows".into(),
                architecture: "x86_64".into(),
                logical_processor_count: 8,
                adapter_description: None,
                adapter_fingerprint_sha256: None,
                physical_ram_bytes: None,
                dedicated_vram_bytes: None,
                hostname_recorded: false,
                environment_variables_recorded: false,
            },
            provenance: ProbeProvenanceV1 {
                runtime_revision: "runtime-1".into(),
                broker_revision: "broker-1".into(),
                compositor_revision: "compositor-1".into(),
                process_load_sampler_revision: "process-load-1".into(),
                game_frame_sampler_revision: "game-frame-1".into(),
                provider_observation_source:
                    crate::ProviderObservationSourceV1::HistoricalObservation,
                timing_clock: "monotonic-nanoseconds".into(),
                system_telemetry_schema: "npc.system-telemetry/resource-snapshot-v1".into(),
                provider_payloads_recorded: false,
                prompts_recorded: false,
                transcripts_recorded: false,
                audio_recorded: false,
                screenshots_recorded: false,
                credentials_recorded: false,
                file_paths_recorded: false,
            },
            coverage: SourceCoverageV1 {
                components: crate::BenchmarkComponent::all()
                    .into_iter()
                    .map(|component| {
                        crate::ComponentReadinessV1::unavailable(
                            component,
                            crate::AvailabilityReason::NoSelectedGameTarget,
                            crate::ActionCode::SelectRunningGame,
                        )
                    })
                    .collect(),
                unavailable_components: crate::BenchmarkComponent::all().into(),
                invalid_receipt_count: 0,
                failed_iteration_count: 0,
            },
            metrics: vec![],
            frame_impact: FrameImpactV1 {
                baseline_frame_time_ms: None,
                active_frame_time_ms: None,
                baseline_fps: None,
                active_fps: None,
                p50_frame_time_delta_ms: None,
                p95_frame_time_delta_ms: None,
                p50_fps_delta: None,
                p50_fps_impact_percent: None,
            },
            persistence: PersistenceEvidenceV1 {
                state: "persisted".into(),
                report_file_name: Some("this-pc-benchmark-20260830T120000Z-1-1.json".into()),
                atomic_write: true,
                report_directory_recorded: false,
            },
        }
    }

    #[test]
    fn writes_only_a_bounded_file_name_and_no_directory_path_in_report() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let store = BenchmarkReportStore::new(temporary.path()).expect("store");
        let report = report();
        let file_name = store.persist(&report).expect("persist");
        assert_eq!(file_name, "this-pc-benchmark-20260830T120000Z-1-1.json");
        let contents = fs::read_to_string(temporary.path().join(file_name)).expect("read");
        assert!(!contents.contains(temporary.path().to_string_lossy().as_ref()));
        assert!(!contents.contains("selected_game_pid"));
    }

    #[test]
    fn rejects_tampered_claims_and_secret_like_provider_identifiers() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let store = BenchmarkReportStore::new(temporary.path()).expect("store");
        let mut tampered = report();
        tampered.classification.acceptance_eligible = true;
        assert!(matches!(
            store.persist(&tampered),
            Err(ReportStoreError::InvalidReport)
        ));

        let mut secret = report();
        secret
            .binding
            .provider_routes
            .push(crate::ProviderRouteBindingV1 {
                role: crate::ProviderRole::Llm,
                provider_id: "nvapi-secret-value".into(),
                model_id: "model-1".into(),
                voice_id: None,
                route_revision: "route-1".into(),
                egress: "provider_cloud:transcript".into(),
                execution_mode: EvidenceExecutionMode::Live,
            });
        assert!(matches!(
            store.persist(&secret),
            Err(ReportStoreError::InvalidReport)
        ));
    }
}
