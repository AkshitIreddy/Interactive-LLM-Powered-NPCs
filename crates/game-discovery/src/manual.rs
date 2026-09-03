use crate::path_security::is_link_or_reparse;
use crate::{
    Confidence, DetectionSource, EditionEvidence, InstallationCandidate, InstallationEvidence,
    StoreKind,
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManualSelectionError {
    #[error("selected path must be absolute")]
    Relative,
    #[error("selected path contains a parent traversal component")]
    Traversal,
    #[error("selected path must have an .exe extension")]
    NotExecutable,
    #[error("selected executable does not exist")]
    Missing,
    #[error("selected path is not a file")]
    NotFile,
    #[error("selected executable is a link or reparse point")]
    LinkOrReparsePoint,
    #[error("selected executable could not be resolved safely")]
    CannotCanonicalize,
    #[error("selected executable has no usable file name or parent directory")]
    InvalidFileName,
}

/// Build a verified, non-launching discovery candidate from an explicit user
/// selection. The selected file is canonicalized and link/reparse checked; the
/// returned evidence authorizes matching only, never execution.
pub fn manual_installation_candidate(
    path: &Path,
) -> Result<InstallationCandidate, ManualSelectionError> {
    let executable = validate_manual_executable(path, true)?;
    let install_dir = executable
        .parent()
        .filter(|parent| parent.is_absolute())
        .ok_or(ManualSelectionError::InvalidFileName)?
        .to_path_buf();
    let display_name = executable
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .ok_or(ManualSelectionError::InvalidFileName)?
        .to_owned();
    Ok(InstallationCandidate {
        store: StoreKind::Manual,
        display_name,
        install_dir,
        executable: Some(executable),
        edition: EditionEvidence {
            edition_id: None,
            store_id: None,
            build_id: None,
        },
        evidence: vec![
            InstallationEvidence {
                source: DetectionSource::ManualSelection,
                confidence: Confidence::Verified,
                detail: "user-selected executable".into(),
            },
            InstallationEvidence {
                source: DetectionSource::VerifiedExecutable,
                confidence: Confidence::Verified,
                detail: "canonical regular executable".into(),
            },
        ],
        warnings: BTreeSet::new(),
    })
}

pub fn validate_manual_executable(
    path: &Path,
    require_existing: bool,
) -> Result<PathBuf, ManualSelectionError> {
    if !path.is_absolute() {
        return Err(ManualSelectionError::Relative);
    }
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(ManualSelectionError::Traversal);
    }
    if path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("exe"))
        != Some(true)
    {
        return Err(ManualSelectionError::NotExecutable);
    }
    if require_existing {
        if !path.exists() {
            return Err(ManualSelectionError::Missing);
        }
        let metadata =
            fs::symlink_metadata(path).map_err(|_| ManualSelectionError::CannotCanonicalize)?;
        if !metadata.is_file() {
            return Err(ManualSelectionError::NotFile);
        }
        if is_link_or_reparse(&metadata) {
            return Err(ManualSelectionError::LinkOrReparsePoint);
        }
        let canonical =
            fs::canonicalize(path).map_err(|_| ManualSelectionError::CannotCanonicalize)?;
        let canonical_metadata = fs::symlink_metadata(&canonical)
            .map_err(|_| ManualSelectionError::CannotCanonicalize)?;
        if !canonical_metadata.is_file() {
            return Err(ManualSelectionError::NotFile);
        }
        if is_link_or_reparse(&canonical_metadata) {
            return Err(ManualSelectionError::LinkOrReparsePoint);
        }
        return Ok(canonical);
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_shape_without_touching_disk() {
        let path = if cfg!(windows) {
            PathBuf::from(r"C:\Games\Example\game.exe")
        } else {
            PathBuf::from("/games/example/game.exe")
        };
        assert_eq!(validate_manual_executable(&path, false), Ok(path));
    }

    #[test]
    fn rejects_non_executable_extensions() {
        let path = if cfg!(windows) {
            PathBuf::from(r"C:\Games\readme.txt")
        } else {
            PathBuf::from("/games/readme.txt")
        };
        assert_eq!(
            validate_manual_executable(&path, false),
            Err(ManualSelectionError::NotExecutable)
        );
    }

    #[test]
    fn existing_manual_selection_returns_a_canonical_regular_file() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("synthetic-target.exe");
        fs::write(&executable, b"fixture").unwrap();
        assert_eq!(
            validate_manual_executable(&executable, true),
            Ok(fs::canonicalize(executable).unwrap())
        );
    }

    #[test]
    fn manual_candidate_binds_verified_evidence_to_the_canonical_file() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("interactive-npcs-synthetic-target.exe");
        fs::write(&executable, b"fixture").unwrap();
        let candidate = manual_installation_candidate(&executable).unwrap();
        assert_eq!(candidate.store, StoreKind::Manual);
        assert_eq!(candidate.display_name, "interactive-npcs-synthetic-target");
        assert!(candidate.has_verified_executable());
        let expected = fs::canonicalize(executable).unwrap();
        assert_eq!(candidate.verified_executable(), Some(expected.as_path()));
    }

    #[test]
    fn existing_manual_selection_rejects_directories_named_like_executables() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("not-a-file.exe");
        fs::create_dir(&directory).unwrap();
        assert_eq!(
            validate_manual_executable(&directory, true),
            Err(ManualSelectionError::NotFile)
        );
    }

    #[cfg(unix)]
    #[test]
    fn existing_manual_selection_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("real.exe");
        let link = temp.path().join("linked.exe");
        fs::write(&executable, b"fixture").unwrap();
        symlink(&executable, &link).unwrap();
        assert_eq!(
            validate_manual_executable(&link, true),
            Err(ManualSelectionError::LinkOrReparsePoint)
        );
    }
}
