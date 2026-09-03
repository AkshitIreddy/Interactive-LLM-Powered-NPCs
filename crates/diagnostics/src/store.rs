use crate::{
    check::is_identifier, DiagnosticEvent, DiagnosticVerbosity, RedactionFinding, Redactor,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use thiserror::Error;

const ACTIVE_LOG_NAME: &str = "diagnostic-events-v1.jsonl";
const MIN_FILE_BYTES: u64 = 4 * 1024;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ROTATED_FILES: u8 = 10;
const MAX_EVENT_BYTES_HARD_LIMIT: usize = 64 * 1024;
const MAX_READ_EVENTS_HARD_LIMIT: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEventLogConfig {
    pub directory: PathBuf,
    pub session_id: String,
    pub max_file_bytes: u64,
    pub max_rotated_files: u8,
    pub max_event_bytes: usize,
    pub verbosity: DiagnosticVerbosity,
}

impl LocalEventLogConfig {
    pub fn conservative(directory: impl Into<PathBuf>, session_id: impl Into<String>) -> Self {
        Self {
            directory: directory.into(),
            session_id: session_id.into(),
            max_file_bytes: 512 * 1024,
            max_rotated_files: 3,
            max_event_bytes: 16 * 1024,
            verbosity: DiagnosticVerbosity::Standard,
        }
    }

    pub fn maximum_disk_bytes(&self) -> u64 {
        self.max_file_bytes
            .saturating_mul(u64::from(self.max_rotated_files) + 1)
    }

    fn validate(&self) -> Result<(), EventStoreError> {
        if !is_identifier(&self.session_id, 128)
            || self.max_file_bytes < MIN_FILE_BYTES
            || self.max_file_bytes > MAX_FILE_BYTES
        {
            return Err(EventStoreError::InvalidConfiguration);
        }
        if self.max_rotated_files > MAX_ROTATED_FILES
            || self.max_event_bytes == 0
            || self.max_event_bytes > MAX_EVENT_BYTES_HARD_LIMIT
            || self.max_event_bytes as u64 > self.max_file_bytes
        {
            return Err(EventStoreError::InvalidConfiguration);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredDiagnosticEvent {
    pub schema_version: String,
    pub session_id: String,
    pub sequence: u64,
    pub event: DiagnosticEvent,
    pub redactions: Vec<RedactionFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventWriteReceipt {
    pub sequence: u64,
    pub serialized_bytes: usize,
    pub redaction_count: usize,
    pub rotated: bool,
    pub discarded_oversized_files: usize,
    pub filtered_by_verbosity: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventReadResult {
    pub events: Vec<StoredDiagnosticEvent>,
    pub skipped_corrupt_records: usize,
}

#[derive(Debug, Error)]
pub enum EventStoreError {
    #[error("local event log configuration is outside hard safety bounds")]
    InvalidConfiguration,
    #[error("diagnostic event is invalid: {0}")]
    InvalidEvent(&'static str),
    #[error("diagnostic event exceeds its serialized size bound")]
    EventTooLarge,
    #[error("requested event read exceeds its hard bound")]
    ReadTooLarge,
    #[error("local event log lock was poisoned")]
    LockPoisoned,
    #[error("local event log I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("local event serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// A bounded, process-local JSONL store. The type has no networking dependency
/// or upload API; moving an export off-device is a separate explicit user flow.
#[derive(Debug)]
pub struct LocalEventLog {
    config: LocalEventLogConfig,
    verbosity: AtomicU8,
    state: Mutex<StoreState>,
    redactor: Redactor,
}

#[derive(Debug, Default)]
struct StoreState {
    next_sequence: u64,
    pending_discarded_oversized_files: usize,
}

impl LocalEventLog {
    pub fn new(config: LocalEventLogConfig) -> Result<Self, EventStoreError> {
        config.validate()?;
        fs::create_dir_all(&config.directory)?;
        let discarded_oversized_files = discard_oversized_managed_files(&config)?;
        Ok(Self {
            verbosity: AtomicU8::new(verbosity_to_u8(config.verbosity)),
            config,
            state: Mutex::new(StoreState {
                next_sequence: 0,
                pending_discarded_oversized_files: discarded_oversized_files,
            }),
            redactor: Redactor,
        })
    }

    pub fn config(&self) -> &LocalEventLogConfig {
        &self.config
    }

    pub fn active_path(&self) -> PathBuf {
        self.config.directory.join(ACTIVE_LOG_NAME)
    }

    /// Updates only the local event filter. The active file, rotation history,
    /// sequence counter, and pending recovery observations remain intact.
    pub fn set_verbosity(&self, verbosity: DiagnosticVerbosity) {
        self.verbosity
            .store(verbosity_to_u8(verbosity), Ordering::Release);
    }

    pub fn verbosity(&self) -> DiagnosticVerbosity {
        verbosity_from_u8(self.verbosity.load(Ordering::Acquire))
    }

    pub fn append(&self, event: &DiagnosticEvent) -> Result<EventWriteReceipt, EventStoreError> {
        event.validate().map_err(EventStoreError::InvalidEvent)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| EventStoreError::LockPoisoned)?;
        if !event.should_record(self.verbosity()) {
            return Ok(EventWriteReceipt {
                sequence: state.next_sequence,
                serialized_bytes: 0,
                redaction_count: 0,
                rotated: false,
                discarded_oversized_files: 0,
                filtered_by_verbosity: true,
            });
        }
        let discarded_oversized_files = state
            .pending_discarded_oversized_files
            .saturating_add(discard_oversized_managed_files(&self.config)?);
        state.pending_discarded_oversized_files = 0;

        let raw_value = serde_json::to_value(event)?;
        let (redacted_value, redactions) = self.redactor.redact_json_value(&raw_value);
        let redacted_event: DiagnosticEvent = serde_json::from_value(redacted_value)?;
        redacted_event
            .validate()
            .map_err(EventStoreError::InvalidEvent)?;
        let stored = StoredDiagnosticEvent {
            schema_version: "1.0.0".into(),
            session_id: self.config.session_id.clone(),
            sequence: state.next_sequence,
            event: redacted_event,
            redactions,
        };
        let mut line = serde_json::to_vec(&stored)?;
        line.push(b'\n');
        if line.len() > self.config.max_event_bytes
            || line.len() > MAX_EVENT_BYTES_HARD_LIMIT
            || line.len() as u64 > self.config.max_file_bytes
        {
            return Err(EventStoreError::EventTooLarge);
        }

        let active = self.active_path();
        let existing_bytes = fs::metadata(&active).map(|value| value.len()).unwrap_or(0);
        let rotated = existing_bytes > 0
            && existing_bytes.saturating_add(line.len() as u64) > self.config.max_file_bytes;
        if rotated {
            rotate(&active, self.config.max_rotated_files)?;
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        set_owner_only_mode(&mut options);
        let mut file = options.open(active)?;
        file.write_all(&line)?;
        file.flush()?;
        let receipt = EventWriteReceipt {
            sequence: state.next_sequence,
            serialized_bytes: line.len(),
            redaction_count: stored.redactions.iter().map(|item| item.count).sum(),
            rotated,
            discarded_oversized_files,
            filtered_by_verbosity: false,
        };
        state.next_sequence = state.next_sequence.saturating_add(1);
        Ok(receipt)
    }

    /// Reads oldest-to-newest while retaining only the requested tail. Corrupt
    /// records are counted but never echoed back to callers.
    pub fn read_recent(&self, max_events: usize) -> Result<EventReadResult, EventStoreError> {
        if max_events > MAX_READ_EVENTS_HARD_LIMIT {
            return Err(EventStoreError::ReadTooLarge);
        }
        let _state = self
            .state
            .lock()
            .map_err(|_| EventStoreError::LockPoisoned)?;
        let mut retained = VecDeque::with_capacity(max_events);
        let mut skipped = 0usize;
        for path in self.paths_oldest_first() {
            let file = match File::open(path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let reader = BufReader::new(file).take(self.config.max_file_bytes + 1);
            for line in reader.lines() {
                let line = line?;
                if line.len() > self.config.max_event_bytes {
                    skipped = skipped.saturating_add(1);
                    continue;
                }
                match serde_json::from_str::<StoredDiagnosticEvent>(&line) {
                    Ok(record)
                        if record.schema_version == "1.0.0"
                            && is_identifier(&record.session_id, 128)
                            && record.event.validate().is_ok() =>
                    {
                        if max_events == 0 {
                            continue;
                        }
                        if retained.len() == max_events {
                            retained.pop_front();
                        }
                        retained.push_back(record);
                    }
                    _ => skipped = skipped.saturating_add(1),
                }
            }
        }
        Ok(EventReadResult {
            events: retained.into_iter().collect(),
            skipped_corrupt_records: skipped,
        })
    }

    fn paths_oldest_first(&self) -> Vec<PathBuf> {
        let mut paths = (1..=self.config.max_rotated_files)
            .rev()
            .map(|index| rotated_path(&self.active_path(), index))
            .collect::<Vec<_>>();
        paths.push(self.active_path());
        paths
    }
}

fn verbosity_to_u8(verbosity: DiagnosticVerbosity) -> u8 {
    match verbosity {
        DiagnosticVerbosity::Essential => 0,
        DiagnosticVerbosity::Standard => 1,
        DiagnosticVerbosity::Verbose => 2,
    }
}

fn verbosity_from_u8(value: u8) -> DiagnosticVerbosity {
    match value {
        0 => DiagnosticVerbosity::Essential,
        2 => DiagnosticVerbosity::Verbose,
        _ => DiagnosticVerbosity::Standard,
    }
}

fn discard_oversized_managed_files(config: &LocalEventLogConfig) -> io::Result<usize> {
    let active = config.directory.join(ACTIVE_LOG_NAME);
    let paths = std::iter::once(active.clone())
        .chain((1..=config.max_rotated_files).map(|index| rotated_path(&active, index)));
    let mut discarded = 0usize;
    for path in paths {
        match fs::metadata(&path) {
            Ok(metadata) if metadata.len() > config.max_file_bytes => {
                fs::remove_file(path)?;
                discarded = discarded.saturating_add(1);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(discarded)
}

fn rotate(active: &Path, retained_files: u8) -> io::Result<()> {
    if retained_files == 0 {
        match fs::remove_file(active) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        }
    }
    let oldest = rotated_path(active, retained_files);
    match fs::remove_file(&oldest) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    for index in (1..retained_files).rev() {
        let source = rotated_path(active, index);
        let destination = rotated_path(active, index + 1);
        match fs::rename(source, destination) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    match fs::rename(active, rotated_path(active, 1)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn rotated_path(active: &Path, index: u8) -> PathBuf {
    active.with_extension(format!("jsonl.{index}"))
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
    use crate::{ClockSource, DiagnosticStatus, ObservationProvenance, Severity, TimingEvidence};
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_directory(test: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "interactive-npcs-event-log-{test}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn event(index: u64) -> DiagnosticEvent {
        DiagnosticEvent {
            monotonic_ns: index,
            component: "runtime".into(),
            event_name: "provider.completed".into(),
            severity: Severity::Info,
            status: DiagnosticStatus::Ok,
            provenance: ObservationProvenance::Measured,
            trace_id: Some(format!("trace-{index}")),
            span_id: None,
            duration_ms: Some(1.0),
            timing: Some(TimingEvidence {
                clock_source: ClockSource::ProcessMonotonic,
                provenance: ObservationProvenance::Measured,
                started_monotonic_ns: index,
                completed_monotonic_ns: index + 1_000_000,
                duration_ms: 1.0,
            }),
            error_code: None,
            provider_id: Some("fixture".into()),
            model_id: None,
            numeric: BTreeMap::new(),
            labels: BTreeMap::from([("backend".into(), "cpu".into())]),
        }
    }

    #[test]
    fn rotates_at_hard_size_bound_and_reads_tail_in_order() {
        let directory = temp_directory("rotation");
        let log = LocalEventLog::new(LocalEventLogConfig {
            directory: directory.clone(),
            session_id: "rotation-test".into(),
            max_file_bytes: MIN_FILE_BYTES,
            max_rotated_files: 2,
            max_event_bytes: 2 * 1024,
            verbosity: DiagnosticVerbosity::Standard,
        })
        .unwrap();
        let mut saw_rotation = false;
        for index in 0..80 {
            saw_rotation |= log.append(&event(index)).unwrap().rotated;
        }
        assert!(saw_rotation);
        for path in log.paths_oldest_first() {
            if let Ok(metadata) = fs::metadata(path) {
                assert!(metadata.len() <= MIN_FILE_BYTES);
            }
        }
        assert_eq!(log.config().maximum_disk_bytes(), MIN_FILE_BYTES * 3);
        let recent = log.read_recent(7).unwrap();
        let ids = recent
            .events
            .iter()
            .map(|record| record.event.monotonic_ns)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![73, 74, 75, 76, 77, 78, 79]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn redacts_secrets_in_every_free_identifier_before_disk_write() {
        let directory = temp_directory("redaction");
        let log = LocalEventLog::new(LocalEventLogConfig::conservative(
            &directory,
            "redaction-test",
        ))
        .unwrap();
        let mut value = event(1);
        value.provider_id = Some(["sk", "-proj-", "canaryabcdefghijklmnop"].concat());
        value.model_id = Some("failed-for-admin@example.com".into());
        let receipt = log.append(&value).unwrap();
        assert!(receipt.redaction_count >= 2);
        let bytes = fs::read(log.active_path()).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("canary"));
        assert!(!text.contains("admin@example.com"));
        let read = log.read_recent(1).unwrap();
        assert_eq!(read.events.len(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn configuration_cannot_create_unbounded_logs() {
        let directory = temp_directory("bounds");
        let error = LocalEventLog::new(LocalEventLogConfig {
            directory,
            session_id: "bounds-test".into(),
            max_file_bytes: MAX_FILE_BYTES + 1,
            max_rotated_files: MAX_ROTATED_FILES + 1,
            max_event_bytes: MAX_EVENT_BYTES_HARD_LIMIT + 1,
            verbosity: DiagnosticVerbosity::Standard,
        })
        .unwrap_err();
        assert!(matches!(error, EventStoreError::InvalidConfiguration));
    }

    #[test]
    fn corrupt_records_are_counted_without_being_returned() {
        let directory = temp_directory("corrupt");
        let log = LocalEventLog::new(LocalEventLogConfig::conservative(
            &directory,
            "corrupt-test",
        ))
        .unwrap();
        fs::write(log.active_path(), b"{not-json}\n").unwrap();
        let read = log.read_recent(10).unwrap();
        assert!(read.events.is_empty());
        assert_eq!(read.skipped_corrupt_records, 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn oversized_external_log_is_discarded_before_it_can_break_disk_bound() {
        let directory = temp_directory("oversized");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(ACTIVE_LOG_NAME),
            vec![b'x'; MIN_FILE_BYTES as usize + 1],
        )
        .unwrap();
        let log = LocalEventLog::new(LocalEventLogConfig {
            directory: directory.clone(),
            session_id: "oversized-test".into(),
            max_file_bytes: MIN_FILE_BYTES,
            max_rotated_files: 1,
            max_event_bytes: 2 * 1024,
            verbosity: DiagnosticVerbosity::Standard,
        })
        .unwrap();
        assert!(!log.active_path().exists());
        let receipt = log.append(&event(1)).unwrap();
        assert_eq!(receipt.discarded_oversized_files, 1);
        assert!(fs::metadata(log.active_path()).unwrap().len() <= MIN_FILE_BYTES);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn configured_verbosity_filters_detail_but_never_warning_or_error() {
        let directory = temp_directory("verbosity");
        let mut config = LocalEventLogConfig::conservative(&directory, "verbosity-test");
        config.verbosity = DiagnosticVerbosity::Essential;
        let log = LocalEventLog::new(config).unwrap();
        assert_eq!(log.verbosity(), DiagnosticVerbosity::Essential);

        let info = event(1);
        let receipt = log.append(&info).unwrap();
        assert!(receipt.filtered_by_verbosity);
        assert!(!log.active_path().exists());

        let mut warning = event(2);
        warning.severity = Severity::Warn;
        let receipt = log.append(&warning).unwrap();
        assert!(!receipt.filtered_by_verbosity);
        assert_eq!(log.read_recent(10).unwrap().events.len(), 1);

        log.set_verbosity(DiagnosticVerbosity::Verbose);
        assert_eq!(log.verbosity(), DiagnosticVerbosity::Verbose);
        let mut debug = event(3);
        debug.severity = Severity::Debug;
        assert!(!log.append(&debug).unwrap().filtered_by_verbosity);
        let events = log.read_recent(10).unwrap().events;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence, 0);
        assert_eq!(events[1].sequence, 1);
        fs::remove_dir_all(directory).unwrap();
    }
}
