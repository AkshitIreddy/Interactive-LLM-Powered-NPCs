use prost::Message;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Semantic version for the transport contract, independent of application version.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Message, Serialize, Deserialize)]
pub struct ProtocolVersion {
    #[prost(uint32, tag = "1")]
    pub major: u32,
    #[prost(uint32, tag = "2")]
    pub minor: u32,
    #[prost(uint32, tag = "3")]
    pub patch: u32,
}

impl ProtocolVersion {
    pub const V1_0_0: Self = Self::new(1, 0, 0);
    pub const CURRENT: Self = Self::V1_0_0;

    #[must_use]
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    #[must_use]
    pub const fn wire_compatible_with(self, other: Self) -> bool {
        self.major != 0 && self.major == other.major
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRange {
    pub minimum: ProtocolVersion,
    pub maximum: ProtocolVersion,
}

impl VersionRange {
    pub const V1: Self = Self {
        minimum: ProtocolVersion::V1_0_0,
        maximum: ProtocolVersion::V1_0_0,
    };

    pub fn new(
        minimum: ProtocolVersion,
        maximum: ProtocolVersion,
    ) -> Result<Self, NegotiationError> {
        let range = Self { minimum, maximum };
        range.validate()?;
        Ok(range)
    }

    pub fn validate(self) -> Result<(), NegotiationError> {
        if self.minimum.major == 0 || self.maximum.major == 0 {
            return Err(NegotiationError::UnstableVersion);
        }
        if self.minimum > self.maximum {
            return Err(NegotiationError::InvertedRange);
        }
        if self.minimum.major != self.maximum.major {
            return Err(NegotiationError::CrossMajorRange);
        }
        Ok(())
    }

    #[must_use]
    pub fn contains(self, version: ProtocolVersion) -> bool {
        version.major == self.minimum.major && version >= self.minimum && version <= self.maximum
    }
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct NegotiationHelloV1 {
    #[prost(message, optional, tag = "1")]
    pub minimum: Option<ProtocolVersion>,
    #[prost(message, optional, tag = "2")]
    pub maximum: Option<ProtocolVersion>,
    #[prost(string, repeated, tag = "3")]
    pub optional_features: Vec<String>,
    #[prost(string, tag = "4")]
    pub peer_name: String,
    #[prost(string, tag = "5")]
    pub peer_build: String,
}

impl NegotiationHelloV1 {
    pub fn range(&self) -> Result<VersionRange, NegotiationError> {
        VersionRange::new(
            self.minimum.ok_or(NegotiationError::MissingRange)?,
            self.maximum.ok_or(NegotiationError::MissingRange)?,
        )
    }
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct NegotiationAcceptedV1 {
    #[prost(message, optional, tag = "1")]
    pub selected: Option<ProtocolVersion>,
    #[prost(string, repeated, tag = "2")]
    pub enabled_features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NegotiationError {
    #[error("protocol major zero is reserved for unstable local experiments")]
    UnstableVersion,
    #[error("minimum protocol version exceeds maximum")]
    InvertedRange,
    #[error("one advertised range cannot span multiple major protocol versions")]
    CrossMajorRange,
    #[error("negotiation hello omitted its version range")]
    MissingRange,
    #[error("the peers have no mutually supported protocol version")]
    NoOverlap,
}

pub fn negotiate(
    local: VersionRange,
    remote: VersionRange,
) -> Result<ProtocolVersion, NegotiationError> {
    local.validate()?;
    remote.validate()?;
    if local.minimum.major != remote.minimum.major {
        return Err(NegotiationError::NoOverlap);
    }
    let minimum = local.minimum.max(remote.minimum);
    let maximum = local.maximum.min(remote.maximum);
    if minimum > maximum {
        Err(NegotiationError::NoOverlap)
    } else {
        // Prefer the newest mutually understood contract.
        Ok(maximum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_selects_newest_shared_version() {
        let local = VersionRange::new(ProtocolVersion::new(1, 1, 0), ProtocolVersion::new(1, 4, 0))
            .unwrap();
        let remote =
            VersionRange::new(ProtocolVersion::new(1, 2, 0), ProtocolVersion::new(1, 3, 5))
                .unwrap();
        assert_eq!(negotiate(local, remote), Ok(ProtocolVersion::new(1, 3, 5)));
    }

    #[test]
    fn major_mismatch_is_not_silently_downgraded() {
        let v2 = VersionRange::new(ProtocolVersion::new(2, 0, 0), ProtocolVersion::new(2, 1, 0))
            .unwrap();
        assert_eq!(
            negotiate(VersionRange::V1, v2),
            Err(NegotiationError::NoOverlap)
        );
    }
}
