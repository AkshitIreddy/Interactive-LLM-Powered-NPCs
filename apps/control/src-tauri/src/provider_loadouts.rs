use crate::commands::{AppState, CommandError};
use npc_provider_loadouts::{
    CatalogDisclosureV1, EgressClassV1, ExecutionLocationV1, LoadoutContextV1, LoadoutId,
    LoadoutScopeV1, ProviderLoadoutDocumentV1, ProviderLoadoutV1, ProviderModelRouteV1,
    ProviderRole, ResolvedProviderLoadoutV1, RoleOverrideV1, RoleRouteV1, TransmittedDataV1,
    ValidationContextV1,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;
use tempfile::NamedTempFile;
use thiserror::Error;

const LOADOUT_FILE_NAME: &str = "provider-loadouts-v1.json";
const RECOVERY_FILE_NAME: &str = "provider-loadouts-v1.last-good.json";
const MAX_LOADOUT_BYTES: u64 = 512 * 1024;
const SEED_CATALOG_REVISION: u64 = 5;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderLoadoutPersistenceHealth {
    Healthy,
    FirstRun,
    RecoveredLastGood,
    RecoveredSeed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLoadoutSnapshot {
    pub document: ProviderLoadoutDocumentV1,
    pub persistence_health: ProviderLoadoutPersistenceHealth,
    pub detail: String,
    pub credentials_checked: bool,
    pub network_request_performed: bool,
}

impl ProviderLoadoutSnapshot {
    fn new(
        document: ProviderLoadoutDocumentV1,
        persistence_health: ProviderLoadoutPersistenceHealth,
    ) -> Self {
        let detail = match persistence_health {
            ProviderLoadoutPersistenceHealth::Healthy => {
                "Provider loadouts were loaded from private application storage."
            }
            ProviderLoadoutPersistenceHealth::FirstRun => {
                "An API-first starter loadout is ready; provider credentials have not been checked."
            }
            ProviderLoadoutPersistenceHealth::RecoveredLastGood => {
                "The primary loadout file was invalid, so the last known-good document was used."
            }
            ProviderLoadoutPersistenceHealth::RecoveredSeed => {
                "Saved loadouts were invalid, so a safe starter loadout was restored in memory."
            }
        };
        Self {
            document,
            persistence_health,
            detail: detail.into(),
            credentials_checked: false,
            network_request_performed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLoadoutReview {
    pub resolved: ResolvedProviderLoadoutV1,
    pub offline: bool,
    pub credentials_checked: bool,
    pub network_request_performed: bool,
    pub detail: String,
}

#[derive(Debug, Error)]
enum ProviderLoadoutStoreError {
    #[error("provider loadout directory is unavailable")]
    Directory,
    #[error("provider loadout file could not be inspected")]
    Inspect,
    #[error("provider loadout file exceeds its safety limit")]
    TooLarge,
    #[error("provider loadout file could not be read")]
    Read,
    #[error("provider loadout JSON is invalid")]
    Json,
    #[error("provider loadout validation failed")]
    Validation,
    #[error("provider loadout contains secret-like material")]
    SecretMaterial,
    #[error("provider loadout file could not be staged")]
    Stage,
    #[error("provider loadout file could not be committed")]
    Commit,
}

#[derive(Debug, Clone)]
struct ProviderLoadoutStore {
    directory: PathBuf,
}

impl ProviderLoadoutStore {
    fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    fn load(&self) -> ProviderLoadoutSnapshot {
        match self.load_path(&self.primary_path()) {
            Ok(Some(document)) => {
                ProviderLoadoutSnapshot::new(document, ProviderLoadoutPersistenceHealth::Healthy)
            }
            Ok(None) => ProviderLoadoutSnapshot::new(
                starter_document(),
                ProviderLoadoutPersistenceHealth::FirstRun,
            ),
            Err(_) => match self.load_path(&self.recovery_path()) {
                Ok(Some(document)) => ProviderLoadoutSnapshot::new(
                    document,
                    ProviderLoadoutPersistenceHealth::RecoveredLastGood,
                ),
                Ok(None) | Err(_) => ProviderLoadoutSnapshot::new(
                    starter_document(),
                    ProviderLoadoutPersistenceHealth::RecoveredSeed,
                ),
            },
        }
    }

    fn save(&self, document: &ProviderLoadoutDocumentV1) -> Result<(), ProviderLoadoutStoreError> {
        document
            .validate()
            .map_err(|_| ProviderLoadoutStoreError::Validation)?;
        reject_secret_material(document)?;
        fs::create_dir_all(&self.directory).map_err(|_| ProviderLoadoutStoreError::Directory)?;
        let bytes =
            serde_json::to_vec_pretty(document).map_err(|_| ProviderLoadoutStoreError::Json)?;
        if bytes.len() as u64 > MAX_LOADOUT_BYTES {
            return Err(ProviderLoadoutStoreError::TooLarge);
        }

        // Write the recovery copy first. If the process stops between commits,
        // the visible primary remains the previous complete document.
        self.atomic_write(&self.recovery_path(), &bytes)?;
        self.atomic_write(&self.primary_path(), &bytes)?;
        if let Ok(directory) = File::open(&self.directory) {
            let _ = directory.sync_all();
        }
        Ok(())
    }

    fn load_path(
        &self,
        path: &Path,
    ) -> Result<Option<ProviderLoadoutDocumentV1>, ProviderLoadoutStoreError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ProviderLoadoutStoreError::Inspect),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(ProviderLoadoutStoreError::Inspect);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(ProviderLoadoutStoreError::Inspect);
            }
        }
        if metadata.len() > MAX_LOADOUT_BYTES {
            return Err(ProviderLoadoutStoreError::TooLarge);
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(path)
            .map_err(|_| ProviderLoadoutStoreError::Read)?
            .take(MAX_LOADOUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ProviderLoadoutStoreError::Read)?;
        if bytes.len() as u64 > MAX_LOADOUT_BYTES {
            return Err(ProviderLoadoutStoreError::TooLarge);
        }
        let document: ProviderLoadoutDocumentV1 =
            serde_json::from_slice(&bytes).map_err(|_| ProviderLoadoutStoreError::Json)?;
        document
            .validate()
            .map_err(|_| ProviderLoadoutStoreError::Validation)?;
        reject_secret_material(&document)?;
        Ok(Some(document))
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> Result<(), ProviderLoadoutStoreError> {
        let mut temporary =
            NamedTempFile::new_in(&self.directory).map_err(|_| ProviderLoadoutStoreError::Stage)?;
        temporary
            .write_all(bytes)
            .map_err(|_| ProviderLoadoutStoreError::Stage)?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| ProviderLoadoutStoreError::Stage)?;
        temporary
            .persist(path)
            .map_err(|_| ProviderLoadoutStoreError::Commit)?;
        Ok(())
    }

    fn primary_path(&self) -> PathBuf {
        self.directory.join(LOADOUT_FILE_NAME)
    }

    fn recovery_path(&self) -> PathBuf {
        self.directory.join(RECOVERY_FILE_NAME)
    }
}

#[derive(Debug)]
pub(crate) struct ProviderLoadoutManager {
    store: ProviderLoadoutStore,
    snapshot: Mutex<ProviderLoadoutSnapshot>,
}

impl ProviderLoadoutManager {
    pub(crate) fn new(directory: PathBuf) -> Self {
        let store = ProviderLoadoutStore::new(directory);
        let snapshot = store.load();
        Self {
            store,
            snapshot: Mutex::new(snapshot),
        }
    }

    fn snapshot(&self) -> Result<ProviderLoadoutSnapshot, CommandError> {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .map_err(|_| CommandError::StateUnavailable)
    }

    fn mutate(
        &self,
        operation: impl FnOnce(&mut ProviderLoadoutDocumentV1) -> Result<(), ()>,
    ) -> Result<ProviderLoadoutSnapshot, CommandError> {
        let mut guard = self
            .snapshot
            .lock()
            .map_err(|_| CommandError::StateUnavailable)?;
        let mut candidate = guard.document.clone();
        operation(&mut candidate).map_err(|()| CommandError::InvalidRequest {
            message: "provider loadout request failed validation".into(),
        })?;
        self.store
            .save(&candidate)
            .map_err(|_| CommandError::Persistence {
                message: "provider loadouts could not be persisted".into(),
            })?;
        *guard = ProviderLoadoutSnapshot::new(candidate, ProviderLoadoutPersistenceHealth::Healthy);
        Ok(guard.clone())
    }

    fn review(
        &self,
        context: LoadoutContextV1,
        offline: bool,
    ) -> Result<ProviderLoadoutReview, CommandError> {
        let guard = self
            .snapshot
            .lock()
            .map_err(|_| CommandError::StateUnavailable)?;
        let validation = if offline {
            ValidationContextV1::offline()
        } else {
            ValidationContextV1::online()
        };
        let resolved = guard.document.resolve(&context, &validation).map_err(|_| {
            CommandError::InvalidRequest {
                message: if offline {
                    "the selected loadout is not permitted in offline mode".into()
                } else {
                    "provider loadout request failed validation".into()
                },
            }
        })?;
        Ok(ProviderLoadoutReview {
            resolved,
            offline,
            credentials_checked: false,
            network_request_performed: false,
            detail: "Routes were resolved from saved configuration only; credentials and providers were not contacted.".into(),
        })
    }
}

#[tauri::command]
pub fn provider_loadout_snapshot(
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state.provider_loadouts.snapshot()
}

#[tauri::command]
pub fn create_provider_loadout(
    loadout: ProviderLoadoutV1,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state
        .provider_loadouts
        .mutate(move |document| document.insert(loadout).map_err(|_| ()))
}

#[tauri::command]
pub fn clone_provider_loadout(
    source_id: LoadoutId,
    new_id: LoadoutId,
    new_name: String,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state.provider_loadouts.mutate(move |document| {
        document
            .clone_loadout(&source_id, new_id, new_name)
            .map_err(|_| ())
    })
}

#[tauri::command]
pub fn rename_provider_loadout(
    id: LoadoutId,
    new_name: String,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state
        .provider_loadouts
        .mutate(move |document| document.rename(&id, new_name).map_err(|_| ()))
}

#[tauri::command]
pub fn update_provider_loadout(
    id: LoadoutId,
    loadout: ProviderLoadoutV1,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state
        .provider_loadouts
        .mutate(move |document| update_document_entry(document, &id, loadout))
}

#[tauri::command]
pub fn delete_provider_loadout(
    id: LoadoutId,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state
        .provider_loadouts
        .mutate(move |document| document.delete(&id).map(|_| ()).map_err(|_| ()))
}

fn update_document_entry(
    document: &mut ProviderLoadoutDocumentV1,
    id: &LoadoutId,
    loadout: ProviderLoadoutV1,
) -> Result<(), ()> {
    if &loadout.id != id {
        return Err(());
    }
    let previous = document.loadouts.get(id).cloned().ok_or(())?;
    if previous.scope != loadout.scope {
        return Err(());
    }
    document.loadouts.insert(id.clone(), loadout);
    if document.validate().is_err() {
        document.loadouts.insert(id.clone(), previous);
        return Err(());
    }
    Ok(())
}

#[tauri::command]
pub fn activate_provider_loadout(
    id: LoadoutId,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state
        .provider_loadouts
        .mutate(move |document| document.activate(&id).map_err(|_| ()))
}

#[tauri::command]
pub fn deactivate_provider_loadout_scope(
    scope: LoadoutScopeV1,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state.provider_loadouts.mutate(move |document| {
        document
            .deactivate_scope(&scope)
            .map(|_| ())
            .map_err(|_| ())
    })
}

#[tauri::command]
pub fn review_provider_loadout(
    context: LoadoutContextV1,
    offline: bool,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutReview, CommandError> {
    state.provider_loadouts.review(context, offline)
}

fn starter_document() -> ProviderLoadoutDocumentV1 {
    let hosted = |provider_id: &str,
                  model_id: &str,
                  privacy_summary: &str,
                  cost_summary: &str,
                  transmitted_data: BTreeSet<TransmittedDataV1>| {
        RoleOverrideV1::Route(Box::new(RoleRouteV1 {
            primary: ProviderModelRouteV1 {
                provider_id: provider_id.into(),
                model_id: model_id.into(),
                credential: None,
                disclosure: CatalogDisclosureV1 {
                    catalog_revision: SEED_CATALOG_REVISION,
                    execution: ExecutionLocationV1::Hosted,
                    egress: EgressClassV1::ProviderCloud,
                    privacy_summary: privacy_summary.into(),
                    cost_summary: cost_summary.into(),
                    transmitted_data,
                },
                explicit_user_selection: false,
            },
            fallbacks: Vec::new(),
        }))
    };
    let default = ProviderLoadoutV1 {
        id: LoadoutId::new("api-first-starter").expect("static starter id"),
        name: "API-first starter".into(),
        scope: LoadoutScopeV1::Global,
        parent: None,
        roles: BTreeMap::from([
            (
                ProviderRole::Llm,
                hosted(
                    "nvidia-nim",
                    "nvidia/nemotron-3-nano-30b-a3b",
                    "Transcript, selected game context, and memory context are sent to NVIDIA NIM when this route is used.",
                    "Uses the user's NVIDIA API account; trial availability and provider limits may change.",
                    BTreeSet::from([
                        TransmittedDataV1::Transcript,
                        TransmittedDataV1::GameContext,
                        TransmittedDataV1::MemoryContext,
                    ]),
                ),
            ),
            (
                ProviderRole::Stt,
                hosted(
                    "assemblyai",
                    "universal-streaming",
                    "Microphone audio is sent to AssemblyAI only while this speech route is used.",
                    "Uses the user's AssemblyAI account and its current trial or paid limits.",
                    BTreeSet::from([TransmittedDataV1::MicrophoneAudio]),
                ),
            ),
            (
                ProviderRole::Tts,
                hosted(
                    "elevenlabs",
                    "eleven-flash-v2.5",
                    "Response text is sent to ElevenLabs only while this voice route is used.",
                    "Uses the user's ElevenLabs account and its current trial or paid limits.",
                    BTreeSet::from([TransmittedDataV1::ResponseText]),
                ),
            ),
            (
                ProviderRole::Retrieval,
                hosted(
                    "nvidia-nim",
                    "nvidia/nemotron-3-embed-1b",
                    "Selected lore and memory text are sent to NVIDIA NIM when retrieval embeddings are requested.",
                    "Uses the user's NVIDIA API account; trial availability and provider limits may change.",
                    BTreeSet::from([TransmittedDataV1::MemoryContext]),
                ),
            ),
        ]),
    };
    ProviderLoadoutDocumentV1::new(default).expect("static starter loadout must validate")
}

fn reject_secret_material(
    document: &ProviderLoadoutDocumentV1,
) -> Result<(), ProviderLoadoutStoreError> {
    let value = serde_json::to_value(document).map_err(|_| ProviderLoadoutStoreError::Json)?;
    if contains_secret_material(&value) {
        Err(ProviderLoadoutStoreError::SecretMaterial)
    } else {
        Ok(())
    }
}

fn contains_secret_material(value: &Value) -> bool {
    match value {
        Value::Object(entries) => entries.iter().any(|(key, value)| {
            let normalized = key.to_ascii_lowercase().replace(['-', '_'], "");
            matches!(
                normalized.as_str(),
                "apikey"
                    | "token"
                    | "secret"
                    | "password"
                    | "authorization"
                    | "credentialvalue"
                    | "privatekey"
                    | "accesstoken"
            ) || contains_secret_material(value)
        }),
        Value::Array(values) => values.iter().any(contains_secret_material),
        Value::String(text) => looks_like_secret(text),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn looks_like_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains("bearer ") || lower.contains("-----begin private key-----") {
        return true;
    }
    const PREFIXES: [&str; 8] = [
        "sk-", "sk_", "nvapi-", "gsk_", "pplx-", "xai-", "aiza", "akia",
    ];
    if PREFIXES
        .iter()
        .any(|prefix| contains_token_prefix(&lower, prefix))
    {
        return true;
    }
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|token| {
            let has_digit = token.bytes().any(|byte| byte.is_ascii_digit());
            let mixed_case = token.bytes().any(|byte| byte.is_ascii_lowercase())
                && token.bytes().any(|byte| byte.is_ascii_uppercase());
            token.len() >= 40 || (token.len() >= 32 && (has_digit || mixed_case))
        })
}

fn contains_token_prefix(text: &str, prefix: &str) -> bool {
    text.match_indices(prefix).any(|(index, _)| {
        let at_boundary = index == 0
            || text[..index]
                .chars()
                .next_back()
                .map_or(true, |character| !character.is_ascii_alphanumeric());
        let tail = text[index + prefix.len()..]
            .chars()
            .take_while(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
            .count();
        at_boundary && tail >= 12
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use npc_provider_loadouts::{CredentialReferenceV1, ExplicitFallbackV1, FallbackActivationV1};
    use pretty_assertions::assert_eq;

    fn id(value: &str) -> LoadoutId {
        LoadoutId::new(value).expect("valid fixture id")
    }

    #[test]
    fn first_run_seed_is_api_first_but_claims_no_credentials_or_network() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let snapshot = ProviderLoadoutStore::new(temp.path()).load();
        assert_eq!(
            snapshot.persistence_health,
            ProviderLoadoutPersistenceHealth::FirstRun
        );
        assert!(!snapshot.credentials_checked);
        assert!(!snapshot.network_request_performed);
        assert_eq!(snapshot.document.loadouts.len(), 1);
        let serialized = serde_json::to_string(&snapshot).expect("serialize seed");
        assert!(!serialized.contains("api_key"));
        assert!(!serialized.contains("credential_value"));
    }

    #[test]
    fn corrupt_primary_recovers_last_good_then_can_be_replaced_atomically() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ProviderLoadoutStore::new(temp.path());
        let document = starter_document();
        store.save(&document).expect("save seed");
        fs::write(store.primary_path(), b"{not-json").expect("corrupt primary fixture");

        let recovered = store.load();
        assert_eq!(
            recovered.persistence_health,
            ProviderLoadoutPersistenceHealth::RecoveredLastGood
        );
        assert_eq!(recovered.document, document);
        store
            .save(&recovered.document)
            .expect("atomically replace corrupt primary");
        assert_eq!(
            store.load().persistence_health,
            ProviderLoadoutPersistenceHealth::Healthy
        );
    }

    #[test]
    fn scope_inheritance_is_preserved_through_native_persistence() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ProviderLoadoutStore::new(temp.path());
        let mut document = starter_document();
        let game = ProviderLoadoutV1 {
            id: id("cyberpunk-streaming"),
            name: "Cyberpunk streaming".into(),
            scope: LoadoutScopeV1::Game {
                game_id: "cyberpunk-2077".into(),
            },
            parent: Some(id("api-first-starter")),
            roles: BTreeMap::from([(ProviderRole::Tts, RoleOverrideV1::Inherit)]),
        };
        document.insert(game).expect("insert game loadout");
        let character = ProviderLoadoutV1 {
            id: id("judy-dialogue"),
            name: "Judy dialogue".into(),
            scope: LoadoutScopeV1::Character {
                game_id: "cyberpunk-2077".into(),
                character_id: "judy-alvarez".into(),
            },
            parent: Some(id("cyberpunk-streaming")),
            roles: BTreeMap::new(),
        };
        document
            .insert(character)
            .expect("insert character loadout");
        document
            .activate(&id("cyberpunk-streaming"))
            .expect("activate game");
        document
            .activate(&id("judy-dialogue"))
            .expect("activate character");
        store.save(&document).expect("save hierarchy");

        let loaded = store.load().document;
        let resolved = loaded
            .resolve(
                &LoadoutContextV1::character("cyberpunk-2077", "judy-alvarez"),
                &ValidationContextV1::online(),
            )
            .expect("resolve persisted hierarchy");
        assert_eq!(
            resolved.inheritance_chain,
            vec![
                id("api-first-starter"),
                id("cyberpunk-streaming"),
                id("judy-dialogue")
            ]
        );
    }

    #[test]
    fn fallback_must_remain_manual_and_explicit_before_native_save() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ProviderLoadoutStore::new(temp.path());
        let mut document = starter_document();
        {
            let loadout = document
                .loadouts
                .get_mut(&id("api-first-starter"))
                .expect("starter loadout");
            let RoleOverrideV1::Route(route) = loadout
                .roles
                .get_mut(&ProviderRole::Llm)
                .expect("llm route")
            else {
                panic!("expected route")
            };
            route.fallbacks.push(ExplicitFallbackV1 {
                route: ProviderModelRouteV1 {
                    provider_id: "anthropic".into(),
                    model_id: "claude-sonnet-4".into(),
                    credential: None,
                    disclosure: route.primary.disclosure.clone(),
                    explicit_user_selection: false,
                },
                activation: FallbackActivationV1::ManualOnly,
                user_authorized: false,
            });
        }
        assert!(store.save(&document).is_err());
        let loadout = document
            .loadouts
            .get_mut(&id("api-first-starter"))
            .expect("starter loadout");
        let RoleOverrideV1::Route(route) = loadout
            .roles
            .get_mut(&ProviderRole::Llm)
            .expect("llm route")
        else {
            panic!("expected route")
        };
        route.fallbacks[0].user_authorized = true;
        store.save(&document).expect("save explicit fallback");
        let resolved = store
            .load()
            .document
            .resolve(&LoadoutContextV1::global(), &ValidationContextV1::online())
            .expect("resolve fallback loadout");
        assert!(resolved
            .pin_turn_routes(1)
            .automatic_fallback(ProviderRole::Llm)
            .is_err());
        assert_eq!(
            resolved
                .manual_fallback_candidate(ProviderRole::Llm, 0)
                .expect("manual candidate")
                .provider_id,
            "anthropic"
        );
    }

    #[test]
    fn api_first_seed_is_denied_in_offline_review() {
        let manager =
            ProviderLoadoutManager::new(tempfile::tempdir().expect("temporary directory").keep());
        let error = manager
            .review(LoadoutContextV1::global(), true)
            .expect_err("cloud routes cannot resolve offline");
        assert!(matches!(error, CommandError::InvalidRequest { .. }));
        assert!(!error.to_string().contains("nvidia"));
    }

    #[test]
    fn secret_like_values_and_fields_never_enter_persistence() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ProviderLoadoutStore::new(temp.path());
        let mut document = starter_document();
        let loadout = document
            .loadouts
            .get_mut(&id("api-first-starter"))
            .expect("starter loadout");
        let RoleOverrideV1::Route(route) = loadout
            .roles
            .get_mut(&ProviderRole::Llm)
            .expect("llm route")
        else {
            panic!("expected route")
        };
        route.primary.credential = Some(CredentialReferenceV1 {
            provider_id: "nvidia-nim".into(),
            reference_id: "personal".into(),
        });
        route.primary.disclosure.cost_summary = "sk-canary-never-persist-this".into();
        assert!(matches!(
            store.save(&document),
            Err(ProviderLoadoutStoreError::SecretMaterial)
        ));
        assert!(!store.primary_path().exists());

        let valid_json = serde_json::to_string(&starter_document()).expect("serialize fixture");
        let malicious = valid_json.replacen(
            "\"credential\":null",
            "\"credential\":null,\"api_key\":\"nvapi-canary\"",
            1,
        );
        fs::write(store.primary_path(), malicious).expect("write malicious fixture");
        assert_eq!(
            store.load().persistence_health,
            ProviderLoadoutPersistenceHealth::RecoveredSeed
        );

        let mut opaque_secret = starter_document();
        let loadout = opaque_secret
            .loadouts
            .get_mut(&id("api-first-starter"))
            .expect("starter loadout");
        let RoleOverrideV1::Route(route) = loadout
            .roles
            .get_mut(&ProviderRole::Llm)
            .expect("llm route")
        else {
            panic!("expected route")
        };
        route.primary.credential = Some(CredentialReferenceV1 {
            provider_id: "nvidia-nim".into(),
            reference_id: "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b".into(),
        });
        assert!(matches!(
            store.save(&opaque_secret),
            Err(ProviderLoadoutStoreError::SecretMaterial)
        ));
        assert!(!looks_like_secret(
            "Risk-based provider routing remains user controlled."
        ));
    }

    #[test]
    fn update_is_transactional_and_preserves_id_and_scope() {
        let mut document = starter_document();
        let before = document.clone();
        let mut replacement = document.loadouts[&id("api-first-starter")].clone();
        replacement.id = id("different-id");
        assert!(
            update_document_entry(&mut document, &id("api-first-starter"), replacement).is_err()
        );
        assert_eq!(document, before);

        let mut replacement = document.loadouts[&id("api-first-starter")].clone();
        replacement.scope = LoadoutScopeV1::Game {
            game_id: "cyberpunk-2077".into(),
        };
        assert!(
            update_document_entry(&mut document, &id("api-first-starter"), replacement).is_err()
        );
        assert_eq!(document, before);

        let mut replacement = document.loadouts[&id("api-first-starter")].clone();
        replacement.name = "My API mix".into();
        update_document_entry(&mut document, &id("api-first-starter"), replacement)
            .expect("valid update");
        assert_eq!(
            document.loadouts[&id("api-first-starter")].name,
            "My API mix"
        );
    }

    #[test]
    fn scoped_activation_can_be_removed_before_delete() {
        let mut document = starter_document();
        let game = ProviderLoadoutV1 {
            id: id("skyrim-api"),
            name: "Skyrim API".into(),
            scope: LoadoutScopeV1::Game {
                game_id: "skyrim-special-edition".into(),
            },
            parent: Some(id("api-first-starter")),
            roles: BTreeMap::new(),
        };
        document.insert(game).expect("insert game loadout");
        document.activate(&id("skyrim-api")).expect("activate game");
        assert!(document.delete(&id("skyrim-api")).is_err());
        document
            .deactivate_scope(&LoadoutScopeV1::Game {
                game_id: "skyrim-special-edition".into(),
            })
            .expect("deactivate game scope");
        document
            .delete(&id("skyrim-api"))
            .expect("delete inactive loadout");
        assert!(document.deactivate_scope(&LoadoutScopeV1::Global).is_err());
    }
}
