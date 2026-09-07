use npc_game_profile::GameProfileV2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::NamedTempFile;

const OVERRIDE_SCHEMA_VERSION: u32 = 1;
const OVERRIDE_FILE_NAME: &str = "character-content-overrides-v1.json";
const MAX_OVERRIDE_FILE_BYTES: usize = 512 * 1024;
const MAX_DISPLAY_NAME_BYTES: usize = 160;
const MAX_BIOGRAPHY_BYTES: usize = 16 * 1024;
const MAX_PROMPT_CONTEXT_BYTES: usize = 1024;
pub(crate) const MAX_EFFECTIVE_PROFILE_BYTES: usize = 768 * 1024;
const PLAYER_CONTEXT_PREFIX: &str = "Player-authored context: ";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharacterContentOverrideV1 {
    pub game_profile_id: String,
    pub character_id: String,
    pub display_name: String,
    pub biography: String,
    #[serde(default)]
    pub prompt_context: String,
    pub updated_at_unix_millis: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveCharacterContentOverrideRequestV1 {
    pub game_profile_id: String,
    pub character_id: String,
    pub display_name: String,
    pub biography: String,
    #[serde(default)]
    pub prompt_context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharacterContentOverrideScopeV1 {
    pub game_profile_id: String,
    pub character_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterContentOverrideSnapshotV1 {
    pub schema_version: u32,
    pub game_profile_id: String,
    pub character_id: String,
    pub saved: Option<CharacterContentOverrideV1>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterContentOverrideMutationV1 {
    pub schema_version: u32,
    pub game_profile_id: String,
    pub character_id: String,
    pub saved: Option<CharacterContentOverrideV1>,
    pub persisted: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CharacterContentOverrideFileV1 {
    schema_version: u32,
    overrides: BTreeMap<String, BTreeMap<String, CharacterContentOverrideV1>>,
}

impl Default for CharacterContentOverrideFileV1 {
    fn default() -> Self {
        Self {
            schema_version: OVERRIDE_SCHEMA_VERSION,
            overrides: BTreeMap::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CharacterContentOverrideError {
    #[error("character content override is invalid: {0}")]
    Invalid(String),
    #[error("character content override state could not be read or persisted: {0}")]
    Persistence(String),
}

#[derive(Debug)]
pub struct CharacterContentOverrideManagerV1 {
    path: PathBuf,
    mutation: Mutex<()>,
}

impl CharacterContentOverrideManagerV1 {
    pub fn new(config_directory: &Path) -> Self {
        Self {
            path: config_directory.join(OVERRIDE_FILE_NAME),
            mutation: Mutex::new(()),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn snapshot(
        &self,
        scope: CharacterContentOverrideScopeV1,
    ) -> Result<CharacterContentOverrideSnapshotV1, CharacterContentOverrideError> {
        validate_identifier("game profile ID", &scope.game_profile_id)?;
        validate_identifier("character ID", &scope.character_id)?;
        let document = load_document(&self.path)?;
        let saved = document
            .overrides
            .get(&scope.game_profile_id)
            .and_then(|characters| characters.get(&scope.character_id))
            .cloned();
        Ok(CharacterContentOverrideSnapshotV1 {
            schema_version: OVERRIDE_SCHEMA_VERSION,
            game_profile_id: scope.game_profile_id,
            character_id: scope.character_id,
            detail: if saved.is_some() {
                "This player-authored character layer is active above the current content pack."
                    .into()
            } else {
                "No player-authored character layer is saved. The current content pack or bundled profile supplies these fields."
                    .into()
            },
            saved,
        })
    }

    pub fn save(
        &self,
        request: SaveCharacterContentOverrideRequestV1,
        base_profile: &GameProfileV2,
    ) -> Result<CharacterContentOverrideMutationV1, CharacterContentOverrideError> {
        validate_request(&request, base_profile)?;
        let _guard = self.mutation.lock().map_err(|_| {
            CharacterContentOverrideError::Persistence("mutation lock is unavailable".into())
        })?;
        let mut document = load_document(&self.path)?;
        let saved = CharacterContentOverrideV1 {
            game_profile_id: request.game_profile_id.clone(),
            character_id: request.character_id.clone(),
            display_name: request.display_name.trim().to_owned(),
            biography: request.biography.trim().to_owned(),
            prompt_context: request.prompt_context.trim().to_owned(),
            updated_at_unix_millis: now_unix_millis()?,
        };
        document
            .overrides
            .entry(request.game_profile_id.clone())
            .or_default()
            .insert(request.character_id.clone(), saved.clone());
        validate_document(&document)?;
        atomic_write_json(&self.path, &document)?;
        Ok(CharacterContentOverrideMutationV1 {
            schema_version: OVERRIDE_SCHEMA_VERSION,
            game_profile_id: request.game_profile_id,
            character_id: request.character_id,
            saved: Some(saved),
            persisted: true,
            detail: "Player-authored name, biography, and prompt context saved above the current pack. Stable identity, memory, voice, and provider choices were unchanged."
                .into(),
        })
    }

    pub fn reset(
        &self,
        scope: CharacterContentOverrideScopeV1,
    ) -> Result<CharacterContentOverrideMutationV1, CharacterContentOverrideError> {
        validate_identifier("game profile ID", &scope.game_profile_id)?;
        validate_identifier("character ID", &scope.character_id)?;
        let _guard = self.mutation.lock().map_err(|_| {
            CharacterContentOverrideError::Persistence("mutation lock is unavailable".into())
        })?;
        let mut document = load_document(&self.path)?;
        if let Some(characters) = document.overrides.get_mut(&scope.game_profile_id) {
            characters.remove(&scope.character_id);
            if characters.is_empty() {
                document.overrides.remove(&scope.game_profile_id);
            }
        }
        atomic_write_json(&self.path, &document)?;
        Ok(CharacterContentOverrideMutationV1 {
            schema_version: OVERRIDE_SCHEMA_VERSION,
            game_profile_id: scope.game_profile_id,
            character_id: scope.character_id,
            saved: None,
            persisted: true,
            detail: "Player-authored character fields reset. The current content pack or bundled profile is visible again; identity, memory, voice, and provider choices were unchanged."
                .into(),
        })
    }
}

pub(crate) fn apply_saved_overrides(
    path: &Path,
    profile: &mut GameProfileV2,
) -> Result<(), CharacterContentOverrideError> {
    let document = load_document(path)?;
    let Some(characters) = document.overrides.get(&profile.id) else {
        return Ok(());
    };
    for character in &mut profile.characters {
        let Some(saved) = characters.get(&character.id) else {
            continue;
        };
        character.display_name.clone_from(&saved.display_name);
        character.biography.clone_from(&saved.biography);
        if !saved.prompt_context.is_empty() {
            character
                .prompt
                .objectives
                .push(format!("{PLAYER_CONTEXT_PREFIX}{}", saved.prompt_context));
        }
    }
    let report = profile.validate();
    let serialized_size = serde_json::to_vec(profile)
        .map_err(|error| CharacterContentOverrideError::Invalid(error.to_string()))?
        .len();
    if !report.is_valid() || serialized_size > MAX_EFFECTIVE_PROFILE_BYTES {
        return Err(CharacterContentOverrideError::Invalid(
            "the saved layer no longer composes within the current profile and runtime-message limits"
                .into(),
        ));
    }
    Ok(())
}

fn validate_request(
    request: &SaveCharacterContentOverrideRequestV1,
    base_profile: &GameProfileV2,
) -> Result<(), CharacterContentOverrideError> {
    validate_identifier("game profile ID", &request.game_profile_id)?;
    validate_identifier("character ID", &request.character_id)?;
    validate_required_text(
        "display name",
        &request.display_name,
        MAX_DISPLAY_NAME_BYTES,
    )?;
    validate_required_text("biography", &request.biography, MAX_BIOGRAPHY_BYTES)?;
    validate_optional_text(
        "prompt context",
        &request.prompt_context,
        MAX_PROMPT_CONTEXT_BYTES,
    )?;
    if base_profile.id != request.game_profile_id {
        return Err(CharacterContentOverrideError::Invalid(
            "game profile ID does not match the current profile".into(),
        ));
    }
    let Some(character) = base_profile
        .characters
        .iter()
        .find(|character| character.id == request.character_id)
    else {
        return Err(CharacterContentOverrideError::Invalid(
            "character ID is not present in the current profile".into(),
        ));
    };
    if !request.prompt_context.trim().is_empty() && character.prompt.objectives.len() >= 64 {
        return Err(CharacterContentOverrideError::Invalid(
            "the current profile has no room for another prompt objective".into(),
        ));
    }
    let mut composed = base_profile.clone();
    let target = composed
        .characters
        .iter_mut()
        .find(|candidate| candidate.id == request.character_id)
        .expect("validated stable character ID");
    target.display_name = request.display_name.trim().to_owned();
    target.biography = request.biography.trim().to_owned();
    if !request.prompt_context.trim().is_empty() {
        target.prompt.objectives.push(format!(
            "{PLAYER_CONTEXT_PREFIX}{}",
            request.prompt_context.trim()
        ));
    }
    let serialized_size = serde_json::to_vec(&composed)
        .map_err(|error| CharacterContentOverrideError::Invalid(error.to_string()))?
        .len();
    if !composed.validate().is_valid() || serialized_size > MAX_EFFECTIVE_PROFILE_BYTES {
        return Err(CharacterContentOverrideError::Invalid(
            "these fields do not satisfy the current game-profile and runtime-message limits"
                .into(),
        ));
    }
    Ok(())
}

fn load_document(
    path: &Path,
) -> Result<CharacterContentOverrideFileV1, CharacterContentOverrideError> {
    if !path.exists() {
        return Ok(CharacterContentOverrideFileV1::default());
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| CharacterContentOverrideError::Persistence(error.to_string()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_OVERRIDE_FILE_BYTES as u64
    {
        return Err(CharacterContentOverrideError::Persistence(
            "override record is linked, oversized, or not a regular file".into(),
        ));
    }
    let bytes = fs::read(path)
        .map_err(|error| CharacterContentOverrideError::Persistence(error.to_string()))?;
    let document: CharacterContentOverrideFileV1 = serde_json::from_slice(&bytes)
        .map_err(|error| CharacterContentOverrideError::Persistence(error.to_string()))?;
    validate_document(&document)?;
    Ok(document)
}

fn validate_document(
    document: &CharacterContentOverrideFileV1,
) -> Result<(), CharacterContentOverrideError> {
    if document.schema_version != OVERRIDE_SCHEMA_VERSION {
        return Err(CharacterContentOverrideError::Persistence(
            "unsupported character override schema".into(),
        ));
    }
    for (game_id, characters) in &document.overrides {
        validate_identifier("game profile ID", game_id)?;
        for (character_id, saved) in characters {
            validate_identifier("character ID", character_id)?;
            if saved.game_profile_id != *game_id || saved.character_id != *character_id {
                return Err(CharacterContentOverrideError::Persistence(
                    "override map keys do not match their stable IDs".into(),
                ));
            }
            validate_required_text("display name", &saved.display_name, MAX_DISPLAY_NAME_BYTES)?;
            validate_required_text("biography", &saved.biography, MAX_BIOGRAPHY_BYTES)?;
            validate_optional_text(
                "prompt context",
                &saved.prompt_context,
                MAX_PROMPT_CONTEXT_BYTES,
            )?;
        }
    }
    Ok(())
}

fn validate_identifier(field: &str, value: &str) -> Result<(), CharacterContentOverrideError> {
    let valid = !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric);
    if valid {
        Ok(())
    } else {
        Err(CharacterContentOverrideError::Invalid(format!(
            "{field} must be a lowercase stable ID"
        )))
    }
}

fn validate_required_text(
    field: &str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), CharacterContentOverrideError> {
    if value.trim().is_empty() || value.len() > maximum_bytes || value.contains('\0') {
        return Err(CharacterContentOverrideError::Invalid(format!(
            "{field} is empty or exceeds its {maximum_bytes}-byte limit"
        )));
    }
    Ok(())
}

fn validate_optional_text(
    field: &str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), CharacterContentOverrideError> {
    if value.len() > maximum_bytes || value.contains('\0') {
        return Err(CharacterContentOverrideError::Invalid(format!(
            "{field} exceeds its {maximum_bytes}-byte limit"
        )));
    }
    Ok(())
}

fn now_unix_millis() -> Result<u64, CharacterContentOverrideError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| CharacterContentOverrideError::Persistence(error.to_string()))?
        .as_millis()
        .try_into()
        .map_err(|_| CharacterContentOverrideError::Persistence("system clock overflow".into()))
}

fn atomic_write_json<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), CharacterContentOverrideError> {
    let directory = path.parent().ok_or_else(|| {
        CharacterContentOverrideError::Persistence("configuration directory is unavailable".into())
    })?;
    fs::create_dir_all(directory)
        .map_err(|error| CharacterContentOverrideError::Persistence(error.to_string()))?;
    let metadata = fs::symlink_metadata(directory)
        .map_err(|error| CharacterContentOverrideError::Persistence(error.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(CharacterContentOverrideError::Persistence(
            "configuration directory is linked or not a directory".into(),
        ));
    }
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| CharacterContentOverrideError::Persistence(error.to_string()))?;
    if bytes.len() > MAX_OVERRIDE_FILE_BYTES {
        return Err(CharacterContentOverrideError::Persistence(
            "character override state exceeds its storage limit".into(),
        ));
    }
    let mut temporary = NamedTempFile::new_in(directory)
        .map_err(|error| CharacterContentOverrideError::Persistence(error.to_string()))?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| CharacterContentOverrideError::Persistence(error.to_string()))?;
    temporary
        .persist(path)
        .map_err(|error| CharacterContentOverrideError::Persistence(error.error.to_string()))?;
    if let Ok(directory) = File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ResourceCatalog;
    use crate::content_packs::{
        ActivateContentPackRequestV1, ContentPackEnvelopeV1, ContentPackKindV1,
        ContentPackManagerV1, ContentPackRightsReviewV1, ContentPackRightsV1,
        InspectContentPackRequestV1,
    };

    fn pack_json(
        resources: &ResourceCatalog,
        version: &str,
        name: &str,
        biography: &str,
    ) -> String {
        let mut profile = resources
            .load_builtin_game_profile("cyberpunk-2077")
            .expect("bundled profile");
        profile.defaults.voice.user_override_allowed = true;
        for character in &mut profile.characters {
            character.voice.user_override_allowed = true;
        }
        let target = profile.characters.first_mut().expect("character");
        target.display_name = name.into();
        target.biography = biography.into();
        serde_json::to_string(&ContentPackEnvelopeV1 {
            format: "npc.content-pack".into(),
            schema_version: 1,
            namespace: "org.example".into(),
            pack_id: "night-city-authored".into(),
            version: version.into(),
            kind: ContentPackKindV1::GameBase,
            game_profile_id: "cyberpunk-2077".into(),
            title: format!("Night City authored context {version}"),
            summary: "Original data-only lore and character defaults.".into(),
            rights: ContentPackRightsV1 {
                license_name: "CC-BY-4.0".into(),
                source_summary: "Original authored text; no publisher media.".into(),
                redistributable: true,
                commercial_use: true,
                derivative_use: true,
                review_status: ContentPackRightsReviewV1::Approved,
            },
            provider_recommendations: Vec::new(),
            profile,
        })
        .expect("pack JSON")
    }

    fn activate(manager: &ContentPackManagerV1, resources: &ResourceCatalog, json_text: String) {
        let preview = manager
            .inspect(
                InspectContentPackRequestV1 {
                    json_text: json_text.clone(),
                },
                resources,
            )
            .expect("preview");
        manager
            .activate(
                ActivateContentPackRequestV1 {
                    json_text,
                    expected_content_sha256: preview.content_sha256,
                },
                resources,
            )
            .expect("activate");
    }

    #[test]
    fn pack_update_preserves_override_and_reset_reveals_updated_pack_values() {
        let directory = tempfile::tempdir().expect("tempdir");
        let content_packs = ContentPackManagerV1::new(directory.path());
        let overrides = CharacterContentOverrideManagerV1::new(directory.path());
        let resources = ResourceCatalog::with_user_content_layers(
            None,
            content_packs.active_profile_root(),
            overrides.path(),
        );
        let provider_sentinel = directory.path().join("provider-loadouts-v1.json");
        fs::write(&provider_sentinel, b"player-provider-route").expect("provider sentinel");
        let first_pack_name = "Pack V1 Resident";
        let first_pack_biography = "First pack biography for the stable test resident.";
        activate(
            &content_packs,
            &resources,
            pack_json(&resources, "1.0.0", first_pack_name, first_pack_biography),
        );
        let stable_id = resources
            .load_game_profile_without_character_overrides("cyberpunk-2077")
            .expect("active profile")
            .characters
            .first()
            .expect("character")
            .id
            .clone();
        overrides
            .save(
                SaveCharacterContentOverrideRequestV1 {
                    game_profile_id: "cyberpunk-2077".into(),
                    character_id: stable_id.clone(),
                    display_name: "My Night City Contact".into(),
                    biography: "My persistent player-authored backstory.".into(),
                    prompt_context: "Treat the player as a trusted returning client.".into(),
                },
                &resources
                    .load_game_profile_without_character_overrides("cyberpunk-2077")
                    .expect("base profile"),
            )
            .expect("save override");

        let second_pack_name = "Pack V2 Resident";
        let second_pack_biography = "Second pack biography which should appear after reset.";
        activate(
            &content_packs,
            &resources,
            pack_json(&resources, "2.0.0", second_pack_name, second_pack_biography),
        );
        let effective = resources
            .load_game_profile("cyberpunk-2077")
            .expect("effective profile");
        let character = effective
            .characters
            .iter()
            .find(|candidate| candidate.id == stable_id)
            .expect("effective character");
        assert_eq!(character.display_name, "My Night City Contact");
        assert_eq!(
            character.biography,
            "My persistent player-authored backstory."
        );
        assert!(character.prompt.objectives.iter().any(|objective| {
            objective == "Player-authored context: Treat the player as a trusted returning client."
        }));

        overrides
            .reset(CharacterContentOverrideScopeV1 {
                game_profile_id: "cyberpunk-2077".into(),
                character_id: stable_id.clone(),
            })
            .expect("reset override");
        let reset = resources
            .load_game_profile("cyberpunk-2077")
            .expect("reset profile");
        let character = reset
            .characters
            .iter()
            .find(|candidate| candidate.id == stable_id)
            .expect("reset character");
        assert_eq!(character.display_name, second_pack_name);
        assert_eq!(character.biography, second_pack_biography);
        assert!(!character
            .prompt
            .objectives
            .iter()
            .any(|objective| objective.starts_with(PLAYER_CONTEXT_PREFIX)));
        assert_eq!(
            fs::read(provider_sentinel).expect("provider sentinel"),
            b"player-provider-route"
        );
    }

    #[test]
    fn rejects_unknown_ids_oversized_context_and_linked_state() {
        let directory = tempfile::tempdir().expect("tempdir");
        let manager = CharacterContentOverrideManagerV1::new(directory.path());
        let resources = ResourceCatalog::new(None);
        let base = resources
            .load_builtin_game_profile("cyberpunk-2077")
            .expect("profile");
        let target = base.characters.first().expect("character");
        let error = manager
            .save(
                SaveCharacterContentOverrideRequestV1 {
                    game_profile_id: "cyberpunk-2077".into(),
                    character_id: "unknown-character".into(),
                    display_name: "Unknown".into(),
                    biography: "Unknown character.".into(),
                    prompt_context: String::new(),
                },
                &base,
            )
            .expect_err("unknown ID rejected");
        assert!(error.to_string().contains("not present"));

        let error = manager
            .save(
                SaveCharacterContentOverrideRequestV1 {
                    game_profile_id: "cyberpunk-2077".into(),
                    character_id: target.id.clone(),
                    display_name: "Valid name".into(),
                    biography: "Valid biography.".into(),
                    prompt_context: "x".repeat(MAX_PROMPT_CONTEXT_BYTES + 1),
                },
                &base,
            )
            .expect_err("oversized context rejected");
        assert!(error.to_string().contains("1024-byte"));
    }
}
