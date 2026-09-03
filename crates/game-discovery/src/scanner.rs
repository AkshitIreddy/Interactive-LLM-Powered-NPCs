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
        reconcile_moved_or_duplicate_store_installs(&mut candidates);
        candidates.sort_by(|left, right| {
            right
                .confidence()
                .cmp(&left.confidence())
                .then_with(|| left.display_name.cmp(&right.display_name))
        });
        (candidates, errors)
    }
}

fn reconcile_moved_or_duplicate_store_installs(candidates: &mut Vec<InstallationCandidate>) {
    let mut identities = BTreeMap::<String, Vec<usize>>::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let Some(store_id) = candidate.edition.store_id.as_deref() else {
            continue;
        };
        identities
            .entry(format!(
                "{:?}:{}",
                candidate.store,
                store_id.to_ascii_lowercase()
            ))
            .or_default()
            .push(index);
    }
    let mut removed = std::collections::BTreeSet::new();
    for indexes in identities.values().filter(|indexes| indexes.len() > 1) {
        let verified = indexes
            .iter()
            .copied()
            .filter(|index| candidates[*index].has_verified_executable())
            .collect::<Vec<_>>();
        if verified.len() == 1 {
            let destination = verified[0];
            for source in indexes
                .iter()
                .copied()
                .filter(|index| *index != destination)
            {
                if candidates[source].has_verified_executable() {
                    continue;
                }
                let stale = candidates[source].clone();
                candidates[destination].merge(stale);
                candidates[destination].warnings.insert(
                    "stale or moved store location was ignored in favor of one verified executable"
                        .into(),
                );
                removed.insert(source);
            }
        } else if verified.len() > 1 {
            for index in indexes {
                candidates[*index].warnings.insert(
                    "multiple verified installations share one store identity; select one explicitly"
                        .into(),
                );
            }
        } else {
            for index in indexes {
                candidates[*index].warnings.insert(
                    "multiple unverified store locations share one store identity; rescan or select the executable manually"
                        .into(),
                );
            }
        }
    }
    let mut index = 0usize;
    candidates.retain(|_| {
        let keep = !removed.contains(&index);
        index += 1;
        keep
    });
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

    fn candidate_at(
        path: &str,
        source: DetectionSource,
        confidence: Confidence,
    ) -> InstallationCandidate {
        let mut candidate = candidate(source, confidence);
        candidate.install_dir = PathBuf::from(path);
        if source == DetectionSource::VerifiedExecutable {
            candidate.executable = Some(candidate.install_dir.join("game.exe"));
        }
        candidate
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

    #[test]
    fn moved_store_install_prefers_the_only_verified_location() {
        let service = DiscoveryService::new()
            .with_scanner(FixedScanner(vec![candidate_at(
                "D:/OldLibrary/Example",
                DetectionSource::StoreManifest,
                Confidence::High,
            )]))
            .with_scanner(FixedScanner(vec![candidate_at(
                "E:/CurrentLibrary/Example",
                DetectionSource::VerifiedExecutable,
                Confidence::Verified,
            )]));
        let (items, errors) = service.discover();
        assert!(errors.is_empty());
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].install_dir,
            PathBuf::from("E:/CurrentLibrary/Example")
        );
        assert!(items[0].has_verified_executable());
        assert!(items[0].warnings.contains(
            "stale or moved store location was ignored in favor of one verified executable"
        ));
    }

    #[test]
    fn duplicate_verified_installs_require_explicit_selection() {
        let service = DiscoveryService::new()
            .with_scanner(FixedScanner(vec![candidate_at(
                "D:/Library/Example",
                DetectionSource::VerifiedExecutable,
                Confidence::Verified,
            )]))
            .with_scanner(FixedScanner(vec![candidate_at(
                "E:/Library/Example",
                DetectionSource::VerifiedExecutable,
                Confidence::Verified,
            )]));
        let (items, errors) = service.discover();
        assert!(errors.is_empty());
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item.warnings.contains(
            "multiple verified installations share one store identity; select one explicitly"
        )));
    }
}
