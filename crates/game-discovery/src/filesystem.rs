use crate::path_security::{
    canonical_existing_directory, inspect_store_executable, is_link_or_reparse,
    ExecutableInspection,
};
use crate::{
    parse_app_manifest, parse_epic_item, parse_gog_info, parse_launcher_installed,
    parse_library_folders, Confidence, DetectionSource, DiscoveryError, EditionEvidence,
    EpicInstall, InstallationCandidate, InstallationEvidence, StoreKind, StoreScanner,
};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonDirectoryRule {
    pub display_name: String,
    pub relative_install_dir: PathBuf,
    pub relative_executable: PathBuf,
    pub store_id: Option<String>,
}

/// Bounded app-authored probes for conventional install roots such as Program
/// Files. This never recursively scans a drive: every relative directory and
/// executable comes from a validated built-in profile rule, and a hit receives
/// verified authority only after canonical containment and regular-file checks.
#[derive(Debug, Clone, Default)]
pub struct CommonDirectoryScanner {
    pub roots: Vec<PathBuf>,
    pub rules: Vec<CommonDirectoryRule>,
}

impl StoreScanner for CommonDirectoryScanner {
    fn id(&self) -> &'static str {
        "common-directory"
    }

    fn scan(&self) -> Result<Vec<InstallationCandidate>, DiscoveryError> {
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        for declared_root in &self.roots {
            let Some(root) = canonical_existing_directory(declared_root) else {
                continue;
            };
            for rule in &self.rules {
                if rule.display_name.trim().is_empty()
                    || !safe_relative_path(&rule.relative_install_dir)
                    || !safe_relative_path(&rule.relative_executable)
                {
                    return Err(scanner_error(
                        self.id(),
                        "an app-authored common-directory rule is invalid",
                    ));
                }
                let install = root.join(&rule.relative_install_dir);
                let ExecutableInspection::Verified {
                    canonical_root,
                    canonical_executable,
                } = inspect_store_executable(&install, &rule.relative_executable)
                else {
                    continue;
                };
                let key = canonical_executable.to_string_lossy().to_ascii_lowercase();
                if !seen.insert(key) {
                    continue;
                }
                candidates.push(InstallationCandidate {
                    store: StoreKind::Standalone,
                    display_name: rule.display_name.clone(),
                    install_dir: canonical_root,
                    executable: Some(canonical_executable),
                    edition: EditionEvidence {
                        edition_id: None,
                        store_id: rule.store_id.clone(),
                        build_id: None,
                    },
                    evidence: vec![
                        InstallationEvidence {
                            source: DetectionSource::CommonDirectory,
                            confidence: Confidence::Medium,
                            detail: "bounded built-in common-directory rule".into(),
                        },
                        InstallationEvidence {
                            source: DetectionSource::VerifiedExecutable,
                            confidence: Confidence::Verified,
                            detail: "canonical contained executable exists".into(),
                        },
                    ],
                    warnings: BTreeSet::new(),
                });
            }
        }
        Ok(candidates)
    }
}

fn safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

#[derive(Debug, Clone, Default)]
pub struct SteamFilesystemScanner {
    pub roots: Vec<PathBuf>,
}

