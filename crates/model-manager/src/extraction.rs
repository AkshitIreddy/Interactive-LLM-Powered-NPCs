use crate::secure_path::{secure_create_dir_all, secure_create_new_file, SecuredFile};
use crate::{
    validate_archive_entries, validate_relative_archive_path, ArchiveEntry, ArchiveEntryKind,
    ArchiveFormatV1, ArchivePolicy, ArchiveValidationError, ArtifactKind, ArtifactV1,
};
use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedArtifact {
    pub files: Vec<PathBuf>,
    pub total_bytes: u64,
}

pub fn extract_artifact(
    artifact: &ArtifactV1,
    verified_source: &Path,
    staging_root: &Path,
    policy: &ArchivePolicy,
) -> Result<ExtractedArtifact, ExtractionError> {
    reject_link(staging_root)?;
    match artifact.kind {
        ArtifactKind::File => install_single_file(artifact, verified_source, staging_root),
        ArtifactKind::Archive => match artifact.archive_format.as_ref() {
            Some(ArchiveFormatV1::Zip) => {
                extract_zip(artifact, verified_source, staging_root, policy)
            }
            Some(ArchiveFormatV1::Tar) => extract_tar_path(
                verified_source,
                TarCompression::None,
                staging_root,
                artifact,
                policy,
            ),
            Some(ArchiveFormatV1::TarGz) => extract_tar_path(
                verified_source,
                TarCompression::Gzip,
                staging_root,
                artifact,
                policy,
            ),
            Some(ArchiveFormatV1::TarBz2) => extract_tar_path(
                verified_source,
                TarCompression::Bzip2,
                staging_root,
                artifact,
                policy,
            ),
            None => Err(ExtractionError::MissingArchiveFormat),
        },
    }
}

fn install_single_file(
    artifact: &ArtifactV1,
    source: &Path,
    staging_root: &Path,
) -> Result<ExtractedArtifact, ExtractionError> {
    let relative = validate_relative_archive_path(&artifact.destination)?;
    let target = staging_root.join(relative.as_str());
    prepare_parent(staging_root, &target)?;
    let mut input = File::open(source).map_err(ExtractionError::Io)?;
    let mut output = create_new_file(staging_root, &target)?;
    let copied = io::copy(
        &mut Read::by_ref(&mut input).take(artifact.size_bytes + 1),
        &mut output,
    )
    .map_err(ExtractionError::Io)?;
    if copied != artifact.size_bytes {
        return Err(ExtractionError::SourceSizeChanged {
            expected: artifact.size_bytes,
            actual: copied,
        });
    }
    output.flush().map_err(ExtractionError::Io)?;
    output.sync_all().map_err(ExtractionError::Io)?;
    Ok(ExtractedArtifact {
        files: vec![target],
        total_bytes: copied,
    })
}

fn extract_zip(
    artifact: &ArtifactV1,
    source: &Path,
    staging_root: &Path,
    policy: &ArchivePolicy,
) -> Result<ExtractedArtifact, ExtractionError> {
    let destination = validate_relative_archive_path(&artifact.destination)?;
    let file = File::open(source).map_err(ExtractionError::Io)?;
    let mut archive = zip::ZipArchive::new(file).map_err(ExtractionError::Zip)?;
    let mut metadata = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let member = archive.by_index(index).map_err(ExtractionError::Zip)?;
        let name = std::str::from_utf8(member.name_raw())
            .map_err(|_| ExtractionError::NonUtf8Path)?
            .to_owned();
        metadata.push((name, zip_entry_kind(&member), member.size()));
    }
    validate_archive_entries(
        metadata.iter().map(|(path, kind, size)| ArchiveEntry {
            path,
            kind: kind.clone(),
            uncompressed_size: *size,
        }),
        policy,
    )?;
    drop(archive);

    let file = File::open(source).map_err(ExtractionError::Io)?;
    let mut archive = zip::ZipArchive::new(file).map_err(ExtractionError::Zip)?;
    let mut files = Vec::new();
    let mut total = 0_u64;
    let mut found = BTreeSet::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).map_err(ExtractionError::Zip)?;
        let name = std::str::from_utf8(member.name_raw())
            .map_err(|_| ExtractionError::NonUtf8Path)?
            .to_owned();
        if zip_entry_kind(&member) == ArchiveEntryKind::Directory {
            continue;
        }
        let Some(relative) = selected_archive_member(artifact, &name, &mut found)? else {
            continue;
        };
        let target = staging_root
            .join(destination.as_str())
            .join(relative.as_str());
        prepare_parent(staging_root, &target)?;
        let mut output = create_new_file(staging_root, &target)?;
        let expected = member.size();
        let copied = io::copy(&mut member.by_ref().take(expected + 1), &mut output)
            .map_err(ExtractionError::Io)?;
        if copied != expected {
            return Err(ExtractionError::ExpandedSizeChanged { path: name });
        }
        output.flush().map_err(ExtractionError::Io)?;
        output.sync_all().map_err(ExtractionError::Io)?;
        total = total
            .checked_add(copied)
            .ok_or(ExtractionError::ExpandedSizeOverflow)?;
        files.push(target);
    }
    require_archive_members(artifact, &found)?;
    Ok(ExtractedArtifact {
        files,
        total_bytes: total,
    })
}

