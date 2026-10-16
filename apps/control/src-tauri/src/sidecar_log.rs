//! Bounded, app-owned log files for hidden sidecar processes.
//!
//! The runtime and media-broker sidecars run without a console, so their
//! stdout/stderr used to be discarded and a failed launch left no trace. Their
//! output now goes to `<app config dir>/logs/<sidecar>.log`, capped at
//! [`MAX_LOG_BYTES`] with exactly one previous generation (`<sidecar>.log.1`).
//!
//! Secrets: neither sidecar logs credential material. The runtime host only
//! writes `npc-runtime: <error>` on a fatal error (fixed error enums, no
//! provider payloads; credentials are resolved from the vault in-process and
//! never printed) and the media broker only writes fixed launch-failure
//! strings. Nothing here adds logging of requests, headers, or environment, and
//! the sidecars' environments are already allowlisted. If a sidecar ever starts
//! printing request data, it must redact it before it reaches stderr.

use crate::private_directory::ensure_private_directory;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt};

/// Rotate once a log reaches this size (5 MiB).
pub(crate) const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const LOGS_DIRECTORY_NAME: &str = "logs";

/// The logs directory that belongs to a runtime `app_data` directory
/// (`<config dir>/runtime-host-data` -> `<config dir>/logs`).
pub(crate) fn logs_directory_for_app_data(app_data: &Path) -> Option<PathBuf> {
    Some(app_data.parent()?.join(LOGS_DIRECTORY_NAME))
}

fn rotated_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

fn open_append(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

/// Moves a full log to `<name>.log.1`, replacing the previous generation.
fn rotate(path: &Path) {
    let previous = rotated_path(path);
    let _ = std::fs::remove_file(&previous);
    if std::fs::rename(path, &previous).is_err() {
        // Could not keep it; truncating still keeps the bound.
        let _ = std::fs::remove_file(path);
    }
}

fn prepare_log_path(directory: &Path, sidecar: &str, max_bytes: u64) -> std::io::Result<PathBuf> {
    let directory = ensure_private_directory(directory)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let path = directory.join(format!("{sidecar}.log"));
    if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() >= max_bytes) {
        rotate(&path);
    }
    Ok(path)
}

/// Opens (rotating first when already full) a log file for a child that writes
/// to it directly through an inherited handle. The size bound is therefore
/// enforced per launch for such children.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn open_for_child(directory: &Path, sidecar: &str) -> std::io::Result<File> {
    let path = prepare_log_path(directory, sidecar, MAX_LOG_BYTES)?;
    let mut file = open_append(&path)?;
    let _ = writeln!(file, "--- {sidecar} launch at unix {} ---", unix_seconds());
    Ok(file)
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Log files the sidecars may have written, current generation first.
const KNOWN_LOG_FILES: [&str; 4] = [
    "runtime.log",
    "runtime.log.1",
    "media-broker.log",
    "media-broker.log.1",
];

/// Creates the private logs directory when it is missing and returns its
/// canonical path.
pub(crate) fn ensure_logs_directory(directory: &Path) -> std::io::Result<PathBuf> {
    ensure_private_directory(directory).map_err(|error| std::io::Error::other(error.to_string()))
}

/// Names of the known sidecar log files that currently exist as regular files.
pub(crate) fn present_log_files(directory: &Path) -> Vec<String> {
    KNOWN_LOG_FILES
        .iter()
        .filter(|name| {
            std::fs::symlink_metadata(directory.join(name))
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        })
        .map(|name| (*name).to_owned())
        .collect()
}

/// Windows `canonicalize` returns verbatim `\\?\C:\...` paths, which Explorer
/// does not open reliably. Drops the prefix for ordinary drive paths only.
pub(crate) fn display_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with(r"UNC\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// Opens `directory` in the platform file manager (Explorer on Windows). The
/// path is app-owned and never comes from the WebView. Explorer reports exit
/// code 1 even on success, so only a failure to start it is an error.
pub(crate) fn open_in_file_manager(directory: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(display_path(directory))
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(windows))]
    {
        let _ = directory;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "opening a folder is only supported on Windows",
        ))
    }
}

/// Size-bounded appending writer: rotates to `<name>.log.1` whenever the next
/// write would pass `max_bytes`, so disk use never exceeds about twice the cap.
pub(crate) struct BoundedLog {
    path: PathBuf,
    file: File,
    written: u64,
    max_bytes: u64,
}

impl BoundedLog {
    pub(crate) fn open(directory: &Path, sidecar: &str) -> std::io::Result<Self> {
        Self::open_with_limit(directory, sidecar, MAX_LOG_BYTES)
    }

