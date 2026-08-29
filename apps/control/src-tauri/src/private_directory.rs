use std::path::{Path, PathBuf};

pub fn ensure_private_directory(path: &Path) -> Result<PathBuf, PrivateDirectoryError> {
    std::fs::create_dir_all(path).map_err(|_| PrivateDirectoryError::Create)?;
    reject_unsafe_directory(path)?;
    #[cfg(windows)]
    apply_windows_acl(path)?;
    #[cfg(not(windows))]
    apply_portable_permissions(path)?;
    reject_unsafe_directory(path)?;
    path.canonicalize()
        .map_err(|_| PrivateDirectoryError::Inspect)
}

fn reject_unsafe_directory(path: &Path) -> Result<(), PrivateDirectoryError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| PrivateDirectoryError::Inspect)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(PrivateDirectoryError::UnsafePath);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(PrivateDirectoryError::UnsafePath);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn apply_portable_permissions(path: &Path) -> Result<(), PrivateDirectoryError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| PrivateDirectoryError::Acl)
}

#[cfg(windows)]
fn apply_windows_acl(path: &Path) -> Result<(), PrivateDirectoryError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        SetFileSecurityW, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    };

    let current_user = current_user_sid_string()?;
    let sddl = format!("D:P(A;OICI;FA;;;{current_user})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)");
    let sddl: Vec<u16> = sddl.encode_utf16().chain([0]).collect();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: the SDDL is NUL-terminated and the output pointer is valid. The
    // returned descriptor is owned locally and released with LocalFree.
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if converted == 0 || descriptor.is_null() {
        return Err(PrivateDirectoryError::Acl);
    }
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: the path and descriptor remain live for this synchronous call.
    // The protected-DACL flag removes inherited broad Users/Everyone grants.
    let applied = unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    // SAFETY: the descriptor was allocated by LocalAlloc through the SDDL API.
    unsafe { LocalFree(descriptor.cast()) };
    if applied == 0 {
        return Err(PrivateDirectoryError::Acl);
    }
    Ok(())
}

#[cfg(windows)]
fn current_user_sid_string() -> Result<String, PrivateDirectoryError> {
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle and token is a valid output.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(PrivateDirectoryError::Identity);
    }
    let mut needed = 0_u32;
    // SAFETY: a null buffer with zero length is the documented sizing call.
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
    if needed < std::mem::size_of::<TOKEN_USER>() as u32 {
        // SAFETY: token is an owned live handle.
        unsafe { CloseHandle(token) };
        return Err(PrivateDirectoryError::Identity);
    }
    let mut buffer = vec![0_u8; needed as usize];
    // SAFETY: the buffer is writable for the exact requested length.
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    // SAFETY: token is an owned live handle and no longer needed.
    unsafe { CloseHandle(token) };
    if read == 0 {
        return Err(PrivateDirectoryError::Identity);
    }
    // SAFETY: GetTokenInformation initialized TOKEN_USER at the buffer start.
    let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
    let mut text = std::ptr::null_mut();
    // SAFETY: the SID is valid while buffer is live; output is LocalAlloc-owned.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 || text.is_null() {
        return Err(PrivateDirectoryError::Identity);
    }
    let mut length = 0;
    // SAFETY: ConvertSidToStringSidW returns a NUL-terminated UTF-16 string.
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: the preceding scan established this initialized slice length.
    let value = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })
        .map_err(|_| PrivateDirectoryError::Identity);
    // SAFETY: text is the LocalAlloc-owned output from the conversion function.
    unsafe { LocalFree(text.cast()) };
    value
}

#[cfg(all(windows, test))]
#[derive(Debug)]
struct AclAudit {
    protected: bool,
    trustees: std::collections::BTreeSet<String>,
    has_inherited_ace: bool,
}

