use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionKind {
    BearerToken,
    ApiCredential,
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
}

fn patterns() -> &'static Vec<(RedactionKind, Regex, &'static str)> {
    static PATTERNS: OnceLock<Vec<(RedactionKind, Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            (
                RedactionKind::BearerToken,
                Regex::new(r"(?i)bearer\s+[a-z0-9._~+/=-]{8,}").unwrap(),
                "Bearer <REDACTED>",
            ),
            (
                RedactionKind::ApiCredential,
                Regex::new(r"(?i)(api[_-]?key|secret|password|token)\s*[:=]\s*[^\s,;]{6,}")
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
                RedactionKind::UserPath,
                Regex::new(r"(?i)\b[A-Z]:\\Users\\[^\\\s]+|/(?:home|Users)/[^/\s]+").unwrap(),
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
        assert_eq!(findings.iter().map(|item| item.count).sum::<usize>(), 5);
    }

    #[test]
    fn leaves_normal_actionable_error_intact() {
        let input = "model verification failed: expected 32 bytes, received 16";
        let (redacted, findings) = Redactor.redact(input);
        assert_eq!(redacted, input);
        assert!(findings.is_empty());
    }
}
