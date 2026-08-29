use crate::domain::{OnboardingPersistence, OnboardingSnapshot, PersistenceHealth};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use tempfile::NamedTempFile;
use thiserror::Error;

const ONBOARDING_FILE_NAME: &str = "onboarding-v1.json";
const MAX_ONBOARDING_BYTES: u64 = 64 * 1024;

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("configuration directory is unavailable: {0}")]
    Directory(#[source] std::io::Error),
    #[error("failed to inspect onboarding state: {0}")]
    Metadata(#[source] std::io::Error),
    #[error("onboarding state exceeds the {MAX_ONBOARDING_BYTES}-byte safety limit")]
    TooLarge,
    #[error("failed to open onboarding state: {0}")]
    Open(#[source] std::io::Error),
    #[error("failed to read onboarding state: {0}")]
    Read(#[source] std::io::Error),
    #[error("onboarding state is invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("onboarding state failed validation: {0}")]
    Validation(String),
    #[error("failed to stage onboarding state: {0}")]
    Stage(#[source] std::io::Error),
    #[error("failed to commit onboarding state: {0}")]
    Commit(#[source] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct OnboardingStore {
    directory: PathBuf,
}

impl OnboardingStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn load(&self) -> (OnboardingSnapshot, OnboardingPersistence) {
        match self.load_checked() {
            Ok(Some(state)) => (
                state,
                OnboardingPersistence {
                    health: PersistenceHealth::Healthy,
                    detail:
                        "Onboarding settings loaded from the current user's app-data directory."
                            .into(),
                },
            ),
            Ok(None) => (
                OnboardingSnapshot::default(),
                OnboardingPersistence {
                    health: PersistenceHealth::FirstRun,
                    detail: "No saved onboarding settings exist yet.".into(),
                },
            ),
            Err(error) => (
                OnboardingSnapshot::default(),
                OnboardingPersistence {
                    health: PersistenceHealth::RecoveredFromInvalidFile,
                    detail: format!("Saved onboarding settings were ignored safely: {error}"),
                },
            ),
        }
    }

    pub fn save(&self, state: &OnboardingSnapshot) -> Result<(), PersistenceError> {
        state.validate().map_err(PersistenceError::Validation)?;
        fs::create_dir_all(&self.directory).map_err(PersistenceError::Directory)?;
        let bytes = serde_json::to_vec_pretty(state)?;
        if bytes.len() as u64 > MAX_ONBOARDING_BYTES {
            return Err(PersistenceError::TooLarge);
        }

        // NamedTempFile uses a same-directory temporary file so persistence can
        // replace the target without crossing volumes. sync_all ensures the
        // bytes reach the filesystem before the visible name changes.
        let mut temporary =
            NamedTempFile::new_in(&self.directory).map_err(PersistenceError::Stage)?;
        temporary
            .write_all(&bytes)
            .map_err(PersistenceError::Stage)?;
        temporary
            .as_file()
            .sync_all()
            .map_err(PersistenceError::Stage)?;
        temporary
            .persist(self.path())
            .map_err(|error| PersistenceError::Commit(error.error))?;

        // Best effort directory sync is useful on filesystems that support it;
        // Windows directory handles are not opened this way, so it is not a
        // portability requirement for a successful save.
        if let Ok(directory) = File::open(&self.directory) {
            let _ = directory.sync_all();
        }
        Ok(())
    }

    fn load_checked(&self) -> Result<Option<OnboardingSnapshot>, PersistenceError> {
        let path = self.path();
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(PersistenceError::Metadata(error)),
        };
        if !metadata.is_file() || metadata.len() > MAX_ONBOARDING_BYTES {
            return Err(PersistenceError::TooLarge);
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(path)
            .map_err(PersistenceError::Open)?
            .take(MAX_ONBOARDING_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(PersistenceError::Read)?;
        if bytes.len() as u64 > MAX_ONBOARDING_BYTES {
            return Err(PersistenceError::TooLarge);
        }
        let state: OnboardingSnapshot = serde_json::from_slice(&bytes)?;
        state.validate().map_err(PersistenceError::Validation)?;
        Ok(Some(state))
    }

    fn path(&self) -> PathBuf {
        self.directory.join(ONBOARDING_FILE_NAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{OnboardingStep, PreferenceSnapshot};
    use pretty_assertions::assert_eq;

    #[test]
    fn first_run_is_explicit() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = OnboardingStore::new(temp.path());
        let (state, health) = store.load();
        assert_eq!(state, OnboardingSnapshot::default());
        assert_eq!(health.health, PersistenceHealth::FirstRun);
    }

    #[test]
    fn state_round_trips_atomically() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = OnboardingStore::new(temp.path());
        let state = OnboardingSnapshot {
            schema_version: crate::domain::ONBOARDING_SCHEMA_VERSION,
            completed: true,
            current_step: OnboardingStep::Ready,
            selected_game_id: Some("skyrim-special-edition".into()),
            preferences: PreferenceSnapshot::default(),
            updated_at_epoch_ms: 123,
        };
        store.save(&state).expect("save state");
        let (loaded, health) = store.load();
        assert_eq!(loaded, state);
        assert_eq!(health.health, PersistenceHealth::Healthy);
        assert!(temp.path().join(ONBOARDING_FILE_NAME).is_file());

        let replacement = OnboardingSnapshot {
            completed: false,
            current_step: OnboardingStep::Presence,
            selected_game_id: Some("cyberpunk-2077".into()),
            updated_at_epoch_ms: 456,
            ..OnboardingSnapshot::default()
        };
        store.save(&replacement).expect("replace state");
        assert_eq!(store.load().0, replacement);
    }

    #[test]
    fn invalid_or_future_state_is_recovered_without_panicking() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = OnboardingStore::new(temp.path());
        fs::write(
            temp.path().join(ONBOARDING_FILE_NAME),
            format!(
                r#"{{"schemaVersion":{},"completed":false}}"#,
                crate::domain::ONBOARDING_SCHEMA_VERSION + 1
            ),
        )
        .expect("write fixture");
        let (loaded, health) = store.load();
        assert_eq!(loaded, OnboardingSnapshot::default());
        assert_eq!(health.health, PersistenceHealth::RecoveredFromInvalidFile);
    }
}
