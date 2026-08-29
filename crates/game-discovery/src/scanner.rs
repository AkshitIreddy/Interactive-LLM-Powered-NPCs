use crate::InstallationCandidate;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("{scanner} scanner failed: {message}")]
    Scanner { scanner: String, message: String },
}

pub trait StoreScanner: Send + Sync {
    fn id(&self) -> &'static str;
    fn scan(&self) -> Result<Vec<InstallationCandidate>, DiscoveryError>;
}

#[derive(Default)]
pub struct DiscoveryService {
    scanners: Vec<Box<dyn StoreScanner>>,
}

impl DiscoveryService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_scanner(mut self, scanner: impl StoreScanner + 'static) -> Self {
        self.scanners.push(Box::new(scanner));
        self
    }

    pub fn discover(&self) -> (Vec<InstallationCandidate>, Vec<DiscoveryError>) {
        let mut merged = BTreeMap::<String, InstallationCandidate>::new();
        let mut errors = Vec::new();

        for scanner in &self.scanners {
            match scanner.scan() {
                Ok(candidates) => {
                    for candidate in candidates {
                        let key = candidate.stable_key();
                        if let Some(existing) = merged.get_mut(&key) {
                            existing.merge(candidate);
                        } else {
                            merged.insert(key, candidate);
                        }
                    }
                }
                Err(error) => errors.push(error),
            }
        }

        let mut candidates: Vec<_> = merged.into_values().collect();
        candidates.sort_by(|left, right| {
            right
                .confidence()
                .cmp(&left.confidence())
                .then_with(|| left.display_name.cmp(&right.display_name))
        });
        (candidates, errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Confidence, DetectionSource, EditionEvidence, InstallationEvidence, StoreKind};
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    struct FixedScanner(Vec<InstallationCandidate>);

    impl StoreScanner for FixedScanner {
        fn id(&self) -> &'static str {
            "fixed"
        }

        fn scan(&self) -> Result<Vec<InstallationCandidate>, DiscoveryError> {
            Ok(self.0.clone())
        }
    }

    fn candidate(source: DetectionSource, confidence: Confidence) -> InstallationCandidate {
        InstallationCandidate {
            store: StoreKind::Steam,
            display_name: "Example".into(),
            install_dir: PathBuf::from("D:/Steam/Example"),
            executable: None,
            edition: EditionEvidence {
                edition_id: None,
                store_id: Some("42".into()),
                build_id: None,
            },
            evidence: vec![InstallationEvidence {
                source,
                confidence,
                detail: "fixture".into(),
            }],
            warnings: BTreeSet::new(),
        }
    }

    #[test]
    fn merges_duplicate_store_evidence() {
        let service = DiscoveryService::new()
            .with_scanner(FixedScanner(vec![candidate(
                DetectionSource::StoreManifest,
                Confidence::High,
            )]))
            .with_scanner(FixedScanner(vec![candidate(
                DetectionSource::VerifiedExecutable,
                Confidence::Verified,
            )]));
        let (items, errors) = service.discover();
        assert!(errors.is_empty());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].evidence.len(), 2);
        assert_eq!(items[0].confidence(), Confidence::Verified);
    }
}
