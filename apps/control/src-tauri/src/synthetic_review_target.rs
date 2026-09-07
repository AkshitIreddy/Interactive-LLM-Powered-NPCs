use crate::sidecar_supervisor::RuntimeSupervisor;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub(crate) const SYNTHETIC_TARGET_BASENAME: &str = "interactive-npcs-synthetic-target.exe";
const FIXTURE_MANIFEST_BASENAME: &str = "REVIEW-FIXTURE-MANIFEST.json";
const MAX_FIXTURE_MANIFEST_BYTES: u64 = 256 * 1024;

#[derive(Debug, Deserialize)]
struct FixtureManifest {
    schema_version: u32,
    component_id: String,
    fixture_kind_compatibility: String,
    distribution_scope: String,
    distribution_class: String,
    files: Vec<FixtureFile>,
}

#[derive(Debug, Deserialize)]
struct FixtureFile {
    path: String,
    sha256: String,
    size_bytes: u64,
    component_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SyntheticReviewTargetError {
    #[error("the packaged synthetic review target is unavailable")]
    Unavailable,
    #[error("the packaged synthetic review target manifest is invalid")]
    InvalidManifest,
    #[error("the packaged synthetic review target failed integrity validation")]
    Integrity,
    #[error("the packaged synthetic review target could not be launched")]
    Launch,
}

#[derive(Clone)]
pub struct SyntheticReviewTargetLauncher {
    control_executable: PathBuf,
    resource_root: PathBuf,
    metadata_path: PathBuf,
    parent_job: RuntimeSupervisor,
}

impl SyntheticReviewTargetLauncher {
    pub fn new(
        control_executable: PathBuf,
        resource_root: PathBuf,
        metadata_path: PathBuf,
        parent_job: RuntimeSupervisor,
    ) -> Self {
        Self {
            control_executable,
            resource_root,
            metadata_path,
            parent_job,
        }
    }

    pub fn executable_path(&self) -> Result<PathBuf, SyntheticReviewTargetError> {
        resolve_synthetic_review_target(&self.control_executable, &self.resource_root)
    }