#[cfg(all(windows, test))]
fn audit_windows_acl(path: &Path) -> Result<AclAudit, PrivateDirectoryError> {
    use std::collections::BTreeSet;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        AclSizeInformation, GetAce, GetAclInformation, GetSecurityDescriptorControl,
        ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION, INHERITED_ACE,
        PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED,
    };
    use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;

    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: path is NUL-terminated and all requested outputs are valid pointers.
    let result = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if result != 0 || descriptor.is_null() || dacl.is_null() {
        return Err(PrivateDirectoryError::Audit);
    }
    let audit = (|| {
        let mut control = 0_u16;
        let mut revision = 0_u32;
        // SAFETY: descriptor is the valid GetNamedSecurityInfoW result.
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
            return Err(PrivateDirectoryError::Audit);
        }
        let mut info = ACL_SIZE_INFORMATION::default();
        // SAFETY: dacl is valid and info has the exact requested size/type.
        if unsafe {
            GetAclInformation(
                dacl,
                (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        } == 0
        {
            return Err(PrivateDirectoryError::Audit);
        }
        let mut trustees = BTreeSet::new();
        let mut inherited = false;
        for index in 0..info.AceCount {
            let mut raw = std::ptr::null_mut();
            // SAFETY: index is bounded by the ACL-reported ACE count.
            if unsafe { GetAce(dacl, index, &mut raw) } == 0 || raw.is_null() {
                return Err(PrivateDirectoryError::Audit);
            }
            // SAFETY: an ACCESS_ALLOWED ACE has the documented header/layout.
            let ace = unsafe { &*(raw.cast::<ACCESS_ALLOWED_ACE>()) };
            if u32::from(ace.Header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
                return Err(PrivateDirectoryError::Audit);
            }
            inherited |= u32::from(ace.Header.AceFlags) & INHERITED_ACE != 0;
            let sid = (&ace.SidStart as *const u32).cast_mut().cast();
            let mut text = std::ptr::null_mut();
            // SAFETY: SidStart begins the variable-length SID owned by this ACE.
            if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
                return Err(PrivateDirectoryError::Audit);
            }
            let mut length = 0;
            // SAFETY: conversion output is NUL-terminated UTF-16.
            while unsafe { *text.add(length) } != 0 {
                length += 1;
            }
            // SAFETY: the scan established the initialized string length.
            let value = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })
                .map_err(|_| PrivateDirectoryError::Audit)?;
            // SAFETY: text is LocalAlloc-owned conversion output.
            unsafe { LocalFree(text.cast()) };
            trustees.insert(value);
        }
        Ok(AclAudit {
            protected: control & SE_DACL_PROTECTED != 0,
            trustees,
            has_inherited_ace: inherited,
        })
    })();
    // SAFETY: descriptor is the LocalAlloc-owned security-info result.
    unsafe { LocalFree(descriptor.cast()) };
    audit
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum PrivateDirectoryError {
    #[error("private application directory could not be created")]
    Create,
    #[error("private application directory could not be inspected")]
    Inspect,
    #[error("private application directory is a symlink, reparse point, or non-directory")]
    UnsafePath,
    #[error("current Windows user identity could not be resolved")]
    Identity,
    #[error("private application directory ACL could not be applied")]
    Acl,
    #[cfg(test)]
    #[error("private application directory ACL could not be audited")]
    Audit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_directory_is_not_a_symlink() {
        let temp = tempfile::tempdir().expect("temp parent");
        let path = temp.path().join("private");
        assert!(ensure_private_directory(&path).is_ok());
        assert!(path.is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn windows_acl_is_protected_and_has_only_explicit_trusted_principals() {
        let temp = tempfile::tempdir().expect("temp parent");
        let path = temp.path().join("private-acl");
        ensure_private_directory(&path).expect("secure directory");
        let audit = audit_windows_acl(&path).expect("audit directory");
        let expected = std::collections::BTreeSet::from([
            current_user_sid_string().expect("current user"),
            "S-1-5-18".to_owned(),
            "S-1-5-32-544".to_owned(),
        ]);
        assert!(audit.protected);
        assert!(!audit.has_inherited_ace);
        assert_eq!(audit.trustees, expected);
    }
}
