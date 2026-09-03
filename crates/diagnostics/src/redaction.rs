use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionKind {
    BearerToken,
    ApiCredential,
    ProviderCredential,
    Jwt,
    PrivateKey,
    UrlCredential,
    SensitiveField,
    Email,
    IpAddress,
    UserPath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionFinding {
    pub kind: RedactionKind,
    pub count: usize,
}

#[derive(Debug, Default, Clone)]
pub struct Redactor;

impl Redactor {
    pub fn redact(&self, input: &str) -> (String, Vec<RedactionFinding>) {
        let mut text = input.to_string();
        let mut findings = Vec::new();

        for (kind, regex, replacement) in patterns() {
            let count = regex.find_iter(&text).count();
            if count > 0 {
                text = regex.replace_all(&text, *replacement).into_owned();
                findings.push(RedactionFinding { kind: *kind, count });
            }
        }
        (text, findings)
    }

    /// Redacts strings recursively and drops the complete value of fields whose
    /// key denotes sensitive content. Findings contain only kind/count metadata.
    pub fn redact_json_value(&self, input: &Value) -> (Value, Vec<RedactionFinding>) {
        let mut value = input.clone();
        let mut findings = Vec::new();
        redact_json_node(self, &mut value, &mut findings);
        (value, consolidate(findings))
    }

    /// Detects supported secret/personal-data shapes without returning the
    /// matched material, a fingerprint, or a surrounding excerpt.
    pub fn inspect(&self, input: &str) -> Vec<RedactionFinding> {
        self.redact(input).1
    }
}

fn redact_json_node(redactor: &Redactor, value: &mut Value, findings: &mut Vec<RedactionFinding>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if is_sensitive_field_name(key) {
                    *child = Value::String("<REDACTED_FIELD>".into());
                    findings.push(RedactionFinding {
                        kind: RedactionKind::SensitiveField,
                        count: 1,
                    });
                } else {
                    redact_json_node(redactor, child, findings);
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                redact_json_node(redactor, child, findings);
            }
        }
        Value::String(text) => {
            let (redacted, found) = redactor.redact(text);
            *text = redacted;
            findings.extend(found);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn consolidate(findings: Vec<RedactionFinding>) -> Vec<RedactionFinding> {
    use std::collections::BTreeMap;
    let mut counts = BTreeMap::new();
    for finding in findings {
        *counts.entry(finding.kind).or_insert(0usize) += finding.count;
    }
    counts
        .into_iter()
        .map(|(kind, count)| RedactionFinding { kind, count })
        .collect()
}

pub(crate) fn is_sensitive_field_name(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace(['-', ' '], "_");
    let compact = normalized
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .collect::<Vec<_>>();
    [
        "authorization",
        "api_key",
        "apikey",
        "access_key",
        "secret",
        "password",
        "passwd",
        "token",
        "credential",
        "private_key",
        "client_secret",
        "cookie",
        "set_cookie",
        "prompt",
        "response_text",
        "transcript",
        "audio_bytes",
        "frame_bytes",
        "screenshot",
        "webcam_frame",
    ]
    .iter()
    .any(|sensitive| {
        let compact_sensitive = sensitive
            .bytes()
            .filter(u8::is_ascii_alphanumeric)
            .collect::<Vec<_>>();
        normalized == *sensitive
            || normalized.ends_with(&format!("_{sensitive}"))
            || compact.ends_with(&compact_sensitive)
    })
}

fn patterns() -> &'static Vec<(RedactionKind, Regex, &'static str)> {
    static PATTERNS: OnceLock<Vec<(RedactionKind, Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            (
                RedactionKind::PrivateKey,
                Regex::new(
                    r"(?s)-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----.*?-----END (?:RSA |EC |OPENSSH )?PRIVATE KEY-----",
                )
                .unwrap(),
                "<REDACTED_PRIVATE_KEY>",
            ),
            (
                RedactionKind::BearerToken,
                Regex::new(r"(?i)bearer\s+[a-z0-9._~+/=-]{8,}").unwrap(),
                "Bearer <REDACTED>",
            ),
            (
                RedactionKind::Jwt,
                Regex::new(r"\beyJ[a-zA-Z0-9_-]{8,}\.[a-zA-Z0-9_-]{8,}\.[a-zA-Z0-9_-]{8,}\b")
                    .unwrap(),
                "<REDACTED_JWT>",
            ),
            (
                RedactionKind::ProviderCredential,
                Regex::new(
                    r"\b(?:sk-(?:proj-)?[a-zA-Z0-9_-]{12,}|nvapi-[a-zA-Z0-9_-]{12,}|AIza[a-zA-Z0-9_-]{20,}|gh[pousr]_[a-zA-Z0-9]{20,}|xox[baprs]-[a-zA-Z0-9-]{10,}|AKIA[A-Z0-9]{16})\b",
                )
                .unwrap(),
                "<REDACTED_PROVIDER_CREDENTIAL>",
            ),
            (
                RedactionKind::UrlCredential,
                Regex::new(r"(?i)\b(https?://)[^\s/@:]+:[^\s/@]+@").unwrap(),
                "$1<REDACTED>@",
            ),
            (
                RedactionKind::UrlCredential,
                Regex::new(
                    r"(?i)([?&](?:api[_-]?key|access[_-]?token|token|secret|password)=)[^&#\s]+",
                )
                .unwrap(),
                "$1<REDACTED>",
            ),
            (
                RedactionKind::ApiCredential,
                Regex::new(
                    r#"(?i)(authorization|api[_-]?key|access[_-]?key|client[_-]?secret|secret|password|passwd|token|credential)\s*[:=]\s*[\"']?[^\s,;\"'}<]{4,}[\"']?"#,
                )
                .unwrap(),
                "$1=<REDACTED>",
            ),
            (
                RedactionKind::Email,
                Regex::new(r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b").unwrap(),
                "<REDACTED_EMAIL>",
            ),
            (
                RedactionKind::IpAddress,
                Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap(),
                "<REDACTED_IP>",
            ),
            (
                RedactionKind::IpAddress,
                Regex::new(r"(?i)\b(?:[a-f0-9]{1,4}:){2,7}[a-f0-9]{1,4}\b").unwrap(),
                "<REDACTED_IP>",
            ),
            (
                RedactionKind::UserPath,
                Regex::new(
                    r"(?i)(?:\b[A-Z]:\\Users\\[^\\\s]+|/(?:home|Users)/[^/\s]+|\\\\[^\\\s]+\\Users\\[^\\\s]+)",
                )
                .unwrap(),
                "<REDACTED_USER_PATH>",
            ),
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_seeded_secret_and_personal_markers() {
        let input = "Authorization: Bearer canary-secret-123 api_key=sk-canary12345 user@example.com C:\\Users\\Akshit\\models 192.168.1.2";
        let (redacted, findings) = Redactor.redact(input);
        assert!(!redacted.contains("canary-secret"));
        assert!(!redacted.contains("sk-canary"));
        assert!(!redacted.contains("user@example.com"));
        assert!(!redacted.contains("Akshit"));
        assert!(!redacted.contains("192.168.1.2"));
        assert!(findings.iter().map(|item| item.count).sum::<usize>() >= 5);
    }

    #[test]
    fn leaves_normal_actionable_error_intact() {
        let input = "model verification failed: expected 32 bytes, received 16";
        let (redacted, findings) = Redactor.redact(input);
        assert_eq!(redacted, input);
        assert!(findings.is_empty());
    }

    #[test]
    fn adversarial_provider_tokens_jwts_urls_and_private_keys_are_removed() {
        let openai = ["sk", "-proj-", "abcdefghijklmnopqrst"].concat();
        let nvidia = ["nv", "api-", "abcdefghijklmnopqrst"].concat();
        let github = ["gh", "p_", "abcdefghijklmnopqrstuvwxyz123456"].concat();
        let private_key = format!(
            "-----BEGIN {}-----\ncanary-private-material\n-----END {}-----",
            "PRIVATE KEY", "PRIVATE KEY"
        );
        let input = format!(
            "{openai} {nvidia} {github} eyJabcdefghijk.abcdefghijkl.abcdefghijkl \
             https://alice:hunter2@example.test/path?api_key=abcdef123456&safe=yes {private_key}"
        );
        let (redacted, findings) = Redactor.redact(&input);
        for canary in [
            "sk-proj-",
            "nvapi-",
            "ghp_",
            "eyJabcdefghijk",
            "alice:hunter2",
            "abcdef123456",
            "canary-private-material",
        ] {
            assert!(!redacted.contains(canary), "leaked {canary}: {redacted}");
        }
        assert!(findings.len() >= 4);
    }

    #[test]
    fn recursively_redacts_sensitive_json_fields_even_when_values_look_innocent() {
        let input = serde_json::json!({
            "provider": {
                "apiKey": "short",
                "nested": [{"client_secret": "canary"}],
                "endpoint": "https://example.test"
            },
            "transcript": ["private words"],
            "status": "failed for user@example.com"
        });
        let (redacted, findings) = Redactor.redact_json_value(&input);
        let serialized = serde_json::to_string(&redacted).unwrap();
        assert!(!serialized.contains("short"));
        assert!(!serialized.contains("canary"));
        assert!(!serialized.contains("private words"));
        assert!(!serialized.contains("user@example.com"));
        assert!(serialized.contains("https://example.test"));
        assert!(findings.iter().map(|item| item.count).sum::<usize>() >= 4);
    }

    #[test]
    fn redaction_is_idempotent_and_never_records_matched_material() {
        let input = "password=canary123456 and admin@example.com";
        let (once, first_findings) = Redactor.redact(input);
        let (twice, second_findings) = Redactor.redact(&once);
        assert_eq!(once, twice);
        assert!(!format!("{first_findings:?}{second_findings:?}").contains("canary"));
    }
}
