use interactive_npcs_credential_vault::{SecretValue, MAX_SECRET_BYTES};
use std::fmt;
use zeroize::Zeroize;

#[derive(Debug, Clone, Copy)]
pub struct NativeWindowOwner(pub usize);

pub enum NativePromptOutcome {
    Submitted(SecretValue),
    Cancelled,
    #[cfg_attr(windows, allow(dead_code))]
    DevelopmentFixtureOnly,
}

impl fmt::Debug for NativePromptOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Submitted(_) => formatter.write_str("Submitted(<REDACTED>)"),
            Self::Cancelled => formatter.write_str("Cancelled"),
            Self::DevelopmentFixtureOnly => formatter.write_str("DevelopmentFixtureOnly"),
        }
    }
}

pub trait CredentialPrompt: Send + Sync + fmt::Debug {
    fn prompt(
        &self,
        owner: NativeWindowOwner,
        provider_id: &str,
        provider_name: &str,
    ) -> Result<NativePromptOutcome, PromptError>;
}

#[derive(Debug)]
pub struct SystemCredentialPrompt {
    #[cfg_attr(windows, allow(dead_code))]
    development_fixture_allowed: bool,
}

impl SystemCredentialPrompt {
    pub fn new(development_fixture_allowed: bool) -> Self {
        Self {
            development_fixture_allowed,
        }
    }
}

impl CredentialPrompt for SystemCredentialPrompt {
    fn prompt(
        &self,
        owner: NativeWindowOwner,
        provider_id: &str,
        provider_name: &str,
    ) -> Result<NativePromptOutcome, PromptError> {
        #[cfg(windows)]
        {
            prompt_windows(owner, provider_id, provider_name)
        }
        #[cfg(not(windows))]
        {
            let _ = (owner, provider_id, provider_name);
            if self.development_fixture_allowed {
                Ok(NativePromptOutcome::DevelopmentFixtureOnly)
            } else {
                Err(PromptError::UnsupportedPlatform)
            }
        }
    }
}

#[cfg(windows)]
fn prompt_windows(
    owner: NativeWindowOwner,
    provider_id: &str,
    provider_name: &str,
) -> Result<NativePromptOutcome, PromptError> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::ERROR_CANCELLED;
    use windows_sys::Win32::Security::Credentials::{
        CredUIPromptForCredentialsW, CREDUI_FLAGS_ALWAYS_SHOW_UI, CREDUI_FLAGS_DO_NOT_PERSIST,
        CREDUI_FLAGS_EXCLUDE_CERTIFICATES, CREDUI_FLAGS_GENERIC_CREDENTIALS,
        CREDUI_FLAGS_KEEP_USERNAME, CREDUI_INFOW, CREDUI_MAX_USERNAME_LENGTH,
    };

    let caption = wide_nul("Save provider credential");
    let message = wide_nul(&format!(
        "Enter the API credential for {provider_name}. It will be written directly to Windows Credential Manager and never sent to the app WebView."
    ));
    let target = wide_nul(&format!("Interactive NPCs / {provider_name}"));
    let mut username = WideSecretBuffer::new(CREDUI_MAX_USERNAME_LENGTH as usize + 1);
    username.copy_from(provider_id)?;
    let mut password = WideSecretBuffer::new(MAX_SECRET_BYTES + 1);
    let info = CREDUI_INFOW {
        cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
        hwndParent: owner.0 as *mut core::ffi::c_void,
        pszMessageText: message.as_ptr(),
        pszCaptionText: caption.as_ptr(),
        hbmBanner: null_mut(),
    };
    let mut save = 0;
    let flags = CREDUI_FLAGS_GENERIC_CREDENTIALS
        | CREDUI_FLAGS_ALWAYS_SHOW_UI
        | CREDUI_FLAGS_DO_NOT_PERSIST
        | CREDUI_FLAGS_EXCLUDE_CERTIFICATES
        | CREDUI_FLAGS_KEEP_USERNAME;
    // SAFETY: the dialog structure and all NUL-terminated strings/buffers remain
    // live for the synchronous call. Buffer capacities are passed in UTF-16 code
    // units. The password buffer is zeroized on every return path.
    let result = unsafe {
        CredUIPromptForCredentialsW(
            &info,
            target.as_ptr(),
            std::ptr::null(),
            0,
            username.as_mut_ptr(),
            username.len() as u32,
            password.as_mut_ptr(),
            password.len() as u32,
            &mut save,
            flags,
        )
    };
    if result == ERROR_CANCELLED {
        return Ok(NativePromptOutcome::Cancelled);
    }
    if result != 0 {
        return Err(PromptError::NativeDialog);
    }
    let secret = validated_native_secret(password.as_slice())?;
    Ok(NativePromptOutcome::Submitted(secret))
}

