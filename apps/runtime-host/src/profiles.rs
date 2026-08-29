use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use interactive_npcs_game_discovery::{InstallationCandidate, StoreKind as DiscoveredStore};
use npc_game_profile::{load_profile, GameProfileV2, StoreKind as ProfileStore};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::REQUIRED_PROFILE_COUNT;

const MAX_PROFILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DIRECTORY_DEPTH: usize = 4;
pub const GENERIC_GAME_ID: &str = "generic-game";

#[derive(Clone, Debug)]
pub struct LoadedProfile {
    pub source: PathBuf,
    pub profile: GameProfileV2,
}

#[derive(Clone, Debug)]
pub struct ProfileCorpus {
    profiles: Vec<LoadedProfile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub id: String,
    pub display_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInstallationMatch {
    pub profile_id: String,
    pub display_name: String,
    pub store: String,
    pub install_dir: PathBuf,
    pub confidence: String,
    pub evidence: Vec<String>,
}

/// The only game-integration boundary exposed by the runtime.
///
/// Authored profiles may contain legacy/declarative capability provenance, but
/// that data is never an executable routing decision. Every game uses the same
/// out-of-process HWND capture path and the same audio/subtitle fallback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeIntegrationPolicy {
    pub mode: &'static str,
    pub capture_routes: [&'static str; 3],
    pub explicit_character_selection_supported: bool,
    pub screen_space_animation_experimental: bool,
    pub native_rig_animation_allowed: bool,
    pub executable_adapters_allowed: bool,
    pub process_injection_allowed: bool,
    pub game_hooks_allowed: bool,
    pub game_module_loading_allowed: bool,
    pub action_proposals_allowed: bool,
    pub protected_online_policy: &'static str,
    pub anti_cheat_policy: &'static str,
}

impl Default for RuntimeIntegrationPolicy {
    fn default() -> Self {
        Self {
            mode: "external_generic_capture",
            capture_routes: [
                "windows_graphics_capture",
                "desktop_duplication",
                "audio_subtitles",
            ],
            explicit_character_selection_supported: true,
            screen_space_animation_experimental: true,
            native_rig_animation_allowed: false,
            executable_adapters_allowed: false,
            process_injection_allowed: false,
            game_hooks_allowed: false,
            game_module_loading_allowed: false,
            action_proposals_allowed: false,
            protected_online_policy: "blocked",
            anti_cheat_policy: "block_when_detected",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenericGameContract {
    pub id: &'static str,
    pub display_name: &'static str,
    pub experimental: bool,
    pub counted_as_authored_profile: bool,
    pub selection: [&'static str; 3],
    pub supported_routes: [&'static str; 4],
    pub experimental_routes: [&'static str; 2],
    pub executable_adapters_allowed: bool,
    pub action_proposals_allowed: bool,
    pub protected_online_policy: &'static str,
    pub anti_cheat_policy: &'static str,
    pub fallback: &'static str,
}

impl Default for GenericGameContract {
    fn default() -> Self {
        Self {
            id: GENERIC_GAME_ID,
            display_name: "Generic Game (Experimental)",
            experimental: true,
            counted_as_authored_profile: false,
            selection: ["manual_game_name", "manual_executable_name", "manual_character_name"],
            supported_routes: ["conversation", "memory", "audio", "subtitles"],
            experimental_routes: ["identity", "screen_space_lip_sync"],
            executable_adapters_allowed: false,
            action_proposals_allowed: false,
            protected_online_policy: "blocked",
            anti_cheat_policy: "block_when_detected",
            fallback: "Explicit character selection with audio and subtitles; the game image remains untouched.",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenericGameSelection {
    pub game_name: String,
    pub executable_name: String,
    pub character_name: String,
    #[serde(default)]
    pub protected_online_detected: bool,
    #[serde(default)]
    pub anti_cheat_detected: bool,
}

impl GenericGameSelection {
    pub fn validate(&self) -> Result<(), GenericGameError> {
        if self.game_name.trim().is_empty()
            || self.game_name.len() > 160
            || self.character_name.trim().is_empty()
            || self.character_name.len() > 160
            || !is_safe_executable_leaf(&self.executable_name)
        {
            return Err(GenericGameError::InvalidSelection);
        }
        if self.protected_online_detected {
            return Err(GenericGameError::ProtectedOnlineBlocked);
        }
        if self.anti_cheat_detected {
            return Err(GenericGameError::AntiCheatBlocked);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum GenericGameError {
    #[error("generic game selection is invalid")]
    InvalidSelection,
    #[error("generic mode is blocked for protected online play")]
    ProtectedOnlineBlocked,
    #[error("generic mode is blocked when anti-cheat is detected")]
    AntiCheatBlocked,
}

impl ProfileCorpus {
    pub fn load(root: impl AsRef<Path>) -> Result<Self, ProfileCorpusError> {
        let root = root.as_ref();
        if !root.is_dir() {
            return Err(ProfileCorpusError::MissingRoot);
        }
        let mut candidates = Vec::new();
        collect_profile_files(root, root, 0, &mut candidates)?;
        candidates.sort();

        let mut ids = BTreeSet::new();
        let mut profiles = Vec::with_capacity(candidates.len());
        for source in candidates {
            let metadata = fs::symlink_metadata(&source).map_err(ProfileCorpusError::Io)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(ProfileCorpusError::UnsafeEntry);
            }
            if metadata.len() > MAX_PROFILE_BYTES {
                return Err(ProfileCorpusError::TooLarge);
            }
            let bytes = fs::read(&source).map_err(ProfileCorpusError::Io)?;
            let profile = load_profile(&bytes).map_err(|_| ProfileCorpusError::InvalidProfile)?;
            if !ids.insert(profile.id.clone()) {
                return Err(ProfileCorpusError::DuplicateId);
            }
            profiles.push(LoadedProfile { source, profile });
        }
        if profiles.len() != REQUIRED_PROFILE_COUNT {
            return Err(ProfileCorpusError::WrongCount {
                expected: REQUIRED_PROFILE_COUNT,
                actual: profiles.len(),
            });
        }
        Ok(Self { profiles })
    }

    #[must_use]
    pub fn profiles(&self) -> &[LoadedProfile] {
        &self.profiles
    }

    #[must_use]
    pub fn profile(&self, id: &str) -> Option<&GameProfileV2> {
        self.profiles
            .iter()
            .find(|loaded| loaded.profile.id == id)
            .map(|loaded| &loaded.profile)
    }

    #[must_use]
    pub fn summaries(&self) -> Vec<ProfileSummary> {
        self.profiles
            .iter()
            .map(|loaded| ProfileSummary {
                id: loaded.profile.id.clone(),
                display_name: loaded.profile.display_name.clone(),
            })
            .collect()
    }

    #[must_use]
    pub fn generic_contract(&self) -> GenericGameContract {
        GenericGameContract::default()
    }

    /// Resolve the immutable runtime integration policy for an authored or
    /// generic game. A profile can change lore/detection hints, never the
    /// process boundary or the set of executable integration mechanisms.
    #[must_use]
    pub fn runtime_integration_policy(&self, game_id: &str) -> Option<RuntimeIntegrationPolicy> {
        (game_id == GENERIC_GAME_ID || self.profile(game_id).is_some())
            .then(RuntimeIntegrationPolicy::default)
    }

    #[must_use]
    pub fn match_installations(
        &self,
        candidates: &[InstallationCandidate],
    ) -> Vec<ProfileInstallationMatch> {
        let mut matches = Vec::new();
        for loaded in &self.profiles {
            for candidate in candidates {
                let Some(store) = loaded
                    .profile
                    .detection
                    .stores
                    .iter()
                    .find(|store| store_matches(store.store, candidate.store))
                else {
                    continue;
                };
                let id_matches = match (&store.app_id, &candidate.edition.store_id) {
                    (Some(expected), Some(actual)) => expected.eq_ignore_ascii_case(actual),
                    (Some(_), None) => false,
                    (None, _) => true,
                };
                let hint_matches = store.install_directory_hints.is_empty()
                    || store.install_directory_hints.iter().any(|hint| {
                        let hint = normalize(hint);
                        let directory = normalize(&candidate.install_dir.to_string_lossy());
                        let display = normalize(&candidate.display_name);
                        directory.contains(&hint) || display.contains(&hint)
                    });
                if !id_matches || !hint_matches {
                    continue;
                }
                let executable_matches = candidate.executable.as_ref().is_some_and(|path| {
                    path.file_name()
                        .and_then(|value| value.to_str())
                        .is_some_and(|leaf| {
                            loaded.profile.detection.processes.iter().any(|process| {
                                process.required && process.executable.eq_ignore_ascii_case(leaf)
                            })
                        })
                });
                let mut evidence = vec!["store_profile_match".to_owned()];
                if store.app_id.is_some() {
                    evidence.push("store_id_match".to_owned());
                }
                if executable_matches {
                    evidence.push("profile_executable_match".to_owned());
                }
                matches.push(ProfileInstallationMatch {
                    profile_id: loaded.profile.id.clone(),
                    display_name: loaded.profile.display_name.clone(),
                    store: format!("{:?}", candidate.store).to_ascii_lowercase(),
                    install_dir: candidate.install_dir.clone(),
                    confidence: format!("{:?}", candidate.confidence()).to_ascii_lowercase(),
                    evidence,
                });
            }
        }
        matches.sort_by(|left, right| {
            left.profile_id
                .cmp(&right.profile_id)
                .then_with(|| left.install_dir.cmp(&right.install_dir))
        });
        matches.dedup_by(|left, right| {
            left.profile_id == right.profile_id && left.install_dir == right.install_dir
        });
        matches
    }
}

fn store_matches(profile: ProfileStore, discovered: DiscoveredStore) -> bool {
    matches!(
        (profile, discovered),
        (ProfileStore::Steam, DiscoveredStore::Steam)
            | (ProfileStore::Epic, DiscoveredStore::Epic)
            | (ProfileStore::Gog, DiscoveredStore::Gog)
            | (
                ProfileStore::Standalone,
                DiscoveredStore::Standalone | DiscoveredStore::Manual
            )
    )
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_safe_executable_leaf(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.len() >= 5
        && trimmed.len() <= 255
        && trimmed.to_ascii_lowercase().ends_with(".exe")
        && !trimmed.contains(['/', '\\', ':'])
        && trimmed != "."
        && trimmed != ".."
}

fn collect_profile_files(
    corpus_root: &Path,
    directory: &Path,
    depth: usize,
    output: &mut Vec<PathBuf>,
) -> Result<(), ProfileCorpusError> {
    if depth > MAX_DIRECTORY_DEPTH {
        return Err(ProfileCorpusError::TooDeep);
    }
    let metadata = fs::symlink_metadata(directory).map_err(ProfileCorpusError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(ProfileCorpusError::UnsafeEntry);
    }
    for entry in fs::read_dir(directory).map_err(ProfileCorpusError::Io)? {
        let entry = entry.map_err(ProfileCorpusError::Io)?;
        let path = entry.path();
        if !path.starts_with(corpus_root) {
            return Err(ProfileCorpusError::UnsafeEntry);
        }
        let kind = entry.file_type().map_err(ProfileCorpusError::Io)?;
        if kind.is_symlink() {
            return Err(ProfileCorpusError::UnsafeEntry);
        }
        if kind.is_dir() {
            collect_profile_files(corpus_root, &path, depth + 1, output)?;
        } else if kind.is_file() && entry.file_name() == "profile.json" {
            output.push(path);
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ProfileCorpusError {
    #[error("profile corpus root is missing")]
    MissingRoot,
    #[error("profile corpus contains an unsafe filesystem entry")]
    UnsafeEntry,
    #[error("profile corpus nesting exceeds the supported limit")]
    TooDeep,
    #[error("a profile exceeds the supported size limit")]
    TooLarge,
    #[error("a game profile failed validation")]
    InvalidProfile,
    #[error("profile corpus contains a duplicate profile id")]
    DuplicateId,
    #[error("profile corpus must contain exactly {expected} profiles; found {actual}")]
    WrongCount { expected: usize, actual: usize },
    #[error("profile corpus could not be read")]
    Io(#[source] std::io::Error),
}
