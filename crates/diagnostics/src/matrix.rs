use crate::{
    check::is_identifier, CheckCategory, CheckResult, DiagnosticStatus, ObservationProvenance,
    SuggestedAction, SuggestedActionKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticMatrixKind {
    ProviderCredentials,
    Microphone,
    Speaker,
    Stt,
    Tts,
    Llm,
    ModelPacks,
    ModelAdmission,
    Capture,
    Gpu,
    Vram,
    Game,
    Overlay,
    Latency,
    Permissions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticMatrixDefinition {
    pub kind: DiagnosticMatrixKind,
    pub check_id: &'static str,
    pub category: CheckCategory,
    pub settings_target: &'static str,
    pub remediation_label: &'static str,
    pub remediation_kind: SuggestedActionKind,
}

pub const PRODUCT_DIAGNOSTIC_MATRIX: &[DiagnosticMatrixDefinition] = &[
    definition(
        DiagnosticMatrixKind::ProviderCredentials,
        "credentials.providers",
        CheckCategory::Credential,
        "providers",
        "Open provider credential settings",
        SuggestedActionKind::OpenSettingsSection,
    ),
    definition(
        DiagnosticMatrixKind::Microphone,
        "audio.microphone",
        CheckCategory::Microphone,
        "microphone",
        "Recheck microphone permissions and device",
        SuggestedActionKind::RecheckPermissions,
    ),
    definition(
        DiagnosticMatrixKind::Speaker,
        "audio.speaker",
        CheckCategory::Speaker,
        "speaker",
        "Open speaker device settings",
        SuggestedActionKind::OpenSettingsSection,
    ),
    definition(
        DiagnosticMatrixKind::Stt,
        "provider.stt",
        CheckCategory::Stt,
        "stt",
        "Run the speech recognition check again",
        SuggestedActionKind::RetryCheck,
    ),
    definition(
        DiagnosticMatrixKind::Tts,
        "provider.tts",
        CheckCategory::Tts,
        "tts",
        "Run the speech synthesis check again",
        SuggestedActionKind::RetryCheck,
    ),
    definition(
        DiagnosticMatrixKind::Llm,
        "provider.llm",
        CheckCategory::Llm,
        "llm",
        "Run the language model check again",
        SuggestedActionKind::RetryCheck,
    ),
    definition(
        DiagnosticMatrixKind::ModelPacks,
        "models.pack_integrity",
        CheckCategory::ModelPack,
        "models",
        "Open local model pack settings",
        SuggestedActionKind::OpenSettingsSection,
    ),
    definition(
        DiagnosticMatrixKind::ModelAdmission,
        "models.admission",
        CheckCategory::Admission,
        "models",
        "Reduce the selected local model load",
        SuggestedActionKind::ReduceLocalModelLoad,
    ),
    definition(
        DiagnosticMatrixKind::Capture,
        "media.capture",
        CheckCategory::Capture,
        "capture",
        "Recheck capture permission and target",
        SuggestedActionKind::RecheckPermissions,
    ),
    definition(
        DiagnosticMatrixKind::Gpu,
        "hardware.gpu",
        CheckCategory::Gpu,
        "hardware",
        "Open GPU troubleshooting help",
        SuggestedActionKind::OpenBundledHelp,
    ),
    definition(
        DiagnosticMatrixKind::Vram,
        "hardware.vram",
        CheckCategory::Vram,
        "models",
        "Reduce local model VRAM usage",
        SuggestedActionKind::ReduceLocalModelLoad,
    ),
    definition(
        DiagnosticMatrixKind::Game,
        "game.target",
        CheckCategory::Game,
        "game",
        "Open game target settings",
        SuggestedActionKind::OpenSettingsSection,
    ),
    definition(
        DiagnosticMatrixKind::Overlay,
        "media.overlay",
        CheckCategory::Overlay,
        "overlay",
        "Disable the optional overlay",
        SuggestedActionKind::DisableOptionalFeature,
    ),
    definition(
        DiagnosticMatrixKind::Latency,
        "runtime.latency",
        CheckCategory::Latency,
        "performance",
        "Run the latency check again",
        SuggestedActionKind::RetryCheck,
    ),
    definition(
        DiagnosticMatrixKind::Permissions,
        "system.permissions",
        CheckCategory::Permission,
        "privacy",
        "Recheck application permissions",
        SuggestedActionKind::RecheckPermissions,
    ),
];

const fn definition(
    kind: DiagnosticMatrixKind,
    check_id: &'static str,
    category: CheckCategory,
    settings_target: &'static str,
    remediation_label: &'static str,
    remediation_kind: SuggestedActionKind,
) -> DiagnosticMatrixDefinition {
    DiagnosticMatrixDefinition {
        kind,
        check_id,
        category,
        settings_target,
        remediation_label,
        remediation_kind,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialPresenceState {
    Present,
    Absent,
    Unavailable,
    Unknown,
}

/// Deliberately has no credential value, fingerprint, length, or hash field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderCredentialPresence {
    pub provider_id: String,
    pub state: CredentialPresenceState,
    pub provenance: ObservationProvenance,
    pub observed_at_utc: Option<String>,
}

impl ProviderCredentialPresence {
    pub fn validate(&self) -> Result<(), DiagnosticMatrixError> {
        if !is_identifier(&self.provider_id, 128) {
            return Err(DiagnosticMatrixError::InvalidCredentialPresence);
        }
        if self.provenance == ObservationProvenance::Measured
            && self.observed_at_utc.as_deref().is_none_or(|value| {
                value.trim().is_empty() || value.len() > 64 || value.chars().any(char::is_control)
            })
        {
            return Err(DiagnosticMatrixError::InvalidCredentialPresence);
        }
        if self.provenance != ObservationProvenance::Measured && self.observed_at_utc.is_some() {
            return Err(DiagnosticMatrixError::InvalidCredentialPresence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticMatrixResult {
    pub schema_version: String,
    pub correlated_turn_id: Option<String>,
    pub credential_presence: Vec<ProviderCredentialPresence>,
    pub checks: Vec<CheckResult>,
}

impl DiagnosticMatrixResult {
    pub fn validate(&self) -> Result<(), DiagnosticMatrixError> {
        if self.schema_version != "1.0.0"
            || self
                .correlated_turn_id
                .as_deref()
                .is_some_and(|value| !is_identifier(value, 128))
        {
            return Err(DiagnosticMatrixError::InvalidIdentity);
        }
        let expected = PRODUCT_DIAGNOSTIC_MATRIX
            .iter()
            .map(|definition| definition.check_id)
            .collect::<BTreeSet<_>>();
        let mut observed = BTreeSet::new();
        for check in &self.checks {
            check
                .validate()
                .map_err(|error| DiagnosticMatrixError::InvalidCheck(error.to_string()))?;
            let definition = definition_for_id(&check.check_id)
                .ok_or_else(|| DiagnosticMatrixError::UnexpectedCheck(check.check_id.clone()))?;
            if check.category != definition.category {
                return Err(DiagnosticMatrixError::WrongCategory(check.check_id.clone()));
            }
            if !observed.insert(check.check_id.as_str()) {
                return Err(DiagnosticMatrixError::DuplicateCheck(
                    check.check_id.clone(),
                ));
            }
        }
        if observed != expected {
            let missing = expected
                .difference(&observed)
                .next()
                .copied()
                .unwrap_or("unknown");
            return Err(DiagnosticMatrixError::MissingCheck(missing.into()));
        }
        let mut providers = BTreeSet::new();
        for presence in &self.credential_presence {
            presence.validate()?;
            if !providers.insert(presence.provider_id.as_str()) {
                return Err(DiagnosticMatrixError::DuplicateProvider(
                    presence.provider_id.clone(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct DiagnosticMatrixBuilder {
    correlated_turn_id: Option<String>,
    credential_presence: Vec<ProviderCredentialPresence>,
    checks: BTreeMap<DiagnosticMatrixKind, CheckResult>,
}

impl DiagnosticMatrixBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn correlated_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.correlated_turn_id = Some(turn_id.into());
        self
    }

    pub fn push_credential_presence(
        &mut self,
        presence: ProviderCredentialPresence,
    ) -> Result<(), DiagnosticMatrixError> {
        presence.validate()?;
        if self
            .credential_presence
            .iter()
            .any(|item| item.provider_id == presence.provider_id)
        {
            return Err(DiagnosticMatrixError::DuplicateProvider(
                presence.provider_id,
            ));
        }
        self.credential_presence.push(presence);
        Ok(())
    }

    pub fn push_check(&mut self, check: CheckResult) -> Result<(), DiagnosticMatrixError> {
        check
            .validate()
            .map_err(|error| DiagnosticMatrixError::InvalidCheck(error.to_string()))?;
        let definition = definition_for_id(&check.check_id)
            .ok_or_else(|| DiagnosticMatrixError::UnexpectedCheck(check.check_id.clone()))?;
        if check.category != definition.category {
            return Err(DiagnosticMatrixError::WrongCategory(check.check_id));
        }
        if self.checks.insert(definition.kind, check).is_some() {
            return Err(DiagnosticMatrixError::DuplicateCheck(
                definition.check_id.into(),
            ));
        }
        Ok(())
    }

    /// Adds explicit unmeasured rows for every check not yet supplied. This is
    /// suitable for initial UI state, never as machine proof.
    pub fn fill_unmeasured(&mut self) {
        for definition in PRODUCT_DIAGNOSTIC_MATRIX {
            self.checks
                .entry(definition.kind)
                .or_insert_with(|| unmeasured_check(definition));
        }
    }

    pub fn build(self) -> Result<DiagnosticMatrixResult, DiagnosticMatrixError> {
        let result = DiagnosticMatrixResult {
            schema_version: "1.0.0".into(),
            correlated_turn_id: self.correlated_turn_id,
            credential_presence: self.credential_presence,
            checks: PRODUCT_DIAGNOSTIC_MATRIX
                .iter()
                .filter_map(|definition| self.checks.get(&definition.kind).cloned())
                .collect(),
        };
        result.validate()?;
        Ok(result)
    }
}

fn definition_for_id(check_id: &str) -> Option<&'static DiagnosticMatrixDefinition> {
    PRODUCT_DIAGNOSTIC_MATRIX
        .iter()
        .find(|definition| definition.check_id == check_id)
}

fn unmeasured_check(definition: &DiagnosticMatrixDefinition) -> CheckResult {
    CheckResult {
        check_id: definition.check_id.into(),
        category: definition.category,
        status: DiagnosticStatus::Skipped,
        provenance: ObservationProvenance::Unmeasured,
        observed_at_utc: None,
        duration_ms: None,
        timing: None,
        summary_code: format!("{}.not_measured", definition.check_id),
        summary: "This check has not run on the current machine and no readiness claim is made."
            .into(),
        error_code: None,
        provider_id: None,
        model_id: None,
        metrics: BTreeMap::new(),
        suggested_actions: vec![SuggestedAction {
            action_id: format!("remediate.{}", definition.check_id),
            kind: definition.remediation_kind,
            label: definition.remediation_label.into(),
            target_id: Some(definition.settings_target.into()),
            requires_confirmation: false,
        }],
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DiagnosticMatrixError {
    #[error("diagnostic matrix identity is invalid")]
    InvalidIdentity,
    #[error("credential presence metadata is invalid")]
    InvalidCredentialPresence,
    #[error("provider {0} appears more than once")]
    DuplicateProvider(String),
    #[error("diagnostic check is invalid: {0}")]
    InvalidCheck(String),
    #[error("diagnostic matrix is missing required check {0}")]
    MissingCheck(String),
    #[error("diagnostic matrix contains unsupported check {0}")]
    UnexpectedCheck(String),
    #[error("diagnostic matrix contains duplicate check {0}")]
    DuplicateCheck(String),
    #[error("diagnostic check {0} has the wrong category")]
    WrongCategory(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_product_matrix_is_exact_actionable_and_unmeasured_by_default() {
        let mut builder = DiagnosticMatrixBuilder::new().correlated_turn_id("turn-42");
        builder.fill_unmeasured();
        let matrix = builder.build().unwrap();
        assert_eq!(matrix.checks.len(), PRODUCT_DIAGNOSTIC_MATRIX.len());
        assert_eq!(matrix.correlated_turn_id.as_deref(), Some("turn-42"));
        assert!(matrix.checks.iter().all(|check| {
            check.provenance == ObservationProvenance::Unmeasured
                && check.status == DiagnosticStatus::Skipped
                && !check.suggested_actions.is_empty()
        }));
    }

    #[test]
    fn credential_contract_serializes_presence_only() {
        let presence = ProviderCredentialPresence {
            provider_id: "openai".into(),
            state: CredentialPresenceState::Present,
            provenance: ObservationProvenance::Measured,
            observed_at_utc: Some("2026-08-30T10:00:00Z".into()),
        };
        let json = serde_json::to_value(&presence).unwrap();
        let object = json.as_object().unwrap();
        assert_eq!(object.len(), 4);
        for forbidden in [
            "credential",
            "secret",
            "token",
            "key",
            "value",
            "hash",
            "length",
        ] {
            assert!(!object
                .keys()
                .any(|key| key.to_ascii_lowercase().contains(forbidden)));
        }
    }

    #[test]
    fn incomplete_or_mislabeled_matrix_fails_closed() {
        let builder = DiagnosticMatrixBuilder::new();
        assert!(matches!(
            builder.build(),
            Err(DiagnosticMatrixError::MissingCheck(_))
        ));

        let mut builder = DiagnosticMatrixBuilder::new();
        let mut check = unmeasured_check(&PRODUCT_DIAGNOSTIC_MATRIX[0]);
        check.category = CheckCategory::Provider;
        assert!(matches!(
            builder.push_check(check),
            Err(DiagnosticMatrixError::WrongCategory(_))
        ));
    }
}
