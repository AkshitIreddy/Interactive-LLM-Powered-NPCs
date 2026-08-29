use crate::{validate_target, CredentialVault, SecretValue, VaultError};
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND, FILETIME};
use windows_sys::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};

#[derive(Debug, Clone)]
pub struct WindowsCredentialVault {
    namespace: String,
}

impl WindowsCredentialVault {
    pub fn new(namespace: impl Into<String>) -> Result<Self, VaultError> {
        let namespace = namespace.into();
        validate_target(&namespace)?;
        Ok(Self { namespace })
    }

    fn target(&self, target: &str) -> Result<Vec<u16>, VaultError> {
        validate_target(target)?;
        let combined = format!("{}/{}", self.namespace.trim_end_matches('/'), target);
        validate_target(&combined)?;
        Ok(combined.encode_utf16().chain([0]).collect())
    }
}

impl CredentialVault for WindowsCredentialVault {
    fn put(
        &self,
        target: &str,
        username: Option<&str>,
        secret: &SecretValue,
    ) -> Result<(), VaultError> {
        let mut target = self.target(target)?;
        let mut username: Vec<u16> = username.unwrap_or("").encode_utf16().chain([0]).collect();
        let mut blob = secret.expose().to_vec();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            Comment: null_mut(),
            LastWritten: FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: null_mut(),
            TargetAlias: null_mut(),
            UserName: username.as_mut_ptr(),
        };
        // SAFETY: all pointers remain valid for the duration of the call; the
        // API copies the credential data into the current user's vault.
        let written = unsafe { CredWriteW(&credential, 0) };
        blob.fill(0);
        if written == 0 {
            // SAFETY: GetLastError has no preconditions.
            return Err(VaultError::System(unsafe { GetLastError() }));
        }
        Ok(())
    }

    fn get(&self, target: &str) -> Result<SecretValue, VaultError> {
        let target = self.target(target)?;
        let mut pointer: *mut CREDENTIALW = null_mut();
        // SAFETY: the target is NUL terminated and `pointer` is a valid output
        // location. A successful allocation is released through CredFree.
        let read = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) };
        if read == 0 {
            let code = unsafe { GetLastError() };
            return if code == ERROR_NOT_FOUND {
                Err(VaultError::NotFound)
            } else {
                Err(VaultError::System(code))
            };
        }
        if pointer.is_null() {
            return Err(VaultError::System(0));
        }
        // SAFETY: CredReadW returned a valid CREDENTIALW and blob for the
        // documented byte length. We copy before releasing the allocation.
        let bytes = unsafe {
            let credential = &*pointer;
            std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            )
            .to_vec()
        };
        unsafe { CredFree(pointer.cast()) };
        SecretValue::new(bytes)
    }

    fn delete(&self, target: &str) -> Result<(), VaultError> {
        let target = self.target(target)?;
        // SAFETY: target is a valid NUL-terminated UTF-16 string.
        let deleted = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if deleted == 0 {
            let code = unsafe { GetLastError() };
            if code == ERROR_NOT_FOUND {
                Err(VaultError::NotFound)
            } else {
                Err(VaultError::System(code))
            }
        } else {
            Ok(())
        }
    }
}
