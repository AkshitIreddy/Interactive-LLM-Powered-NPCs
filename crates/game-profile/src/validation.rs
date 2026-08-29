use crate::{
    BuildStatus, CapabilityEvidenceKind, CapabilityTier, CaptureMethod, GameProfileV2,
    IdentityEvidence, IdentityStrategy, ProvenanceKind, GAME_PROFILE_V2_SCHEMA,
    GAME_PROFILE_V2_VERSION,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use thiserror::Error;

pub const MAX_PROFILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOTAL_STRING_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    Schema,
    UnsupportedVersion,
    DuplicateId,
    BrokenReference,
    UnsafeRegex,
    UnsafePath,
    UnsafeUrl,
    ForbiddenExecutableField,
    InconsistentCapability,
    MissingRationale,
    ExcessiveContent,
    InvalidRegion,
    MissingPrimaryProcess,
    MissingCapabilityEvidence,
    InvalidCapabilityEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub code: IssueCode,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    fn error(&mut self, code: IssueCode, path: impl Into<String>, message: impl Into<String>) {
        self.errors.push(ValidationIssue {
            code,
            path: path.into(),
            message: message.into(),
        });
    }

    fn warning(&mut self, code: IssueCode, path: impl Into<String>, message: impl Into<String>) {
        self.warnings.push(ValidationIssue {
            code,
            path: path.into(),
            message: message.into(),
        });
    }
}

#[derive(Debug, Error)]
pub enum ProfileLoadError {
    #[error("profile is {actual} bytes; maximum is {maximum}")]
    TooLarge { actual: usize, maximum: usize },
    #[error("profile is not valid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("profile failed validation")]
    Validation(ValidationReport),
}

/// Validate untyped input against the bundled JSON Schema.
pub fn validate_json_schema(value: &Value) -> ValidationReport {
    let mut report = ValidationReport::default();
    let schema: Value = match serde_json::from_str(GAME_PROFILE_V2_SCHEMA) {
        Ok(schema) => schema,
        Err(error) => {
            report.error(
                IssueCode::Schema,
                "$schema",
                format!("bundled schema is invalid: {error}"),
            );
            return report;
        }
    };

    let validator = match jsonschema::validator_for(&schema) {
        Ok(validator) => validator,
        Err(error) => {
            report.error(
                IssueCode::Schema,
                "$schema",
                format!("bundled schema cannot compile: {error}"),
            );
            return report;
        }
    };
    for error in validator.iter_errors(value) {
        report.error(
            IssueCode::Schema,
            error.instance_path.to_string(),
            error.to_string(),
        );
    }
    report
}

/// Parse and validate a profile as untrusted input.
pub fn load_profile(input: &[u8]) -> Result<GameProfileV2, ProfileLoadError> {
    if input.len() > MAX_PROFILE_BYTES {
        return Err(ProfileLoadError::TooLarge {
            actual: input.len(),
            maximum: MAX_PROFILE_BYTES,
        });
    }
    let value: Value = serde_json::from_slice(input)?;
    let mut report = validate_json_schema(&value);
    validate_untyped_security(&value, &mut report);
    if !report.is_valid() {
        return Err(ProfileLoadError::Validation(report));
    }
    let profile: GameProfileV2 = serde_json::from_value(value)?;
    report = profile.validate();
    if !report.is_valid() {
        return Err(ProfileLoadError::Validation(report));
    }
    Ok(profile)
}