fn zip_entry_kind(member: &zip::read::ZipFile<'_>) -> ArchiveEntryKind {
    if member.is_dir() {
        return ArchiveEntryKind::Directory;
    }
    match member.unix_mode().map(|mode| mode & 0o170000) {
        Some(0o040000) => ArchiveEntryKind::Directory,
        Some(0o120000) => ArchiveEntryKind::Symlink,
        Some(0o060000) | Some(0o020000) => ArchiveEntryKind::Device,
        _ => ArchiveEntryKind::File,
    }
}

fn extract_tar_path(
    source: &Path,
    compression: TarCompression,
    staging_root: &Path,
    artifact: &ArtifactV1,
    policy: &ArchivePolicy,
) -> Result<ExtractedArtifact, ExtractionError> {
    let destination = validate_relative_archive_path(&artifact.destination)?;
    let preflight_file = File::open(source).map_err(ExtractionError::Io)?;
    let preflight_reader: Box<dyn Read> = match compression {
        TarCompression::None => Box::new(preflight_file),
        TarCompression::Gzip => Box::new(GzDecoder::new(preflight_file)),
        TarCompression::Bzip2 => Box::new(BzDecoder::new(preflight_file)),
    };
    let mut preflight = tar::Archive::new(preflight_reader);
    let mut owned_metadata = Vec::new();
    for entry in preflight.entries().map_err(ExtractionError::Io)? {
        let entry = entry.map_err(ExtractionError::Io)?;
        let path = std::str::from_utf8(entry.path_bytes().as_ref())
            .map_err(|_| ExtractionError::NonUtf8Path)?
            .to_owned();
        owned_metadata.push((
            path,
            tar_entry_kind(entry.header().entry_type()),
            entry.size(),
        ));
    }
    validate_archive_entries(
        owned_metadata
            .iter()
            .map(|(path, kind, size)| ArchiveEntry {
                path,
                kind: kind.clone(),
                uncompressed_size: *size,
            }),
        policy,
    )?;
    drop(preflight);

    let extraction_file = File::open(source).map_err(ExtractionError::Io)?;
    let extraction_reader: Box<dyn Read> = match compression {
        TarCompression::None => Box::new(extraction_file),
        TarCompression::Gzip => Box::new(GzDecoder::new(extraction_file)),
        TarCompression::Bzip2 => Box::new(BzDecoder::new(extraction_file)),
    };
    let mut archive = tar::Archive::new(extraction_reader);
    let mut files = Vec::new();
    let mut total = 0_u64;
    let mut found = BTreeSet::new();
    for entry in archive.entries().map_err(ExtractionError::Io)? {
        let mut entry = entry.map_err(ExtractionError::Io)?;
        let path = std::str::from_utf8(entry.path_bytes().as_ref())
            .map_err(|_| ExtractionError::NonUtf8Path)?
            .to_owned();
        match tar_entry_kind(entry.header().entry_type()) {
            ArchiveEntryKind::Directory => continue,
            ArchiveEntryKind::File => {
                let Some(relative) = selected_archive_member(artifact, &path, &mut found)? else {
                    continue;
                };
                let target = staging_root
                    .join(destination.as_str())
                    .join(relative.as_str());
                prepare_parent(staging_root, &target)?;
                let mut output = create_new_file(staging_root, &target)?;
                let expected = entry.size();
                let copied = io::copy(&mut entry.by_ref().take(expected + 1), &mut output)
                    .map_err(ExtractionError::Io)?;
                if copied != expected {
                    return Err(ExtractionError::ExpandedSizeChanged { path });
                }
                output.flush().map_err(ExtractionError::Io)?;
                output.sync_all().map_err(ExtractionError::Io)?;
                total = total
                    .checked_add(copied)
                    .ok_or(ExtractionError::ExpandedSizeOverflow)?;
                files.push(target);
            }
            other => return Err(ExtractionError::UnsupportedTarType(other)),
        }
    }
    require_archive_members(artifact, &found)?;
    Ok(ExtractedArtifact {
        files,
        total_bytes: total,
    })
}