impl StoreScanner for SteamFilesystemScanner {
    fn id(&self) -> &'static str {
        "steam"
    }

    fn scan(&self) -> Result<Vec<InstallationCandidate>, DiscoveryError> {
        let mut libraries = Vec::new();
        for root in &self.roots {
            let Some(canonical_root) = canonical_existing_directory(root) else {
                continue;
            };
            libraries.push(canonical_root.clone());
            let layout = canonical_root.join("steamapps").join("libraryfolders.vdf");
            if let Ok(text) = read_text_bounded(&layout, 8 * 1024 * 1024) {
                let parsed = parse_library_folders(&text)
                    .map_err(|error| scanner_error(self.id(), error))?;
                libraries.extend(parsed.into_iter().map(|library| library.path));
            }
        }
        libraries.sort();
        libraries.dedup();

        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        for declared_library in libraries {
            let Some(library) = canonical_existing_directory(&declared_library) else {
                continue;
            };
            let steamapps = library.join("steamapps");
            let Ok(entries) = fs::read_dir(&steamapps) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
                    continue;
                }
                let text = read_text_bounded(&entry.path(), 4 * 1024 * 1024)
                    .map_err(|error| scanner_error(self.id(), error))?;
                let app =
                    parse_app_manifest(&text).map_err(|error| scanner_error(self.id(), error))?;
                let declared_install_dir = steamapps.join("common").join(&app.install_dir_name);
                let canonical_install_dir = canonical_existing_directory(&declared_install_dir);
                let install_dir = canonical_install_dir
                    .clone()
                    .unwrap_or(declared_install_dir);
                let key = format!(
                    "{}:{}",
                    app.app_id,
                    install_dir.to_string_lossy().to_ascii_lowercase()
                );
                if !seen.insert(key) {
                    continue;
                }
                let mut warnings = BTreeSet::new();
                if canonical_install_dir.is_none() {
                    warnings.insert(
                        "store manifest exists but install directory is unavailable".into(),
                    );
                }
                if app.state_flags.is_some_and(|flags| flags & 4 == 0) {
                    warnings.insert(
                        "Steam reports an incomplete or non-installed application state".into(),
                    );
                }
                candidates.push(InstallationCandidate {
                    store: StoreKind::Steam,
                    display_name: app.name,
                    install_dir,
                    executable: None,
                    edition: EditionEvidence {
                        edition_id: None,
                        store_id: Some(app.app_id),
                        build_id: app.build_id,
                    },
                    evidence: vec![InstallationEvidence {
                        source: DetectionSource::StoreManifest,
                        confidence: Confidence::High,
                        detail: "Steam appmanifest".into(),
                    }],
                    warnings,
                });
            }
        }
        Ok(candidates)
    }
}

#[derive(Debug, Clone)]
pub struct EpicFilesystemScanner {
    pub program_data: PathBuf,
}

impl StoreScanner for EpicFilesystemScanner {
    fn id(&self) -> &'static str {
        "epic"
    }

    fn scan(&self) -> Result<Vec<InstallationCandidate>, DiscoveryError> {
        let launcher = self
            .program_data
            .join("Epic")
            .join("UnrealEngineLauncher")
            .join("LauncherInstalled.dat");
        let manifest_root = self
            .program_data
            .join("Epic")
            .join("EpicGamesLauncher")
            .join("Data")
            .join("Manifests");
        let mut installs = Vec::new();
        if let Ok(text) = read_text_bounded(&launcher, 16 * 1024 * 1024) {
            installs.extend(
                parse_launcher_installed(&text).map_err(|error| scanner_error(self.id(), error))?,
            );
        }
        if let Ok(entries) = fs::read_dir(&manifest_root) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|value| value.to_str()) != Some("item") {
                    continue;
                }
                let text = read_text_bounded(&entry.path(), 8 * 1024 * 1024)
                    .map_err(|error| scanner_error(self.id(), error))?;
                if let Some(install) =
                    parse_epic_item(&text).map_err(|error| scanner_error(self.id(), error))?
                {
                    installs.push(install);
                }
            }
        }
        Ok(merge_epic(installs))
    }
}

