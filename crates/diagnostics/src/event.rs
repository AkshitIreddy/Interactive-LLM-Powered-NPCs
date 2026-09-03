use crate::{check::is_identifier, ObservationProvenance};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Controls only local structured-event detail. Failures and warnings are
/// never hidden; this setting cannot enable remote telemetry or content logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticVerbosity {
    Essential,
    Standard,
    Verbose,
}

impl DiagnosticVerbosity {
    pub fn permits(self, severity: Severity) -> bool {
        match self {
            Self::Essential => matches!(severity, Severity::Warn | Severity::Error),
            Self::Standard => matches!(severity, Severity::Info | Severity::Warn | Severity::Error),
            Self::Verbose => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticStatus {
    Ok,
    Degraded,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockSource {
    ProcessMonotonic,
    OperatingSystemMonotonic,
    ProviderReported,
    FixtureClock,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingEvidence {
    pub clock_source: ClockSource,
    pub provenance: ObservationProvenance,
    pub started_monotonic_ns: u64,
    pub completed_monotonic_ns: u64,
    pub duration_ms: f64,
}

impl TimingEvidence {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.provenance == ObservationProvenance::Unmeasured {
            return Err("timing evidence cannot be unmeasured");
        }
        if self.clock_source == ClockSource::FixtureClock
            && self.provenance != ObservationProvenance::Fixture
        {
            return Err("fixture clocks require fixture provenance");
        }
        if self.clock_source != ClockSource::FixtureClock
            && self.provenance == ObservationProvenance::Fixture
        {
            return Err("fixture provenance requires a fixture clock");
        }
        if self.completed_monotonic_ns < self.started_monotonic_ns
            || !self.duration_ms.is_finite()
            || self.duration_ms < 0.0
        {
            return Err("timing evidence is invalid");
        }
        Ok(())
    }
}

/// A deliberately content-free structured event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvent {
    pub monotonic_ns: u64,
    pub component: String,
    pub event_name: String,
    pub severity: Severity,
    pub status: DiagnosticStatus,
    pub provenance: ObservationProvenance,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub duration_ms: Option<f64>,
    pub timing: Option<TimingEvidence>,
    pub error_code: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub numeric: BTreeMap<String, f64>,
    pub labels: BTreeMap<String, String>,
}

impl DiagnosticEvent {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.component.is_empty() || self.event_name.is_empty() {
            return Err("component and event_name are required");
        }
        if !is_identifier(&self.component, 80) || !is_identifier(&self.event_name, 120) {
            return Err("event identity exceeds length limit");
        }
        if self.numeric.len() > 32 || self.labels.len() > 24 {
            return Err("event attribute count exceeds limit");
        }
        if self
            .numeric
            .iter()
            .any(|(key, value)| key.is_empty() || key.len() > 64 || !value.is_finite())
        {
            return Err("event numeric attribute is unsafe or invalid");
        }
        if self.labels.iter().any(|(key, value)| {
            key.is_empty()
                || key.len() > 64
                || value.len() > 256
                || value.chars().any(char::is_control)
                || is_forbidden_label_key(key)
        }) {
            return Err("event label is unsafe or too large");
        }
        if self
            .duration_ms
            .is_some_and(|value| !value.is_finite() || value < 0.0)
        {
            return Err("event duration is invalid");
        }
        if let Some(timing) = &self.timing {
            timing.validate()?;
            if self.provenance != timing.provenance {
                return Err("event and timing provenance do not match");
            }
            if self
                .duration_ms
                .is_some_and(|duration| (duration - timing.duration_ms).abs() > 0.001)
            {
                return Err("event duration and timing evidence do not match");
            }
        }
        if self.duration_ms.is_some() && self.timing.is_none() {
            return Err("duration requires timing provenance");
        }
        for value in [
            self.trace_id.as_deref(),
            self.span_id.as_deref(),
            self.error_code.as_deref(),
            self.provider_id.as_deref(),
            self.model_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.is_empty() || value.len() > 200 || value.chars().any(char::is_control) {
                return Err("event identifier is unsafe or too large");
            }
        }
        Ok(())
    }

    /// Correlates a content-free subsystem event with one turn. The value is
    /// kept in the already bounded/redacted labels map for wire compatibility.
    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Result<Self, &'static str> {
        let turn_id = turn_id.into();
        if !is_identifier(&turn_id, 128) {
            return Err("turn correlation id is invalid");
        }
        self.labels.insert("turn_id".into(), turn_id);
        self.validate()?;
        Ok(self)
    }

    pub fn turn_id(&self) -> Option<&str> {
        self.labels.get("turn_id").map(String::as_str)
    }

    pub fn should_record(&self, verbosity: DiagnosticVerbosity) -> bool {
        verbosity.permits(self.severity)
    }
}

fn is_forbidden_label_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    [
        "prompt",
        "response",
        "transcript",
        "authorization",
        "token",
        "secret",
        "password",
        "audio",
        "voice",
        "frame",
        "image",
        "pixel",
        "screenshot",
        "webcam",
        "memory_text",
        "conversation",
        "username",
        "email",
        "ip_address",
        "file_path",
        "home_path",
    ]
    .iter()
    .any(|forbidden| normalized.contains(forbidden))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> DiagnosticEvent {
        DiagnosticEvent {
            monotonic_ns: 1,
            component: "runtime".into(),
            event_name: "turn.completed".into(),
            severity: Severity::Info,
            status: DiagnosticStatus::Ok,
            provenance: ObservationProvenance::Measured,
            trace_id: None,
            span_id: None,
            duration_ms: Some(412.0),
            timing: Some(TimingEvidence {
                clock_source: ClockSource::ProcessMonotonic,
                provenance: ObservationProvenance::Measured,
                started_monotonic_ns: 1,
                completed_monotonic_ns: 412_000_001,
                duration_ms: 412.0,
            }),
            error_code: None,
            provider_id: Some("local".into()),
            model_id: Some("fixture".into()),
            numeric: BTreeMap::new(),
            labels: BTreeMap::new(),
        }
    }