impl GameProfileV2 {
    /// Cross-field, referential, and security validation not expressible in JSON Schema.
    pub fn validate(&self) -> ValidationReport {
        let mut report = ValidationReport::default();
        if self.schema_version != GAME_PROFILE_V2_VERSION {
            report.error(
                IssueCode::UnsupportedVersion,
                "/schema_version",
                format!(
                    "expected {GAME_PROFILE_V2_VERSION}, got {}",
                    self.schema_version
                ),
            );
        }

        unique_ids(
            self.characters.iter().map(|v| v.id.as_str()),
            "/characters",
            &mut report,
        );
        unique_ids(
            self.content.provenance.iter().map(|v| v.id.as_str()),
            "/content/provenance",
            &mut report,
        );
        unique_ids(
            self.content.spoiler_tiers.iter().map(|v| v.id.as_str()),
            "/content/spoiler_tiers",
            &mut report,
        );
        unique_ids(
            self.diagnostics.iter().map(|v| v.id.as_str()),
            "/diagnostics",
            &mut report,
        );
        unique_ids(
            self.detection.builds.iter().map(|v| v.build_id.as_str()),
            "/detection/builds",
            &mut report,
        );
        unique_ids(
            self.detection
                .capture
                .ui_regions
                .iter()
                .map(|v| v.id.as_str()),
            "/detection/capture/ui_regions",
            &mut report,
        );

        let characters: HashSet<_> = self.characters.iter().map(|v| v.id.as_str()).collect();
        if !characters.contains(self.defaults.character_id.as_str()) {
            report.error(
                IssueCode::BrokenReference,
                "/defaults/character_id",
                "default character does not exist",
            );
        }
        let provenance: HashSet<_> = self
            .content
            .provenance
            .iter()
            .map(|v| v.id.as_str())
            .collect();
        let spoiler_tiers: HashSet<_> = self
            .content
            .spoiler_tiers
            .iter()
            .map(|v| v.id.as_str())
            .collect();
        for (index, character) in self.characters.iter().enumerate() {
            for reference in &character.prompt.knowledge_refs {
                if !provenance.contains(reference.as_str())
                    && !spoiler_tiers.contains(reference.as_str())
                {
                    report.error(
                        IssueCode::BrokenReference,
                        format!("/characters/{index}/prompt/knowledge_refs"),
                        format!("unknown provenance or spoiler-tier reference `{reference}`"),
                    );
                }
            }
            if character.identity.strategy != IdentityStrategy::ExplicitSelection
                || character.identity.evidence != [IdentityEvidence::ExplicitSelection]
            {
                report.error(
                    IssueCode::InconsistentCapability,
                    format!("/characters/{index}/identity"),
                    "generic profiles require explicit character selection as their sole identity evidence",
                );
            }
        }

        for (index, process) in self.detection.processes.iter().enumerate() {
            validate_executable_leaf(
                &process.executable,
                format!("/detection/processes/{index}/executable"),
                &mut report,
            );
            if let Some(pattern) = &process.window_title_regex {
                validate_safe_regex(
                    pattern,
                    format!("/detection/processes/{index}/window_title_regex"),
                    &mut report,
                );
            }
        }
        if !self
            .detection
            .processes
            .iter()
            .any(|process| process.required)
        {
            report.error(
                IssueCode::MissingPrimaryProcess,
                "/detection/processes",
                "at least one required primary process is needed; required entries are OR alternatives",
            );
        }
        for (index, pattern) in self
            .detection
            .capture
            .excluded_window_title_regexes
            .iter()
            .enumerate()
        {
            validate_safe_regex(
                pattern,
                format!("/detection/capture/excluded_window_title_regexes/{index}"),
                &mut report,
            );
        }
        for (index, region) in self.detection.capture.ui_regions.iter().enumerate() {
            let rect = region.rect;
            let finite = [rect.x, rect.y, rect.width, rect.height]
                .into_iter()
                .all(f64::is_finite);
            if !finite
                || rect.x < 0.0
                || rect.y < 0.0
                || rect.width <= 0.0
                || rect.height <= 0.0
                || rect.x + rect.width > 1.0
                || rect.y + rect.height > 1.0
            {
                report.error(
                    IssueCode::InvalidRegion,
                    format!("/detection/capture/ui_regions/{index}/rect"),
                    "normalized rectangle must be finite, positive, and entirely within the 0..1 frame",
                );
            }
        }
        for (store_index, store) in self.detection.stores.iter().enumerate() {
            for (hint_index, hint) in store.install_directory_hints.iter().enumerate() {
                validate_relative_leaf(
                    hint,
                    format!("/detection/stores/{store_index}/install_directory_hints/{hint_index}"),
                    &mut report,
                );
            }
        }
        if let Some(url) = &self.game.official_url {
            validate_https_url(url, "/game/official_url", &mut report);
        }
        for (index, item) in self.content.provenance.iter().enumerate() {
            if let Some(url) = &item.source_url {
                validate_https_url(
                    url,
                    format!("/content/provenance/{index}/source_url"),
                    &mut report,
                );
            }
            if matches!(
                item.kind,
                ProvenanceKind::Official | ProvenanceKind::Licensed
            ) && item.source_url.is_none()
            {
                report.error(
                    IssueCode::BrokenReference,
                    format!("/content/provenance/{index}/source_url"),
                    "non-original content requires an HTTPS source URL",
                );
            }
        }
        for (index, build) in self.detection.builds.iter().enumerate() {
            if build.status == BuildStatus::Blocked
                && build
                    .reason
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty()
            {
                report.error(
                    IssueCode::MissingRationale,
                    format!("/detection/builds/{index}/reason"),
                    "a blocked build requires a reason",
                );
            }
        }
        if !self
            .detection
            .capture
            .preferred_methods
            .contains(&CaptureMethod::AudioSubtitles)
        {
            report.error(
                IssueCode::InconsistentCapability,
                "/detection/capture/preferred_methods",
                "generic profiles must retain audio and subtitles as a capture-independent path",
            );
        }
        for (name, claim) in self.capabilities.iter() {
            let path = format!("/capabilities/{name}");
            if claim.fallback.trim().is_empty() {
                report.error(
                    IssueCode::MissingRationale,
                    format!("{path}/fallback"),
                    "every capability needs an explicit fallback",
                );
            }
            if claim.evidence.is_empty() {
                report.error(
                    IssueCode::MissingCapabilityEvidence,
                    format!("{path}/evidence"),
                    "every capability claim needs honest evidence metadata",
                );
            }
            let has_replay = claim
                .evidence
                .iter()
                .any(|item| item.kind == CapabilityEvidenceKind::DeterministicReplay);
            if claim.tier == CapabilityTier::ReplayVerified && !has_replay {
                report.error(
                    IssueCode::InvalidCapabilityEvidence,
                    format!("{path}/evidence"),
                    "replay_verified requires deterministic_replay evidence",
                );
            }
            if claim.tier == CapabilityTier::LiveCertified {
                report.error(
                    IssueCode::InvalidCapabilityEvidence,
                    format!("{path}/tier"),
                    "live_certified is reserved until external live-game evidence is attached",
                );
            }
            for (index, evidence) in claim.evidence.iter().enumerate() {
                if evidence.reference.trim().is_empty() || evidence.summary.trim().is_empty() {
                    report.error(
                        IssueCode::MissingCapabilityEvidence,
                        format!("{path}/evidence/{index}"),
                        "capability evidence needs a non-empty reference and summary",
                    );
                }
            }
        }
        if self.capabilities.screen_space_lip_sync.tier != CapabilityTier::Experimental
            || self
                .capabilities
                .screen_space_lip_sync
                .evidence
                .iter()
                .any(|evidence| evidence.kind != CapabilityEvidenceKind::NotVerified)
        {
            report.error(
                IssueCode::InconsistentCapability,
                "/capabilities/screen_space_lip_sync",
                "generic screen-space lip sync must remain experimental with only not_verified evidence",
            );
        }
        if self.capabilities.conversation.tier == CapabilityTier::Unsupported {
            report.warning(
                IssueCode::InconsistentCapability,
                "/capabilities/conversation",
                "a profile without conversation support cannot provide the core experience",
            );
        }
        report
    }
}

fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>, path: &str, report: &mut ValidationReport) {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            report.error(IssueCode::DuplicateId, path, format!("duplicate id `{id}`"));
        }
    }
}

fn validate_safe_regex(pattern: &str, path: String, report: &mut ValidationReport) {
    let lower = pattern.to_ascii_lowercase();
    let unsupported_group = lower.replace("(?i)", "").replace("(?:", "(").contains("(?");
    let forbidden = pattern.len() > 256
        || unsupported_group
        || lower.contains("\\k<")
        || (1..=9).any(|digit| pattern.contains(&format!("\\{digit}")))
        || has_nested_quantifier(pattern);
    if forbidden {
        report.error(
            IssueCode::UnsafeRegex,
            path,
            "regex uses a forbidden high-complexity construct",
        );
        return;
    }
    if let Err(error) = Regex::new(pattern) {
        report.error(
            IssueCode::UnsafeRegex,
            path,
            format!("invalid regex: {error}"),
        );
    }
}

fn has_nested_quantifier(pattern: &str) -> bool {
    // Conservatively reject `(x+)+`, `(x*){n}`, and close variants. Profiles do
    // not need advanced regex features; conservative false positives favor safety.
    pattern.contains("+)+")
        || pattern.contains("*)+")
        || pattern.contains("+)*")
        || pattern.contains("*)*")
        || pattern.contains("+){")
        || pattern.contains("*){")
}

