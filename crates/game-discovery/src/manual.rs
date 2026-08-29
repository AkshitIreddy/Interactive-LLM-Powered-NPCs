use std::path::{Component, Path, PathBuf};
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
        if !path.is_file() {
            return Err(ManualSelectionError::NotFile);
        }
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
}
