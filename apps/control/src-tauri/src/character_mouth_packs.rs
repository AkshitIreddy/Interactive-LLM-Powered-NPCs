use crate::identity_runtime::NativeSelectedActorLockV1;
use crate::private_directory::ensure_private_directory;
use crate::visual_runtime::{validate_character_mouth_atlas_root, ValidatedCharacterMouthAtlasV1};
use npc_game_profile::GameProfileV2;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::NamedTempFile;

const REGISTRY_SCHEMA_VERSION: u32 = 1;
const REGISTRY_FILE_NAME: &str = "registry-v1.json";
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_TEXTURE_BYTES: usize = 16 * 1024 * 1024;
const MAX_BASE64_BYTES: usize = MAX_TEXTURE_BYTES.div_ceil(3) * 4;
const MAX_INSTALLED_REVISIONS: usize = 256;
const MAX_ENABLED_BINDINGS: usize = 64;

#[derive(Clone, Debug)]
pub(crate) struct SelectedCharacterMouthAuthorityV1 {
    game_profile_id: String,
    character_id: String,
}

impl SelectedCharacterMouthAuthorityV1 {
    pub(crate) fn from_persisted_selection(
        profile: &GameProfileV2,
        selected_character_id: Option<&str>,
    ) -> Result<Self, CharacterMouthPackError> {
        let selected_character_id =
            selected_character_id.ok_or(CharacterMouthPackError::Authority(
                "select a character before managing its mouth pack".into(),
            ))?;
        let character_exists = profile
            .characters
            .iter()
            .any(|character| character.id == selected_character_id);
        if !character_exists {
            return Err(CharacterMouthPackError::Authority(
                "the persisted selected character is absent from the active game profile".into(),
            ));
        }
        Ok(Self {
            game_profile_id: profile.id.clone(),
            character_id: selected_character_id.to_owned(),
        })
    }

    pub(crate) fn game_profile_id(&self) -> &str {
        &self.game_profile_id
    }