    pub fn launch(&self) -> Result<LaunchedSyntheticReviewTarget, SyntheticReviewTargetError> {
        let executable_path = self.executable_path()?;
        let metadata_parent = self
            .metadata_path
            .parent()
            .ok_or(SyntheticReviewTargetError::Launch)?;
        std::fs::create_dir_all(metadata_parent).map_err(|_| SyntheticReviewTargetError::Launch)?;
        let mut command = Command::new(&executable_path);
        command
            .arg("--metadata")
            .arg(&self.metadata_path)
            .arg("--mute")
            .current_dir(
                executable_path
                    .parent()
                    .ok_or(SyntheticReviewTargetError::Launch)?,
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let child = command
            .spawn()
            .map_err(|_| SyntheticReviewTargetError::Launch)?;
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            if self
                .parent_job
                .assign_raw_process_to_parent_job(child.as_raw_handle() as usize)
                .is_err()
            {
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                return Err(SyntheticReviewTargetError::Launch);
            }
        }
        Ok(LaunchedSyntheticReviewTarget {
            executable_path,
            child,
        })
    }
}

pub struct LaunchedSyntheticReviewTarget {
    pub executable_path: PathBuf,
    pub child: Child,
}

pub fn resolve_synthetic_review_target(
    control_executable: &Path,
    resource_root: &Path,
) -> Result<PathBuf, SyntheticReviewTargetError> {
    let control_directory = control_executable
        .parent()
        .ok_or(SyntheticReviewTargetError::Unavailable)?;
    let mut candidates = vec![control_directory
        .join("local-app-data")
        .join("test-game")
        .join(SYNTHETIC_TARGET_BASENAME)];
    if let Some(parent) = control_directory.parent() {
        candidates.push(
            parent
                .join("local-app-data")
                .join("test-game")
                .join(SYNTHETIC_TARGET_BASENAME),
        );
    }
    if let Some(parent) = resource_root.parent() {
        candidates.push(
            parent
                .join("local-app-data")
                .join("test-game")
                .join(SYNTHETIC_TARGET_BASENAME),
        );
    }

    let mut seen = BTreeSet::new();
    let mut first_validation_error = None;
    for candidate in candidates {
        let key = candidate.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        match validate_candidate(&candidate) {
            Ok(path) => return Ok(path),
            Err(SyntheticReviewTargetError::Unavailable) => {}
            Err(error) if first_validation_error.is_none() => first_validation_error = Some(error),
            Err(_) => {}
        }
    }
    Err(first_validation_error.unwrap_or(SyntheticReviewTargetError::Unavailable))
}

fn validate_candidate(path: &Path) -> Result<PathBuf, SyntheticReviewTargetError> {
    if path.file_name().and_then(|value| value.to_str()) != Some(SYNTHETIC_TARGET_BASENAME) {
        return Err(SyntheticReviewTargetError::Unavailable);
    }
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(SyntheticReviewTargetError::Unavailable)
        }
        Err(_) => return Err(SyntheticReviewTargetError::Integrity),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() == 0 {
        return Err(SyntheticReviewTargetError::Integrity);
    }
    let directory = path.parent().ok_or(SyntheticReviewTargetError::Integrity)?;
    let manifest_path = directory.join(FIXTURE_MANIFEST_BASENAME);
    let manifest_metadata = std::fs::symlink_metadata(&manifest_path)
        .map_err(|_| SyntheticReviewTargetError::InvalidManifest)?;
    if !manifest_metadata.is_file()
        || manifest_metadata.file_type().is_symlink()
        || manifest_metadata.len() == 0
        || manifest_metadata.len() > MAX_FIXTURE_MANIFEST_BYTES
    {
        return Err(SyntheticReviewTargetError::InvalidManifest);
    }
    let manifest_bytes =
        std::fs::read(&manifest_path).map_err(|_| SyntheticReviewTargetError::InvalidManifest)?;
    let manifest: FixtureManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| SyntheticReviewTargetError::InvalidManifest)?;
    if manifest.schema_version != 1
        || manifest.component_id != "project:synthetic-review-target"
        || manifest.fixture_kind_compatibility != "synthetic-original-video-replay"
        || manifest.distribution_scope != "review-test-game"
        || manifest.distribution_class != "project-owned-source-built-review-fixture"
    {
        return Err(SyntheticReviewTargetError::InvalidManifest);
    }
    let matching = manifest
        .files
        .iter()
        .filter(|entry| entry.path == SYNTHETIC_TARGET_BASENAME)
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(SyntheticReviewTargetError::InvalidManifest);
    }
    let entry = matching[0];
    if entry.component_id != "project:synthetic-review-target"
        || entry.size_bytes != metadata.len()
        || entry.sha256.len() != 64
        || !entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(SyntheticReviewTargetError::InvalidManifest);
    }
    let bytes = std::fs::read(path).map_err(|_| SyntheticReviewTargetError::Integrity)?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    if !actual.eq_ignore_ascii_case(&entry.sha256) {
        return Err(SyntheticReviewTargetError::Integrity);
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| SyntheticReviewTargetError::Integrity)?;
    let canonical_directory = directory
        .canonicalize()
        .map_err(|_| SyntheticReviewTargetError::Integrity)?;
    if canonical.parent() != Some(canonical_directory.as_path()) {
        return Err(SyntheticReviewTargetError::Integrity);
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn write_fixture(root: &Path) -> PathBuf {
        let directory = root.join("local-app-data").join("test-game");
        std::fs::create_dir_all(&directory).expect("create portable fixture directory");
        let executable = directory.join(SYNTHETIC_TARGET_BASENAME);
        std::fs::write(&executable, b"synthetic-review-executable")
            .expect("write fixture executable");
        let executable_bytes = b"synthetic-review-executable";
        let digest = format!("{:x}", Sha256::digest(executable_bytes));
        let manifest = serde_json::json!({
            "schema_version": 1,
            "component_id": "project:synthetic-review-target",
            "fixture_kind_compatibility": "synthetic-original-video-replay",
            "distribution_scope": "review-test-game",
            "distribution_class": "project-owned-source-built-review-fixture",
            "files": [{
                "path": SYNTHETIC_TARGET_BASENAME,
                "sha256": digest,
                "size_bytes": executable_bytes.len(),
                "component_id": "project:synthetic-review-target"
            }]
        });
        std::fs::write(
            directory.join("REVIEW-FIXTURE-MANIFEST.json"),
            serde_json::to_vec(&manifest).expect("serialize manifest"),
        )
        .expect("write manifest");
        executable
    }

    #[test]
    fn resolves_only_manifest_attested_portable_sibling_target() {
        let root = tempfile::tempdir().expect("portable review root");
        let expected = write_fixture(root.path());
        let control = root.path().join("interactive-npcs-control.exe");
        std::fs::write(&control, b"control").expect("write control executable");

        let resolved = resolve_synthetic_review_target(&control, root.path())
            .expect("attested co-located target");
        assert_eq!(resolved, expected.canonicalize().expect("canonical target"));
    }

    #[test]
    fn rejects_tampered_or_unmanifested_target() {
        let root = tempfile::tempdir().expect("portable review root");
        let executable = write_fixture(root.path());
        let control = root.path().join("interactive-npcs-control.exe");
        std::fs::write(&control, b"control").expect("write control executable");
        std::fs::write(&executable, b"tampered").expect("tamper target");

        assert!(matches!(
            resolve_synthetic_review_target(&control, root.path()),
            Err(SyntheticReviewTargetError::InvalidManifest)
                | Err(SyntheticReviewTargetError::Integrity)
        ));
        std::fs::remove_file(
            root.path()
                .join("local-app-data/test-game/REVIEW-FIXTURE-MANIFEST.json"),
        )
        .expect("remove manifest");
        assert!(matches!(
            resolve_synthetic_review_target(&control, root.path()),
            Err(SyntheticReviewTargetError::InvalidManifest)
        ));
    }
}