fn merge_epic(installs: Vec<EpicInstall>) -> Vec<InstallationCandidate> {
    let mut candidates = std::collections::BTreeMap::<String, InstallationCandidate>::new();
    for install in installs {
        let (install_dir, executable, executable_verified, warning) = inspect_declared_executable(
            &install.install_location,
            install.launch_executable.as_deref(),
        );
        let key = format!(
            "{}:{}",
            install.app_name.to_ascii_lowercase(),
            install_dir.to_string_lossy().to_ascii_lowercase()
        );
        let mut evidence = vec![InstallationEvidence {
            source: DetectionSource::LauncherManifest,
            confidence: Confidence::High,
            detail: "Epic launcher manifest".into(),
        }];
        if executable_verified {
            evidence.push(InstallationEvidence {
                source: DetectionSource::VerifiedExecutable,
                confidence: Confidence::Verified,
                detail: "Epic launch executable exists".into(),
            });
        }
        let mut warnings = BTreeSet::new();
        if let Some(warning) = warning {
            warnings.insert(warning.into());
        }
        let candidate = InstallationCandidate {
            store: StoreKind::Epic,
            display_name: install.display_name,
            install_dir,
            executable,
            edition: EditionEvidence {
                edition_id: install.artifact_id,
                store_id: install.catalog_item_id.or(install.catalog_namespace),
                build_id: install.app_version,
            },
            evidence,
            warnings,
        };
        if let Some(existing) = candidates.get_mut(&key) {
            existing.merge(candidate);
        } else {
            candidates.insert(key, candidate);
        }
    }
    candidates.into_values().collect()
}

#[derive(Debug, Clone, Default)]
pub struct GogFilesystemScanner {
    pub registry_installations: Vec<(String, PathBuf)>,
}

impl StoreScanner for GogFilesystemScanner {
    fn id(&self) -> &'static str {
        "gog"
    }

    fn scan(&self) -> Result<Vec<InstallationCandidate>, DiscoveryError> {
        let mut candidates = Vec::new();
        for (product_id, declared_install_dir) in &self.registry_installations {
            let canonical_install_dir = canonical_existing_directory(declared_install_dir);
            let install_dir = canonical_install_dir
                .clone()
                .unwrap_or_else(|| declared_install_dir.clone());
            let info_path = canonical_install_dir.as_ref().and_then(|root| {
                fs::read_dir(root)
                    .ok()?
                    .flatten()
                    .map(|entry| entry.path())
                    .find(|path| {
                        path.file_name()
                            .and_then(|value| value.to_str())
                            .is_some_and(|name| {
                                name.starts_with("goggame-") && name.ends_with(".info")
                            })
                    })
            });
            let parsed = info_path
                .as_ref()
                .and_then(|path| read_text_bounded(path, 4 * 1024 * 1024).ok())
                .and_then(|text| parse_gog_info(&text, install_dir.clone()).ok());
            let metadata_failed_validation = info_path.is_some() && parsed.is_none();
            let relative_executable = parsed
                .as_ref()
                .and_then(|install| install.play_tasks.first())
                .cloned();
            let (install_dir, executable, executable_verified, warning) =
                inspect_declared_executable(&install_dir, relative_executable.as_deref());
            let mut evidence = vec![InstallationEvidence {
                source: DetectionSource::Registry,
                confidence: Confidence::Medium,
                detail: "GOG registry installation".into(),
            }];
            if parsed.is_some() {
                evidence.push(InstallationEvidence {
                    source: DetectionSource::StoreManifest,
                    confidence: Confidence::High,
                    detail: "goggame metadata".into(),
                });
            }
            if executable_verified {
                evidence.push(InstallationEvidence {
                    source: DetectionSource::VerifiedExecutable,
                    confidence: Confidence::Verified,
                    detail: "GOG primary play task exists".into(),
                });
            }
            let mut warnings = BTreeSet::new();
            if let Some(warning) = warning {
                warnings.insert(warning.into());
            }
            if metadata_failed_validation {
                warnings.insert("GOG metadata could not be validated".into());
            }
            candidates.push(InstallationCandidate {
                store: StoreKind::Gog,
                display_name: parsed.as_ref().map_or_else(
                    || format!("GOG product {product_id}"),
                    |install| install.name.clone(),
                ),
                install_dir,
                executable,
                edition: EditionEvidence {
                    edition_id: None,
                    store_id: Some(
                        parsed.as_ref().map_or_else(
                            || product_id.clone(),
                            |install| install.product_id.clone(),
                        ),
                    ),
                    build_id: parsed.and_then(|install| install.build_id),
                },
                evidence,
                warnings,
            });
        }
        Ok(candidates)
    }
}