    fn open_with_limit(directory: &Path, sidecar: &str, max_bytes: u64) -> std::io::Result<Self> {
        let path = prepare_log_path(directory, sidecar, max_bytes)?;
        let file = open_append(&path)?;
        let written = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        let mut log = Self {
            path,
            file,
            written,
            max_bytes,
        };
        log.write(format!("--- {sidecar} launch at unix {} ---\n", unix_seconds()).as_bytes());
        Ok(log)
    }

    pub(crate) fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        // A single oversized chunk is truncated to the cap rather than
        // allowed to exceed it.
        let bytes = &bytes[..bytes.len().min(self.max_bytes as usize)];
        if self.written.saturating_add(bytes.len() as u64) > self.max_bytes {
            rotate(&self.path);
            match open_append(&self.path) {
                Ok(file) => {
                    self.file = file;
                    self.written = 0;
                }
                Err(_) => return,
            }
        }
        if self.file.write_all(bytes).is_ok() {
            self.written = self.written.saturating_add(bytes.len() as u64);
        }
    }
}

pub(crate) type SharedLog = Arc<Mutex<BoundedLog>>;

/// Opens a shared bounded log, or `None` (callers fall back to discarding
/// output) when the directory or file is unavailable.
pub(crate) fn open_shared(directory: &Path, sidecar: &str) -> Option<SharedLog> {
    BoundedLog::open(directory, sidecar)
        .ok()
        .map(|log| Arc::new(Mutex::new(log)))
}

/// Copies a child pipe into the log until EOF (child exit). Must be called
/// from within a tokio runtime.
pub(crate) fn pump<R>(mut reader: R, log: SharedLog)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if let Ok(mut log) = log.lock() {
                        log.write(&buffer[..read]);
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_directory_is_a_sibling_of_runtime_app_data() {
        assert_eq!(
            logs_directory_for_app_data(Path::new("cfg/runtime-host-data")),
            Some(PathBuf::from("cfg/logs"))
        );
        assert_eq!(logs_directory_for_app_data(Path::new("")), None);
    }

    #[test]
    fn logs_directory_is_created_and_lists_only_known_regular_files() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let directory = temp.path().join("logs");
        let created = ensure_logs_directory(&directory).expect("create logs directory");
        assert!(created.is_dir());
        assert!(present_log_files(&created).is_empty());
        std::fs::write(created.join("runtime.log"), b"a").expect("runtime log");
        std::fs::write(created.join("media-broker.log.1"), b"b").expect("rotated log");
        std::fs::write(created.join("unrelated.txt"), b"c").expect("other file");
        assert_eq!(
            present_log_files(&created),
            vec!["runtime.log".to_owned(), "media-broker.log.1".to_owned()]
        );
        // Idempotent when the directory already exists.
        assert!(ensure_logs_directory(&directory).is_ok());
    }

    #[test]
    fn verbatim_drive_prefix_is_dropped_for_the_file_manager() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\Users\me\logs")),
            PathBuf::from(r"C:\Users\me\logs")
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\logs")),
            PathBuf::from(r"\\?\UNC\server\share\logs")
        );
        assert_eq!(
            display_path(Path::new("relative/logs")),
            PathBuf::from("relative/logs")
        );
    }

    #[test]
    fn log_rotates_at_the_cap_and_keeps_exactly_one_previous_generation() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let directory = temp.path().join("logs");
        let mut log = BoundedLog::open_with_limit(&directory, "runtime", 256).expect("open log");
        for index in 0..40 {
            log.write(format!("line {index:04} {}\n", "x".repeat(40)).as_bytes());
        }
        let current = directory.join("runtime.log");
        let previous = directory.join("runtime.log.1");
        assert!(current.is_file());
        assert!(previous.is_file());
        assert!(!directory.join("runtime.log.2").exists());
        assert!(std::fs::metadata(&current).expect("current").len() <= 256);
        assert!(std::fs::metadata(&previous).expect("previous").len() <= 256);
        let text = std::fs::read_to_string(&current).expect("read current");
        assert!(
            text.contains("line 0039"),
            "newest output stays in the live log"
        );

        // An oversized single write is truncated to the cap.
        log.write(&vec![b'y'; 4096]);
        assert!(std::fs::metadata(&current).expect("current").len() <= 256);
    }

    #[test]
    fn an_already_full_log_is_rotated_when_reopened() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let directory = temp.path().join("logs");
        std::fs::create_dir_all(&directory).expect("logs directory");
        std::fs::write(directory.join("broker.log"), vec![b'a'; 300]).expect("old log");
        let log = BoundedLog::open_with_limit(&directory, "broker", 256).expect("open log");
        drop(log);
        assert_eq!(
            std::fs::metadata(directory.join("broker.log.1"))
                .expect("rotated")
                .len(),
            300
        );
        assert!(
            std::fs::metadata(directory.join("broker.log"))
                .expect("fresh")
                .len()
                < 256
        );
    }
}