fn validated_native_secret(wide: &[u16]) -> Result<SecretValue, PromptError> {
    let terminator = wide
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(wide.len());
    if terminator == 0 {
        return Err(PromptError::InvalidCredential);
    }
    let mut value =
        String::from_utf16(&wide[..terminator]).map_err(|_| PromptError::InvalidCredential)?;
    let valid = value.len() <= MAX_SECRET_BYTES
        && value.trim() == value
        && !value.chars().any(char::is_control);
    if !valid {
        value.zeroize();
        return Err(PromptError::InvalidCredential);
    }
    SecretValue::new(value.into_bytes()).map_err(|_| PromptError::InvalidCredential)
}

fn wide_nul(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

struct WideSecretBuffer(Vec<u16>);

impl WideSecretBuffer {
    fn new(length: usize) -> Self {
        Self(vec![0; length])
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn as_mut_ptr(&mut self) -> *mut u16 {
        self.0.as_mut_ptr()
    }

    fn as_slice(&self) -> &[u16] {
        &self.0
    }

    fn copy_from(&mut self, value: &str) -> Result<(), PromptError> {
        let encoded: Vec<_> = value.encode_utf16().collect();
        if encoded.len() >= self.0.len() {
            return Err(PromptError::InvalidProvider);
        }
        self.0[..encoded.len()].copy_from_slice(&encoded);
        self.0[encoded.len()] = 0;
        Ok(())
    }
}

impl Drop for WideSecretBuffer {
    fn drop(&mut self) {
        zeroize_wide_buffer(&mut self.0);
    }
}

fn zeroize_wide_buffer(buffer: &mut [u16]) {
    buffer.zeroize();
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum PromptError {
    #[error("native credential entry is unavailable on this platform")]
    #[cfg_attr(windows, allow(dead_code))]
    UnsupportedPlatform,
    #[error("native credential dialog failed")]
    NativeDialog,
    #[error("provider identifier is invalid")]
    InvalidProvider,
    #[error("credential must contain 1-{MAX_SECRET_BYTES} bytes with no surrounding whitespace or control characters")]
    InvalidCredential,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_outcome_debug_redacts_secret() {
        const CANARY: &str = "sk-canary-native-only";
        let secret = SecretValue::new(CANARY.as_bytes().to_vec()).expect("secret");
        let debug = format!("{:?}", NativePromptOutcome::Submitted(secret));
        assert_eq!(debug, "Submitted(<REDACTED>)");
        assert!(!debug.contains(CANARY));
    }

    #[test]
    fn native_buffer_zeroization_is_observable() {
        let mut buffer = "sk-canary".encode_utf16().collect::<Vec<_>>();
        zeroize_wide_buffer(&mut buffer);
        assert!(buffer.iter().all(|value| *value == 0));
    }

    #[test]
    fn native_secret_validation_rejects_controls_and_whitespace() {
        let with_nul = |value: &str| value.encode_utf16().chain([0]).collect::<Vec<_>>();
        assert!(validated_native_secret(&with_nul("valid-token_123")).is_ok());
        assert!(validated_native_secret(&with_nul(" leading")).is_err());
        assert!(validated_native_secret(&with_nul("line\nbreak")).is_err());
        assert!(validated_native_secret(&[0]).is_err());
    }
}
