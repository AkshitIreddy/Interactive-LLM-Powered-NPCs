use crate::{check::is_identifier, SuggestedAction, SuggestedActionKind};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

const MARKER_FILE_NAME: &str = "diagnostics-crash-marker-v1.json";
const MAX_MARKER_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPhase {
    Starting,
    LoadingConfiguration,
    StartingRuntime,
    Running,
    ShuttingDown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashMarker {
    pub schema_version: String,
    pub session_id: String,
    pub application_version: String,
    pub started_at_utc: String,
    pub updated_at_utc: String,
    pub phase: RecoveryPhase,
    pub last_event_monotonic_ns: Option<u64>,
    pub recovery_attempt: u16,
}

impl CrashMarker {
    pub fn validate(&self) -> Result<(), RecoveryError> {
        if self.schema_version != "1.0.0"
            || !is_identifier(&self.session_id, 128)
            || !is_identifier(&self.application_version, 64)
            || !valid_timestamp(&self.started_at_utc)
            || !valid_timestamp(&self.updated_at_utc)
            || self.recovery_attempt > 100
        {
            return Err(RecoveryError::InvalidMarker);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviousExitStatus {
    NoMarker,
    UncleanExit,
    CorruptMarker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousSessionMetadata {
    pub status: PreviousExitStatus,
    pub session_id: Option<String>,
    pub application_version: Option<String>,
    pub started_at_utc: Option<String>,
    pub last_updated_at_utc: Option<String>,
    pub last_phase: Option<RecoveryPhase>,
    pub last_event_monotonic_ns: Option<u64>,
    pub prior_recovery_attempt: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryMetadata {
    pub schema_version: String,
    pub current_session_id: String,
    pub previous_session: PreviousSessionMetadata,
    pub suggested_actions: Vec<SuggestedAction>,
}

impl RecoveryMetadata {
    pub fn validate(&self) -> Result<(), RecoveryError> {
        if self.schema_version != "1.0.0"
            || !is_identifier(&self.current_session_id, 128)
            || self.suggested_actions.len() > crate::MAX_SUGGESTED_ACTIONS
            || self
                .suggested_actions
                .iter()
                .any(|action| action.validate().is_err())
        {
            return Err(RecoveryError::InvalidRecoveryMetadata);
        }
        let previous = &self.previous_session;
        let details_absent = previous.session_id.is_none()
            && previous.application_version.is_none()
            && previous.started_at_utc.is_none()
            && previous.last_updated_at_utc.is_none()
            && previous.last_phase.is_none()
            && previous.last_event_monotonic_ns.is_none()
            && previous.prior_recovery_attempt.is_none();
        match previous.status {
            PreviousExitStatus::NoMarker | PreviousExitStatus::CorruptMarker if !details_absent => {
                return Err(RecoveryError::InvalidRecoveryMetadata);
            }
            PreviousExitStatus::UncleanExit => {
                let valid = previous
                    .session_id
                    .as_deref()
                    .is_some_and(|value| is_identifier(value, 128))
                    && previous.session_id.as_deref() != Some(&self.current_session_id)
                    && previous
                        .application_version
                        .as_deref()
                        .is_some_and(|value| is_identifier(value, 64))
                    && previous
                        .started_at_utc
                        .as_deref()
                        .is_some_and(valid_timestamp)
                    && previous
                        .last_updated_at_utc
                        .as_deref()
                        .is_some_and(valid_timestamp)
                    && previous.last_phase.is_some()
                    && previous
                        .prior_recovery_attempt
                        .is_some_and(|value| value <= 100);
                if !valid {
                    return Err(RecoveryError::InvalidRecoveryMetadata);
                }
            }
            PreviousExitStatus::NoMarker | PreviousExitStatus::CorruptMarker => {}
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("crash marker is invalid")]
    InvalidMarker,
    #[error("recovery metadata is internally inconsistent")]
    InvalidRecoveryMetadata,
    #[error("crash marker exceeds its size bound")]
    MarkerTooLarge,
    #[error("crash marker belongs to another active session")]
    SessionMismatch,
    #[error("crash marker I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("crash marker serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct CrashMarkerStore {
    directory: PathBuf,
}

impl CrashMarkerStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn marker_path(&self) -> PathBuf {
        self.directory.join(MARKER_FILE_NAME)
    }

    /// Detects a prior unclean session and atomically installs the current
    /// marker. It never includes raw crash logs, stack dumps, or user content.
    pub fn begin_session(&self, marker: CrashMarker) -> Result<RecoveryMetadata, RecoveryError> {
        marker.validate()?;
        fs::create_dir_all(&self.directory)?;
        let previous_session = self.read_previous()?;
        let actions = match previous_session.status {
            PreviousExitStatus::NoMarker => Vec::new(),
            PreviousExitStatus::UncleanExit => vec![
                SuggestedAction {
                    action_id: "recovery.restart_component".into(),
                    kind: SuggestedActionKind::RestartComponent,
                    label: "Restart the affected local component".into(),
                    target_id: Some("runtime".into()),
                    requires_confirmation: true,
                },
                SuggestedAction {
                    action_id: "recovery.open_help".into(),
                    kind: SuggestedActionKind::OpenBundledHelp,
                    label: "Open the bundled recovery guide".into(),
                    target_id: Some("startup_recovery".into()),
                    requires_confirmation: false,
                },
            ],
            PreviousExitStatus::CorruptMarker => vec![SuggestedAction {
                action_id: "recovery.retry_check".into(),
                kind: SuggestedActionKind::RetryCheck,
                label: "Run startup diagnostics".into(),
                target_id: Some("startup.integrity".into()),
                requires_confirmation: false,
            }],
        };
        self.write_marker(&marker)?;
        let recovery = RecoveryMetadata {
            schema_version: "1.0.0".into(),
            current_session_id: marker.session_id,
            previous_session,
            suggested_actions: actions,
        };
        recovery.validate()?;
        Ok(recovery)
    }

    pub fn update_phase(
        &self,
        session_id: &str,
        phase: RecoveryPhase,
        updated_at_utc: impl Into<String>,
        last_event_monotonic_ns: Option<u64>,
    ) -> Result<(), RecoveryError> {
        let mut marker = self.read_current()?.ok_or(RecoveryError::SessionMismatch)?;
        if marker.session_id != session_id {
            return Err(RecoveryError::SessionMismatch);
        }
        marker.phase = phase;
        marker.updated_at_utc = updated_at_utc.into();
        marker.last_event_monotonic_ns = last_event_monotonic_ns;
        marker.validate()?;
        self.write_marker(&marker)
    }

    /// A marker is removed only when its session id matches, so one process
    /// cannot accidentally erase another process's recovery evidence.
    pub fn mark_clean_exit(&self, session_id: &str) -> Result<(), RecoveryError> {
        let Some(marker) = self.read_current()? else {
            return Ok(());
        };
        if marker.session_id != session_id {
            return Err(RecoveryError::SessionMismatch);
        }
        fs::remove_file(self.marker_path())?;
        Ok(())
    }

    fn read_previous(&self) -> Result<PreviousSessionMetadata, RecoveryError> {
        match self.read_current() {
            Ok(Some(marker)) => Ok(PreviousSessionMetadata {
                status: PreviousExitStatus::UncleanExit,
                session_id: Some(marker.session_id),
                application_version: Some(marker.application_version),
                started_at_utc: Some(marker.started_at_utc),
                last_updated_at_utc: Some(marker.updated_at_utc),
                last_phase: Some(marker.phase),
                last_event_monotonic_ns: marker.last_event_monotonic_ns,
                prior_recovery_attempt: Some(marker.recovery_attempt),
            }),
            Ok(None) => Ok(empty_previous(PreviousExitStatus::NoMarker)),
            Err(RecoveryError::InvalidMarker | RecoveryError::MarkerTooLarge) => {
                Ok(empty_previous(PreviousExitStatus::CorruptMarker))
            }
            Err(error) => Err(error),
        }
    }

    fn read_current(&self) -> Result<Option<CrashMarker>, RecoveryError> {
        let path = self.marker_path();
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if file.metadata()?.len() > MAX_MARKER_BYTES {
            return Err(RecoveryError::MarkerTooLarge);
        }
        let mut bytes = Vec::new();
        file.take(MAX_MARKER_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_MARKER_BYTES {
            return Err(RecoveryError::MarkerTooLarge);
        }
        let marker: CrashMarker =
            serde_json::from_slice(&bytes).map_err(|_| RecoveryError::InvalidMarker)?;
        marker.validate()?;
        Ok(Some(marker))
    }

    fn write_marker(&self, marker: &CrashMarker) -> Result<(), RecoveryError> {
        let bytes = serde_json::to_vec(marker)?;
        if bytes.len() as u64 > MAX_MARKER_BYTES {
            return Err(RecoveryError::MarkerTooLarge);
        }
        fs::create_dir_all(&self.directory)?;
        let temporary = self
            .directory
            .join(format!("{MARKER_FILE_NAME}.{}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        set_owner_only_mode(&mut options);
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, &self.marker_path())?;
        Ok(())
    }
}

fn empty_previous(status: PreviousExitStatus) -> PreviousSessionMetadata {
    PreviousSessionMetadata {
        status,
        session_id: None,
        application_version: None,
        started_at_utc: None,
        last_updated_at_utc: None,
        last_phase: None,
        last_event_monotonic_ns: None,
        prior_recovery_attempt: None,
    }
}

fn valid_timestamp(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 64
        && !value.chars().any(char::is_control)
        && value.contains('T')
        && (value.ends_with('Z') || value.contains('+'))
}

fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::AlreadyExists | io::ErrorKind::PermissionDenied
            ) && destination.exists() =>
        {
            fs::remove_file(destination)?;
            fs::rename(source, destination)
        }
        Err(error) => Err(error),
    }
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_directory(test: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "interactive-npcs-diagnostics-{test}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn marker(session_id: &str) -> CrashMarker {
        CrashMarker {
            schema_version: "1.0.0".into(),
            session_id: session_id.into(),
            application_version: "2.0.0-alpha.1".into(),
            started_at_utc: "2026-08-30T10:00:00Z".into(),
            updated_at_utc: "2026-08-30T10:00:00Z".into(),
            phase: RecoveryPhase::Starting,
            last_event_monotonic_ns: None,
            recovery_attempt: 0,
        }
    }

    #[test]
    fn detects_unclean_exit_and_clean_exit_removes_only_matching_marker() {
        let directory = temp_directory("lifecycle");
        let store = CrashMarkerStore::new(&directory);
        let first = store.begin_session(marker("session-1")).unwrap();
        assert_eq!(first.previous_session.status, PreviousExitStatus::NoMarker);
        store
            .update_phase(
                "session-1",
                RecoveryPhase::Running,
                "2026-08-30T10:01:00Z",
                Some(42),
            )
            .unwrap();

        let second = store.begin_session(marker("session-2")).unwrap();
        assert_eq!(
            second.previous_session.status,
            PreviousExitStatus::UncleanExit
        );
        assert_eq!(
            second.previous_session.session_id.as_deref(),
            Some("session-1")
        );
        assert_eq!(
            second.previous_session.last_phase,
            Some(RecoveryPhase::Running)
        );
        assert_eq!(second.previous_session.last_event_monotonic_ns, Some(42));
        assert_eq!(
            store.mark_clean_exit("session-1").unwrap_err().to_string(),
            RecoveryError::SessionMismatch.to_string()
        );
        store.mark_clean_exit("session-2").unwrap();
        assert!(!store.marker_path().exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn corrupt_marker_is_reported_without_echoing_its_content() {
        let directory = temp_directory("corrupt");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(MARKER_FILE_NAME),
            br#"{"secret":"canary-do-not-export"}"#,
        )
        .unwrap();
        let store = CrashMarkerStore::new(&directory);
        let recovery = store.begin_session(marker("session-safe")).unwrap();
        assert_eq!(
            recovery.previous_session.status,
            PreviousExitStatus::CorruptMarker
        );
        let serialized = serde_json::to_string(&recovery).unwrap();
        assert!(!serialized.contains("canary-do-not-export"));
        store.mark_clean_exit("session-safe").unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn oversized_marker_is_never_parsed_or_exported() {
        let directory = temp_directory("oversized");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(MARKER_FILE_NAME),
            vec![b'x'; MAX_MARKER_BYTES as usize + 1],
        )
        .unwrap();
        let store = CrashMarkerStore::new(&directory);
        let recovery = store.begin_session(marker("session-safe")).unwrap();
        assert_eq!(
            recovery.previous_session.status,
            PreviousExitStatus::CorruptMarker
        );
        store.mark_clean_exit("session-safe").unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recovery_metadata_rejects_detail_leaks_for_corrupt_markers() {
        let metadata = RecoveryMetadata {
            schema_version: "1.0.0".into(),
            current_session_id: "session-safe".into(),
            previous_session: PreviousSessionMetadata {
                status: PreviousExitStatus::CorruptMarker,
                session_id: Some("should-not-be-present".into()),
                application_version: None,
                started_at_utc: None,
                last_updated_at_utc: None,
                last_phase: None,
                last_event_monotonic_ns: None,
                prior_recovery_attempt: None,
            },
            suggested_actions: Vec::new(),
        };
        assert!(matches!(
            metadata.validate(),
            Err(RecoveryError::InvalidRecoveryMetadata)
        ));
    }
}