    pub(crate) fn character_id(&self) -> &str {
        &self.character_id
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CurrentCharacterMouthEnableAuthorityV1 {
    selected: SelectedCharacterMouthAuthorityV1,
}

impl CurrentCharacterMouthEnableAuthorityV1 {
    pub(crate) fn from_current_actor(
        selected: SelectedCharacterMouthAuthorityV1,
        lock: &NativeSelectedActorLockV1,
        now_unix_ms: u64,
    ) -> Result<Self, CharacterMouthPackError> {
        if selected.game_profile_id() != lock.game_profile_id
            || selected.character_id() != lock.character_id
            || lock.runtime_actor_id == 0
            || lock.expires_at_unix_ms <= now_unix_ms
        {
            return Err(CharacterMouthPackError::Authority(
                "the selected character and current native actor lock do not match".into(),
            ));
        }
        Ok(Self { selected })
    }

    fn selected(&self) -> &SelectedCharacterMouthAuthorityV1 {
        &self.selected
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharacterMouthPackFilesV1 {
    pub game_profile_id: String,
    pub atlas_json_text: String,
    pub texture_file_name: String,
    pub texture_base64: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportCharacterMouthPackRequestV1 {
    pub files: CharacterMouthPackFilesV1,
    pub expected_content_sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnableCharacterMouthPackRequestV1 {
    pub game_profile_id: String,
    pub expected_content_sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisableCharacterMouthPackRequestV1 {
    pub game_profile_id: String,
    pub expected_content_sha256: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterMouthPackPreviewV1 {
    pub schema_version: u32,
    pub game_profile_id: String,
    pub character_id: String,
    pub atlas_schema_version: u32,
    pub identity_revision: u64,
    pub manifest_sha256: String,
    pub texture_file_name: String,
    pub texture_size_bytes: usize,
    pub content_sha256: String,
    pub enrollment_binding_sha256: String,
    pub private_review_binding_validated: bool,
    pub network_request_performed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstalledCharacterMouthPackV1 {
    pub game_profile_id: String,
    pub character_id: String,
    pub atlas_schema_version: u32,
    pub identity_revision: u64,
    pub manifest_sha256: String,
    pub texture_file_name: String,
    pub texture_size_bytes: usize,
    pub content_sha256: String,
    pub enrollment_binding_sha256: String,
    pub imported_at_unix_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnabledCharacterMouthPackV1 {
    pub game_profile_id: String,
    pub character_id: String,
    pub content_sha256: String,
    pub enrollment_binding_sha256: String,
    pub enabled_at_unix_ms: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DisabledCharacterMouthPackV1 {
    pub game_profile_id: String,
    pub character_id: String,
    pub content_sha256: String,
    pub disabled: bool,
    pub retained_installed_revision: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterMouthPackStateV1 {
    pub schema_version: u32,
    pub installed: Vec<InstalledCharacterMouthPackV1>,
    pub enabled: Vec<EnabledCharacterMouthPackV1>,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedCharacterMouthPackV1 {
    pub root: PathBuf,
    pub identity_revision: u64,
    pub content_sha256: String,
    pub enrollment_binding_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CharacterMouthPackRegistryV1 {
    schema_version: u32,
    installed: Vec<InstalledCharacterMouthPackV1>,
    enabled: Vec<EnabledCharacterMouthPackV1>,
}

impl Default for CharacterMouthPackRegistryV1 {
    fn default() -> Self {
        Self {
            schema_version: REGISTRY_SCHEMA_VERSION,
            installed: Vec::new(),
            enabled: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct CharacterMouthPackManagerV1 {
    root: PathBuf,
    registry_path: PathBuf,
    registry: Mutex<CharacterMouthPackRegistryV1>,
    mutation: Mutex<()>,
}

impl CharacterMouthPackManagerV1 {
    pub(crate) fn new(config_directory: &Path) -> Result<Self, CharacterMouthPackError> {
        let root = ensure_private_directory(&config_directory.join("character-mouth-packs-v1"))
            .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
        let registry_path = root.join(REGISTRY_FILE_NAME);
        let registry = load_registry(&registry_path)?;
        Ok(Self {
            root,
            registry_path,
            registry: Mutex::new(registry),
            mutation: Mutex::new(()),
        })
    }

    pub(crate) fn inspect(
        &self,
        authority: &SelectedCharacterMouthAuthorityV1,
        files: &CharacterMouthPackFilesV1,
    ) -> Result<CharacterMouthPackPreviewV1, CharacterMouthPackError> {
        if files.game_profile_id != authority.game_profile_id() {
            return Err(CharacterMouthPackError::Authority(
                "the requested game does not match the active selected-character profile".into(),
            ));
        }
        validate_file_name(&files.texture_file_name)?;
        if files.atlas_json_text.is_empty() || files.atlas_json_text.len() > MAX_MANIFEST_BYTES {
            return Err(CharacterMouthPackError::Size);
        }
        let (declared_texture_file, declared_texture_bytes) =
            declared_texture_authority(&files.atlas_json_text)?;
        if declared_texture_file != files.texture_file_name {
            return Err(CharacterMouthPackError::Invalid(
                "the selected texture file does not match atlas.json".into(),
            ));
        }
        let texture = decode_texture_base64(&files.texture_base64, declared_texture_bytes)?;
        let staging = tempfile::tempdir_in(&self.root)
            .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
        write_new_file(
            &staging.path().join("atlas.json"),
            files.atlas_json_text.as_bytes(),
        )?;
        write_new_file(&staging.path().join(&files.texture_file_name), &texture)?;
        let atlas = validate_character_mouth_atlas_root(
            staging.path(),
            authority.game_profile_id(),
            authority.character_id(),
        )
        .map_err(|error| CharacterMouthPackError::Invalid(error.to_string()))?;
        Ok(preview_from_atlas(authority, &atlas, texture.len()))
    }

    pub(crate) fn import(
        &self,
        authority: &SelectedCharacterMouthAuthorityV1,
        request: ImportCharacterMouthPackRequestV1,
        now_unix_ms: u64,
    ) -> Result<InstalledCharacterMouthPackV1, CharacterMouthPackError> {
        let preview = self.inspect(authority, &request.files)?;
        if !is_lowercase_sha256(&request.expected_content_sha256)
            || preview.content_sha256 != request.expected_content_sha256
        {
            return Err(CharacterMouthPackError::DigestMismatch);
        }
        let _mutation = self
            .mutation
            .lock()
            .map_err(|_| CharacterMouthPackError::State)?;
        let (_, declared_texture_bytes) =
            declared_texture_authority(&request.files.atlas_json_text)?;
        let texture = decode_texture_base64(&request.files.texture_base64, declared_texture_bytes)?;
        let parent = ensure_private_directory(
            &self
                .root
                .join("revisions")
                .join(authority.game_profile_id())
                .join(authority.character_id()),
        )
        .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
        if !parent.starts_with(&self.root) {
            return Err(CharacterMouthPackError::UnsafePath);
        }
        {
            let registry = self
                .registry
                .lock()
                .map_err(|_| CharacterMouthPackError::State)?;
            let already_indexed = registry.installed.iter().any(|entry| {
                entry.game_profile_id == preview.game_profile_id
                    && entry.character_id == preview.character_id
                    && entry.content_sha256 == preview.content_sha256
            });
            if !already_indexed && registry.installed.len() >= MAX_INSTALLED_REVISIONS {
                return Err(CharacterMouthPackError::RegistryLimit);
            }
        }
        let destination = parent.join(&preview.content_sha256);
        if destination.exists() {
            validate_existing_revision(&destination, &preview)?;
        } else {
            let staging = tempfile::tempdir_in(&parent)
                .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
            write_new_file(
                &staging.path().join("atlas.json"),
                request.files.atlas_json_text.as_bytes(),
            )?;
            write_new_file(
                &staging.path().join(&request.files.texture_file_name),
                &texture,
            )?;
            validate_existing_revision(staging.path(), &preview)?;
            let staging_path = staging.keep();
            fs::rename(&staging_path, &destination)
                .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
            if let Ok(directory) = File::open(&parent) {
                let _ = directory.sync_all();
            }
        }
        let installed = InstalledCharacterMouthPackV1 {
            game_profile_id: preview.game_profile_id,
            character_id: preview.character_id,
            atlas_schema_version: preview.atlas_schema_version,
            identity_revision: preview.identity_revision,
            manifest_sha256: preview.manifest_sha256,
            texture_file_name: preview.texture_file_name,
            texture_size_bytes: preview.texture_size_bytes,
            content_sha256: preview.content_sha256,
            enrollment_binding_sha256: preview.enrollment_binding_sha256,
            imported_at_unix_ms: now_unix_ms,
        };
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| CharacterMouthPackError::State)?;
        let mut candidate = registry.clone();
        candidate.installed.retain(|entry| {
            entry.game_profile_id != installed.game_profile_id
                || entry.character_id != installed.character_id
                || entry.content_sha256 != installed.content_sha256
        });
        candidate.installed.push(installed.clone());
        sort_registry(&mut candidate);
        atomic_write_registry(&self.registry_path, &candidate)?;
        *registry = candidate;
        Ok(installed)
    }

    pub(crate) fn enable(
        &self,
        authority: &CurrentCharacterMouthEnableAuthorityV1,
        request: EnableCharacterMouthPackRequestV1,
        now_unix_ms: u64,
    ) -> Result<EnabledCharacterMouthPackV1, CharacterMouthPackError> {
        let selected = authority.selected();
        if request.game_profile_id != selected.game_profile_id() {
            return Err(CharacterMouthPackError::Authority(
                "the enable request does not match the selected game profile".into(),
            ));
        }
        if !is_lowercase_sha256(&request.expected_content_sha256) {
            return Err(CharacterMouthPackError::DigestMismatch);
        }
        let _mutation = self
            .mutation
            .lock()
            .map_err(|_| CharacterMouthPackError::State)?;
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| CharacterMouthPackError::State)?;
        let installed = registry
            .installed
            .iter()
            .find(|entry| {
                entry.game_profile_id == selected.game_profile_id()
                    && entry.character_id == selected.character_id()
                    && entry.content_sha256 == request.expected_content_sha256
            })
            .cloned()
            .ok_or(CharacterMouthPackError::NotInstalled)?;
        let root = self.revision_root(&installed);
        validate_existing_revision(&root, &preview_from_installed(&installed))?;
        let enabled = EnabledCharacterMouthPackV1 {
            game_profile_id: installed.game_profile_id,
            character_id: installed.character_id,
            content_sha256: installed.content_sha256,
            enrollment_binding_sha256: installed.enrollment_binding_sha256,
            enabled_at_unix_ms: now_unix_ms,
        };
        let mut candidate = registry.clone();
        candidate.enabled.retain(|entry| {
            entry.game_profile_id != enabled.game_profile_id
                || entry.character_id != enabled.character_id
        });
        if candidate.enabled.len() >= MAX_ENABLED_BINDINGS {
            return Err(CharacterMouthPackError::RegistryLimit);
        }
        candidate.enabled.push(enabled.clone());
        sort_registry(&mut candidate);
        atomic_write_registry(&self.registry_path, &candidate)?;
        *registry = candidate;
        Ok(enabled)
    }

    pub(crate) fn disable(
        &self,
        authority: &SelectedCharacterMouthAuthorityV1,
        request: DisableCharacterMouthPackRequestV1,
    ) -> Result<DisabledCharacterMouthPackV1, CharacterMouthPackError> {
        if request.game_profile_id != authority.game_profile_id() {
            return Err(CharacterMouthPackError::Authority(
                "the disable request does not match the selected game profile".into(),
            ));
        }
        if !is_lowercase_sha256(&request.expected_content_sha256) {
            return Err(CharacterMouthPackError::DigestMismatch);
        }
        let _mutation = self
            .mutation
            .lock()
            .map_err(|_| CharacterMouthPackError::State)?;
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| CharacterMouthPackError::State)?;
        let active = registry
            .enabled
            .iter()
            .find(|entry| {
                entry.game_profile_id == authority.game_profile_id()
                    && entry.character_id == authority.character_id()
            })
            .cloned()
            .ok_or(CharacterMouthPackError::NotEnabled)?;
        if active.content_sha256 != request.expected_content_sha256 {
            return Err(CharacterMouthPackError::DigestMismatch);
        }
        let mut candidate = registry.clone();
        candidate.enabled.retain(|entry| {
            entry.game_profile_id != authority.game_profile_id()
                || entry.character_id != authority.character_id()
        });
        atomic_write_registry(&self.registry_path, &candidate)?;
        *registry = candidate;
        Ok(DisabledCharacterMouthPackV1 {
            game_profile_id: active.game_profile_id,
            character_id: active.character_id,
            content_sha256: active.content_sha256,
            disabled: true,
            retained_installed_revision: true,
        })
    }

    pub(crate) fn resolve_enabled(
        &self,
        game_profile_id: &str,
        character_id: &str,
    ) -> Result<Option<ResolvedCharacterMouthPackV1>, CharacterMouthPackError> {
        let registry = self
            .registry
            .lock()
            .map_err(|_| CharacterMouthPackError::State)?;
        let Some(enabled) = registry.enabled.iter().find(|entry| {
            entry.game_profile_id == game_profile_id && entry.character_id == character_id
        }) else {
            return Ok(None);
        };
        let installed = registry
            .installed
            .iter()
            .find(|entry| {
                entry.game_profile_id == game_profile_id
                    && entry.character_id == character_id
                    && entry.content_sha256 == enabled.content_sha256
                    && entry.enrollment_binding_sha256 == enabled.enrollment_binding_sha256
            })
            .ok_or(CharacterMouthPackError::State)?;
        let root = self.revision_root(installed);
        let metadata = fs::symlink_metadata(&root)
            .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
        let canonical = root
            .canonicalize()
            .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || !canonical.starts_with(&self.root)
        {
            return Err(CharacterMouthPackError::UnsafePath);
        }
        Ok(Some(ResolvedCharacterMouthPackV1 {
            root: canonical,
            identity_revision: installed.identity_revision,
            content_sha256: enabled.content_sha256.clone(),
            enrollment_binding_sha256: enabled.enrollment_binding_sha256.clone(),
        }))
    }

    pub(crate) fn state(&self) -> Result<CharacterMouthPackStateV1, CharacterMouthPackError> {
        let registry = self
            .registry
            .lock()
            .map_err(|_| CharacterMouthPackError::State)?;
        Ok(CharacterMouthPackStateV1 {
            schema_version: REGISTRY_SCHEMA_VERSION,
            installed: registry.installed.clone(),
            enabled: registry.enabled.clone(),
            detail: format!(
                "{} private mouth-pack revision{} installed; {} explicitly enabled character binding{}.",
                registry.installed.len(),
                if registry.installed.len() == 1 { "" } else { "s" },
                registry.enabled.len(),
                if registry.enabled.len() == 1 { "" } else { "s" }
            ),
        })
    }

    fn revision_root(&self, installed: &InstalledCharacterMouthPackV1) -> PathBuf {
        self.root
            .join("revisions")
            .join(&installed.game_profile_id)
            .join(&installed.character_id)
            .join(&installed.content_sha256)
    }
}

fn preview_from_atlas(
    authority: &SelectedCharacterMouthAuthorityV1,
    atlas: &ValidatedCharacterMouthAtlasV1,
    texture_size_bytes: usize,
) -> CharacterMouthPackPreviewV1 {
    CharacterMouthPackPreviewV1 {
        schema_version: 1,
        game_profile_id: authority.game_profile_id().to_owned(),
        character_id: authority.character_id().to_owned(),
        atlas_schema_version: atlas.schema_version,
        identity_revision: atlas.identity_revision,
        manifest_sha256: atlas.manifest_sha256.clone(),
        texture_file_name: atlas.texture_file_name.clone(),
        texture_size_bytes,
        content_sha256: atlas.content_sha256.clone(),
        enrollment_binding_sha256: atlas.enrollment_binding_sha256.clone(),
        private_review_binding_validated: true,
        network_request_performed: false,
    }
}

fn preview_from_installed(
    installed: &InstalledCharacterMouthPackV1,
) -> CharacterMouthPackPreviewV1 {
    CharacterMouthPackPreviewV1 {
        schema_version: 1,
        game_profile_id: installed.game_profile_id.clone(),
        character_id: installed.character_id.clone(),
        atlas_schema_version: installed.atlas_schema_version,
        identity_revision: installed.identity_revision,
        manifest_sha256: installed.manifest_sha256.clone(),
        texture_file_name: installed.texture_file_name.clone(),
        texture_size_bytes: installed.texture_size_bytes,
        content_sha256: installed.content_sha256.clone(),
        enrollment_binding_sha256: installed.enrollment_binding_sha256.clone(),
        private_review_binding_validated: true,
        network_request_performed: false,
    }
}

fn validate_existing_revision(
    root: &Path,
    expected: &CharacterMouthPackPreviewV1,
) -> Result<(), CharacterMouthPackError> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(CharacterMouthPackError::UnsafePath);
    }
    let atlas = validate_character_mouth_atlas_root(
        root,
        &expected.game_profile_id,
        &expected.character_id,
    )
    .map_err(|error| CharacterMouthPackError::Invalid(error.to_string()))?;
    let texture_metadata = fs::symlink_metadata(root.join(&atlas.texture_file_name))
        .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
    if atlas.schema_version != expected.atlas_schema_version
        || atlas.identity_revision != expected.identity_revision
        || atlas.manifest_sha256 != expected.manifest_sha256
        || atlas.texture_file_name != expected.texture_file_name
        || texture_metadata.len() != expected.texture_size_bytes as u64
        || atlas.content_sha256 != expected.content_sha256
        || atlas.enrollment_binding_sha256 != expected.enrollment_binding_sha256
    {
        return Err(CharacterMouthPackError::DigestMismatch);
    }
    Ok(())
}

fn validate_file_name(value: &str) -> Result<(), CharacterMouthPackError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 128
        || path.components().count() != 1
        || path.extension().and_then(|value| value.to_str()) != Some("bin")
    {
        return Err(CharacterMouthPackError::Invalid(
            "texture name must be one bounded .bin file name".into(),
        ));
    }
    Ok(())
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), CharacterMouthPackError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))
}

fn load_registry(path: &Path) -> Result<CharacterMouthPackRegistryV1, CharacterMouthPackError> {
    if !path.exists() {
        return Ok(CharacterMouthPackRegistryV1::default());
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024 {
        return Err(CharacterMouthPackError::UnsafePath);
    }
    let bytes =
        fs::read(path).map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
    let registry: CharacterMouthPackRegistryV1 = serde_json::from_slice(&bytes)
        .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
    validate_registry(&registry)?;
    Ok(registry)
}

fn validate_registry(
    registry: &CharacterMouthPackRegistryV1,
) -> Result<(), CharacterMouthPackError> {
    let installed_unique = registry.installed.iter().enumerate().all(|(index, entry)| {
        !registry.installed[..index].iter().any(|previous| {
            previous.game_profile_id == entry.game_profile_id
                && previous.character_id == entry.character_id
                && previous.content_sha256 == entry.content_sha256
        })
    });
    let enabled_unique = registry.enabled.iter().enumerate().all(|(index, entry)| {
        !registry.enabled[..index].iter().any(|previous| {
            previous.game_profile_id == entry.game_profile_id
                && previous.character_id == entry.character_id
        })
    });
    let enabled_installed = registry.enabled.iter().all(|enabled| {
        registry.installed.iter().any(|installed| {
            installed.game_profile_id == enabled.game_profile_id
                && installed.character_id == enabled.character_id
                && installed.content_sha256 == enabled.content_sha256
                && installed.enrollment_binding_sha256 == enabled.enrollment_binding_sha256
        })
    });
    if registry.schema_version != REGISTRY_SCHEMA_VERSION
        || registry.installed.len() > MAX_INSTALLED_REVISIONS
        || registry.enabled.len() > MAX_ENABLED_BINDINGS
        || !installed_unique
        || !enabled_unique
        || !enabled_installed
        || registry.installed.iter().any(|entry| {
            !is_identifier(&entry.game_profile_id)
                || !is_identifier(&entry.character_id)
                || !is_lowercase_sha256(&entry.content_sha256)
                || !is_lowercase_sha256(&entry.manifest_sha256)
                || !is_lowercase_sha256(&entry.enrollment_binding_sha256)
                || validate_file_name(&entry.texture_file_name).is_err()
                || entry.texture_size_bytes == 0
                || entry.texture_size_bytes > MAX_TEXTURE_BYTES
        })
        || registry.enabled.iter().any(|entry| {
            !is_identifier(&entry.game_profile_id)
                || !is_identifier(&entry.character_id)
                || !is_lowercase_sha256(&entry.content_sha256)
                || !is_lowercase_sha256(&entry.enrollment_binding_sha256)
        })
    {
        return Err(CharacterMouthPackError::State);
    }
    Ok(())
}

fn sort_registry(registry: &mut CharacterMouthPackRegistryV1) {
    registry.installed.sort_by(|left, right| {
        (
            &left.game_profile_id,
            &left.character_id,
            &left.content_sha256,
        )
            .cmp(&(
                &right.game_profile_id,
                &right.character_id,
                &right.content_sha256,
            ))
    });
    registry.enabled.sort_by(|left, right| {
        (&left.game_profile_id, &left.character_id)
            .cmp(&(&right.game_profile_id, &right.character_id))
    });
}

fn atomic_write_registry(
    path: &Path,
    registry: &CharacterMouthPackRegistryV1,
) -> Result<(), CharacterMouthPackError> {
    validate_registry(registry)?;
    let bytes = serde_json::to_vec_pretty(registry)
        .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
    let directory = path.parent().ok_or(CharacterMouthPackError::UnsafePath)?;
    let mut temporary = NamedTempFile::new_in(directory)
        .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| CharacterMouthPackError::Persistence(error.to_string()))?;
    temporary
        .persist(path)
        .map_err(|error| CharacterMouthPackError::Persistence(error.error.to_string()))?;
    if let Ok(directory) = File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn declared_texture_authority(
    atlas_json_text: &str,
) -> Result<(String, usize), CharacterMouthPackError> {
    let document: serde_json::Value = serde_json::from_str(atlas_json_text)
        .map_err(|error| CharacterMouthPackError::Invalid(error.to_string()))?;
    let texture = document
        .as_object()
        .and_then(|root| root.get("texture"))
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| CharacterMouthPackError::Invalid("atlas texture is missing".into()))?;
    let file = texture
        .get("file")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| CharacterMouthPackError::Invalid("atlas texture file is missing".into()))?;
    validate_file_name(file)?;
    let integer = |field: &str| {
        texture
            .get(field)
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                CharacterMouthPackError::Invalid(format!(
                    "atlas texture {field} is missing or invalid"
                ))
            })
    };
    let width = integer("width")?;
    let height = integer("height")?;
    let stride = integer("strideBytes")?;
    let state_count = integer("stateCount")?;
    let state_bytes = integer("stateBytes")?;
    let dimensions_valid = (16..=512).contains(&width)
        && (16..=512).contains(&height)
        && stride >= width.saturating_mul(4)
        && (4..=16).contains(&state_count)
        && state_bytes == stride.checked_mul(height).unwrap_or_default();
    let total_bytes = usize::try_from(state_bytes)
        .ok()
        .and_then(|bytes| bytes.checked_mul(state_count as usize))
        .filter(|bytes| *bytes > 0 && *bytes <= MAX_TEXTURE_BYTES);
    if !dimensions_valid || total_bytes.is_none() {
        return Err(CharacterMouthPackError::Invalid(
            "atlas texture dimensions are invalid".into(),
        ));
    }
    Ok((file.to_owned(), total_bytes.unwrap_or_default()))
}

fn decode_texture_base64(
    value: &str,
    expected_decoded_bytes: usize,
) -> Result<Vec<u8>, CharacterMouthPackError> {
    let expected_encoded_bytes = expected_decoded_bytes
        .checked_add(2)
        .map(|bytes| (bytes / 3) * 4)
        .ok_or(CharacterMouthPackError::Size)?;
    if expected_decoded_bytes == 0
        || expected_decoded_bytes > MAX_TEXTURE_BYTES
        || value.is_empty()
        || value.len() > MAX_BASE64_BYTES
        || value.len() != expected_encoded_bytes
        || value.len() % 4 != 0
    {
        return Err(CharacterMouthPackError::Size);
    }
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(expected_decoded_bytes);
    for (group_index, group) in bytes.chunks_exact(4).enumerate() {
        let last = group_index + 1 == bytes.len() / 4;
        let padding = usize::from(group[3] == b'=') + usize::from(group[2] == b'=');
        if (!last && padding != 0) || padding > 2 || (group[2] == b'=' && group[3] != b'=') {
            return Err(CharacterMouthPackError::Base64);
        }
        let a = base64_value(group[0]).ok_or(CharacterMouthPackError::Base64)?;
        let b = base64_value(group[1]).ok_or(CharacterMouthPackError::Base64)?;
        let c = if group[2] == b'=' {
            0
        } else {
            base64_value(group[2]).ok_or(CharacterMouthPackError::Base64)?
        };
        let d = if group[3] == b'=' {
            0
        } else {
            base64_value(group[3]).ok_or(CharacterMouthPackError::Base64)?
        };
        if (padding == 2 && b & 0x0f != 0) || (padding == 1 && c & 0x03 != 0) {
            return Err(CharacterMouthPackError::Base64);
        }
        decoded.push((a << 2) | (b >> 4));
        if padding < 2 {
            decoded.push((b << 4) | (c >> 2));
        }
        if padding == 0 {
            decoded.push((c << 6) | d);
        }
        if decoded.len() > MAX_TEXTURE_BYTES {
            return Err(CharacterMouthPackError::Size);
        }
    }
    if decoded.len() != expected_decoded_bytes {
        return Err(CharacterMouthPackError::Size);
    }
    Ok(decoded)
}

fn base64_value(value: u8) -> Option<u8> {
    match value {
        b'A'..=b'Z' => Some(value - b'A'),
        b'a'..=b'z' => Some(value - b'a' + 26),
        b'0'..=b'9' => Some(value - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte))
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum CharacterMouthPackError {
    #[error("mouth-pack files are empty or exceed their local import limit")]
    Size,
    #[error("mouth-pack texture is not strict padded base64")]
    Base64,
    #[error("mouth-pack selection authority is invalid: {0}")]
    Authority(String),
    #[error("mouth-pack is invalid: {0}")]
    Invalid(String),
    #[error("mouth-pack no longer matches the reviewed digest")]
    DigestMismatch,
    #[error("mouth-pack revision is not installed for the selected character")]
    NotInstalled,
    #[error("mouth-pack revision is not enabled for the selected character")]
    NotEnabled,
    #[error("mouth-pack registry reached its bounded revision limit")]
    RegistryLimit,
    #[error("mouth-pack path is linked, escaped, or not a regular path")]
    UnsafePath,
    #[error("mouth-pack state is invalid")]
    State,
    #[error("mouth-pack state could not be persisted: {0}")]
    Persistence(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn encode_base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let a = chunk[0];
            let b = chunk.get(1).copied().unwrap_or_default();
            let c = chunk.get(2).copied().unwrap_or_default();
            encoded.push(ALPHABET[(a >> 2) as usize] as char);
            encoded.push(ALPHABET[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
            encoded.push(if chunk.len() > 1 {
                ALPHABET[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char
            } else {
                '='
            });
            encoded.push(if chunk.len() > 2 {
                ALPHABET[(c & 0x3f) as usize] as char
            } else {
                '='
            });
        }
        encoded
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn test_authority(character_id: &str) -> SelectedCharacterMouthAuthorityV1 {
        SelectedCharacterMouthAuthorityV1 {
            game_profile_id: "cyberpunk-2077".into(),
            character_id: character_id.into(),
        }
    }

    fn test_files() -> CharacterMouthPackFilesV1 {
        let state_bytes = 24 * 16 * 4;
        let mut pixels = vec![0_u8; state_bytes * 4];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[20, 40, 80, 255]);
        }
        let states = (0..4)
            .map(|index| {
                serde_json::json!({
                    "index": index,
                    "coefficients": [index as f64 / 3.0, 1.0 - index as f64 / 3.0,
                                      0.0, 0.0, 0.0, 0.0, 0.0, index as f64 / 3.0],
                    "enrolledPose": [0.0, 0.0, 0.0]
                })
            })
            .collect::<Vec<_>>();
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "identityRevision": 23,
            "enrollmentBinding": {
                "schemaVersion": 1,
                "gameProfileId": "cyberpunk-2077",
                "characterId": "misty",
                "referenceProvenanceSha256": ["b".repeat(64)],
                "reviewStatus": "reviewed-private",
                "reviewEvidenceSha256": "c".repeat(64)
            },
            "texture": {
                "file": "atlas.bin",
                "sha256": sha256_hex(&pixels),
                "representation": "normalized-oral-interior-v1",
                "width": 24,
                "height": 16,
                "strideBytes": 96,
                "stateCount": 4,
                "stateBytes": state_bytes
            },
            "states": states
        });
        CharacterMouthPackFilesV1 {
            game_profile_id: "cyberpunk-2077".into(),
            atlas_json_text: serde_json::to_string_pretty(&manifest).expect("manifest"),
            texture_file_name: "atlas.bin".into(),
            texture_base64: encode_base64(&pixels),
        }
    }

    #[test]
    fn strict_base64_decoder_rejects_noncanonical_and_oversized_input() {
        assert_eq!(
            decode_texture_base64("AAEC", 3).expect("base64"),
            vec![0, 1, 2]
        );
        assert_eq!(decode_texture_base64("AA==", 1).expect("base64"), vec![0]);
        assert!(matches!(
            decode_texture_base64("AB==", 1),
            Err(CharacterMouthPackError::Base64)
        ));
        assert!(matches!(
            decode_texture_base64("AA=A", 1),
            Err(CharacterMouthPackError::Base64)
        ));
        assert!(matches!(
            decode_texture_base64("AAEC\n", 3),
            Err(CharacterMouthPackError::Size)
        ));
    }

    #[test]
    fn offline_selection_authority_uses_the_active_profile_without_an_actor_lock() {
        let resources = crate::catalog::ResourceCatalog::new(None);
        let profile = resources
            .load_game_profile("cyberpunk-2077")
            .expect("bundled Cyberpunk profile");
        let selected = profile
            .characters
            .first()
            .expect("profile character")
            .id
            .as_str();
        let authority =
            SelectedCharacterMouthAuthorityV1::from_persisted_selection(&profile, Some(selected))
                .expect("offline selected-character authority");
        assert_eq!(authority.game_profile_id(), "cyberpunk-2077");
        assert_eq!(authority.character_id(), selected);
        assert!(matches!(
            SelectedCharacterMouthAuthorityV1::from_persisted_selection(&profile, None),
            Err(CharacterMouthPackError::Authority(_))
        ));
        assert!(matches!(
            SelectedCharacterMouthAuthorityV1::from_persisted_selection(
                &profile,
                Some("not-a-profile-character"),
            ),
            Err(CharacterMouthPackError::Authority(_))
        ));
    }

    #[test]
    fn registry_rejects_path_like_identity_and_unbounded_texture() {
        let mut registry = CharacterMouthPackRegistryV1::default();
        registry.installed.push(InstalledCharacterMouthPackV1 {
            game_profile_id: "../cyberpunk".into(),
            character_id: "misty".into(),
            atlas_schema_version: 3,
            identity_revision: 1,
            manifest_sha256: "a".repeat(64),
            texture_file_name: "atlas.bin".into(),
            texture_size_bytes: MAX_TEXTURE_BYTES + 1,
            content_sha256: "b".repeat(64),
            enrollment_binding_sha256: "c".repeat(64),
            imported_at_unix_ms: 1,
        });
        assert!(matches!(
            validate_registry(&registry),
            Err(CharacterMouthPackError::State)
        ));
    }

    #[test]
    fn import_is_immutable_separate_from_enable_and_survives_reload() {
        let directory = tempfile::tempdir().expect("config root");
        let manager = CharacterMouthPackManagerV1::new(directory.path()).expect("manager");
        let authority = test_authority("misty");
        let files = test_files();
        let preview = manager.inspect(&authority, &files).expect("preview");
        assert!(preview.private_review_binding_validated);
        assert!(!preview.network_request_performed);

        let installed = manager
            .import(
                &authority,
                ImportCharacterMouthPackRequestV1 {
                    files,
                    expected_content_sha256: preview.content_sha256.clone(),
                },
                41,
            )
            .expect("import");
        let before_enable = manager.state().expect("state");
        assert_eq!(before_enable.installed, vec![installed.clone()]);
        assert!(before_enable.enabled.is_empty());

        let enabled = manager
            .enable(
                &CurrentCharacterMouthEnableAuthorityV1 {
                    selected: authority.clone(),
                },
                EnableCharacterMouthPackRequestV1 {
                    game_profile_id: "cyberpunk-2077".into(),
                    expected_content_sha256: installed.content_sha256.clone(),
                },
                42,
            )
            .expect("enable");
        let resolved = manager
            .resolve_enabled("cyberpunk-2077", "misty")
            .expect("resolve")
            .expect("enabled revision");
        assert_eq!(resolved.content_sha256, enabled.content_sha256);
        let file_names = fs::read_dir(&resolved.root)
            .expect("revision directory")
            .map(|entry| {
                entry
                    .expect("revision entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            file_names,
            std::collections::BTreeSet::from(["atlas.bin".into(), "atlas.json".into()])
        );

        let reloaded = CharacterMouthPackManagerV1::new(directory.path()).expect("reload");
        assert_eq!(
            reloaded.state().expect("reloaded state").enabled,
            vec![enabled.clone()]
        );
        let disabled = reloaded
            .disable(
                &authority,
                DisableCharacterMouthPackRequestV1 {
                    game_profile_id: "cyberpunk-2077".into(),
                    expected_content_sha256: enabled.content_sha256,
                },
            )
            .expect("disable");
        assert!(disabled.disabled);
        assert!(disabled.retained_installed_revision);
        let after_disable = reloaded.state().expect("disabled state");
        assert!(after_disable.enabled.is_empty());
        assert_eq!(after_disable.installed, vec![installed]);
    }

    #[test]
    fn import_rejects_cross_character_binding_and_checks_dimensions_before_decode() {
        let directory = tempfile::tempdir().expect("config root");
        let manager = CharacterMouthPackManagerV1::new(directory.path()).expect("manager");
        let files = test_files();
        assert!(matches!(
            manager.inspect(&test_authority("judy-alvarez"), &files),
            Err(CharacterMouthPackError::Invalid(_))
        ));

        let mut document: serde_json::Value =
            serde_json::from_str(&files.atlas_json_text).expect("manifest");
        document["texture"]["width"] = serde_json::json!(u32::MAX);
        let invalid_dimensions = CharacterMouthPackFilesV1 {
            game_profile_id: "cyberpunk-2077".into(),
            atlas_json_text: serde_json::to_string(&document).expect("manifest"),
            texture_file_name: "atlas.bin".into(),
            texture_base64: "AA==".into(),
        };
        assert!(matches!(
            manager.inspect(&test_authority("misty"), &invalid_dimensions),
            Err(CharacterMouthPackError::Invalid(message))
                if message.contains("dimensions")
        ));
    }
}
