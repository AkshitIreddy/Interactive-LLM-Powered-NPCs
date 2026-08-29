use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::ops::{Deref, DerefMut};
use std::path::Path;

/// File plus exclusive directory handles retained for the entire I/O operation.
/// On Windows those handles deny rename/delete sharing, preventing an ancestor from
/// being replaced with a junction between validation and the final file open.
pub(crate) struct SecuredFile {
    file: File,
    #[cfg(windows)]
    _ancestor_guards: Vec<File>,
}

pub(crate) struct SecuredAsyncFile {
    file: tokio::fs::File,
    #[cfg(windows)]
    _ancestor_guards: Vec<File>,
}

impl SecuredFile {
    pub(crate) fn into_tokio(self) -> SecuredAsyncFile {
        SecuredAsyncFile {
            file: tokio::fs::File::from_std(self.file),
            #[cfg(windows)]
            _ancestor_guards: self._ancestor_guards,
        }
    }
}

impl SecuredAsyncFile {
    pub(crate) fn file_mut(&mut self) -> &mut tokio::fs::File {
        &mut self.file
    }
}

impl Deref for SecuredFile {
    type Target = File;

    fn deref(&self) -> &Self::Target {
        &self.file
    }
}

impl DerefMut for SecuredFile {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.file
    }
}

impl Read for SecuredFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.file.read(buffer)
    }
}

impl Write for SecuredFile {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.file.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Seek for SecuredFile {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}

pub(crate) fn secure_create_dir_all(root: &Path, target: &Path) -> io::Result<()> {
    validate_target(root, target)?;
    platform::create_dir_all(root, target)
}

pub(crate) fn secure_create_new_file(root: &Path, target: &Path) -> io::Result<SecuredFile> {
    validate_target(root, target)?;
    platform::open_file(root, target, true)
}

pub(crate) fn secure_open_rw_file(root: &Path, target: &Path) -> io::Result<SecuredFile> {
    validate_target(root, target)?;
    platform::open_file(root, target, false)
}

pub(crate) fn with_protected_root<T>(
    root: &Path,
    operation: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    if !root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "protected root must be absolute",
        ));
    }
    platform::with_protected_root(root, operation)
}

