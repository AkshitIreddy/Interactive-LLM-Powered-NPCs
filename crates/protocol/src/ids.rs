use prost::Message;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use thiserror::Error;
use uuid::Uuid;

/// Errors common to every opaque protocol identifier.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdError {
    #[error("identifier must contain exactly 16 bytes, got {0}")]
    InvalidLength(usize),
    #[error("the all-zero identifier is reserved and cannot identify a live object")]
    Nil,
    #[error("invalid UUID: {0}")]
    InvalidUuid(String),
}

macro_rules! define_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, PartialEq, Eq, Hash, Message, Serialize, Deserialize)]
        #[serde(try_from = "Uuid", into = "Uuid")]
        pub struct $name {
            #[prost(bytes = "vec", tag = "1")]
            value: Vec<u8>,
        }

        impl $name {
            pub const BYTE_LEN: usize = 16;

            #[must_use]
            pub fn new() -> Self {
                Self::from_uuid(Uuid::new_v4())
            }

            #[must_use]
            pub fn from_uuid(value: Uuid) -> Self {
                Self {
                    value: value.as_bytes().to_vec(),
                }
            }

            pub fn from_bytes(value: impl AsRef<[u8]>) -> Result<Self, IdError> {
                let bytes = value.as_ref();
                if bytes.len() != Self::BYTE_LEN {
                    return Err(IdError::InvalidLength(bytes.len()));
                }
                if bytes.iter().all(|byte| *byte == 0) {
                    return Err(IdError::Nil);
                }
                Ok(Self {
                    value: bytes.to_vec(),
                })
            }

            pub fn validate(&self) -> Result<(), IdError> {
                Self::from_bytes(&self.value).map(|_| ())
            }

            #[must_use]
            pub fn as_bytes(&self) -> &[u8] {
                &self.value
            }

            #[must_use]
            pub fn to_uuid(&self) -> Option<Uuid> {
                Uuid::from_slice(&self.value)
                    .ok()
                    .filter(|value| !value.is_nil())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self.to_uuid() {
                    Some(value) => value.fmt(formatter),
                    None => write!(formatter, "<invalid-{}>", stringify!($name)),
                }
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value)
                    .map_err(|error| IdError::InvalidUuid(error.to_string()))
                    .and_then(|uuid| {
                        if uuid.is_nil() {
                            Err(IdError::Nil)
                        } else {
                            Ok(Self::from_uuid(uuid))
                        }
                    })
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.to_uuid().unwrap_or_else(Uuid::nil)
            }
        }

        impl TryFrom<Uuid> for $name {
            type Error = IdError;

            fn try_from(value: Uuid) -> Result<Self, Self::Error> {
                if value.is_nil() {
                    Err(IdError::Nil)
                } else {
                    Ok(Self::from_uuid(value))
                }
            }
        }
    };
}

define_id!(
    LaunchNonce,
    "Random per-process-launch nonce used to reject stale peers."
);
define_id!(SessionId, "Conversation/game session identifier.");
define_id!(TurnId, "One user-to-NPC conversational turn.");
define_id!(RequestId, "One provider or worker request within a turn.");
define_id!(ActorId, "Stable profile or encounter-scoped NPC identity.");
define_id!(TraceId, "Cross-process diagnostic trace identifier.");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_text_and_json() {
        let session = SessionId::new();
        let text = session.to_string();
        assert_eq!(text.parse::<SessionId>().unwrap(), session);
        let json = serde_json::to_string(&session).unwrap();
        assert_eq!(serde_json::from_str::<SessionId>(&json).unwrap(), session);
    }

    #[test]
    fn nil_and_malformed_ids_are_rejected() {
        assert_eq!(SessionId::from_bytes([0; 16]), Err(IdError::Nil));
        assert_eq!(
            SessionId::from_bytes([1; 15]),
            Err(IdError::InvalidLength(15))
        );
        assert!("00000000-0000-0000-0000-000000000000"
            .parse::<SessionId>()
            .is_err());
    }
}
