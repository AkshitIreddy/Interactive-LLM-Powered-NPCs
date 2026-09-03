use crate::catalog::ResourceCatalog;
use npc_character_db::CharacterDatabase;
use npc_game_profile::{KnowledgeAuthority, ProvenanceRecord};
use npc_memory::{
    AuthorityScope, CharacterMemoryErasureRequest as StoreErasureRequest, ContextQuery,
    MemoryStore, SpoilerPolicy,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SELECTION_SCHEMA_VERSION: u32 = 1;
const SELECTION_FILE_NAME: &str = "selected-characters-v1.json";
const MEMORY_BACKUP_DIRECTORY_NAME: &str = "memory-backups-v1";
const MEMORY_BACKUP_PREFIX: &str = "memory-backup-";
const MEMORY_BACKUP_SUFFIX: &str = ".sqlite3";
const MEMORY_LIFECYCLE_AUDIT_FILE_NAME: &str = "memory-lifecycle-audit-v1.jsonl";
const NATIVE_USER_ID: &str = "local-user";
const NATIVE_SESSION_ID: &str = "response-console-simulation";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SelectedCharactersFile {
    schema_version: u32,
    by_game_profile: BTreeMap<String, String>,
}

#[derive(Debug)]
pub struct CharacterWorkspace {
    selection_path: PathBuf,
    memory_path: PathBuf,
    memory_backup_directory: PathBuf,
    memory_lifecycle_audit_path: PathBuf,
    selected: Mutex<SelectedCharactersFile>,
}

#[derive(Debug, thiserror::Error)]
pub enum CharacterWorkspaceError {
    #[error("character workspace state is unavailable")]
    State,
    #[error("character selection is invalid: {0}")]
    Invalid(String),
    #[error("character selection could not be persisted: {0}")]
    Persistence(String),
    #[error("character memory could not be read: {0}")]
    Memory(String),
    #[error("character memory backup failed: {0}")]
    Backup(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharacterInspectionRequest {
    pub game_profile_id: String,
    pub character_id: Option<String>,
    #[serde(default = "default_recent_turn_limit")]
    pub recent_turn_limit: usize,
}

fn default_recent_turn_limit() -> usize {
    20
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterInspection {
    pub schema_version: u32,
    pub game_profile_id: String,
    pub game_display_name: String,
    pub selected_character_id: Option<String>,
    pub character: CharacterProfileInspection,
    pub authored_knowledge: Vec<CharacterKnowledgeInspection>,
    pub provenance: Vec<ProvenanceInspection>,
    pub delivered_memory: Vec<DeliveredMemoryInspection>,
    pub memory_scope: MemoryScopeInspection,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterProfileInspection {
    pub id: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub biography: String,
    pub personality: String,
    pub dialogue_style: String,
    pub style_examples: Vec<StyleExampleInspection>,
    pub opening_lines: Vec<String>,
    pub background_npc: bool,
    pub prompt_role: String,
    pub prompt_objectives: Vec<String>,
    pub prompt_constraints: Vec<String>,
    pub knowledge_refs: Vec<String>,
    pub voice: VoiceInspection,
    pub identity: IdentityInspection,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StyleExampleInspection {
    pub id: String,
    pub speaker: String,
    pub text: String,
    pub situation_tags: Vec<String>,
    pub tone_tags: Vec<String>,
    pub weight_millis: u16,
    pub provenance_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VoiceInspection {
    pub description: String,
    pub locale: String,
    pub style_tags: Vec<String>,
    pub provider_voice_id: Option<String>,
    pub adapter_id: Option<String>,
    pub catalog_version: Option<String>,
    pub license: Option<String>,
    pub user_override_allowed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInspection {
    pub strategy: String,
    pub evidence: Vec<String>,
    pub fallback: String,
    pub automatic_face_recognition_claimed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterKnowledgeInspection {
    pub id: String,
    pub authority: String,
    pub owner_character_id: Option<String>,
    pub text: String,
    pub topic_tags: Vec<String>,
    pub spoiler_tier: String,
    pub provenance_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceInspection {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub source_url: Option<String>,
    pub license: Option<String>,
    pub notes: Option<String>,
    pub source_revision: Option<String>,
    pub source_path: Option<String>,
    pub source_sha256: Option<String>,
    pub review_status: Option<String>,
    pub transform_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeliveredMemoryInspection {
    pub turn_id: String,
    pub provenance_source_kind: String,
    pub provenance_source_id: Option<String>,
    pub provenance_source_uri: Option<String>,
    pub speaker: String,
    pub delivered_text: String,
    pub content_sha256: String,
    pub delivered_at_ms: i64,
    pub sequence: u64,
    pub provider_id: Option<String>,
    pub delivery_receipt_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryScopeInspection {
    pub user_id: String,
    pub profile_id: String,
    pub game_id: String,
    pub character_id: String,
    pub session_id: Option<String>,
    pub save_id: Option<String>,
    pub cross_game_widening_allowed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedCharacterResult {
    pub game_profile_id: String,
    pub character_id: String,
    pub persisted: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharacterMemoryStatusRequest {
    pub game_profile_id: String,
    pub character_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterMemoryStatus {
    pub schema_version: u32,
    pub game_profile_id: String,
    pub character_id: String,
    pub delivered_turns: usize,
    pub structured_memories: usize,
    pub legacy_items: usize,
    pub has_memory: bool,
    pub store_schema_version: i64,
    pub integrity_ok: bool,
    pub integrity_messages: Vec<String>,
    pub database_bytes: u64,
    pub cross_game_widening_allowed: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharacterMemoryBackupRequest {
    pub explicit_user_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterMemoryBackupResult {
    pub backup_id: String,
    pub created_at_ms: i64,
    pub bytes: u64,
    pub pages_copied: i32,
    pub contains_all_local_memory: bool,
    pub local_only: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterMemoryBackupEntry {
    pub backup_id: String,
    pub bytes: u64,
    pub contains_all_local_memory: bool,
    pub local_only: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharacterMemoryBackupDeleteRequest {
    pub backup_id: String,
    pub explicit_user_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterMemoryBackupDeleteResult {
    pub backup_id: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharacterMemoryEraseRequest {
    pub game_profile_id: String,
    pub character_id: String,
    pub explicit_user_confirmation: bool,
    #[serde(default)]
    pub backup_before_erasure: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterMemoryEraseResult {
    pub game_profile_id: String,
    pub character_id: String,
    pub erasure_id: String,
    pub scope_sha256: String,
    pub delivered_turns_deleted: usize,
    pub structured_memories_deleted: usize,
    pub legacy_items_deleted: usize,
    pub outbox_jobs_deleted: usize,
    pub erased_at_ms: i64,
    pub backup: Option<CharacterMemoryBackupResult>,
    pub backup_retains_erased_data: bool,
    pub cross_game_widening_allowed: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharacterMemoryRestoreRequest {
    pub backup_id: String,
    pub explicit_user_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterMemoryRestoreResult {
    pub operation_id: String,
    pub backup_id: String,
    pub restored: bool,
    pub prior_store_quarantined: bool,
    pub reopened_integrity_ok: bool,
    pub store_schema_version: i64,
    pub delivered_turns: u64,
    pub structured_memories: u64,
    pub backups_preserved: bool,
    pub runtime_restarts_on_next_use: bool,
    pub audit_persisted: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveAllLocalMemoryRequest {
    pub explicit_user_confirmation: bool,
    #[serde(default)]
    pub include_backups: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoveAllLocalMemoryResult {
    pub operation_id: String,
    pub removed_artifacts: usize,
    pub removed_bytes: u64,
    pub backups_removed: usize,
    pub backups_preserved: usize,
    pub prior_store_quarantines_removed: usize,
    pub empty_store_reopened: bool,
    pub reopened_integrity_ok: bool,
    pub store_schema_version: i64,
    pub runtime_restarts_on_next_use: bool,
    pub audit_persisted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryLifecycleAuditRecordV1 {
    schema_version: u32,
    operation_id: String,
    operation: &'static str,
    occurred_at_ms: i64,
    include_backups: bool,
    removed_artifacts: usize,
    removed_bytes: u64,
    prior_store_quarantined: bool,
    reopened_integrity_ok: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterCatalogSnapshot {
    pub schema_version: u32,
    pub game_profile_id: String,
    pub game_display_name: String,
    pub selected_character_id: Option<String>,
    pub default_character_id: String,
    pub characters: Vec<CharacterCatalogEntry>,
    pub editable_authored_data: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CharacterCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub background_npc: bool,
    pub voice_description: String,
    pub identity_strategy: String,
}

impl CharacterWorkspace {
    pub fn new(config_directory: &Path) -> Result<Self, CharacterWorkspaceError> {
        let selection_path = config_directory.join(SELECTION_FILE_NAME);
        let selected = load_selection(&selection_path);
        Ok(Self {
            selection_path,
            // This is the runtime-host's canonical memory store. Keeping the
            // inspector on the exact same path prevents a parallel empty DB
            // from masquerading as delivered runtime memory.
            memory_path: config_directory
                .join("runtime-host-data")
                .join("runtime")
                .join("memory.sqlite3"),
            memory_backup_directory: config_directory.join(MEMORY_BACKUP_DIRECTORY_NAME),
            memory_lifecycle_audit_path: config_directory.join(MEMORY_LIFECYCLE_AUDIT_FILE_NAME),
            selected: Mutex::new(selected),
        })
    }

    pub fn selected_for_game(&self, game_profile_id: &str) -> Option<String> {
        self.selected
            .lock()
            .ok()
            .and_then(|state| state.by_game_profile.get(game_profile_id).cloned())
    }

    pub fn resolve_for_turn(
        &self,
        resources: &ResourceCatalog,
        game_profile_id: &str,
        requested_character_id: Option<&str>,
    ) -> Result<String, CharacterWorkspaceError> {
        let profile = resources
            .load_game_profile(game_profile_id)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        let database = CharacterDatabase::new(profile)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        let candidate = requested_character_id
            .map(str::to_owned)
            .or_else(|| self.selected_for_game(game_profile_id))
            .unwrap_or_else(|| database.profile().defaults.character_id.clone());
        database
            .require_character(&candidate)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        Ok(candidate)
    }

    pub fn catalog(
        &self,
        resources: &ResourceCatalog,
        game_profile_id: &str,
    ) -> Result<CharacterCatalogSnapshot, CharacterWorkspaceError> {
        let profile = resources
            .load_game_profile(game_profile_id)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        let database = CharacterDatabase::new(profile)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        Ok(CharacterCatalogSnapshot {
            schema_version: 1,
            game_profile_id: database.profile().id.clone(),
            game_display_name: database.profile().display_name.clone(),
            selected_character_id: self.selected_for_game(game_profile_id),
            default_character_id: database.profile().defaults.character_id.clone(),
            characters: database
                .profile()
                .characters
                .iter()
                .map(|character| CharacterCatalogEntry {
                    id: character.id.clone(),
                    display_name: character.display_name.clone(),
                    aliases: character.aliases.clone(),
                    background_npc: character.background_npc,
                    voice_description: character.voice.description.clone(),
                    identity_strategy: format!("{:?}", character.identity.strategy)
                        .to_ascii_lowercase(),
                })
                .collect(),
            editable_authored_data: false,
        })
    }

    pub fn select(
        &self,
        resources: &ResourceCatalog,
        game_profile_id: &str,
        character_id: &str,
    ) -> Result<SelectedCharacterResult, CharacterWorkspaceError> {
        let profile = resources
            .load_game_profile(game_profile_id)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        let database = CharacterDatabase::new(profile)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        database
            .require_character(character_id)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        let mut state = self
            .selected
            .lock()
            .map_err(|_| CharacterWorkspaceError::State)?;
        state.schema_version = SELECTION_SCHEMA_VERSION;
        state
            .by_game_profile
            .insert(game_profile_id.to_owned(), character_id.to_owned());
        persist_selection(&self.selection_path, &state)?;
        Ok(SelectedCharacterResult {
            game_profile_id: game_profile_id.to_owned(),
            character_id: character_id.to_owned(),
            persisted: true,
        })
    }

    pub async fn inspect(
        &self,
        resources: &ResourceCatalog,
        request: CharacterInspectionRequest,
    ) -> Result<CharacterInspection, CharacterWorkspaceError> {
        validate_inspection_request(&request)?;
        let profile = resources
            .load_game_profile(&request.game_profile_id)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        let database = CharacterDatabase::new(profile)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        let selected_character_id = self.selected_for_game(&request.game_profile_id);
        let requested_id = request
            .character_id
            .as_deref()
            .or(selected_character_id.as_deref())
            .unwrap_or(database.profile().defaults.character_id.as_str());
        let character = database
            .require_character(requested_id)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        let scope = AuthorityScope {
            user_id: NATIVE_USER_ID.into(),
            profile_id: request.game_profile_id.clone(),
            game_id: request.game_profile_id.clone(),
            character_id: Some(character.id.clone()),
            encounter_id: None,
            session_id: Some(NATIVE_SESSION_ID.into()),
            save_id: None,
        };
        scope
            .validate()
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        let store = MemoryStore::open(&self.memory_path)
            .await
            .map_err(memory_error)?;
        let context_result = store
            .retrieve_context(ContextQuery {
                scope: scope.clone(),
                text: None,
                spoiler_policy: SpoilerPolicy::default(),
                recent_turn_limit: request.recent_turn_limit,
                per_class_limit: 0,
                now_ms: current_epoch_ms_i64(),
            })
            .await;
        let close_result = store.close().await;
        let delivered_memory = context_result
            .map_err(memory_error)?
            .recent_dialogue
            .into_iter()
            .map(|turn| {
                let provenance = turn.provenance;
                DeliveredMemoryInspection {
                    turn_id: turn.turn_id,
                    provenance_source_kind: provenance.source_kind,
                    provenance_source_id: provenance.source_id,
                    provenance_source_uri: provenance.source_uri,
                    speaker: format!("{:?}", turn.speaker).to_ascii_lowercase(),
                    delivered_text: turn.delivered_text,
                    content_sha256: turn.content_sha256,
                    delivered_at_ms: turn.delivered_at_ms,
                    sequence: turn.sequence,
                    provider_id: turn.provider_id,
                    delivery_receipt_id: turn.delivery_receipt_id,
                }
            })
            .collect();
        close_result.map_err(memory_error)?;
        let authored_knowledge = database
            .profile()
            .content
            .knowledge
            .iter()
            .filter(|record| {
                record.authority != KnowledgeAuthority::CharacterAuthored
                    || record.owner_character_id.as_deref() == Some(character.id.as_str())
            })
            .map(|record| CharacterKnowledgeInspection {
                id: record.id.clone(),
                authority: format!("{:?}", record.authority).to_ascii_lowercase(),
                owner_character_id: record.owner_character_id.clone(),
                text: record.text.clone(),
                topic_tags: record.topic_tags.clone(),
                spoiler_tier: record.spoiler_tier.clone(),
                provenance_id: record.provenance_id.clone(),
            })
            .collect();
        let provenance = database
            .profile()
            .content
            .provenance
            .iter()
            .map(provenance_dto)
            .collect();
        Ok(CharacterInspection {
            schema_version: 1,
            game_profile_id: database.profile().id.clone(),
            game_display_name: database.profile().display_name.clone(),
            selected_character_id,
            character: CharacterProfileInspection {
                id: character.id.clone(),
                display_name: character.display_name.clone(),
                aliases: character.aliases.clone(),
                biography: character.biography.clone(),
                personality: character.personality.clone(),
                dialogue_style: character.dialogue_style.clone(),
                style_examples: character
                    .style_examples
                    .iter()
                    .map(|example| StyleExampleInspection {
                        id: example.id.clone(),
                        speaker: example.speaker.clone(),
                        text: example.text.clone(),
                        situation_tags: example.situation_tags.clone(),
                        tone_tags: example.tone_tags.clone(),
                        weight_millis: example.weight_millis,
                        provenance_id: example.provenance_id.clone(),
                    })
                    .collect(),
                opening_lines: character.opening_lines.clone(),
                background_npc: character.background_npc,
                prompt_role: character.prompt.role.clone(),
                prompt_objectives: character.prompt.objectives.clone(),
                prompt_constraints: character.prompt.constraints.clone(),
                knowledge_refs: character.prompt.knowledge_refs.clone(),
                voice: VoiceInspection {
                    description: character.voice.description.clone(),
                    locale: character.voice.locale.clone(),
                    style_tags: character.voice.style_tags.clone(),
                    provider_voice_id: character.voice.provider_voice_id.clone(),
                    adapter_id: character.voice.adapter_id.clone(),
                    catalog_version: character.voice.catalog_version.clone(),
                    license: character.voice.license.clone(),
                    user_override_allowed: character.voice.user_override_allowed,
                },
                identity: IdentityInspection {
                    strategy: format!("{:?}", character.identity.strategy).to_ascii_lowercase(),
                    evidence: character
                        .identity
                        .evidence
                        .iter()
                        .map(|evidence| format!("{evidence:?}").to_ascii_lowercase())
                        .collect(),
                    fallback: format!("{:?}", character.identity.fallback).to_ascii_lowercase(),
                    automatic_face_recognition_claimed: false,
                },
            },
            authored_knowledge,
            provenance,
            delivered_memory,
            memory_scope: MemoryScopeInspection {
                user_id: scope.user_id,
                profile_id: scope.profile_id,
                game_id: scope.game_id,
                character_id: scope.character_id.unwrap_or_default(),
                session_id: scope.session_id,
                save_id: scope.save_id,
                cross_game_widening_allowed: false,
            },
        })
    }

    pub async fn memory_status(
        &self,
        resources: &ResourceCatalog,
        request: CharacterMemoryStatusRequest,
    ) -> Result<CharacterMemoryStatus, CharacterWorkspaceError> {
        let scope = self.character_memory_scope(
            resources,
            &request.game_profile_id,
            &request.character_id,
        )?;
        let store = MemoryStore::open(&self.memory_path)
            .await
            .map_err(memory_error)?;
        let status_result = store.character_memory_status(scope).await;
        let integrity_result = store.integrity_check().await;
        let close_result = store.close().await;
        let status = status_result.map_err(memory_error)?;
        let integrity = integrity_result.map_err(memory_error)?;
        close_result.map_err(memory_error)?;
        Ok(CharacterMemoryStatus {
            schema_version: 1,
            game_profile_id: request.game_profile_id,
            character_id: request.character_id,
            delivered_turns: status.delivered_turns,
            structured_memories: status.structured_memories,
            legacy_items: status.legacy_items,
            has_memory: status.delivered_turns > 0
                || status.structured_memories > 0
                || status.legacy_items > 0,
            store_schema_version: integrity.schema_version,
            integrity_ok: integrity.ok,
            integrity_messages: integrity.messages,
            database_bytes: sqlite_artifact_bytes(&self.memory_path),
            cross_game_widening_allowed: false,
        })
    }

    pub async fn backup_all_local_memory(
        &self,
        request: CharacterMemoryBackupRequest,
    ) -> Result<CharacterMemoryBackupResult, CharacterWorkspaceError> {
        require_confirmation(
            request.explicit_user_confirmation,
            "memory backup requires explicit user confirmation",
        )?;
        let store = MemoryStore::open(&self.memory_path)
            .await
            .map_err(memory_error)?;
        let backup = self.create_memory_backup(&store).await;
        let close_result = store.close().await;
        let backup = backup?;
        close_result.map_err(memory_error)?;
        Ok(backup)
    }

    pub fn list_memory_backups(
        &self,
    ) -> Result<Vec<CharacterMemoryBackupEntry>, CharacterWorkspaceError> {
        if !self.memory_backup_directory.exists() {
            return Ok(Vec::new());
        }
        self.ensure_safe_backup_directory(false)?;
        let entries = fs::read_dir(&self.memory_backup_directory)
            .map_err(|error| CharacterWorkspaceError::Backup(error.to_string()))?;
        let mut backups = Vec::new();
        for entry in entries {
            let entry =
                entry.map_err(|error| CharacterWorkspaceError::Backup(error.to_string()))?;
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| CharacterWorkspaceError::Backup(error.to_string()))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(backup_id) = backup_id_from_file_name(&file_name) else {
                continue;
            };
            backups.push(CharacterMemoryBackupEntry {
                backup_id,
                bytes: metadata.len(),
                contains_all_local_memory: true,
                local_only: true,
            });
        }
        backups.sort_by(|left, right| right.backup_id.cmp(&left.backup_id));
        Ok(backups)
    }

    pub fn delete_memory_backup(
        &self,
        request: CharacterMemoryBackupDeleteRequest,
    ) -> Result<CharacterMemoryBackupDeleteResult, CharacterWorkspaceError> {
        require_confirmation(
            request.explicit_user_confirmation,
            "memory backup deletion requires explicit user confirmation",
        )?;
        let backup_id = normalized_backup_id(&request.backup_id)?;
        self.ensure_safe_backup_directory(false)?;
        let path = self.memory_backup_path(&backup_id);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| CharacterWorkspaceError::Invalid("memory backup does not exist".into()))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(CharacterWorkspaceError::Invalid(
                "memory backup is not a safe regular file".into(),
            ));
        }
        fs::remove_file(&path)
            .map_err(|error| CharacterWorkspaceError::Backup(error.to_string()))?;
        Ok(CharacterMemoryBackupDeleteResult {
            backup_id,
            deleted: true,
        })
    }

    pub async fn erase_character_memory(
        &self,
        resources: &ResourceCatalog,
        request: CharacterMemoryEraseRequest,
    ) -> Result<CharacterMemoryEraseResult, CharacterWorkspaceError> {
        require_confirmation(
            request.explicit_user_confirmation,
            "character memory erasure requires explicit user confirmation",
        )?;
        let scope = self.character_memory_scope(
            resources,
            &request.game_profile_id,
            &request.character_id,
        )?;
        let store = MemoryStore::open(&self.memory_path)
            .await
            .map_err(memory_error)?;
        let backup = if request.backup_before_erasure {
            match self.create_memory_backup(&store).await {
                Ok(backup) => Some(backup),
                Err(error) => {
                    // A failed optional pre-erasure backup must not leave the
                    // writer thread holding the SQLite files on Windows.
                    let _ = store.close().await;
                    return Err(error);
                }
            }
        } else {
            None
        };
        let report_result = store
            .erase_character_memory(StoreErasureRequest {
                scope,
                erased_at_ms: current_epoch_ms_i64(),
            })
            .await;
        let close_result = store.close().await;
        let report = report_result.map_err(memory_error)?;
        close_result.map_err(memory_error)?;
        Ok(CharacterMemoryEraseResult {
            game_profile_id: request.game_profile_id,
            character_id: request.character_id,
            erasure_id: report.erasure_id,
            scope_sha256: report.scope_sha256,
            delivered_turns_deleted: report.delivered_turns_deleted,
            structured_memories_deleted: report.structured_memories_deleted,
            legacy_items_deleted: report.legacy_items_deleted,
            outbox_jobs_deleted: report.outbox_jobs_deleted,
            erased_at_ms: report.erased_at_ms,
            backup_retains_erased_data: backup.is_some(),
            backup,
            cross_game_widening_allowed: false,
        })
    }

    /// Restores only an app-owned backup ID. The command layer must stop the
    /// runtime sidecar before entering this method so SQLite has no live writer.
    pub async fn restore_local_memory_backup(
        &self,
        request: CharacterMemoryRestoreRequest,
    ) -> Result<CharacterMemoryRestoreResult, CharacterWorkspaceError> {
        require_confirmation(
            request.explicit_user_confirmation,
            "memory restore requires explicit user confirmation",
        )?;
        let backup_id = normalized_backup_id(&request.backup_id)?;
        self.ensure_safe_backup_directory(false)?;
        let backup_path = self.memory_backup_path(&backup_id);
        let metadata = fs::symlink_metadata(&backup_path)
            .map_err(|_| CharacterWorkspaceError::Invalid("memory backup does not exist".into()))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(CharacterWorkspaceError::Invalid(
                "memory backup is not a safe regular file".into(),
            ));
        }
        if let Some(runtime_directory) = self.memory_path.parent() {
            validate_safe_memory_directory(runtime_directory)?;
        }
        let operation_id = uuid::Uuid::new_v4().to_string();
        let recovery = MemoryStore::recover_from_backup(&backup_path, &self.memory_path, true)
            .await
            .map_err(memory_error)?;
        let reopened = MemoryStore::open(&self.memory_path)
            .await
            .map_err(memory_error)?;
        let integrity_result = reopened.integrity_check().await;
        let close_result = reopened.close().await;
        let integrity = integrity_result.map_err(memory_error)?;
        close_result.map_err(memory_error)?;
        if !integrity.ok {
            return Err(CharacterWorkspaceError::Memory(
                "restored memory did not pass reopen integrity validation".into(),
            ));
        }
        let audit_persisted = self.persist_memory_lifecycle_audit(&MemoryLifecycleAuditRecordV1 {
            schema_version: 1,
            operation_id: operation_id.clone(),
            operation: "restore_app_owned_backup",
            occurred_at_ms: current_epoch_ms_i64(),
            include_backups: false,
            removed_artifacts: 0,
            removed_bytes: 0,
            prior_store_quarantined: recovery.quarantined_database.is_some(),
            reopened_integrity_ok: integrity.ok,
        });
        Ok(CharacterMemoryRestoreResult {
            operation_id,
            backup_id,
            restored: true,
            prior_store_quarantined: recovery.quarantined_database.is_some(),
            reopened_integrity_ok: integrity.ok,
            store_schema_version: integrity.schema_version,
            delivered_turns: integrity.delivered_turns,
            structured_memories: integrity.derived_memories,
            backups_preserved: true,
            runtime_restarts_on_next_use: true,
            audit_persisted,
        })
    }

    /// Removes only recognized local-memory artifacts. Backups are preserved by
    /// default and require a second explicit boolean to enter the removal set.
    /// The command layer must stop the runtime sidecar before this method runs.
    pub async fn remove_all_local_memory(
        &self,
        request: RemoveAllLocalMemoryRequest,
    ) -> Result<RemoveAllLocalMemoryResult, CharacterWorkspaceError> {
        require_confirmation(
            request.explicit_user_confirmation,
            "complete local-memory removal requires explicit user confirmation",
        )?;
        let operation_id = uuid::Uuid::new_v4().to_string();
        let mut removed_artifacts = 0usize;
        let mut removed_bytes = 0u64;
        let mut quarantines_removed = 0usize;

        if let Some(runtime_directory) = self.memory_path.parent() {
            validate_safe_memory_directory(runtime_directory)?;
        }

        for path in sqlite_artifact_paths(&self.memory_path) {
            let removed = remove_exact_memory_artifact(&path)?;
            removed_artifacts = removed_artifacts.saturating_add(removed.0);
            removed_bytes = removed_bytes.saturating_add(removed.1);
        }
        if let Some(runtime_directory) = self.memory_path.parent() {
            if runtime_directory.exists() {
                for entry in fs::read_dir(runtime_directory)
                    .map_err(|error| CharacterWorkspaceError::Memory(error.to_string()))?
                {
                    let entry = entry
                        .map_err(|error| CharacterWorkspaceError::Memory(error.to_string()))?;
                    let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                        continue;
                    };
                    let is_quarantine = name.starts_with("memory.sqlite3.quarantine-");
                    let is_restore_temp =
                        name.starts_with(".memory.sqlite3.restore-") && name.ends_with(".tmp");
                    if !is_quarantine && !is_restore_temp {
                        continue;
                    }
                    let removed = remove_exact_memory_artifact(&entry.path())?;
                    if is_quarantine && removed.0 > 0 {
                        quarantines_removed = quarantines_removed.saturating_add(1);
                    }
                    removed_artifacts = removed_artifacts.saturating_add(removed.0);
                    removed_bytes = removed_bytes.saturating_add(removed.1);
                }
            }
        }

        let backup_inventory = self.list_memory_backups()?;
        let mut backups_removed = 0usize;
        if request.include_backups {
            for backup in &backup_inventory {
                let removed =
                    remove_exact_memory_artifact(&self.memory_backup_path(&backup.backup_id))?;
                if removed.0 > 0 {
                    backups_removed = backups_removed.saturating_add(1);
                }
                removed_artifacts = removed_artifacts.saturating_add(removed.0);
                removed_bytes = removed_bytes.saturating_add(removed.1);
            }
            if self.memory_backup_directory.exists() {
                let _ = fs::remove_dir(&self.memory_backup_directory);
            }
        }
        let backups_preserved = backup_inventory.len().saturating_sub(backups_removed);

        let reopened = MemoryStore::open(&self.memory_path)
            .await
            .map_err(memory_error)?;
        let integrity_result = reopened.integrity_check().await;
        let close_result = reopened.close().await;
        let integrity = integrity_result.map_err(memory_error)?;
        close_result.map_err(memory_error)?;
        let empty_store_reopened =
            integrity.ok && integrity.delivered_turns == 0 && integrity.derived_memories == 0;
        if !empty_store_reopened {
            return Err(CharacterWorkspaceError::Memory(
                "empty memory store could not be reopened after removal".into(),
            ));
        }
        let audit_persisted = self.persist_memory_lifecycle_audit(&MemoryLifecycleAuditRecordV1 {
            schema_version: 1,
            operation_id: operation_id.clone(),
            operation: "remove_all_local_memory",
            occurred_at_ms: current_epoch_ms_i64(),
            include_backups: request.include_backups,
            removed_artifacts,
            removed_bytes,
            prior_store_quarantined: quarantines_removed > 0,
            reopened_integrity_ok: integrity.ok,
        });
        Ok(RemoveAllLocalMemoryResult {
            operation_id,
            removed_artifacts,
            removed_bytes,
            backups_removed,
            backups_preserved,
            prior_store_quarantines_removed: quarantines_removed,
            empty_store_reopened,
            reopened_integrity_ok: integrity.ok,
            store_schema_version: integrity.schema_version,
            runtime_restarts_on_next_use: true,
            audit_persisted,
        })
    }

    fn character_memory_scope(
        &self,
        resources: &ResourceCatalog,
        game_profile_id: &str,
        character_id: &str,
    ) -> Result<AuthorityScope, CharacterWorkspaceError> {
        let profile = resources
            .load_game_profile(game_profile_id)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        let database = CharacterDatabase::new(profile)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        database
            .require_character(character_id)
            .map_err(|error| CharacterWorkspaceError::Invalid(error.to_string()))?;
        Ok(AuthorityScope {
            user_id: NATIVE_USER_ID.into(),
            profile_id: game_profile_id.to_owned(),
            game_id: game_profile_id.to_owned(),
            character_id: Some(character_id.to_owned()),
            encounter_id: None,
            session_id: None,
            save_id: None,
        })
    }

    async fn create_memory_backup(
        &self,
        store: &MemoryStore,
    ) -> Result<CharacterMemoryBackupResult, CharacterWorkspaceError> {
        self.ensure_safe_backup_directory(true)?;
        let backup_id = uuid::Uuid::new_v4().to_string();
        let destination = self.memory_backup_path(&backup_id);
        let created_at_ms = current_epoch_ms_i64();
        let report = store.backup(&destination).await.map_err(memory_error)?;
        let bytes = fs::symlink_metadata(&destination)
            .map_err(|error| CharacterWorkspaceError::Backup(error.to_string()))?
            .len();
        Ok(CharacterMemoryBackupResult {
            backup_id,
            created_at_ms,
            bytes,
            pages_copied: report.pages_copied,
            contains_all_local_memory: true,
            local_only: true,
        })
    }

    fn memory_backup_path(&self, backup_id: &str) -> PathBuf {
        self.memory_backup_directory.join(format!(
            "{MEMORY_BACKUP_PREFIX}{backup_id}{MEMORY_BACKUP_SUFFIX}"
        ))
    }

    fn ensure_safe_backup_directory(&self, create: bool) -> Result<(), CharacterWorkspaceError> {
        if create {
            fs::create_dir_all(&self.memory_backup_directory)
                .map_err(|error| CharacterWorkspaceError::Backup(error.to_string()))?;
        }
        let metadata = fs::symlink_metadata(&self.memory_backup_directory)
            .map_err(|error| CharacterWorkspaceError::Backup(error.to_string()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(CharacterWorkspaceError::Invalid(
                "memory backup directory is not a safe local directory".into(),
            ));
        }
        Ok(())
    }

    fn persist_memory_lifecycle_audit(&self, record: &MemoryLifecycleAuditRecordV1) -> bool {
        let Some(parent) = self.memory_lifecycle_audit_path.parent() else {
            return false;
        };
        if fs::create_dir_all(parent).is_err() {
            return false;
        }
        let Ok(metadata) = fs::symlink_metadata(parent) else {
            return false;
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return false;
        }
        if self.memory_lifecycle_audit_path.exists() {
            let Ok(metadata) = fs::symlink_metadata(&self.memory_lifecycle_audit_path) else {
                return false;
            };
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > 8 * 1024 * 1024
            {
                return false;
            }
        }
        let Ok(mut bytes) = serde_json::to_vec(record) else {
            return false;
        };
        bytes.push(b'\n');
        let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.memory_lifecycle_audit_path)
        else {
            return false;
        };
        file.write_all(&bytes).and_then(|_| file.sync_all()).is_ok()
    }
}

fn memory_error(error: npc_memory::MemoryError) -> CharacterWorkspaceError {
    CharacterWorkspaceError::Memory(error.to_string())
}

fn require_confirmation(confirmed: bool, message: &str) -> Result<(), CharacterWorkspaceError> {
    if !confirmed {
        return Err(CharacterWorkspaceError::Invalid(message.to_owned()));
    }
    Ok(())
}

fn normalized_backup_id(value: &str) -> Result<String, CharacterWorkspaceError> {
    let parsed = uuid::Uuid::parse_str(value)
        .map_err(|_| CharacterWorkspaceError::Invalid("memory backup ID is invalid".into()))?;
    let normalized = parsed.to_string();
    if value != normalized {
        return Err(CharacterWorkspaceError::Invalid(
            "memory backup ID must use canonical lowercase UUID form".into(),
        ));
    }
    Ok(normalized)
}

fn backup_id_from_file_name(file_name: &str) -> Option<String> {
    let id = file_name
        .strip_prefix(MEMORY_BACKUP_PREFIX)?
        .strip_suffix(MEMORY_BACKUP_SUFFIX)?;
    normalized_backup_id(id).ok()
}

fn sqlite_artifact_bytes(database: &Path) -> u64 {
    sqlite_artifact_paths(database)
        .into_iter()
        .filter_map(|path| fs::symlink_metadata(path).ok())
        .filter(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .map(|metadata| metadata.len())
        .sum()
}

fn sqlite_artifact_paths(database: &Path) -> [PathBuf; 3] {
    let with_suffix = |suffix: &str| {
        let mut path = database.as_os_str().to_os_string();
        path.push(suffix);
        PathBuf::from(path)
    };
    [
        database.to_path_buf(),
        with_suffix("-wal"),
        with_suffix("-shm"),
    ]
}

fn validate_safe_memory_directory(directory: &Path) -> Result<(), CharacterWorkspaceError> {
    if !directory.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(directory)
        .map_err(|error| CharacterWorkspaceError::Memory(error.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(CharacterWorkspaceError::Invalid(
            "runtime memory directory is not a safe local directory".into(),
        ));
    }
    Ok(())
}

fn remove_exact_memory_artifact(path: &Path) -> Result<(usize, u64), CharacterWorkspaceError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(error) => return Err(CharacterWorkspaceError::Memory(error.to_string())),
    };
    if metadata.file_type().is_symlink() {
        fs::remove_file(path)
            .map_err(|error| CharacterWorkspaceError::Memory(error.to_string()))?;
        return Ok((1, 0));
    }
    if metadata.is_file() {
        fs::remove_file(path)
            .map_err(|error| CharacterWorkspaceError::Memory(error.to_string()))?;
        return Ok((1, metadata.len()));
    }
    if metadata.is_dir() {
        let mut entries = 0usize;
        let bytes = bounded_artifact_tree_bytes(path, 0, &mut entries)?;
        fs::remove_dir_all(path)
            .map_err(|error| CharacterWorkspaceError::Memory(error.to_string()))?;
        return Ok((1, bytes));
    }
    Err(CharacterWorkspaceError::Invalid(
        "memory artifact has an unsupported filesystem type".into(),
    ))
}

fn bounded_artifact_tree_bytes(
    directory: &Path,
    depth: usize,
    entries: &mut usize,
) -> Result<u64, CharacterWorkspaceError> {
    if depth > 4 || *entries > 10_000 {
        return Err(CharacterWorkspaceError::Invalid(
            "memory quarantine exceeds its bounded removal shape".into(),
        ));
    }
    let mut bytes = 0u64;
    for entry in fs::read_dir(directory)
        .map_err(|error| CharacterWorkspaceError::Memory(error.to_string()))?
    {
        let entry = entry.map_err(|error| CharacterWorkspaceError::Memory(error.to_string()))?;
        *entries = (*entries).saturating_add(1);
        if *entries > 10_000 {
            return Err(CharacterWorkspaceError::Invalid(
                "memory quarantine contains too many artifacts".into(),
            ));
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| CharacterWorkspaceError::Memory(error.to_string()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() {
            bytes = bytes.saturating_add(metadata.len());
        } else if metadata.is_dir() {
            bytes = bytes.saturating_add(bounded_artifact_tree_bytes(
                &entry.path(),
                depth.saturating_add(1),
                entries,
            )?);
        } else {
            return Err(CharacterWorkspaceError::Invalid(
                "memory quarantine contains an unsupported filesystem type".into(),
            ));
        }
    }
    Ok(bytes)
}

fn validate_inspection_request(
    request: &CharacterInspectionRequest,
) -> Result<(), CharacterWorkspaceError> {
    if request.recent_turn_limit > 100 {
        return Err(CharacterWorkspaceError::Invalid(
            "recent turn limit cannot exceed 100".into(),
        ));
    }
    Ok(())
}

fn provenance_dto(record: &ProvenanceRecord) -> ProvenanceInspection {
    ProvenanceInspection {
        id: record.id.clone(),
        title: record.title.clone(),
        kind: format!("{:?}", record.kind).to_ascii_lowercase(),
        source_url: record.source_url.clone(),
        license: record.license.clone(),
        notes: record.notes.clone(),
        source_revision: record.source_revision.clone(),
        source_path: record.source_path.clone(),
        source_sha256: record.source_sha256.clone(),
        review_status: record
            .review_status
            .map(|status| format!("{status:?}").to_ascii_lowercase()),
        transform_version: record.transform_version.clone(),
    }
}

fn load_selection(path: &Path) -> SelectedCharactersFile {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return SelectedCharactersFile {
            schema_version: SELECTION_SCHEMA_VERSION,
            ..SelectedCharactersFile::default()
        };
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 256 * 1024 {
        return SelectedCharactersFile {
            schema_version: SELECTION_SCHEMA_VERSION,
            ..SelectedCharactersFile::default()
        };
    }
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SelectedCharactersFile>(&bytes).ok())
        .filter(|state| state.schema_version == SELECTION_SCHEMA_VERSION)
        .unwrap_or_else(|| SelectedCharactersFile {
            schema_version: SELECTION_SCHEMA_VERSION,
            ..SelectedCharactersFile::default()
        })
}

fn persist_selection(
    path: &Path,
    state: &SelectedCharactersFile,
) -> Result<(), CharacterWorkspaceError> {
    let parent = path.parent().ok_or_else(|| {
        CharacterWorkspaceError::Persistence("selection path has no parent".into())
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| CharacterWorkspaceError::Persistence(error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| CharacterWorkspaceError::Persistence(error.to_string()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| CharacterWorkspaceError::Persistence(error.to_string()))?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| CharacterWorkspaceError::Persistence(error.to_string()))?;
    temporary
        .persist(path)
        .map_err(|error| CharacterWorkspaceError::Persistence(error.error.to_string()))?;
    Ok(())
}

fn current_epoch_ms_i64() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn commit_workspace_turn(workspace: &CharacterWorkspace, turn_id: &str) {
        let store = MemoryStore::open(&workspace.memory_path)
            .await
            .expect("memory store");
        store
            .commit_turn(npc_memory::TurnCommitInput {
                turn_id: turn_id.into(),
                scope: AuthorityScope {
                    user_id: NATIVE_USER_ID.into(),
                    profile_id: "skyrim-special-edition".into(),
                    game_id: "skyrim-special-edition".into(),
                    character_id: Some("lydia".into()),
                    encounter_id: Some("encounter-a".into()),
                    session_id: Some(NATIVE_SESSION_ID.into()),
                    save_id: Some("save-a".into()),
                },
                speaker: npc_memory::TurnSpeaker::Npc,
                text: format!("delivered fixture {turn_id}"),
                delivery: npc_memory::DeliveryDisposition::Delivered { delivered_at_ms: 2 },
                created_at_ms: 1,
                sequence: 1,
                cancellation_generation: 0,
                provider_id: Some("fixture".into()),
                delivery_receipt_id: Some(format!("receipt-{turn_id}")),
                provenance: npc_memory::Provenance::default(),
            })
            .await
            .expect("commit turn");
        store.close().await.expect("close memory store");
    }

    #[test]
    fn selections_are_namespaced_by_game_and_persisted() {
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace = CharacterWorkspace::new(directory.path()).expect("workspace");
        {
            let mut selected = workspace.selected.lock().expect("selection lock");
            selected.schema_version = SELECTION_SCHEMA_VERSION;
            selected
                .by_game_profile
                .insert("game-a".into(), "actor-a".into());
            selected
                .by_game_profile
                .insert("game-b".into(), "actor-b".into());
            persist_selection(&workspace.selection_path, &selected).expect("persist");
        }
        let reopened = CharacterWorkspace::new(directory.path()).expect("reopen");
        assert_eq!(
            reopened.selected_for_game("game-a").as_deref(),
            Some("actor-a")
        );
        assert_eq!(
            reopened.selected_for_game("game-b").as_deref(),
            Some("actor-b")
        );
        assert_eq!(reopened.selected_for_game("game-c"), None);
    }

    #[test]
    fn inspector_uses_the_runtime_hosts_only_memory_database() {
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace = CharacterWorkspace::new(directory.path()).expect("workspace");
        assert_eq!(
            workspace.memory_path,
            directory
                .path()
                .join("runtime-host-data")
                .join("runtime")
                .join("memory.sqlite3")
        );
        assert_ne!(
            workspace.memory_path,
            directory.path().join("memory").join("memory-v2.sqlite3")
        );
    }

    #[test]
    fn inspection_is_bounded_and_rejects_blank_authority() {
        let invalid = CharacterInspectionRequest {
            game_profile_id: "skyrim-special-edition".into(),
            character_id: None,
            recent_turn_limit: 101,
        };
        assert!(validate_inspection_request(&invalid).is_err());
    }

    #[test]
    fn webview_inspection_request_cannot_mint_authority_principal() {
        let wire = serde_json::json!({
            "gameProfileId": "skyrim-special-edition",
            "characterId": "lydia",
            "recentTurnLimit": 20,
            "userId": "another-user"
        });
        assert!(serde_json::from_value::<CharacterInspectionRequest>(wire).is_err());
        assert_eq!(NATIVE_USER_ID, "local-user");
        assert_eq!(NATIVE_SESSION_ID, "response-console-simulation");
    }

    #[tokio::test]
    async fn backup_inventory_and_deletion_use_only_app_owned_uuid_paths() {
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace = CharacterWorkspace::new(directory.path()).expect("workspace");
        let store = MemoryStore::open(&workspace.memory_path)
            .await
            .expect("memory store");
        store
            .upsert(npc_memory::MemoryInput {
                id: Some("backup-fixture".into()),
                namespace: npc_memory::MemoryNamespace::Fact,
                scope: npc_memory::MemoryScope {
                    profile_id: Some("skyrim-special-edition".into()),
                    game_id: Some("skyrim-special-edition".into()),
                    character_id: Some("lydia".into()),
                    session_id: None,
                    save_id: None,
                },
                visibility: npc_memory::Visibility::Character,
                content: "backup fixture".into(),
                provenance: npc_memory::Provenance::default(),
                confidence: 1.0,
                importance: 1.0,
                observed_at_ms: 1,
                expires_at_ms: None,
                expected_revision: None,
            })
            .await
            .expect("seed memory");
        store.close().await.expect("close seeded store");

        assert!(workspace
            .backup_all_local_memory(CharacterMemoryBackupRequest {
                explicit_user_confirmation: false,
            })
            .await
            .is_err());
        let backup = workspace
            .backup_all_local_memory(CharacterMemoryBackupRequest {
                explicit_user_confirmation: true,
            })
            .await
            .expect("backup");
        assert!(backup.bytes > 0);
        assert!(backup.pages_copied > 0);
        assert!(backup.contains_all_local_memory);
        assert!(backup.local_only);
        let inventory = workspace.list_memory_backups().expect("inventory");
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].backup_id, backup.backup_id);
        assert!(workspace
            .delete_memory_backup(CharacterMemoryBackupDeleteRequest {
                backup_id: "../../memory.sqlite3".into(),
                explicit_user_confirmation: true,
            })
            .is_err());
        assert!(workspace
            .delete_memory_backup(CharacterMemoryBackupDeleteRequest {
                backup_id: backup.backup_id.clone(),
                explicit_user_confirmation: false,
            })
            .is_err());
        let deleted = workspace
            .delete_memory_backup(CharacterMemoryBackupDeleteRequest {
                backup_id: backup.backup_id,
                explicit_user_confirmation: true,
            })
            .expect("delete backup");
        assert!(deleted.deleted);
        assert!(workspace
            .list_memory_backups()
            .expect("empty inventory")
            .is_empty());

        let untrusted_path = serde_json::json!({
            "explicitUserConfirmation": true,
            "destinationPath": "C:/untrusted.sqlite3"
        });
        assert!(serde_json::from_value::<CharacterMemoryBackupRequest>(untrusted_path).is_err());
    }

    #[tokio::test]
    async fn status_and_erasure_are_native_principal_game_character_scoped() {
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace = CharacterWorkspace::new(directory.path()).expect("workspace");
        let store = MemoryStore::open(&workspace.memory_path)
            .await
            .expect("memory store");
        let scope = AuthorityScope {
            user_id: NATIVE_USER_ID.into(),
            profile_id: "skyrim-special-edition".into(),
            game_id: "skyrim-special-edition".into(),
            character_id: Some("lydia".into()),
            encounter_id: Some("encounter-a".into()),
            session_id: Some(NATIVE_SESSION_ID.into()),
            save_id: Some("save-a".into()),
        };
        store
            .commit_turn(npc_memory::TurnCommitInput {
                turn_id: "workspace-delete-fixture".into(),
                scope,
                speaker: npc_memory::TurnSpeaker::Npc,
                text: "I am sworn to carry your burdens.".into(),
                delivery: npc_memory::DeliveryDisposition::Delivered { delivered_at_ms: 2 },
                created_at_ms: 1,
                sequence: 1,
                cancellation_generation: 0,
                provider_id: Some("fixture".into()),
                delivery_receipt_id: Some("receipt".into()),
                provenance: npc_memory::Provenance::default(),
            })
            .await
            .expect("commit turn");
        store.close().await.expect("close seeded store");
        let resources = ResourceCatalog::new(None);
        let status_request = CharacterMemoryStatusRequest {
            game_profile_id: "skyrim-special-edition".into(),
            character_id: "lydia".into(),
        };
        let status = workspace
            .memory_status(&resources, status_request.clone())
            .await
            .expect("status");
        assert_eq!(status.delivered_turns, 1);
        assert!(status.has_memory);
        assert!(status.integrity_ok);
        assert!(!status.cross_game_widening_allowed);

        assert!(workspace
            .erase_character_memory(
                &resources,
                CharacterMemoryEraseRequest {
                    game_profile_id: status_request.game_profile_id.clone(),
                    character_id: status_request.character_id.clone(),
                    explicit_user_confirmation: false,
                    backup_before_erasure: false,
                },
            )
            .await
            .is_err());
        let erased = workspace
            .erase_character_memory(
                &resources,
                CharacterMemoryEraseRequest {
                    game_profile_id: status_request.game_profile_id.clone(),
                    character_id: status_request.character_id.clone(),
                    explicit_user_confirmation: true,
                    backup_before_erasure: true,
                },
            )
            .await
            .expect("erase");
        assert_eq!(erased.delivered_turns_deleted, 1);
        assert!(erased.backup.is_some());
        assert!(erased.backup_retains_erased_data);
        assert!(!erased.cross_game_widening_allowed);
        let after = workspace
            .memory_status(&resources, status_request)
            .await
            .expect("status after");
        assert_eq!(after.delivered_turns, 0);
        assert!(!after.has_memory);

        let wrong_game_character = workspace
            .memory_status(
                &resources,
                CharacterMemoryStatusRequest {
                    game_profile_id: "cyberpunk-2077".into(),
                    character_id: "lydia".into(),
                },
            )
            .await;
        assert!(wrong_game_character.is_err());
    }

    #[tokio::test]
    async fn restore_quarantines_atomically_and_complete_removal_preserves_backups_by_default() {
        let directory = tempfile::tempdir().expect("tempdir");
        let workspace = CharacterWorkspace::new(directory.path()).expect("workspace");
        commit_workspace_turn(&workspace, "backup-turn").await;
        let first_backup = workspace
            .backup_all_local_memory(CharacterMemoryBackupRequest {
                explicit_user_confirmation: true,
            })
            .await
            .expect("first backup");
        commit_workspace_turn(&workspace, "later-turn").await;

        assert!(workspace
            .restore_local_memory_backup(CharacterMemoryRestoreRequest {
                backup_id: first_backup.backup_id.clone(),
                explicit_user_confirmation: false,
            })
            .await
            .is_err());
        let restored = workspace
            .restore_local_memory_backup(CharacterMemoryRestoreRequest {
                backup_id: first_backup.backup_id.clone(),
                explicit_user_confirmation: true,
            })
            .await
            .expect("restore");
        assert!(restored.restored);
        assert!(restored.prior_store_quarantined);
        assert!(restored.reopened_integrity_ok);
        assert_eq!(restored.delivered_turns, 1);
        assert!(restored.backups_preserved);
        assert!(restored.runtime_restarts_on_next_use);
        assert!(restored.audit_persisted);

        let reopened = MemoryStore::open(&workspace.memory_path)
            .await
            .expect("reopen restored store");
        assert!(reopened
            .get_delivered_turn("backup-turn")
            .await
            .expect("read backup turn")
            .is_some());
        assert!(reopened
            .get_delivered_turn("later-turn")
            .await
            .expect("read later turn")
            .is_none());
        reopened.close().await.expect("close restored store");

        let corrupt_backup = workspace
            .backup_all_local_memory(CharacterMemoryBackupRequest {
                explicit_user_confirmation: true,
            })
            .await
            .expect("second backup");
        fs::write(
            workspace.memory_backup_path(&corrupt_backup.backup_id),
            b"not a sqlite database",
        )
        .expect("corrupt test backup");
        assert!(workspace
            .restore_local_memory_backup(CharacterMemoryRestoreRequest {
                backup_id: corrupt_backup.backup_id.clone(),
                explicit_user_confirmation: true,
            })
            .await
            .is_err());
        let still_live = MemoryStore::open(&workspace.memory_path)
            .await
            .expect("live store survives corrupt restore");
        assert!(still_live
            .get_delivered_turn("backup-turn")
            .await
            .expect("read surviving turn")
            .is_some());
        still_live.close().await.expect("close surviving store");

        assert!(workspace
            .remove_all_local_memory(RemoveAllLocalMemoryRequest {
                explicit_user_confirmation: false,
                include_backups: false,
            })
            .await
            .is_err());
        let preserved = workspace
            .remove_all_local_memory(RemoveAllLocalMemoryRequest {
                explicit_user_confirmation: true,
                include_backups: false,
            })
            .await
            .expect("remove live memory");
        assert!(preserved.empty_store_reopened);
        assert!(preserved.reopened_integrity_ok);
        assert_eq!(preserved.backups_removed, 0);
        assert_eq!(preserved.backups_preserved, 2);
        assert!(preserved.prior_store_quarantines_removed >= 1);
        assert!(preserved.runtime_restarts_on_next_use);
        assert!(preserved.audit_persisted);
        let empty = MemoryStore::open(&workspace.memory_path)
            .await
            .expect("empty reopened store");
        assert!(empty
            .get_delivered_turn("backup-turn")
            .await
            .expect("read empty store")
            .is_none());
        empty.close().await.expect("close empty store");
        assert_eq!(workspace.list_memory_backups().expect("backups").len(), 2);

        let all_removed = workspace
            .remove_all_local_memory(RemoveAllLocalMemoryRequest {
                explicit_user_confirmation: true,
                include_backups: true,
            })
            .await
            .expect("remove live memory and backups");
        assert!(all_removed.empty_store_reopened);
        assert_eq!(all_removed.backups_removed, 2);
        assert_eq!(all_removed.backups_preserved, 0);
        assert!(all_removed.audit_persisted);
        assert!(workspace
            .list_memory_backups()
            .expect("empty backups")
            .is_empty());

        let audit = fs::read_to_string(&workspace.memory_lifecycle_audit_path)
            .expect("content-free lifecycle audit");
        assert!(audit.contains("restore_app_owned_backup"));
        assert!(audit.contains("remove_all_local_memory"));
        assert!(!audit.contains("backup-turn"));
        assert!(!audit.contains("later-turn"));
        assert!(!audit.contains("delivered fixture"));
    }
}
