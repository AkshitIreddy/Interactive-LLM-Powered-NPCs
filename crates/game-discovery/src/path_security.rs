use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// A store manifest executable is data, never an authority to select an
/// arbitrary path. Validation is deliberately platform-independent so hostile
/// Windows paths are rejected even when profiles are prepared or tested on a
/// non-Windows host.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum StoreRelativePathError {
    #[error("store path is empty")]
    Empty,
    #[error("store path must be relative")]
    AbsoluteOrPrefixed,
    #[error("store path contains parent traversal")]
    ParentTraversal,
    #[error("store path contains an alternate data stream")]
    AlternateDataStream,
    #[error("store path contains encoded path syntax")]
    EncodedPathSyntax,
    #[error("store path contains an unsafe component")]
    UnsafeComponent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutableInspection {
    Verified {
        canonical_root: PathBuf,
        canonical_executable: PathBuf,
    },
    RootUnavailable,
    ExecutableUnavailable {
        canonical_root: PathBuf,
    },
    Unsafe {
        canonical_root: Option<PathBuf>,
    },
}

/// Validate and normalize a manifest-provided relative path without touching
/// the filesystem or following links.
pub(crate) fn normalize_store_relative(value: &str) -> Result<PathBuf, StoreRelativePathError> {
    if value.is_empty() || value.trim().is_empty() {
        return Err(StoreRelativePathError::Empty);
    }
    if value
        .bytes()
        .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(StoreRelativePathError::UnsafeComponent);
    }
    if contains_percent_escape(value) {
        return Err(StoreRelativePathError::EncodedPathSyntax);
    }

    // Treat both slash styles as separators on every host. A leading slash is
    // rooted; a colon is either a drive prefix or a Windows ADS separator.
    if value.starts_with('/') || value.starts_with('\\') {
        return Err(StoreRelativePathError::AbsoluteOrPrefixed);
    }
    if value.contains(':') {
        return Err(StoreRelativePathError::AlternateDataStream);
    }

    let mut normalized = PathBuf::new();
    for component in value.split(['/', '\\']) {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(StoreRelativePathError::ParentTraversal);
        }
        // Win32 name normalization historically trims trailing dots/spaces.
        // Reject them so validation and later file opening cannot disagree.
        if component.ends_with('.')
            || component.ends_with(' ')
            || component.trim().is_empty()
            || is_windows_device_name(component)
        {
            return Err(StoreRelativePathError::UnsafeComponent);
        }
        normalized.push(component);
    }

    if normalized.as_os_str().is_empty() {
        Err(StoreRelativePathError::Empty)
    } else {
        Ok(normalized)
    }
}

pub(crate) fn canonical_existing_directory(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_dir() {
        return None;
    }
    fs::canonicalize(path).ok()
}

/// Prove that an executable is a regular non-link/reparse file strictly below
/// the canonical install root. This provides discovery evidence only; callers
/// must revalidate at open/launch time if execution is ever added.
pub(crate) fn inspect_store_executable(
    install_root: &Path,
    relative_executable: &Path,
) -> ExecutableInspection {
    let Some(canonical_root) = canonical_existing_directory(install_root) else {
        return ExecutableInspection::RootUnavailable;
    };

    let Some(relative_text) = relative_executable.to_str() else {
        return ExecutableInspection::Unsafe {
            canonical_root: Some(canonical_root),
        };
    };
    let Ok(normalized) = normalize_store_relative(relative_text) else {
        return ExecutableInspection::Unsafe {
            canonical_root: Some(canonical_root),
        };
    };

    let lexical_executable = canonical_root.join(normalized);
    let Ok(lexical_metadata) = fs::symlink_metadata(&lexical_executable) else {
        return ExecutableInspection::ExecutableUnavailable { canonical_root };
    };
    if !lexical_metadata.is_file() || is_link_or_reparse(&lexical_metadata) {
        return ExecutableInspection::Unsafe {
            canonical_root: Some(canonical_root),
        };
    }

    let Ok(canonical_executable) = fs::canonicalize(&lexical_executable) else {
        return ExecutableInspection::ExecutableUnavailable { canonical_root };
    };
    let Ok(canonical_metadata) = fs::symlink_metadata(&canonical_executable) else {
        return ExecutableInspection::ExecutableUnavailable { canonical_root };
    };
    if !canonical_metadata.is_file() || is_link_or_reparse(&canonical_metadata) {
        return ExecutableInspection::Unsafe {
            canonical_root: Some(canonical_root),
        };
    }

    let contained = canonical_executable
        .strip_prefix(&canonical_root)
        .ok()
        .is_some_and(|remainder| !remainder.as_os_str().is_empty());
    if !contained {
        return ExecutableInspection::Unsafe {
            canonical_root: Some(canonical_root),
        };
    }

    ExecutableInspection::Verified {
        canonical_root,
        canonical_executable,
    }
}

fn contains_percent_escape(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.windows(3).any(|window| {
        window[0] == b'%' && window[1].is_ascii_hexdigit() && window[2].is_ascii_hexdigit()
    })
}

fn is_windows_device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component);
    let stem = stem.trim_end_matches([' ', '.']);
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

