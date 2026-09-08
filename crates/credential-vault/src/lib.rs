//! Provider credentials never belong in configuration files, SQLite, command
//! lines, environment variables, logs, the WebView, or ML worker processes.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use zeroize::Zeroize;

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::WindowsCredentialVault;

pub const MAX_SECRET_BYTES: usize = 2_560;
pub const MAX_TARGET_CHARS: usize = 256;

pub const PRODUCTION_APPLICATION_NAMESPACE: &str = "io.github.akshitireddy.interactive-npcs";
pub const REVIEW_APPLICATION_NAMESPACE: &str = "io.github.akshitireddy.interactive-npcs.review";
pub const DEBUG_APPLICATION_NAMESPACE: &str = "io.github.akshitireddy.interactive-npcs.debug";

pub const PRODUCTION_CREDENTIAL_NAMESPACE: &str = "interactive-npcs/v2";
pub const REVIEW_CREDENTIAL_NAMESPACE: &str = "interactive-npcs/v2/review";
pub const DEBUG_CREDENTIAL_NAMESPACE: &str = "interactive-npcs/v2/debug";

/// Maps the native application identifier to its exact Windows Credential
/// Manager namespace. Keeping this allowlist in the vault crate gives the
/// control shell and runtime sidecar one shared authority and prevents a
/// private review import from becoming visible to the production app.
#[must_use]
pub fn credential_namespace_for_application(application_namespace: &str) -> Option<&'static str> {
    match application_namespace {
        PRODUCTION_APPLICATION_NAMESPACE => Some(PRODUCTION_CREDENTIAL_NAMESPACE),
        REVIEW_APPLICATION_NAMESPACE => Some(REVIEW_CREDENTIAL_NAMESPACE),
        DEBUG_APPLICATION_NAMESPACE => Some(DEBUG_CREDENTIAL_NAMESPACE),
        _ => None,
    }
}

#[derive(PartialEq, Eq)]
pub struct SecretValue(Vec<u8>);

impl SecretValue {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, VaultError> {
        let bytes = bytes.into();
        if bytes.is_empty() {
            return Err(VaultError::EmptySecret);
        }
        if bytes.len() > MAX_SECRET_BYTES {
            return Err(VaultError::SecretTooLarge(bytes.len()));
        }
        Ok(Self(bytes))
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretValue")
            .field("bytes", &"<REDACTED>")
            .finish()
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VaultError {
    #[error("credential target is empty")]
    EmptyTarget,
    #[error("credential target exceeds {MAX_TARGET_CHARS} characters")]
    TargetTooLong,
    #[error("credential target contains a control character")]
    InvalidTarget,
    #[error("credential is empty")]
    EmptySecret,
    #[error("credential is {0} bytes; maximum is {MAX_SECRET_BYTES}")]
    SecretTooLarge(usize),
    #[error("credential was not found")]
    NotFound,
    #[error("credential vault operation failed with system code {0}")]
    System(u32),
    #[error("credential vault lock was poisoned")]
    Poisoned,
}

pub trait CredentialVault: Send + Sync {
    fn put(
        &self,
        target: &str,
        username: Option<&str>,
        secret: &SecretValue,
    ) -> Result<(), VaultError>;
    fn get(&self, target: &str) -> Result<SecretValue, VaultError>;
    fn delete(&self, target: &str) -> Result<(), VaultError>;
}

pub fn validate_target(target: &str) -> Result<(), VaultError> {
    if target.trim().is_empty() {
        return Err(VaultError::EmptyTarget);
    }
    if target.encode_utf16().count() > MAX_TARGET_CHARS {
        return Err(VaultError::TargetTooLong);
    }
    if target.chars().any(char::is_control) {
        return Err(VaultError::InvalidTarget);
    }
    Ok(())
}

/// Deterministic test/simulation vault. Production configuration must select
/// `WindowsCredentialVault`; this type intentionally has no persistence.
#[derive(Debug, Clone, Default)]
pub struct MemoryCredentialVault {
    values: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
}

impl CredentialVault for MemoryCredentialVault {
    fn put(
        &self,
        target: &str,
        _username: Option<&str>,
        secret: &SecretValue,
    ) -> Result<(), VaultError> {
        validate_target(target)?;
        self.values
            .lock()
            .map_err(|_| VaultError::Poisoned)?
            .insert(target.to_owned(), secret.expose().to_vec());
        Ok(())
    }

    fn get(&self, target: &str) -> Result<SecretValue, VaultError> {
        validate_target(target)?;
        let value = self
            .values
            .lock()
            .map_err(|_| VaultError::Poisoned)?
            .get(target)
            .cloned()
            .ok_or(VaultError::NotFound)?;
        SecretValue::new(value)
    }

    fn delete(&self, target: &str) -> Result<(), VaultError> {
        validate_target(target)?;
        let removed = self
            .values
            .lock()
            .map_err(|_| VaultError::Poisoned)?
            .remove(target);
        if removed.is_some() {
            Ok(())
        } else {
            Err(VaultError::NotFound)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_exposes_secret() {
        let secret = SecretValue::new(b"sk-canary-secret".to_vec()).unwrap();
        let debug = format!("{secret:?}");
        assert_eq!(debug, "SecretValue { bytes: \"<REDACTED>\" }");
        assert!(!debug.contains("canary"));
    }

    #[test]
    fn memory_vault_round_trips_and_deletes() {
        let vault = MemoryCredentialVault::default();
        let secret = SecretValue::new(b"fixture-only".to_vec()).unwrap();
        vault
            .put("interactive-npcs/provider/example", None, &secret)
            .unwrap();
        assert_eq!(
            vault
                .get("interactive-npcs/provider/example")
                .unwrap()
                .expose(),
            b"fixture-only"
        );
        vault.delete("interactive-npcs/provider/example").unwrap();
        assert_eq!(
            vault.get("interactive-npcs/provider/example"),
            Err(VaultError::NotFound)
        );
    }

    #[test]
    fn validates_limits_before_storage() {
        assert_eq!(validate_target(""), Err(VaultError::EmptyTarget));
        assert_eq!(
            SecretValue::new(vec![1; MAX_SECRET_BYTES + 1]),
            Err(VaultError::SecretTooLarge(MAX_SECRET_BYTES + 1))
        );
    }

    #[test]
    fn application_distributions_use_isolated_credential_namespaces() {
        assert_eq!(
            credential_namespace_for_application(PRODUCTION_APPLICATION_NAMESPACE),
            Some("interactive-npcs/v2")
        );
        assert_eq!(
            credential_namespace_for_application(REVIEW_APPLICATION_NAMESPACE),
            Some("interactive-npcs/v2/review")
        );
        assert_eq!(
            credential_namespace_for_application(DEBUG_APPLICATION_NAMESPACE),
            Some("interactive-npcs/v2/debug")
        );
        assert_eq!(credential_namespace_for_application("forged.app"), None);
    }
}