fn inspect_declared_executable(
    declared_install_dir: &Path,
    relative_executable: Option<&Path>,
) -> (PathBuf, Option<PathBuf>, bool, Option<&'static str>) {
    let Some(relative_executable) = relative_executable else {
        return match canonical_existing_directory(declared_install_dir) {
            Some(root) => (root, None, false, None),
            None => (
                declared_install_dir.to_path_buf(),
                None,
                false,
                Some("store manifest exists but install directory is unavailable"),
            ),
        };
    };

    match inspect_store_executable(declared_install_dir, relative_executable) {
        ExecutableInspection::Verified {
            canonical_root,
            canonical_executable,
        } => (canonical_root, Some(canonical_executable), true, None),
        ExecutableInspection::RootUnavailable => (
            declared_install_dir.to_path_buf(),
            None,
            false,
            Some("store manifest exists but install directory is unavailable"),
        ),
        ExecutableInspection::ExecutableUnavailable { canonical_root } => (
            canonical_root,
            None,
            false,
            Some("store launch executable is unavailable"),
        ),
        ExecutableInspection::Unsafe { canonical_root } => (
            canonical_root.unwrap_or_else(|| declared_install_dir.to_path_buf()),
            None,
            false,
            Some("store launch executable failed path-safety validation"),
        ),
    }
}

fn read_text_bounded(path: &Path, maximum: u64) -> Result<String, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || is_link_or_reparse(&metadata) || metadata.len() > maximum {
        return Err("manifest path is unsafe, not a file, or oversized".into());
    }
    fs::read_to_string(path).map_err(|error| error.to_string())
}

