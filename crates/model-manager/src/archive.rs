use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "clock$", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
    "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedArchivePath(String);

impl ValidatedArchivePath {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validates an archive member using Windows semantics even on non-Windows build hosts.
/// The returned path always uses `/`, contains no `.` components, and is relative.
pub fn validate_relative_archive_path(
    input: &str,
) -> Result<ValidatedArchivePath, ArchivePathError> {
    if input.is_empty() {
        return Err(ArchivePathError::Empty);
    }
    if input.len() > 1_024 {
        return Err(ArchivePathError::TooLong);
    }
    if input.contains('\0') || input.chars().any(|c| c.is_control()) {
        return Err(ArchivePathError::ControlCharacter);
    }
    let normalized = input.replace('\\', "/");
    if normalized.starts_with('/') || normalized.starts_with("//") {
        return Err(ArchivePathError::AbsoluteOrUnc);
    }
    if normalized.as_bytes().get(1) == Some(&b':') {
        return Err(ArchivePathError::DrivePrefix);
    }
    let mut clean = Vec::new();
    for component in normalized.split('/') {
        if component.is_empty() {
            return Err(ArchivePathError::EmptyComponent);
        }
        if component == "." || component == ".." {
            return Err(ArchivePathError::Traversal);
        }
        if component.contains(':') {
            return Err(ArchivePathError::AlternateDataStream);
        }
        if component.ends_with([' ', '.']) {
            return Err(ArchivePathError::TrailingDotOrSpace);
        }
        if component.len() > 255 {
            return Err(ArchivePathError::ComponentTooLong);
        }
        let base = component
            .split('.')
            .next()
            .unwrap_or(component)
            .to_ascii_lowercase();
        if WINDOWS_RESERVED.contains(&base.as_str()) {
            return Err(ArchivePathError::ReservedDeviceName(component.to_owned()));
        }
        if component.contains(['<', '>', '"', '|', '?', '*']) {
            return Err(ArchivePathError::InvalidWindowsCharacter);
        }
        clean.push(component);
    }
    Ok(ValidatedArchivePath(clean.join("/")))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArchiveEntryKind {
    File,
    Directory,
    Symlink,
    Hardlink,
    Device,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveEntry<'a> {
    pub path: &'a str,
    pub kind: ArchiveEntryKind,
    pub uncompressed_size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchivePolicy {
    pub max_entries: usize,
    pub max_uncompressed_bytes: u64,
    pub max_single_file_bytes: u64,
}

impl Default for ArchivePolicy {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_uncompressed_bytes: 512 * 1024 * 1024 * 1024,
            max_single_file_bytes: 128 * 1024 * 1024 * 1024,
        }
    }
}

/// Preflights all metadata before extraction. Extractors must still open output files with
/// no-follow semantics and verify the final canonical path remains inside the staging root.
pub fn validate_archive_entries<'a>(
    entries: impl IntoIterator<Item = ArchiveEntry<'a>>,
    policy: &ArchivePolicy,
) -> Result<Vec<ValidatedArchivePath>, ArchiveValidationError> {
    let entries: Vec<_> = entries.into_iter().collect();
    if entries.len() > policy.max_entries {
        return Err(ArchiveValidationError::EntryLimit);
    }
    let mut seen = BTreeSet::new();
    let mut kinds = BTreeMap::new();
    let mut total = 0_u64;
    let mut validated = Vec::with_capacity(entries.len());
    for entry in entries {
        if !matches!(
            entry.kind,
            ArchiveEntryKind::File | ArchiveEntryKind::Directory
        ) {
            return Err(ArchiveValidationError::UnsupportedEntryType {
                path: entry.path.to_owned(),
                kind: entry.kind,
            });
        }
        if entry.uncompressed_size > policy.max_single_file_bytes {
            return Err(ArchiveValidationError::SingleFileLimit(
                entry.path.to_owned(),
            ));
        }
        total = total
            .checked_add(entry.uncompressed_size)
            .ok_or(ArchiveValidationError::SizeLimit)?;
        if total > policy.max_uncompressed_bytes {
            return Err(ArchiveValidationError::SizeLimit);
        }
        // ZIP directory records conventionally carry one terminal `/`. Treat
        // that marker as metadata, while still rejecting a root entry, doubled
        // separators, and terminal separators on files.
        let candidate = if entry.kind == ArchiveEntryKind::Directory {
            entry.path.strip_suffix('/').unwrap_or(entry.path)
        } else {
            entry.path
        };
        let path = validate_relative_archive_path(candidate)?;
        let folded = path.as_str().to_ascii_lowercase();
        if !seen.insert(folded.clone()) {
            return Err(ArchiveValidationError::CaseFoldCollision(
                path.as_str().to_owned(),
            ));
        }
        let components: Vec<_> = folded.split('/').collect();
        for i in 1..components.len() {
            let parent = components[..i].join("/");
            if kinds.get(&parent) == Some(&ArchiveEntryKind::File) {
                return Err(ArchiveValidationError::FileAsParent(parent));
            }
        }
        kinds.insert(folded, entry.kind);
        validated.push(path);
    }
    Ok(validated)
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ArchivePathError {
    #[error("path is empty")]
    Empty,
    #[error("path is too long")]
    TooLong,
    #[error("path contains a control character")]
    ControlCharacter,
    #[error("absolute and UNC paths are forbidden")]
    AbsoluteOrUnc,
    #[error("drive-prefixed paths are forbidden")]
    DrivePrefix,
    #[error("path contains an empty component")]
    EmptyComponent,
    #[error("path traversal is forbidden")]
    Traversal,
    #[error("alternate data streams are forbidden")]
    AlternateDataStream,
    #[error("component has a trailing dot or space")]
    TrailingDotOrSpace,
    #[error("component is too long")]
    ComponentTooLong,
    #[error("reserved Windows device name: {0}")]
    ReservedDeviceName(String),
    #[error("invalid Windows path character")]
    InvalidWindowsCharacter,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ArchiveValidationError {
    #[error(transparent)]
    Path(#[from] ArchivePathError),
    #[error("archive entry limit exceeded")]
    EntryLimit,
    #[error("archive total uncompressed size limit exceeded")]
    SizeLimit,
    #[error("archive member exceeds the single-file limit: {0}")]
    SingleFileLimit(String),
    #[error("unsupported archive entry type {kind:?}: {path}")]
    UnsupportedEntryType {
        path: String,
        kind: ArchiveEntryKind,
    },
    #[error("archive paths collide under Windows case folding: {0}")]
    CaseFoldCollision(String),
    #[error("archive file is also used as a parent directory: {0}")]
    FileAsParent(String),
}