#[cfg(unix)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::fs;

    #[test]
    fn accepts_and_normalizes_nested_relative_executables() {
        assert_eq!(
            normalize_store_relative(r"bin\.\x64//game.exe"),
            Ok(PathBuf::from("bin").join("x64").join("game.exe"))
        );
    }

    #[test]
    fn rejects_absolute_unc_device_ads_and_drive_paths_on_every_host() {
        for hostile in [
            r"C:\Games\game.exe",
            r"C:game.exe",
            r"\\server\share\game.exe",
            r"\\?\C:\game.exe",
            r"\\.\GLOBALROOT\Device\game.exe",
            r"\??\C:\game.exe",
            "/usr/bin/game.exe",
            r"bin\game.exe:payload",
        ] {
            assert!(
                normalize_store_relative(hostile).is_err(),
                "accepted hostile path: {hostile}"
            );
        }
    }

    #[test]
    fn rejects_plain_mixed_and_encoded_traversal() {
        for hostile in [
            "../game.exe",
            r"bin\..\game.exe",
            "bin/../game.exe",
            r"bin\../game.exe",
            "%2e%2e/game.exe",
            "%2E%2E%5Cgame.exe",
            "bin/%2f/game.exe",
        ] {
            assert!(
                normalize_store_relative(hostile).is_err(),
                "accepted traversal path: {hostile}"
            );
        }
    }

    #[test]
    fn rejects_windows_normalization_and_device_name_tricks() {
        for hostile in [
            r"bin\.. \game.exe",
            r"bin\game.exe. ",
            r"CON.exe",
            r"bin\NuL.txt",
            r"bin\game%2Eexe",
        ] {
            assert!(
                normalize_store_relative(hostile).is_err(),
                "accepted normalization trick: {hostile}"
            );
        }
    }

    #[test]
    fn verifies_regular_file_strictly_inside_canonical_root() {
        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("Game");
        fs::create_dir_all(install.join("bin/x64")).unwrap();
        fs::write(install.join("bin/x64/game.exe"), b"fixture").unwrap();

        let result = inspect_store_executable(&install, Path::new("bin/x64/game.exe"));
        let ExecutableInspection::Verified {
            canonical_root,
            canonical_executable,
        } = result
        else {
            panic!("expected verified executable")
        };
        assert!(canonical_executable.starts_with(&canonical_root));
        assert_eq!(
            canonical_executable.file_name(),
            Some(OsStr::new("game.exe"))
        );

        fs::create_dir_all(install.join("not-a-file.exe")).unwrap();
        assert!(matches!(
            inspect_store_executable(&install, Path::new("not-a-file.exe")),
            ExecutableInspection::Unsafe { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape_and_case_prefix_trick() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("Game");
        let prefix_sibling = temp.path().join("game-evil");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&prefix_sibling).unwrap();
        fs::write(prefix_sibling.join("GAME.EXE"), b"fixture").unwrap();
        symlink(prefix_sibling.join("GAME.EXE"), install.join("game.exe")).unwrap();

        assert!(matches!(
            inspect_store_executable(&install, Path::new("game.exe")),
            ExecutableInspection::Unsafe { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_directory_symlink_escape_to_case_similar_sibling() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("Game");
        let prefix_sibling = temp.path().join("GameEvil");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&prefix_sibling).unwrap();
        fs::write(prefix_sibling.join("GAME.EXE"), b"fixture").unwrap();
        symlink(&prefix_sibling, install.join("bin")).unwrap();

        assert!(matches!(
            inspect_store_executable(&install, Path::new("bin/GAME.EXE")),
            ExecutableInspection::Unsafe { .. }
        ));
    }

    #[cfg(windows)]
    #[test]
    fn rejects_symlink_or_reparse_escape_and_case_prefix_trick() {
        use std::os::windows::fs::symlink_file;

        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("Game");
        let prefix_sibling = temp.path().join("game-evil");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&prefix_sibling).unwrap();
        fs::write(prefix_sibling.join("GAME.EXE"), b"fixture").unwrap();
        match symlink_file(prefix_sibling.join("GAME.EXE"), install.join("game.exe")) {
            Ok(()) => assert!(matches!(
                inspect_store_executable(&install, Path::new("game.exe")),
                ExecutableInspection::Unsafe { .. }
            )),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                // Windows without Developer Mode cannot create the fixture. The
                // reparse-bit branch is still compiled and exercised in CI
                // environments where creating symlinks is permitted.
            }
            Err(error) => panic!("could not create symlink fixture: {error}"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn rejects_directory_reparse_escape_to_case_similar_sibling() {
        use std::os::windows::fs::symlink_dir;

        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("Game");
        let prefix_sibling = temp.path().join("GameEvil");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&prefix_sibling).unwrap();
        fs::write(prefix_sibling.join("GAME.EXE"), b"fixture").unwrap();
        match symlink_dir(&prefix_sibling, install.join("bin")) {
            Ok(()) => assert!(matches!(
                inspect_store_executable(&install, Path::new("bin/GAME.EXE")),
                ExecutableInspection::Unsafe { .. }
            )),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
            Err(error) => panic!("could not create directory-link fixture: {error}"),
        }
    }
}