fn validate_target(root: &Path, target: &Path) -> io::Result<()> {
    if !root.is_absolute() || !target.is_absolute() || !target.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "secure target is not beneath its absolute protected root",
        ));
    }
    if target
        .strip_prefix(root)
        .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "target escaped root"))?
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "secure target contains a non-normal component",
        ));
    }
    Ok(())
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, CREATE_NEW,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        OPEN_ALWAYS, OPEN_EXISTING,
    };

    const GENERIC_READ_ACCESS: u32 = 0x8000_0000;
    const GENERIC_WRITE_ACCESS: u32 = 0x4000_0000;

    pub(super) fn create_dir_all(root: &Path, target: &Path) -> io::Result<()> {
        let mut guards = Vec::new();
        let mut current = root.to_owned();
        let root_guard = open_directory(&current)?;
        let root_volume = information(&root_guard)?.dwVolumeSerialNumber;
        guards.push(root_guard);
        let relative = target.strip_prefix(root).map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "target escaped protected root",
            )
        })?;
        for component in relative.components() {
            current.push(component);
            match fs::create_dir(&current) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            let guard = open_directory(&current)?;
            if information(&guard)?.dwVolumeSerialNumber != root_volume {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "protected directory crossed a volume boundary",
                ));
            }
            guards.push(guard);
        }
        Ok(())
    }

    pub(super) fn with_protected_root<T>(
        root: &Path,
        operation: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        let _guard = open_directory(root)?;
        operation()
    }

    pub(super) fn open_file(
        root: &Path,
        target: &Path,
        create_new: bool,
    ) -> io::Result<SecuredFile> {
        let parent = target
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no parent"))?;
        let mut guards = Vec::new();
        let mut current = root.to_owned();
        let root_guard = open_directory(&current)?;
        let root_volume = information(&root_guard)?.dwVolumeSerialNumber;
        guards.push(root_guard);
        for component in parent
            .strip_prefix(root)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "parent escaped protected root",
                )
            })?
            .components()
        {
            current.push(component);
            let guard = open_directory(&current)?;
            if information(&guard)?.dwVolumeSerialNumber != root_volume {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "protected ancestor crossed a volume boundary",
                ));
            }
            guards.push(guard);
        }
        let wide = wide_path(target);
        // No sharing is granted. In particular, FILE_SHARE_DELETE is absent, so the file
        // and all held ancestors cannot be renamed into/out of the protected path.
        // SAFETY: `wide` is NUL-terminated and all pointer arguments remain valid for the call.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ_ACCESS | GENERIC_WRITE_ACCESS,
                0,
                std::ptr::null(),
                if create_new { CREATE_NEW } else { OPEN_ALWAYS },
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateFileW returned a unique owned handle, transferred exactly once to File.
        let file = unsafe { File::from_raw_handle(handle as _) };
        let information = information(&file)?;
        if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "secure file target is a reparse point or directory",
            ));
        }
        if information.dwVolumeSerialNumber != root_volume {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "secure file target crossed a volume boundary",
            ));
        }
        let canonical_root = fs::canonicalize(root)?;
        let canonical = fs::canonicalize(target)?;
        if !canonical.starts_with(&canonical_root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "post-open file path escaped the protected root",
            ));
        }
        Ok(SecuredFile {
            file,
            _ancestor_guards: guards,
        })
    }

    fn open_directory(path: &Path) -> io::Result<File> {
        let wide = wide_path(path);
        // SAFETY: `wide` is NUL-terminated and all pointer arguments remain valid for the call.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_READ_ATTRIBUTES,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateFileW returned a unique owned handle, transferred exactly once to File.
        let file = unsafe { File::from_raw_handle(handle as _) };
        let information = information(&file)?;
        if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("protected ancestor is a reparse point: {}", path.display()),
            ));
        }
        Ok(file)
    }

    fn information(file: &File) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the raw handle is live and `information` is a valid writable output buffer.
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut information) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(information)
        }
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn exclusive_ancestor_handles_deny_directory_replacement_during_file_io() {
        let temporary = tempfile::TempDir::new().expect("temporary directory");
        let root = temporary.path().join("protected");
        let parent = root.join("nested");
        fs::create_dir_all(&parent).expect("fixture directories");
        let target = parent.join("model.part");
        let mut secured = secure_open_rw_file(&root, &target).expect("secure file");
        secured.write_all(b"guarded").expect("write fixture");

        let replacement = root.join("replacement");
        assert!(fs::rename(&parent, &replacement).is_err());
        assert!(target.exists());
    }

    #[test]
    fn reparse_ancestor_fails_closed_when_platform_can_create_one() {
        use std::os::windows::fs::symlink_dir;

        let temporary = tempfile::TempDir::new().expect("temporary directory");
        let root = temporary.path().join("protected");
        let outside = temporary.path().join("outside");
        fs::create_dir_all(&root).expect("protected root");
        fs::create_dir_all(&outside).expect("outside root");
        let reparse = root.join("redirect");
        if symlink_dir(&outside, &reparse).is_err() {
            // Windows without Developer Mode/admin cannot create the adversarial fixture.
            return;
        }
        let target = reparse.join("escaped.bin");
        assert!(secure_create_new_file(&root, &target).is_err());
        assert!(!outside.join("escaped.bin").exists());
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub(super) fn create_dir_all(root: &Path, target: &Path) -> io::Result<()> {
        reject_links(root, target.parent().unwrap_or(target))?;
        fs::create_dir_all(target)?;
        reject_links(root, target)
    }

    pub(super) fn with_protected_root<T>(
        root: &Path,
        operation: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        reject_links(root, root)?;
        operation()
    }

    pub(super) fn open_file(
        root: &Path,
        target: &Path,
        create_new: bool,
    ) -> io::Result<SecuredFile> {
        reject_links(root, target.parent().unwrap_or(root))?;
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(!create_new)
            .create_new(create_new)
            .truncate(false)
            .open(target)?;
        if fs::symlink_metadata(target)?.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "secure target is a symbolic link",
            ));
        }
        Ok(SecuredFile { file })
    }

    fn reject_links(root: &Path, target: &Path) -> io::Result<()> {
        let mut current = root.to_owned();
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "root is symlink",
            ));
        }
        for component in target
            .strip_prefix(root)
            .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "target escaped root"))?
            .components()
        {
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "target ancestor is symlink",
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}