    #[test]
    fn rejects_content_bearing_label_names() {
        let mut value = event();
        value
            .labels
            .insert("user_transcript".into(), "hello".into());
        assert!(value.validate().is_err());
    }

    #[test]
    fn accepts_bounded_operational_metadata() {
        let mut value = event();
        value.labels.insert("backend".into(), "cpu".into());
        assert_eq!(value.validate(), Ok(()));
    }

    #[test]
    fn rejects_non_finite_metrics_and_unproven_durations() {
        let mut value = event();
        value.numeric.insert("latency".into(), f64::NAN);
        assert!(value.validate().is_err());

        value.numeric.clear();
        value.timing = None;
        assert_eq!(value.validate(), Err("duration requires timing provenance"));
    }

    #[test]
    fn fixture_timing_cannot_masquerade_as_measurement() {
        let mut value = event();
        value.timing.as_mut().unwrap().clock_source = ClockSource::FixtureClock;
        assert_eq!(
            value.validate(),
            Err("fixture clocks require fixture provenance")
        );
    }

    #[test]
    fn duration_and_timing_evidence_must_agree() {
        let mut value = event();
        value.timing.as_mut().unwrap().duration_ms = 99.0;
        assert_eq!(
            value.validate(),
            Err("event duration and timing evidence do not match")
        );
    }

    #[test]
    fn verbosity_never_suppresses_warning_or_error_and_turn_ids_are_bounded() {
        let mut info = event();
        assert!(!info.should_record(DiagnosticVerbosity::Essential));
        assert!(info.should_record(DiagnosticVerbosity::Standard));
        info.severity = Severity::Warn;
        assert!(info.should_record(DiagnosticVerbosity::Essential));

        let correlated = info.with_turn_id("turn-0042").unwrap();
        assert_eq!(correlated.turn_id(), Some("turn-0042"));
        assert!(event().with_turn_id("contains spaces").is_err());
    }
}
