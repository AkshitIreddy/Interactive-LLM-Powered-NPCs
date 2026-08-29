use crate::secure_path::{secure_create_dir_all, with_protected_root};
use crate::{DownloadJournal, PackRevision};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const DOWNLOAD_JOURNAL_SCHEMA_V1: &str = "npc.download-journal/v1";
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PersistentDownloadJournalV1 {
    pub schema: String,
    pub identity: PackRevision,
    pub journal: DownloadJournal,
    pub updated_unix_seconds: u64,
}

#[derive(Clone, Debug)]
pub struct FileDownloadJournalStore {
    root: PathBuf,
}

impl FileDownloadJournalStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, JournalError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(JournalError::Io)?;
        reject_symlink(&root)?;
        let root = fs::canonicalize(root).map_err(JournalError::Io)?;
        reject_symlink(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_for(
        &self,
        identity: &PackRevision,
        artifact_id: &str,
    ) -> Result<PathBuf, JournalError> {
        validate_artifact_id(artifact_id)?;
        Ok(self
            .root
            .join(identity.pack_id.as_str())
            .join(identity.revision.as_str())
            .join(format!("{artifact_id}.json")))
    }

    pub async fn load(
        &self,
        identity: &PackRevision,
        artifact_id: &str,
    ) -> Result<Option<DownloadJournal>, JournalError> {
        let path = self.path_for(identity, artifact_id)?;
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(JournalError::Io(error)),
        };
        let persisted: PersistentDownloadJournalV1 =
            serde_json::from_slice(&bytes).map_err(JournalError::Malformed)?;
        if persisted.schema != DOWNLOAD_JOURNAL_SCHEMA_V1 {
            return Err(JournalError::UnsupportedSchema(persisted.schema));
        }
        if &persisted.identity != identity || persisted.journal.artifact_id != artifact_id {
            return Err(JournalError::IdentityMismatch);
        }
        Ok(Some(persisted.journal))
    }

    pub async fn save(
        &self,
        identity: &PackRevision,
        journal: &DownloadJournal,
    ) -> Result<(), JournalError> {
        let path = self.path_for(identity, &journal.artifact_id)?;
        let persisted = PersistentDownloadJournalV1 {
            schema: DOWNLOAD_JOURNAL_SCHEMA_V1.to_owned(),
            identity: identity.clone(),
            journal: journal.clone(),
            updated_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let bytes = serde_json::to_vec_pretty(&persisted).map_err(JournalError::Serialize)?;
        let parent = path.parent().ok_or_else(|| {
            JournalError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "journal path has no parent",
            ))
        })?;
        secure_create_dir_all(&self.root, parent).map_err(JournalError::Io)?;
        tokio::task::spawn_blocking(move || atomic_write_bytes(&path, &bytes))
            .await
            .map_err(|error| JournalError::Join(error.to_string()))?
            .map_err(JournalError::Io)?;
        Ok(())
    }

    pub async fn clear(
        &self,
        identity: &PackRevision,
        artifact_id: &str,
    ) -> Result<(), JournalError> {
        let path = self.path_for(identity, artifact_id)?;
        match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(JournalError::Io(error)),
        }
    }
}

pub(crate) fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), JournalError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(JournalError::Serialize)?;
    atomic_write_bytes(path, &bytes).map_err(JournalError::Io)
}

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, JournalError> {
    let bytes = fs::read(path).map_err(JournalError::Io)?;
    serde_json::from_slice(&bytes).map_err(JournalError::Malformed)
}

pub(crate) fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    reject_symlink_io(parent)?;
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), counter));
    let result = with_protected_root(parent, || {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        atomic_replace(&temporary, path)?;
        sync_directory(parent)?;
        Ok(())
    });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both buffers are NUL-terminated and remain alive for the duration of the call.
    let ok = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        // Windows durability is provided by MOVEFILE_WRITE_THROUGH above. Opening a
        // directory without backup-semantics would fail with the standard library.
        let _ = path;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::File::open(path)?.sync_all()
    }
}

fn reject_symlink(path: &Path) -> Result<(), JournalError> {
    reject_symlink_io(path).map_err(JournalError::Io)
}

fn reject_symlink_io(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link_or_reparse(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "journal root cannot be a symlink or reparse link",
        ));
    }
    Ok(())
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn validate_artifact_id(value: &str) -> Result<(), JournalError> {
    if !(1..=128).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(JournalError::InvalidArtifactId(value.to_owned()));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum JournalError {
    #[error("journal I/O failed: {0}")]
    Io(io::Error),
    #[error("journal serialization failed: {0}")]
    Serialize(serde_json::Error),
    #[error("journal is malformed: {0}")]
    Malformed(serde_json::Error),
    #[error("unsupported journal schema: {0}")]
    UnsupportedSchema(String),
    #[error("journal identity does not match its path")]
    IdentityMismatch,
    #[error("invalid artifact id: {0}")]
    InvalidArtifactId(String),
    #[error("journal blocking task failed: {0}")]
    Join(String),
}
