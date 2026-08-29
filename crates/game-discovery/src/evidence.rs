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
        self.evidence.iter().any(|item| {
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
        if self.executable.is_none() {
            self.executable = other.executable;
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
            if !self.evidence.contains(&evidence) {
                self.evidence.push(evidence);
            }
        }
        self.warnings.extend(other.warnings);
        self.evidence.sort_by_key(|item| item.source);
    }
}
