use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use zeroize::{Zeroize, Zeroizing};

use crate::{RetrievalError, RetrievalErrorKind};

const MAX_REFERENCE_BYTES: usize = 256;
const MAX_SECRET_BYTES: usize = 4_096;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SecretReference(String);

impl SecretReference {
    pub fn new(value: impl Into<String>) -> Result<Self, RetrievalError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_REFERENCE_BYTES
            || value.chars().any(char::is_control)
            || value.contains("..")
        {
            return Err(RetrievalError::new(
                RetrievalErrorKind::InvalidRequest,
                "credential reference is invalid",
            ));
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

/// Request-scoped credential material. It is deliberately non-Clone and non-Serialize.
///
/// ```compile_fail
/// use npc_providers_retrieval::SecretString;
/// let secret = SecretString::new("do not copy").unwrap();
/// let _copy = secret.clone();
/// ```
///
/// ```compile_fail
/// use npc_providers_retrieval::SecretString;
/// let secret = SecretString::new("do not serialize").unwrap();
/// let _json = serde_json::to_string(&secret).unwrap();
/// ```
pub struct SecretString(Zeroizing<Box<[u8]>>);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Result<Self, RetrievalError> {
        let mut value = value.into().into_bytes();
        if value.is_empty()
            || value.len() > MAX_SECRET_BYTES
            || value.iter().any(|byte| byte.is_ascii_control())
        {
            value.zeroize();
            return Err(RetrievalError::new(
                RetrievalErrorKind::CredentialUnavailable,
                "provider credential is invalid",
            ));
        }
        Ok(Self(Zeroizing::new(value.into_boxed_slice())))
    }

    pub(crate) fn expose(&self) -> &str {
        // Construction consumes a String, so safe callers cannot violate UTF-8.
        std::str::from_utf8(self.0.as_ref()).expect("SecretString remains valid UTF-8")
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
        #[cfg(test)]
        drop_observer::observe(self.0.as_ref());
    }
}

#[async_trait]
pub trait SecretResolver: Send + Sync {
    async fn resolve(&self, reference: &SecretReference) -> Result<SecretString, RetrievalError>;
}

/// Deterministic fixture implementation. Production hosts bridge `SecretResolver` to Windows
/// Credential Manager and should not retain credential values in this structure.
#[derive(Clone, Default)]
pub struct MemorySecretResolver {
    values: Arc<RwLock<BTreeMap<SecretReference, Zeroizing<Vec<u8>>>>>,
}

impl fmt::Debug for MemorySecretResolver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemorySecretResolver")
            .field("values", &"[REDACTED]")
            .finish()
    }
}

impl MemorySecretResolver {
    pub fn insert(
        &self,
        reference: SecretReference,
        secret: SecretString,
    ) -> Result<(), RetrievalError> {
        let mut values = self.values.write().map_err(|_| {
            RetrievalError::new(
                RetrievalErrorKind::CredentialUnavailable,
                "credential service is unavailable",
            )
        })?;
        values.insert(
            reference,
            Zeroizing::new(secret.expose().as_bytes().to_vec()),
        );
        Ok(())
    }
}

#[async_trait]
impl SecretResolver for MemorySecretResolver {
    async fn resolve(&self, reference: &SecretReference) -> Result<SecretString, RetrievalError> {
        let values = self.values.read().map_err(|_| {
            RetrievalError::new(
                RetrievalErrorKind::CredentialUnavailable,
                "credential service is unavailable",
            )
        })?;
        let value = values.get(reference).ok_or_else(|| {
            RetrievalError::new(
                RetrievalErrorKind::CredentialUnavailable,
                "credential reference was not found",
            )
        })?;
        SecretString::new(String::from_utf8_lossy(value.as_slice()).into_owned())
    }
}

#[cfg(test)]
mod drop_observer {
    use std::sync::Mutex;

    static OBSERVED: Mutex<Option<Vec<u8>>> = Mutex::new(None);

    pub(super) fn observe(bytes: &[u8]) {
        *OBSERVED.lock().expect("drop observer lock") = Some(bytes.to_vec());
    }

    pub(super) fn take() -> Option<Vec<u8>> {
        OBSERVED.lock().expect("drop observer lock").take()
    }
}

#[cfg(test)]
mod tests {
    use super::{drop_observer, SecretString};

    #[test]
    fn debug_redacts_and_drop_zeroizes_secret() {
        let secret = SecretString::new("nvapi-fixture-canary").expect("valid fixture");
        assert_eq!(format!("{secret:?}"), "SecretString([REDACTED])");
        drop(secret);

        let observed = drop_observer::take().expect("drop observed");
        assert_eq!(observed.len(), "nvapi-fixture-canary".len());
        assert!(observed.iter().all(|byte| *byte == 0));
    }
}