fn scanner_error(scanner: &str, error: impl std::fmt::Display) -> DiscoveryError {
    DiscoveryError::Scanner {
        scanner: scanner.to_owned(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn steam_scanner_follows_library_manifest_and_appmanifest() {
        let root = tempfile::tempdir().unwrap();
        let steamapps = root.path().join("steamapps");
        fs::create_dir_all(steamapps.join("common").join("Example Game")).unwrap();
        fs::write(
            steamapps.join("libraryfolders.vdf"),
            format!(
                r#""libraryfolders" {{ "0" {{ "path" "{}" }} }}"#,
                root.path().display()
            ),
        )
        .unwrap();
        fs::write(
            steamapps.join("appmanifest_42.acf"),
            r#""AppState" { "appid" "42" "name" "Example Game" "StateFlags" "4" "installdir" "Example Game" "buildid" "99" }"#,
        )
        .unwrap();
        let result = SteamFilesystemScanner {
            roots: vec![root.path().to_path_buf()],
        }
        .scan()
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].edition.store_id.as_deref(), Some("42"));
        assert_eq!(result[0].confidence(), Confidence::High);
    }

    #[test]
    fn steam_scanner_rejects_traversal_before_constructing_candidate() {
        let root = tempfile::tempdir().unwrap();
        let steamapps = root.path().join("steamapps");
        fs::create_dir_all(&steamapps).unwrap();
        fs::write(
            steamapps.join("appmanifest_42.acf"),
            r#""AppState" { "appid" "42" "name" "Hostile" "installdir" "../outside" }"#,
        )
        .unwrap();

        let error = SteamFilesystemScanner {
            roots: vec![root.path().to_path_buf()],
        }
        .scan()
        .unwrap_err();
        let diagnostic = error.to_string();
        assert!(diagnostic.contains("unsafe Steam install directory"));
        assert!(!diagnostic.contains(root.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn epic_scanner_deduplicates_launcher_and_item_evidence() {
        let root = tempfile::tempdir().unwrap();
        let program_data = root.path();
        let launcher = program_data.join("Epic/UnrealEngineLauncher");
        let manifests = program_data.join("Epic/EpicGamesLauncher/Data/Manifests");
        let install = program_data.join("Installed/Eclipse");
        fs::create_dir_all(&launcher).unwrap();
        fs::create_dir_all(&manifests).unwrap();
        fs::create_dir_all(install.join("bin/x64")).unwrap();
        let mut executable = fs::File::create(install.join("bin/x64/Eclipse.exe")).unwrap();
        executable.write_all(b"fixture").unwrap();
        fs::write(
            launcher.join("LauncherInstalled.dat"),
            serde_json::to_vec(&serde_json::json!({
                "InstallationList": [{
                    "InstallLocation": install,
                    "AppName": "Eclipse",
                    "ArtifactId": "artifact",
                    "AppVersion": "1"
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            manifests.join("eclipse.item"),
            serde_json::to_vec(&serde_json::json!({
                "AppName": "Eclipse",
                "DisplayName": "Eclipse Harbor",
                "InstallLocation": install,
                "LaunchExecutable": "bin\\x64/Eclipse.exe",
                "ArtifactId": "artifact",
                "CatalogItemId": "catalog",
                "bIsApplication": true
            }))
            .unwrap(),
        )
        .unwrap();
        let result = EpicFilesystemScanner {
            program_data: program_data.to_path_buf(),
        }
        .scan()
        .unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].has_verified_executable());
        assert_eq!(
            result[0].verified_executable_leaf(),
            Some(std::ffi::OsStr::new("Eclipse.exe"))
        );
        assert!(result[0]
            .verified_executable()
            .unwrap()
            .starts_with(&result[0].install_dir));
        assert_eq!(
            result[0].verified_install_root(),
            Some(result[0].install_dir.as_path())
        );
    }

    #[test]
    fn epic_missing_root_has_generic_diagnostic_and_no_executable_evidence() {
        let root = tempfile::tempdir().unwrap();
        let program_data = root.path();
        let manifests = program_data.join("Epic/EpicGamesLauncher/Data/Manifests");
        let missing = program_data.join("private-user-folder/MissingGame");
        fs::create_dir_all(&manifests).unwrap();
        fs::write(
            manifests.join("missing.item"),
            serde_json::to_vec(&serde_json::json!({
                "AppName": "Missing",
                "DisplayName": "Missing",
                "InstallLocation": missing,
                "LaunchExecutable": "bin/game.exe",
                "bIsApplication": true
            }))
            .unwrap(),
        )
        .unwrap();

        let result = EpicFilesystemScanner {
            program_data: program_data.to_path_buf(),
        }
        .scan()
        .unwrap();
        assert_eq!(result.len(), 1);
        assert!(!result[0].has_verified_executable());
        assert!(result[0].executable.is_none());
        let diagnostic = result[0].warnings.iter().next().unwrap();
        assert_eq!(
            diagnostic,
            "store manifest exists but install directory is unavailable"
        );
        assert!(!diagnostic.contains("private-user-folder"));
        assert!(!diagnostic.contains(root.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn epic_scanner_rejects_absolute_executable_without_disclosing_it() {
        let root = tempfile::tempdir().unwrap();
        let manifests = root.path().join("Epic/EpicGamesLauncher/Data/Manifests");
        let install = root.path().join("Installed/Game");
        let outside = root.path().join("private-user-folder/outside.exe");
        fs::create_dir_all(&manifests).unwrap();
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(outside.parent().unwrap()).unwrap();
        fs::write(&outside, b"fixture").unwrap();
        fs::write(
            manifests.join("hostile.item"),
            serde_json::to_vec(&serde_json::json!({
                "AppName": "Hostile",
                "DisplayName": "Hostile",
                "InstallLocation": install,
                "LaunchExecutable": outside,
                "bIsApplication": true
            }))
            .unwrap(),
        )
        .unwrap();

        let error = EpicFilesystemScanner {
            program_data: root.path().to_path_buf(),
        }
        .scan()
        .unwrap_err();
        let diagnostic = error.to_string();
        assert!(diagnostic.contains("unsafe launch executable"));
        assert!(!diagnostic.contains("private-user-folder"));
        assert!(!diagnostic.contains(root.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn gog_scanner_verifies_only_nested_contained_regular_file() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("GOG Game");
        fs::create_dir_all(install.join("bin/x64")).unwrap();
        fs::write(install.join("bin/x64/game.exe"), b"fixture").unwrap();
        fs::write(
            install.join("goggame-42.info"),
            serde_json::to_vec(&serde_json::json!({
                "gameId": "42",
                "name": "GOG Game",
                "playTasks": [{
                    "type": "FileTask",
                    "path": "bin\\x64/game.exe",
                    "isPrimary": true
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        let result = GogFilesystemScanner {
            registry_installations: vec![("42".into(), install)],
        }
        .scan()
        .unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].has_verified_executable());
        assert_eq!(
            result[0].verified_executable_leaf(),
            Some(std::ffi::OsStr::new("game.exe"))
        );
    }

    #[test]
    fn gog_missing_root_preserves_generic_diagnostic() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("private-user-folder/MissingGame");
        let result = GogFilesystemScanner {
            registry_installations: vec![("42".into(), missing)],
        }
        .scan()
        .unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].executable.is_none());
        let diagnostic = result[0].warnings.iter().next().unwrap();
        assert_eq!(
            diagnostic,
            "store manifest exists but install directory is unavailable"
        );
        assert!(!diagnostic.contains("private-user-folder"));
        assert!(!diagnostic.contains(root.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn gog_scanner_rejects_traversal_even_when_outside_file_exists() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("Game");
        fs::create_dir_all(&install).unwrap();
        fs::write(root.path().join("outside.exe"), b"fixture").unwrap();
        fs::write(
            install.join("goggame-42.info"),
            r#"{"gameId":"42","name":"Hostile","playTasks":[{"type":"FileTask","path":"../outside.exe","isPrimary":true}]}"#,
        )
        .unwrap();

        let result = GogFilesystemScanner {
            registry_installations: vec![("42".into(), install)],
        }
        .scan()
        .unwrap();
        assert_eq!(result.len(), 1);
        assert!(!result[0].has_verified_executable());
        assert!(result[0].executable.is_none());
        assert!(result[0]
            .warnings
            .contains("GOG metadata could not be validated"));
    }

    #[test]
    fn common_directory_scanner_is_bounded_and_verifies_exact_profile_rule() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("Example Game");
        fs::create_dir_all(install.join("bin")).unwrap();
        fs::write(install.join("bin/game.exe"), b"fixture").unwrap();
        fs::write(root.path().join("unlisted.exe"), b"must not be scanned").unwrap();

        let result = CommonDirectoryScanner {
            roots: vec![root.path().to_path_buf()],
            rules: vec![CommonDirectoryRule {
                display_name: "Example Game".into(),
                relative_install_dir: PathBuf::from("Example Game"),
                relative_executable: PathBuf::from("bin/game.exe"),
                store_id: Some("example-common".into()),
            }],
        }
        .scan()
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].store, StoreKind::Standalone);
        assert!(result[0].has_verified_executable());
        let canonical_install = fs::canonicalize(install).unwrap();
        assert_eq!(
            result[0].verified_install_root(),
            Some(canonical_install.as_path())
        );
        assert!(result[0]
            .evidence
            .iter()
            .any(|item| item.source == DetectionSource::CommonDirectory));
    }

    #[test]
    fn common_directory_scanner_rejects_traversal_rules_without_probing() {
        let root = tempfile::tempdir().unwrap();
        let result = CommonDirectoryScanner {
            roots: vec![root.path().to_path_buf()],
            rules: vec![CommonDirectoryRule {
                display_name: "Hostile".into(),
                relative_install_dir: PathBuf::from("../outside"),
                relative_executable: PathBuf::from("game.exe"),
                store_id: None,
            }],
        }
        .scan();
        assert!(matches!(result, Err(DiscoveryError::Scanner { .. })));
    }
}