fn validate_executable_leaf(value: &str, path: String, report: &mut ValidationReport) {
    if is_unsafe_path(value) || !value.to_ascii_lowercase().ends_with(".exe") {
        report.error(
            IssueCode::UnsafePath,
            path,
            "executable must be a leaf .exe filename, never a path",
        );
    }
}

fn validate_relative_leaf(value: &str, path: String, report: &mut ValidationReport) {
    let safe_segments = value.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && !segment.starts_with('.')
            && !segment.contains('\0')
    });
    if !safe_segments
        || value.starts_with('/')
        || value.contains('\\')
        || value.contains(':')
        || value.len() > 512
    {
        report.error(
            IssueCode::UnsafePath,
            path,
            "install hint must be a safe relative path without traversal, drive, UNC, or device syntax",
        );
    }
}

fn is_unsafe_path(value: &str) -> bool {
    value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains(':')
        || value.starts_with('.')
        || value.contains('\0')
}

fn validate_https_url(value: &str, path: impl Into<String>, report: &mut ValidationReport) {
    let lower = value.to_ascii_lowercase();
    if !lower.starts_with("https://")
        || lower.contains('@')
        || lower.contains("javascript:")
        || lower.contains("data:")
    {
        report.error(
            IssueCode::UnsafeUrl,
            path,
            "only credential-free HTTPS URLs are allowed",
        );
    }
}

fn validate_untyped_security(value: &Value, report: &mut ValidationReport) {
    const FORBIDDEN_KEYS: &[&str] = &[
        "command",
        "commands",
        "script",
        "script_path",
        "shell",
        "powershell",
        "python",
        "code",
        "eval",
        "exec",
        "arguments",
        "dll",
        "inject",
        "hook_address",
        "download_url",
        "environment",
        "working_directory",
        "registry_write",
        "network_request",
    ];
    fn walk(value: &Value, path: &str, total: &mut usize, report: &mut ValidationReport) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    let child_path = format!("{path}/{key}");
                    if FORBIDDEN_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
                        report.error(
                            IssueCode::ForbiddenExecutableField,
                            &child_path,
                            "profiles are declarative data and may not contain executable fields",
                        );
                    }
                    walk(child, &child_path, total, report);
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    walk(child, &format!("{path}/{index}"), total, report);
                }
            }
            Value::String(text) => *total = total.saturating_add(text.len()),
            _ => {}
        }
    }
    let mut total = 0;
    walk(value, "", &mut total, report);
    if total > MAX_TOTAL_STRING_BYTES {
        report.error(
            IssueCode::ExcessiveContent,
            "",
            format!("string content totals {total} bytes; maximum is {MAX_TOTAL_STRING_BYTES}"),
        );
    }
}
