use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    MicrophoneAudio,
    Transcript,
    SelectedGameFrame,
    WebcamFrame,
    GameContext,
    ConversationMemory,
    DiagnosticFacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EgressManifest {
    pub feature_id: String,
    pub provider_id: String,
    pub destination_hosts: BTreeSet<String>,
    pub data_classes: BTreeSet<DataClass>,
    pub required: bool,
    pub retention_policy_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    Offline,
    LocalOnly,
    Cloud,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressDecision {
    Allowed,
    BlockedOffline,
    BlockedLocalOnly,
    BlockedHost,
    BlockedDataClass(DataClass),
}

#[derive(Debug, Clone)]
pub struct NetworkPolicy {
    pub mode: NetworkMode,
    pub allowed_hosts: BTreeSet<String>,
    pub allowed_data_classes: BTreeSet<DataClass>,
}

impl NetworkPolicy {
    pub fn evaluate(&self, manifest: &EgressManifest) -> EgressDecision {
        match self.mode {
            NetworkMode::Offline => return EgressDecision::BlockedOffline,
            NetworkMode::LocalOnly => return EgressDecision::BlockedLocalOnly,
            NetworkMode::Cloud => {}
        }
        if !manifest
            .destination_hosts
            .iter()
            .all(|host| self.allowed_hosts.contains(&host.to_ascii_lowercase()))
        {
            return EgressDecision::BlockedHost;
        }
        if let Some(blocked) = manifest
            .data_classes
            .iter()
            .find(|class| !self.allowed_data_classes.contains(class))
        {
            return EgressDecision::BlockedDataClass(*blocked);
        }
        EgressDecision::Allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> EgressManifest {
        EgressManifest {
            feature_id: "llm".into(),
            provider_id: "example".into(),
            destination_hosts: BTreeSet::from(["api.example.test".into()]),
            data_classes: BTreeSet::from([DataClass::Transcript, DataClass::GameContext]),
            required: true,
            retention_policy_url: None,
        }
    }

    #[test]
    fn offline_is_absolute() {
        let policy = NetworkPolicy {
            mode: NetworkMode::Offline,
            allowed_hosts: BTreeSet::from(["api.example.test".into()]),
            allowed_data_classes: BTreeSet::from([DataClass::Transcript]),
        };
        assert_eq!(policy.evaluate(&manifest()), EgressDecision::BlockedOffline);
    }

    #[test]
    fn cloud_requires_host_and_data_authorization() {
        let policy = NetworkPolicy {
            mode: NetworkMode::Cloud,
            allowed_hosts: BTreeSet::from(["api.example.test".into()]),
            allowed_data_classes: BTreeSet::from([DataClass::Transcript]),
        };
        assert_eq!(
            policy.evaluate(&manifest()),
            EgressDecision::BlockedDataClass(DataClass::GameContext)
        );
    }
}
