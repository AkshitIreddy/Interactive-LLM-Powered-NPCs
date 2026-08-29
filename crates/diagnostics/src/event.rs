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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticStatus {
    Ok,
    Degraded,
    Failed,
    Skipped,
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
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub duration_ms: Option<f64>,
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
        if self.component.len() > 80 || self.event_name.len() > 120 {
            return Err("event identity exceeds length limit");
        }
        if self.numeric.len() > 32 || self.labels.len() > 24 {
            return Err("event attribute count exceeds limit");
        }
        if self
            .labels
            .iter()
            .any(|(key, value)| key.len() > 64 || value.len() > 256 || is_forbidden_label_key(key))
        {
            return Err("event label is unsafe or too large");
        }
        Ok(())
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
        "frame",
        "screenshot",
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
            trace_id: None,
            span_id: None,
            duration_ms: Some(412.0),
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
}