fn selected_archive_member(
    artifact: &ArtifactV1,
    member: &str,
    found: &mut BTreeSet<String>,
) -> Result<Option<crate::ValidatedArchivePath>, ExtractionError> {
    let member = validate_relative_archive_path(member)?;
    let relative = if let Some(prefix) = &artifact.strip_prefix {
        let prefix = validate_relative_archive_path(prefix)?;
        let expected = format!("{}/", prefix.as_str());
        let stripped = member
            .as_str()
            .strip_prefix(&expected)
            .ok_or_else(|| ExtractionError::MemberOutsideStripPrefix(member.as_str().to_owned()))?;
        validate_relative_archive_path(stripped)?
    } else {
        member
    };
    if !artifact.required_paths.is_empty()
        && !artifact
            .required_paths
            .iter()
            .any(|required| required.eq_ignore_ascii_case(relative.as_str()))
    {
        return Ok(None);
    }
    if !found.insert(relative.as_str().to_ascii_lowercase()) {
        return Err(ExtractionError::DuplicateRequiredMember(
            relative.as_str().to_owned(),
        ));
    }
    Ok(Some(relative))
}

fn require_archive_members(
    artifact: &ArtifactV1,
    found: &BTreeSet<String>,
) -> Result<(), ExtractionError> {
    for required in &artifact.required_paths {
        if !found.contains(&required.to_ascii_lowercase()) {
            return Err(ExtractionError::MissingRequiredMember(required.clone()));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum TarCompression {
    None,
    Gzip,
    Bzip2,
}

fn tar_entry_kind(entry_type: tar::EntryType) -> ArchiveEntryKind {
    if entry_type.is_dir() {
        ArchiveEntryKind::Directory
    } else if entry_type.is_file() {
        ArchiveEntryKind::File
    } else if entry_type.is_symlink() {
        ArchiveEntryKind::Symlink
    } else if entry_type.is_hard_link() {
        ArchiveEntryKind::Hardlink
    } else {
        ArchiveEntryKind::Device
    }
}

fn prepare_parent(root: &Path, target: &Path) -> Result<(), ExtractionError> {
    let parent = target.parent().ok_or(ExtractionError::TargetOutsideRoot)?;
    create_directory(root, parent)
}

fn create_directory(root: &Path, target: &Path) -> Result<(), ExtractionError> {
    if !target.starts_with(root) {
        return Err(ExtractionError::TargetOutsideRoot);
    }
    secure_create_dir_all(root, target).map_err(ExtractionError::Io)
}

fn create_new_file(root: &Path, path: &Path) -> Result<SecuredFile, ExtractionError> {
    secure_create_new_file(root, path).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            ExtractionError::DestinationCollision(path.to_owned())
        } else {
            ExtractionError::Io(error)
        }
    })
}

fn reject_link(path: &Path) -> Result<(), ExtractionError> {
    let metadata = fs::symlink_metadata(path).map_err(ExtractionError::Io)?;
    if is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(ExtractionError::ReparseOrNonDirectory(path.to_owned()));
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

#[derive(Debug, Error)]
pub enum ExtractionError {
    #[error("archive path validation failed: {0}")]
    Path(#[from] crate::ArchivePathError),
    #[error("archive preflight failed: {0}")]
    Archive(#[from] ArchiveValidationError),
    #[error("archive I/O failed: {0}")]
    Io(io::Error),
    #[error("ZIP decoding failed: {0}")]
    Zip(zip::result::ZipError),
    #[error("archive path is not UTF-8")]
    NonUtf8Path,
    #[error("archive artifact has no declared format")]
    MissingArchiveFormat,
    #[error("target escaped the staging root")]
    TargetOutsideRoot,
    #[error("target ancestor is a reparse link or non-directory: {0}")]
    ReparseOrNonDirectory(PathBuf),
    #[error("two artifacts collide at destination: {0}")]
    DestinationCollision(PathBuf),
    #[error("verified source size changed: expected {expected}, actual {actual}")]
    SourceSizeChanged { expected: u64, actual: u64 },
    #[error("archive member expanded to a different size: {path}")]
    ExpandedSizeChanged { path: String },
    #[error("expanded archive size overflow")]
    ExpandedSizeOverflow,
    #[error("unsupported tar entry type: {0:?}")]
    UnsupportedTarType(ArchiveEntryKind),
    #[error("archive member is outside the declared strip prefix: {0}")]
    MemberOutsideStripPrefix(String),
    #[error("required archive member is absent: {0}")]
    MissingRequiredMember(String),
    #[error("required archive member is duplicated: {0}")]
    DuplicateRequiredMember(String),
}
