use crate::{event::TimingEvidence, DiagnosticStatus};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const MAX_CHECK_RESULTS: usize = 256;
pub const MAX_CHECK_METRICS: usize = 32;
pub const MAX_SUGGESTED_ACTIONS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckCategory {
    Credential,
    Microphone,
    Speaker,
    Stt,
    Tts,
    Llm,
    Provider,
    ModelPack,
    Admission,
    Capture,
    Audio,
    Model,
    Hardware,
    Gpu,
    Vram,
    Game,
    Overlay,
    Latency,
    Permission,
}

/// States how a diagnostic conclusion was obtained. A fixture result must never
/// be presented as proof about the machine currently running the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationProvenance {
    Measured,
    Unmeasured,
    Fixture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricUnit {
    Milliseconds,
    Bytes,
    Mebibytes,
    Percent,
    Hertz,
    FramesPerSecond,
    Count,
    Ratio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticMetric {
    pub value: f64,
    pub unit: MetricUnit,
    pub provenance: ObservationProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestedActionKind {
    RetryCheck,
    RestartComponent,
    RecheckPermissions,
    OpenSettingsSection,
    OpenBundledHelp,
    SelectAlternativeProvider,
    ReduceLocalModelLoad,
    DisableOptionalFeature,
}

/// A closed, declarative action. Consumers choose how to render or execute the
/// action; diagnostics can never supply a command line, executable, or URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedAction {
    pub action_id: String,
    pub kind: SuggestedActionKind,
    pub label: String,
    pub target_id: Option<String>,
    pub requires_confirmation: bool,
}

impl SuggestedAction {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !is_identifier(&self.action_id, 128)
            || !is_safe_action_label(&self.label)
            || self
                .target_id
                .as_deref()
                .is_some_and(|value| !is_action_target(value))
        {
            return Err("suggested action is unsafe or exceeds its bounds");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub check_id: String,
    pub category: CheckCategory,
    pub status: DiagnosticStatus,
    pub provenance: ObservationProvenance,
    pub observed_at_utc: Option<String>,
    pub duration_ms: Option<f64>,
    pub timing: Option<TimingEvidence>,
    pub summary_code: String,
    pub summary: String,
    pub error_code: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub metrics: BTreeMap<String, DiagnosticMetric>,
    pub suggested_actions: Vec<SuggestedAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CheckValidationError {
    #[error("check identity is missing or exceeds its bound")]
    InvalidIdentity,
    #[error("check summary is missing or exceeds its bound")]
    InvalidSummary,
    #[error("measured checks require an observation timestamp")]
    MissingMeasuredTimestamp,
    #[error("duration must be finite, non-negative, and measured")]
    InvalidDuration,
    #[error("check has too many metrics")]
    TooManyMetrics,
    #[error("metric {0} has an invalid name or non-finite value")]
    InvalidMetric(String),
    #[error("check has too many suggested actions")]
    TooManyActions,
    #[error("suggested action {0} is unsafe or exceeds its bounds")]
    InvalidAction(usize),
    #[error("provider or model identifier exceeds its bound")]
    InvalidSubject,
}

impl CheckResult {
    pub fn validate(&self) -> Result<(), CheckValidationError> {
        if !is_identifier(&self.check_id, 128) || !is_identifier(&self.summary_code, 128) {
            return Err(CheckValidationError::InvalidIdentity);
        }
        if self.summary.trim().is_empty() || self.summary.len() > 2_000 {
            return Err(CheckValidationError::InvalidSummary);
        }
        if self.provenance == ObservationProvenance::Measured
            && self
                .observed_at_utc
                .as_deref()
                .is_none_or(|value| value.trim().is_empty() || value.len() > 64)
        {
            return Err(CheckValidationError::MissingMeasuredTimestamp);
        }
        match (self.duration_ms, self.timing.as_ref()) {
            (None, None) => {}
            (Some(duration), Some(timing))
                if duration.is_finite()
                    && duration >= 0.0
                    && timing.validate().is_ok()
                    && timing.provenance == self.provenance
                    && (duration - timing.duration_ms).abs() <= 0.001 => {}
            _ => return Err(CheckValidationError::InvalidDuration),
        }
        if self.metrics.len() > MAX_CHECK_METRICS {
            return Err(CheckValidationError::TooManyMetrics);
        }
        for (name, metric) in &self.metrics {
            if !is_identifier(name, 64)
                || !metric.value.is_finite()
                || (self.provenance == ObservationProvenance::Unmeasured
                    && metric.provenance != ObservationProvenance::Unmeasured)
                || (self.provenance == ObservationProvenance::Fixture
                    && metric.provenance == ObservationProvenance::Measured)
            {
                return Err(CheckValidationError::InvalidMetric(name.clone()));
            }
        }
        if self.suggested_actions.len() > MAX_SUGGESTED_ACTIONS {
            return Err(CheckValidationError::TooManyActions);
        }
        for (index, action) in self.suggested_actions.iter().enumerate() {
            if action.validate().is_err() {
                return Err(CheckValidationError::InvalidAction(index));
            }
        }
        if self
            .provider_id
            .as_deref()
            .is_some_and(|value| !is_subject(value, 160))
            || self
                .model_id
                .as_deref()
                .is_some_and(|value| !is_subject(value, 200))
        {
            return Err(CheckValidationError::InvalidSubject);
        }
        if self
            .error_code
            .as_deref()
            .is_some_and(|value| !is_identifier(value, 128))
        {
            return Err(CheckValidationError::InvalidIdentity);
        }
        Ok(())
    }
}

pub(crate) fn is_identifier(value: &str, max_len: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max_len
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
        })
}

fn is_action_target(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn is_subject(value: &str, max_len: usize) -> bool {
    is_identifier(value, max_len)
        || (value.starts_with("<REDACTED_")
            && value.ends_with('>')
            && value.len() <= max_len
            && value[10..value.len() - 1]
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte == b'_'))
}

fn is_safe_action_label(value: &str) -> bool {
    if value.trim().is_empty() || value.len() > 240 || value.chars().any(char::is_control) {
        return false;
    }
    let normalized = value.to_ascii_lowercase();
    ![
        "http://",
        "https://",
        "powershell",
        "cmd.exe",
        ".exe",
        "javascript:",
        "file://",
        "\\\\",
    ]
    .iter()
    .any(|forbidden| normalized.contains(forbidden))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured_check(category: CheckCategory) -> CheckResult {
        CheckResult {
            check_id: "provider.connection".into(),
            category,
            status: DiagnosticStatus::Ok,
            provenance: ObservationProvenance::Measured,
            observed_at_utc: Some("2026-08-30T10:00:00Z".into()),
            duration_ms: Some(123.5),
            timing: Some(TimingEvidence {
                clock_source: crate::ClockSource::ProcessMonotonic,
                provenance: ObservationProvenance::Measured,
                started_monotonic_ns: 1,
                completed_monotonic_ns: 123_500_001,
                duration_ms: 123.5,
            }),
            summary_code: "provider.reachable".into(),
            summary: "Provider endpoint responded.".into(),
            error_code: None,
            provider_id: Some("nvidia_nim".into()),
            model_id: Some("meta/llama".into()),
            metrics: BTreeMap::from([(
                "time_to_headers".into(),
                DiagnosticMetric {
                    value: 123.5,
                    unit: MetricUnit::Milliseconds,
                    provenance: ObservationProvenance::Measured,
                },
            )]),
            suggested_actions: vec![SuggestedAction {
                action_id: "retry.provider".into(),
                kind: SuggestedActionKind::RetryCheck,
                label: "Run the provider check again".into(),
                target_id: Some("provider.connection".into()),
                requires_confirmation: false,
            }],
        }
    }

    #[test]
    fn every_supported_check_category_uses_the_same_truthful_contract() {
        for category in [
            CheckCategory::Credential,
            CheckCategory::Microphone,
            CheckCategory::Speaker,
            CheckCategory::Stt,
            CheckCategory::Tts,
            CheckCategory::Llm,
            CheckCategory::Provider,
            CheckCategory::ModelPack,
            CheckCategory::Admission,
            CheckCategory::Capture,
            CheckCategory::Audio,
            CheckCategory::Model,
            CheckCategory::Hardware,
            CheckCategory::Gpu,
            CheckCategory::Vram,
            CheckCategory::Game,
            CheckCategory::Overlay,
            CheckCategory::Latency,
            CheckCategory::Permission,
        ] {
            assert_eq!(measured_check(category).validate(), Ok(()));
        }
    }

    #[test]
    fn measured_claims_require_time_evidence() {
        let mut check = measured_check(CheckCategory::Audio);
        check.observed_at_utc = None;
        assert_eq!(
            check.validate(),
            Err(CheckValidationError::MissingMeasuredTimestamp)
        );
    }

    #[test]
    fn unmeasured_checks_cannot_smuggle_measured_duration_or_metrics() {
        let mut check = measured_check(CheckCategory::Hardware);
        check.provenance = ObservationProvenance::Unmeasured;
        assert_eq!(check.validate(), Err(CheckValidationError::InvalidDuration));

        check.duration_ms = None;
        check.timing = None;
        assert!(matches!(
            check.validate(),
            Err(CheckValidationError::InvalidMetric(_))
        ));
    }

    #[test]
    fn fixture_check_timing_remains_explicitly_fixture() {
        let mut check = measured_check(CheckCategory::Model);
        check.provenance = ObservationProvenance::Fixture;
        check.observed_at_utc = None;
        check.timing = Some(TimingEvidence {
            clock_source: crate::ClockSource::FixtureClock,
            provenance: ObservationProvenance::Fixture,
            started_monotonic_ns: 10,
            completed_monotonic_ns: 123_500_010,
            duration_ms: 123.5,
        });
        for metric in check.metrics.values_mut() {
            metric.provenance = ObservationProvenance::Fixture;
        }
        assert_eq!(check.validate(), Ok(()));
    }

    #[test]
    fn suggested_actions_cannot_encode_commands_or_urls() {
        let mut check = measured_check(CheckCategory::Capture);
        check.suggested_actions[0].target_id = Some("powershell -enc bad".into());
        assert_eq!(
            check.validate(),
            Err(CheckValidationError::InvalidAction(0))
        );

        check.suggested_actions[0].target_id = Some("https://attacker.example".into());
        assert_eq!(
            check.validate(),
            Err(CheckValidationError::InvalidAction(0))
        );

        check.suggested_actions[0].target_id = Some("capture_permissions".into());
        check.suggested_actions[0].label = "Run powershell -enc bad".into();
        assert_eq!(
            check.validate(),
            Err(CheckValidationError::InvalidAction(0))
        );
    }
}
