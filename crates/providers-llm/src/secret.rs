use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use zeroize::{Zeroize, Zeroizing};

use crate::SecretError;

const MAX_REFERENCE_BYTES: usize = 256;
const MAX_SECRET_BYTES: usize = 4_096;

/// Opaque lookup key understood by the trusted desktop/runtime host.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SecretReference(String);

impl SecretReference {
    pub fn new(value: impl Into<String>) -> Result<Self, SecretError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_REFERENCE_BYTES
            || value.chars().any(char::is_control)
            || value.contains("..")
        {
            return Err(SecretError::Invalid);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SecretReference")
            .field(&self.0)
            .finish()
    }
}

/// Ephemeral credential bytes. Debug and Display never expose the value and the
/// owned allocation is zeroized on drop.
pub struct SecretBytes(Zeroizing<Vec<u8>>);

impl SecretBytes {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, SecretError> {
        let mut bytes = bytes.into();
        if bytes.is_empty()
            || bytes.len() > MAX_SECRET_BYTES
            || bytes.iter().any(|byte| byte.is_ascii_control())
        {
            bytes.zeroize();
            return Err(SecretError::Invalid);
        }
        Ok(Self(Zeroizing::new(bytes)))
    }

    pub(crate) fn expose(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretBytes(<REDACTED>)")
    }
}

#[async_trait]
pub trait SecretProvider: Send + Sync {
    /// Resolve a credential for immediate use by the trusted network adapter.
    /// Implementations must return a fresh owned value and must not log it.
    async fn resolve(&self, reference: &SecretReference) -> Result<SecretBytes, SecretError>;
}

/// Test/simulation implementation. Production hosts should bridge this trait to
/// Windows Credential Manager instead of keeping credentials in process memory.
#[derive(Clone, Default)]
pub struct MemorySecretProvider {
    values: Arc<RwLock<BTreeMap<SecretReference, Zeroizing<Vec<u8>>>>>,
}

impl fmt::Debug for MemorySecretProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemorySecretProvider")
            .field("values", &"<REDACTED>")
            .finish()
    }
}

impl MemorySecretProvider {
    pub fn insert(
        &self,
        reference: SecretReference,
        secret: SecretBytes,
    ) -> Result<(), SecretError> {
        let mut values = self.values.write().map_err(|_| SecretError::Unavailable)?;
        values.insert(reference, Zeroizing::new(secret.expose().to_vec()));
        Ok(())
    }
}

#[async_trait]
impl SecretProvider for MemorySecretProvider {
    async fn resolve(&self, reference: &SecretReference) -> Result<SecretBytes, SecretError> {
        let values = self.values.read().map_err(|_| SecretError::Unavailable)?;
        let value = values.get(reference).ok_or(SecretError::NotFound)?;
        SecretBytes::new(value.as_slice().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let secret = SecretBytes::new(b"sk-test-canary".to_vec()).expect("valid fixture");
        let debug = format!("{secret:?}");
        assert_eq!(debug, "SecretBytes(<REDACTED>)");
        assert!(!debug.contains("canary"));
    }

    #[test]
    fn rejects_header_injection() {
        assert_eq!(
            SecretBytes::new(b"valid\r\nInjected: yes".to_vec()).expect_err("must reject"),
            SecretError::Invalid
        );
    }
}
