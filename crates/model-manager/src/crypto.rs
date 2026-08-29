use crate::CatalogSignatureVerifier;
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use std::collections::BTreeMap;
use thiserror::Error;

pub const ED25519_CATALOG_ALGORITHM: &str = "ed25519";

/// Concrete catalog verifier backed by `ed25519-dalek`'s strict verification path.
/// Keys are supplied by the application's pinned TUF root metadata, not by the catalog.
#[derive(Clone, Debug, Default)]
pub struct Ed25519CatalogVerifier {
    trusted_keys: BTreeMap<String, VerifyingKey>,
}

impl Ed25519CatalogVerifier {
    pub fn new(
        keys: impl IntoIterator<Item = (String, [u8; 32])>,
    ) -> Result<Self, CatalogKeyError> {
        let mut trusted_keys = BTreeMap::new();
        for (key_id, bytes) in keys {
            validate_key_id(&key_id)?;
            let key = VerifyingKey::from_bytes(&bytes)
                .map_err(|_| CatalogKeyError::InvalidPublicKey(key_id.clone()))?;
            if trusted_keys.insert(key_id.clone(), key).is_some() {
                return Err(CatalogKeyError::DuplicateKeyId(key_id));
            }
        }
        if trusted_keys.is_empty() {
            return Err(CatalogKeyError::EmptyKeySet);
        }
        Ok(Self { trusted_keys })
    }

    pub fn key_count(&self) -> usize {
        self.trusted_keys.len()
    }
}

impl CatalogSignatureVerifier for Ed25519CatalogVerifier {
    fn is_trusted_key(&self, key_id: &str) -> bool {
        self.trusted_keys.contains_key(key_id)
    }

    fn verify(&self, key_id: &str, algorithm: &str, message: &[u8], signature: &str) -> bool {
        if algorithm != ED25519_CATALOG_ALGORITHM {
            return false;
        }
        let Some(key) = self.trusted_keys.get(key_id) else {
            return false;
        };
        let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(signature) else {
            return false;
        };
        let Ok(bytes) = <[u8; 64]>::try_from(bytes.as_slice()) else {
            return false;
        };
        key.verify_strict(message, &Signature::from_bytes(&bytes))
            .is_ok()
    }
}

fn validate_key_id(key_id: &str) -> Result<(), CatalogKeyError> {
    if !(8..=128).contains(&key_id.len())
        || !key_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(CatalogKeyError::InvalidKeyId(key_id.to_owned()));
    }
    Ok(())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CatalogKeyError {
    #[error("catalog trust root cannot be empty")]
    EmptyKeySet,
    #[error("invalid catalog key id: {0}")]
    InvalidKeyId(String),
    #[error("invalid Ed25519 public key: {0}")]
    InvalidPublicKey(String),
    #[error("duplicate catalog key id: {0}")]
    DuplicateKeyId(String),
}
