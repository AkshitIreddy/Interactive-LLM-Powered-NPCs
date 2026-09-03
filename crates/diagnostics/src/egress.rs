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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalDiagnosticDataClass {
    DiagnosticFacts,
    DiagnosticEvents,
    CrashRecoveryMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryPolicy {
    Prohibited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportInitiation {
    UserInitiatedOnly,
}

/// Machine-readable privacy promise embedded in every diagnostics export.
/// There is intentionally no remote destination field or enabled variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyEgressDeclaration {
    pub schema_version: String,
    pub remote_telemetry: TelemetryPolicy,
    pub automatic_upload: TelemetryPolicy,
    pub export_initiation: ExportInitiation,
    pub locally_recorded: BTreeSet<LocalDiagnosticDataClass>,
    pub excluded_by_design: BTreeSet<DataClass>,
}

impl PrivacyEgressDeclaration {
    pub fn local_diagnostics_v1() -> Self {
        Self {
            schema_version: "1.0.0".into(),
            remote_telemetry: TelemetryPolicy::Prohibited,
            automatic_upload: TelemetryPolicy::Prohibited,
            export_initiation: ExportInitiation::UserInitiatedOnly,
            locally_recorded: BTreeSet::from([
                LocalDiagnosticDataClass::DiagnosticFacts,
                LocalDiagnosticDataClass::DiagnosticEvents,
                LocalDiagnosticDataClass::CrashRecoveryMetadata,
            ]),
            excluded_by_design: BTreeSet::from([
                DataClass::MicrophoneAudio,
                DataClass::Transcript,
                DataClass::SelectedGameFrame,
                DataClass::WebcamFrame,
                DataClass::GameContext,
                DataClass::ConversationMemory,
            ]),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != "1.0.0" {
            return Err("unsupported privacy declaration schema");
        }
        let permitted = BTreeSet::from([
            LocalDiagnosticDataClass::DiagnosticFacts,
            LocalDiagnosticDataClass::DiagnosticEvents,
            LocalDiagnosticDataClass::CrashRecoveryMetadata,
        ]);
        if self.locally_recorded != permitted {
            return Err("privacy declaration omits required local diagnostic classes");
        }
        let required_exclusions = BTreeSet::from([
            DataClass::MicrophoneAudio,
            DataClass::Transcript,
            DataClass::SelectedGameFrame,
            DataClass::WebcamFrame,
            DataClass::GameContext,
            DataClass::ConversationMemory,
        ]);
        if !required_exclusions.is_subset(&self.excluded_by_design) {
            return Err("privacy declaration omits a content-bearing exclusion");
        }
        Ok(())
    }
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

    #[test]
    fn local_diagnostics_declaration_has_no_remote_telemetry_escape_hatch() {
        let declaration = PrivacyEgressDeclaration::local_diagnostics_v1();
        assert_eq!(declaration.validate(), Ok(()));
        assert_eq!(declaration.remote_telemetry, TelemetryPolicy::Prohibited);
        assert_eq!(declaration.automatic_upload, TelemetryPolicy::Prohibited);
        assert!(declaration
            .excluded_by_design
            .contains(&DataClass::MicrophoneAudio));
        assert!(declaration
            .excluded_by_design
            .contains(&DataClass::SelectedGameFrame));
    }

    #[test]
    fn privacy_declaration_rejects_missing_local_class_or_content_exclusion() {
        let mut declaration = PrivacyEgressDeclaration::local_diagnostics_v1();
        declaration
            .locally_recorded
            .remove(&LocalDiagnosticDataClass::DiagnosticEvents);
        assert_eq!(
            declaration.validate(),
            Err("privacy declaration omits required local diagnostic classes")
        );

        let mut declaration = PrivacyEgressDeclaration::local_diagnostics_v1();
        declaration
            .excluded_by_design
            .remove(&DataClass::Transcript);
        assert_eq!(
            declaration.validate(),
            Err("privacy declaration omits a content-bearing exclusion")
        );
    }
}
