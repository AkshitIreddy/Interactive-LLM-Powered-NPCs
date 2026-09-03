use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreKind {
    Steam,
    Epic,
    Gog,
    Standalone,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionSource {
    Registry,
    LauncherManifest,
    StoreManifest,
    CommonDirectory,
    VerifiedExecutable,
    ManualSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
    Verified,
}

impl Confidence {
    pub fn score(self) -> u8 {
        match self {
            Self::Low => 20,
            Self::Medium => 50,
            Self::High => 80,
            Self::Verified => 100,
        }
    }
}

impl Ord for Confidence {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score().cmp(&other.score())
    }
}

impl PartialOrd for Confidence {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationEvidence {
    pub source: DetectionSource,
    pub confidence: Confidence,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditionEvidence {
    pub edition_id: Option<String>,
    pub store_id: Option<String>,
    pub build_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationCandidate {
    pub store: StoreKind,
    pub display_name: String,
    pub install_dir: PathBuf,
    pub executable: Option<PathBuf>,
    pub edition: EditionEvidence,
    pub evidence: Vec<InstallationEvidence>,
    pub warnings: BTreeSet<String>,
}

impl InstallationCandidate {
    pub fn confidence(&self) -> Confidence {
        self.evidence
            .iter()
            .map(|item| item.confidence)
            .max()
            .unwrap_or(Confidence::Low)
    }

    pub fn has_verified_executable(&self) -> bool {
        self.executable.is_some()
            && self.evidence.iter().any(|item| {
                item.source == DetectionSource::VerifiedExecutable
                    && item.confidence == Confidence::Verified
            })
    }

    /// Return the canonical executable only when it is backed by verified
    /// containment evidence. Consumers may safely use the leaf for profile
    /// matching, but discovery does not grant authority to launch it.
    pub fn verified_executable(&self) -> Option<&std::path::Path> {
        self.has_verified_executable()
            .then_some(self.executable.as_deref())
            .flatten()
    }

    /// The install root paired with `verified_executable`. Scanners store its
    /// canonical form whenever executable verification succeeds.
    pub fn verified_install_root(&self) -> Option<&std::path::Path> {
        self.has_verified_executable()
            .then_some(self.install_dir.as_path())
    }

    pub fn verified_executable_leaf(&self) -> Option<&OsStr> {
        self.verified_executable().and_then(|path| path.file_name())
    }

    pub fn stable_key(&self) -> String {
        let store_id = self.edition.store_id.as_deref().unwrap_or("unknown");
        let path = self.install_dir.to_string_lossy().to_ascii_lowercase();
        format!("{:?}:{store_id}:{path}", self.store)
    }

    pub fn merge(&mut self, other: Self) {
        const EXECUTABLE_CONFLICT: &str =
            "conflicting verified executable evidence; select the game window explicitly";
        let self_verified = self.has_verified_executable();
        let other_verified = other.has_verified_executable();
        let conflicting_verified_executables = self.warnings.contains(EXECUTABLE_CONFLICT)
            || other.warnings.contains(EXECUTABLE_CONFLICT)
            || (self_verified
                && other_verified
                && self
                    .executable
                    .as_ref()
                    .map(|path| normalized_path_key(path))
                    != other
                        .executable
                        .as_ref()
                        .map(|path| normalized_path_key(path)));

        if conflicting_verified_executables {
            // Never let evidence for one file authorize another file. Store
            // databases can be stale while an edition is moved or updated.
            self.executable = None;
            self.evidence
                .retain(|item| item.source != DetectionSource::VerifiedExecutable);
            self.warnings.insert(EXECUTABLE_CONFLICT.into());
        } else if other_verified && !self_verified {
            self.executable = other.executable.clone();
            self.install_dir = other.install_dir.clone();
            self.display_name = other.display_name.clone();
        } else if self.executable.is_none() && !other_verified {
            self.executable = other.executable.clone();
        }
        if self.edition.edition_id.is_none() {
            self.edition.edition_id = other.edition.edition_id;
        }
        if self.edition.store_id.is_none() {
            self.edition.store_id = other.edition.store_id;
        }
        if self.edition.build_id.is_none() {
            self.edition.build_id = other.edition.build_id;
        }
        for evidence in other.evidence {
            if conflicting_verified_executables
                && evidence.source == DetectionSource::VerifiedExecutable
            {
                continue;
            }
            if !self.evidence.contains(&evidence) {
                self.evidence.push(evidence);
            }
        }
        self.warnings.extend(other.warnings);
        self.evidence.sort_by_key(|item| item.source);
    }
}

fn normalized_path_key(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(executable: Option<&str>, verified: bool) -> InstallationCandidate {
        InstallationCandidate {
            store: StoreKind::Epic,
            display_name: "Example".into(),
            install_dir: PathBuf::from(r"C:\Games\Example"),
            executable: executable.map(PathBuf::from),
            edition: EditionEvidence {
                edition_id: Some("example".into()),
                store_id: Some("42".into()),
                build_id: None,
            },
            evidence: vec![InstallationEvidence {
                source: if verified {
                    DetectionSource::VerifiedExecutable
                } else {
                    DetectionSource::StoreManifest
                },
                confidence: if verified {
                    Confidence::Verified
                } else {
                    Confidence::High
                },
                detail: "fixture".into(),
            }],
            warnings: BTreeSet::new(),
        }
    }

    #[test]
    fn merge_moves_the_executable_with_its_verified_evidence() {
        let mut store_only = candidate(None, false);
        store_only.merge(candidate(Some(r"C:\Games\Example\game.exe"), true));
        assert!(store_only.has_verified_executable());
        let expected = PathBuf::from(r"C:\Games\Example\game.exe");
        assert_eq!(store_only.verified_executable(), Some(expected.as_path()));
    }

    #[test]
    fn merge_fails_closed_on_conflicting_verified_executables() {
        let mut first = candidate(Some(r"C:\Games\Example\game.exe"), true);
        first.merge(candidate(Some(r"C:\Games\Example\stale-game.exe"), true));
        assert!(!first.has_verified_executable());
        assert!(first.verified_executable().is_none());
        assert!(first.warnings.contains(
            "conflicting verified executable evidence; select the game window explicitly"
        ));

        first.merge(candidate(Some(r"C:\Games\Example\game.exe"), true));
        assert!(!first.has_verified_executable());
        assert!(first.verified_executable().is_none());
    }
}
