use crate::domain::{
    CatalogState, CredentialReferenceStatus, GameProfileSummary, ModelInstallation, ModelSummary,
    ProfileSafety, ProviderCredentialSummary,
};
use interactive_npcs_credential_vault::{
    credential_namespace_for_application, CredentialVault, SecretValue, VaultError,
    PRODUCTION_CREDENTIAL_NAMESPACE,
};
use npc_provider_catalog::{
    CatalogDocument, ExecutionLocation, Lifecycle, Modality, RouteAvailability,
};
use std::path::{Path, PathBuf};

/// Shared with `apps/runtime-host/src/bootstrap.rs`. Both processes must resolve
/// the identical Windows Credential Manager target namespace.
pub(crate) const CREDENTIAL_NAMESPACE: &str = PRODUCTION_CREDENTIAL_NAMESPACE;

const BUNDLED_PROVIDER_CATALOG: &[u8] = include_bytes!("../../../../catalog/v1/catalog.json");

fn bundled_provider_catalog() -> CatalogDocument {
    CatalogDocument::parse(BUNDLED_PROVIDER_CATALOG)
        .expect("the bundled provider catalog must remain schema-valid")
}

pub trait CredentialPresence: Send + Sync + std::fmt::Debug {
    fn status(&self, reference: &str) -> CredentialReferenceStatus;
    fn save(&self, reference: &str, secret: &SecretValue) -> Result<(), CredentialMutationError>;
    fn delete(&self, reference: &str) -> Result<(), CredentialMutationError>;
    fn availability_detail(&self) -> &'static str;
    fn credential_namespace(&self) -> &str {
        CREDENTIAL_NAMESPACE
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum CredentialMutationError {
    #[error("credential storage is unavailable on this platform")]
    Unavailable,
    #[error("credential storage operation failed")]
    Storage,
}

#[cfg(windows)]
#[derive(Debug)]
pub struct SystemCredentialPresence {
    vault: interactive_npcs_credential_vault::WindowsCredentialVault,
    credential_namespace: &'static str,
}

#[cfg(windows)]
impl SystemCredentialPresence {
    pub fn new() -> Result<Self, VaultError> {
        Self::new_for_application(
            interactive_npcs_credential_vault::PRODUCTION_APPLICATION_NAMESPACE,
        )
    }

    pub fn new_for_application(application_namespace: &str) -> Result<Self, VaultError> {
        let credential_namespace = credential_namespace_for_application(application_namespace)
            .ok_or(VaultError::InvalidTarget)?;
        Ok(Self {
            vault: interactive_npcs_credential_vault::WindowsCredentialVault::new(
                credential_namespace,
            )?,
            credential_namespace,
        })
    }
}

#[cfg(windows)]
impl CredentialPresence for SystemCredentialPresence {
    fn status(&self, reference: &str) -> CredentialReferenceStatus {
        match self.vault.get(reference) {
            Ok(secret) => {
                // The secret is immediately dropped and zeroized. Only presence
                // crosses the command boundary; no API in this shell returns it.
                drop(secret);
                CredentialReferenceStatus::Present
            }
            Err(VaultError::NotFound) => CredentialReferenceStatus::Missing,
            Err(_) => CredentialReferenceStatus::Unavailable,
        }
    }

    fn save(&self, reference: &str, secret: &SecretValue) -> Result<(), CredentialMutationError> {
        self.vault
            .put(reference, None, secret)
            .map_err(|_| CredentialMutationError::Storage)
    }

    fn delete(&self, reference: &str) -> Result<(), CredentialMutationError> {
        match self.vault.delete(reference) {
            Ok(()) | Err(VaultError::NotFound) => Ok(()),
            Err(_) => Err(CredentialMutationError::Storage),
        }
    }

    fn availability_detail(&self) -> &'static str {
        "Credential references are checked in Windows Credential Manager; values never enter the WebView."
    }

    fn credential_namespace(&self) -> &str {
        self.credential_namespace
    }
}

#[cfg(not(windows))]
#[derive(Debug)]
pub struct SystemCredentialPresence {
    credential_namespace: &'static str,
}

#[cfg(not(windows))]
impl SystemCredentialPresence {
    pub fn new() -> Result<Self, VaultError> {
        Self::new_for_application(
            interactive_npcs_credential_vault::PRODUCTION_APPLICATION_NAMESPACE,
        )
    }

    pub fn new_for_application(application_namespace: &str) -> Result<Self, VaultError> {
        credential_namespace_for_application(application_namespace)
            .ok_or(VaultError::InvalidTarget)
            .map(|credential_namespace| Self {
                credential_namespace,
            })
    }
}

#[cfg(not(windows))]
impl CredentialPresence for SystemCredentialPresence {
    fn status(&self, _reference: &str) -> CredentialReferenceStatus {
        CredentialReferenceStatus::Unavailable
    }

    fn save(&self, _reference: &str, _secret: &SecretValue) -> Result<(), CredentialMutationError> {
        Err(CredentialMutationError::Unavailable)
    }

    fn delete(&self, _reference: &str) -> Result<(), CredentialMutationError> {
        Err(CredentialMutationError::Unavailable)
    }

    fn availability_detail(&self) -> &'static str {
        "Credential status is unavailable outside the supported Windows runtime."
    }

    fn credential_namespace(&self) -> &str {
        self.credential_namespace
    }
}

pub fn provider_summaries(presence: &dyn CredentialPresence) -> Vec<ProviderCredentialSummary> {
    bundled_provider_catalog()
        .content
        .providers
        .into_iter()
        .map(|provider| {
            let reference = provider
                .credential
                .required
                .then(|| format!("providers/{}", provider.id));
            match reference {
                Some(reference) => {
                    let status = presence.status(&reference);
                    let detail = match status {
                        CredentialReferenceStatus::Present => {
                            "A credential reference is available."
                        }
                        CredentialReferenceStatus::Missing => {
                            "No credential is saved for this provider."
                        }
                        CredentialReferenceStatus::Unavailable => presence.availability_detail(),
                        CredentialReferenceStatus::NotRequired => "No credential is required.",
                    };
                    ProviderCredentialSummary {
                        provider_id: provider.id,
                        display_name: provider.display_name,
                        credential_reference: Some(format!(
                            "{}/{reference}",
                            presence.credential_namespace()
                        )),
                        status,
                        detail: detail.into(),
                    }
                }
                None => ProviderCredentialSummary {
                    provider_id: provider.id,
                    display_name: provider.display_name,
                    credential_reference: None,
                    status: CredentialReferenceStatus::NotRequired,
                    detail: "This local provider does not require a cloud credential.".into(),
                },
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct GameDeclaration {
    id: &'static str,
    name: &'static str,
    wave: &'static str,
    safety: ProfileSafety,
}

const GAMES: &[GameDeclaration] = &[
    game(
        "skyrim-special-edition",
        "The Elder Scrolls V: Skyrim Special Edition",
        "A",
    ),
    game("cyberpunk-2077", "Cyberpunk 2077", "A"),
    game("baldurs-gate-3", "Baldur's Gate 3", "A"),
    game("fallout-4", "Fallout 4", "B"),
    game("fallout-new-vegas", "Fallout: New Vegas", "B"),
    game("the-witcher-3", "The Witcher 3: Wild Hunt", "B"),
    game("starfield", "Starfield", "B"),
    game(
        "mount-and-blade-2-bannerlord",
        "Mount & Blade II: Bannerlord",
        "B",
    ),
    game(
        "kingdom-come-deliverance-2",
        "Kingdom Come: Deliverance II",
        "B",
    ),
    game(
        "oblivion-remastered",
        "The Elder Scrolls IV: Oblivion Remastered",
        "B",
    ),
    game("the-sims-4", "The Sims 4", "C"),
    game("stardew-valley", "Stardew Valley", "C"),
    game("minecraft-java", "Minecraft: Java Edition", "C"),
    game("divinity-original-sin-2", "Divinity: Original Sin 2", "C"),
    game(
        "mass-effect-legendary-edition",
        "Mass Effect Legendary Edition",
        "C",
    ),
    game("dragon-age-inquisition", "Dragon Age: Inquisition", "C"),
    game("kenshi", "Kenshi", "C"),
    offline_game(
        "red-dead-redemption-2-story",
        "Red Dead Redemption 2 — Story Mode",
    ),
    offline_game("gta-v-story", "Grand Theft Auto V — Story Mode"),
    offline_game("elden-ring-offline", "Elden Ring — Offline"),
];

const fn game(id: &'static str, name: &'static str, wave: &'static str) -> GameDeclaration {
    GameDeclaration {
        id,
        name,
        wave,
        safety: ProfileSafety::SinglePlayerOnly,
    }
}

const fn offline_game(id: &'static str, name: &'static str) -> GameDeclaration {
    GameDeclaration {
        id,
        name,
        wave: "Risk-gated",
        safety: ProfileSafety::OfflineOnly,
    }
}

#[derive(Debug, Clone)]
pub struct ResourceCatalog {
    installed_resource_root: Option<PathBuf>,
    active_content_profile_root: Option<PathBuf>,
    character_content_override_path: Option<PathBuf>,
}

impl ResourceCatalog {
    pub fn new(installed_resource_root: Option<PathBuf>) -> Self {
        Self {
            installed_resource_root,
            active_content_profile_root: None,
            character_content_override_path: None,
        }
    }

    pub fn with_active_content_profiles(
        installed_resource_root: Option<PathBuf>,
        active_content_profile_root: PathBuf,
    ) -> Self {
        Self {
            installed_resource_root,
            active_content_profile_root: Some(active_content_profile_root),
            character_content_override_path: None,
        }
    }

    pub fn with_user_content_layers(
        installed_resource_root: Option<PathBuf>,
        active_content_profile_root: PathBuf,
        character_content_override_path: PathBuf,
    ) -> Self {
        Self {
            installed_resource_root,
            active_content_profile_root: Some(active_content_profile_root),
            character_content_override_path: Some(character_content_override_path),
        }
    }

    pub fn game_summaries(&self) -> Vec<GameProfileSummary> {
        GAMES
            .iter()
            .map(|game| GameProfileSummary {
                id: game.id.into(),
                display_name: game.name.into(),
                wave: game.wave.into(),
                safety: game.safety,
                catalog_state: if self.profile_exists(game.id) {
                    CatalogState::Bundled
                } else {
                    CatalogState::MissingFromBundle
                },
                default_fallback: "Explicit character selection, audio, and subtitles".into(),
            })
            .collect()
    }

    /// Load one bundled profile through a fixed catalog identity. The WebView
    /// never supplies a path and a missing/linked/oversized resource fails
    /// closed before deserialization.
    pub fn load_game_profile(
        &self,
        id: &str,
    ) -> Result<npc_game_profile::GameProfileV2, ResourceProfileError> {
        if !GAMES.iter().any(|game| game.id == id) {
            return Err(ResourceProfileError::UnknownProfile);
        }
        let mut profile = self.load_fixed_game_profile(id)?;
        if let Some(path) = &self.character_content_override_path {
            crate::character_content_overrides::apply_saved_overrides(path, &mut profile)
                .map_err(|error| ResourceProfileError::Overrides(error.to_string()))?;
        }
        Ok(profile)
    }

    pub(crate) fn load_game_profile_without_character_overrides(
        &self,
        id: &str,
    ) -> Result<npc_game_profile::GameProfileV2, ResourceProfileError> {
        if !GAMES.iter().any(|game| game.id == id) {
            return Err(ResourceProfileError::UnknownProfile);
        }
        self.load_fixed_game_profile(id)
    }

    /// The synthetic review profile is deliberately excluded from the twenty
    /// product game summaries, but the debug-only native capture command still
    /// needs to load its immutable process/window policy. Keeping this entry
    /// point separate prevents the fixture from appearing as a supported game.
    #[cfg(debug_assertions)]
    pub(crate) fn load_debug_synthetic_review_profile(
        &self,
    ) -> Result<npc_game_profile::GameProfileV2, ResourceProfileError> {
        self.load_fixed_game_profile("eclipse-harbor")
    }

    fn load_fixed_game_profile(
        &self,
        id: &str,
    ) -> Result<npc_game_profile::GameProfileV2, ResourceProfileError> {
        if let Some(path) = self.active_content_profile_path(id) {
            if path.is_file() {
                return self.load_active_content_profile(id, &path);
            }
        }
        self.load_builtin_game_profile(id)
    }

    pub(crate) fn load_builtin_game_profile(
        &self,
        id: &str,
    ) -> Result<npc_game_profile::GameProfileV2, ResourceProfileError> {
        let path = self
            .builtin_profile_paths(id)
            .into_iter()
            .find(|path| path.is_file())
            .ok_or(ResourceProfileError::Missing)?;
        let metadata = std::fs::symlink_metadata(&path).map_err(ResourceProfileError::Io)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > 4 * 1024 * 1024
        {
            return Err(ResourceProfileError::UnsafeResource);
        }
        let bytes = std::fs::read(path).map_err(ResourceProfileError::Io)?;
        let profile: npc_game_profile::GameProfileV2 =
            serde_json::from_slice(&bytes).map_err(ResourceProfileError::Decode)?;
        if profile.id != id {
            return Err(ResourceProfileError::IdentityMismatch);
        }
        let report = profile.validate();
        if !report.is_valid() {
            return Err(ResourceProfileError::InvalidProfile);
        }
        Ok(profile)
    }

    pub(crate) fn load_bundled_catalog_profile(
        &self,
        id: &str,
    ) -> Result<npc_game_profile::GameProfileV2, ResourceProfileError> {
        if !GAMES.iter().any(|game| game.id == id) {
            return Err(ResourceProfileError::UnknownProfile);
        }
        self.load_builtin_game_profile(id)
    }

    fn load_active_content_profile(
        &self,
        id: &str,
        path: &Path,
    ) -> Result<npc_game_profile::GameProfileV2, ResourceProfileError> {
        let metadata = std::fs::symlink_metadata(path).map_err(ResourceProfileError::Io)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > 4 * 1024 * 1024
        {
            return Err(ResourceProfileError::UnsafeResource);
        }
        let bytes = std::fs::read(path).map_err(ResourceProfileError::Io)?;
        let active: crate::content_packs::ActiveProfileDocumentV1 =
            serde_json::from_slice(&bytes).map_err(ResourceProfileError::Decode)?;
        if active.schema_version != 1 || active.game_profile_id != id || active.profile.id != id {
            return Err(ResourceProfileError::IdentityMismatch);
        }
        let profile_bytes =
            serde_json::to_vec(&active.profile).map_err(ResourceProfileError::Decode)?;
        let profile = npc_game_profile::load_profile(&profile_bytes)
            .map_err(|_| ResourceProfileError::InvalidProfile)?;
        Ok(profile)
    }

    fn active_content_profile_path(&self, id: &str) -> Option<PathBuf> {
        self.active_content_profile_root
            .as_ref()
            .map(|root| root.join(format!("{id}.json")))
    }

    fn builtin_profile_paths(&self, id: &str) -> Vec<PathBuf> {
        let mut paths = Vec::with_capacity(2);
        if let Some(root) = &self.installed_resource_root {
            paths.push(
                root.join("profiles")
                    .join("games")
                    .join(id)
                    .join("profile.json"),
            );
        }
        paths.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../profiles/games")
                .join(id)
                .join("profile.json"),
        );
        paths
    }

    fn profile_exists(&self, id: &str) -> bool {
        self.active_content_profile_path(id)
            .is_some_and(|path| path.is_file())
            || self
                .builtin_profile_paths(id)
                .into_iter()
                .any(|path| path.is_file())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceProfileError {
    #[error("unknown game profile")]
    UnknownProfile,
    #[error("game profile is missing from the application bundle")]
    Missing,
    #[error("game profile resource is linked, oversized, or not a regular file")]
    UnsafeResource,
    #[error("game profile identity does not match its catalog entry")]
    IdentityMismatch,
    #[error("game profile failed schema validation")]
    InvalidProfile,
    #[error("game profile could not be read: {0}")]
    Io(std::io::Error),
    #[error("game profile JSON is invalid: {0}")]
    Decode(serde_json::Error),
    #[error("saved character content override is invalid: {0}")]
    Overrides(String),
}

pub fn model_summaries() -> Vec<ModelSummary> {
    let catalog = bundled_provider_catalog();
    let executions = catalog
        .content
        .providers
        .iter()
        .map(|provider| (provider.id.as_str(), provider.execution))
        .collect::<std::collections::BTreeMap<_, _>>();
    catalog
        .content
        .models
        .into_iter()
        .map(|model| {
            let installation =
                match model.availability {
                    RouteAvailability::PackCandidateUnqualified
                        if model.upstream_id.starts_with("$user_") =>
                    {
                        ModelInstallation::UserImportRequired
                    }
                    RouteAvailability::PackCandidateUnqualified
                    | RouteAvailability::CatalogOnly => ModelInstallation::CatalogOnly,
                    RouteAvailability::ImplementedAdapter
                    | RouteAvailability::InstalledQualified => ModelInstallation::NotInspected,
                };
            ModelSummary {
                id: model.id,
                display_name: model.display_name,
                purpose: match model.modality {
                    Modality::Llm => "Thinking",
                    Modality::Stt => "Speech in",
                    Modality::Tts => "Voice out",
                    Modality::Embedding | Modality::Rerank => "Memory",
                }
                .into(),
                execution: match executions.get(model.provider_id.as_str()).copied() {
                    Some(ExecutionLocation::Hosted) => "hosted",
                    Some(ExecutionLocation::Local) => "local",
                    Some(ExecutionLocation::ExternalLocal) => "externalLocal",
                    None => "unknown",
                }
                .into(),
                lifecycle: match model.lifecycle {
                    Lifecycle::Stable => "stable",
                    Lifecycle::Experimental => "experimental",
                    Lifecycle::QualificationRequired => "qualificationRequired",
                    Lifecycle::Deprecated => "deprecated",
                }
                .into(),
                installation,
                qualification_note: Some(model.availability_note),
            }
        })
        .collect()
}

pub fn is_known_provider(id: &str) -> bool {
    bundled_provider_catalog()
        .content
        .providers
        .iter()
        .any(|provider| provider.id == id)
}

pub fn credential_reference_for(provider_id: &str) -> Option<String> {
    bundled_provider_catalog()
        .content
        .providers
        .into_iter()
        .find(|provider| provider.id == provider_id && provider.credential.required)
        .map(|provider| format!("providers/{}", provider.id))
}

pub fn credential_provider_name(provider_id: &str) -> Option<String> {
    bundled_provider_catalog()
        .content
        .providers
        .into_iter()
        .find(|provider| provider.id == provider_id && provider.credential.required)
        .map(|provider| provider.display_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FixedPresence;

    impl CredentialPresence for FixedPresence {
        fn status(&self, reference: &str) -> CredentialReferenceStatus {
            if reference == "providers/openai" {
                CredentialReferenceStatus::Present
            } else {
                CredentialReferenceStatus::Missing
            }
        }

        fn save(
            &self,
            _reference: &str,
            _secret: &SecretValue,
        ) -> Result<(), CredentialMutationError> {
            Ok(())
        }

        fn delete(&self, _reference: &str) -> Result<(), CredentialMutationError> {
            Ok(())
        }

        fn availability_detail(&self) -> &'static str {
            "available"
        }
    }

    #[test]
    fn provider_contract_never_contains_values() {
        let summaries = provider_summaries(&FixedPresence);
        let json = serde_json::to_string(&summaries).expect("serialize");
        assert!(json.contains("interactive-npcs/v2/providers/openai"));
        assert!(!json.to_ascii_lowercase().contains("api_key"));
        assert!(!json.to_ascii_lowercase().contains("secret"));
    }

    #[test]
    fn review_provider_contract_names_only_the_review_vault() {
        #[derive(Debug)]
        struct ReviewPresence;
        impl CredentialPresence for ReviewPresence {
            fn status(&self, _reference: &str) -> CredentialReferenceStatus {
                CredentialReferenceStatus::Missing
            }
            fn save(
                &self,
                _reference: &str,
                _secret: &SecretValue,
            ) -> Result<(), CredentialMutationError> {
                Ok(())
            }
            fn delete(&self, _reference: &str) -> Result<(), CredentialMutationError> {
                Ok(())
            }
            fn availability_detail(&self) -> &'static str {
                "available"
            }
            fn credential_namespace(&self) -> &str {
                interactive_npcs_credential_vault::REVIEW_CREDENTIAL_NAMESPACE
            }
        }

        let summaries = provider_summaries(&ReviewPresence);
        let groq = summaries
            .iter()
            .find(|provider| provider.provider_id == "groq")
            .expect("Groq provider");
        assert_eq!(
            groq.credential_reference.as_deref(),
            Some("interactive-npcs/v2/review/providers/groq")
        );
        assert!(summaries.iter().all(|provider| provider
            .credential_reference
            .as_deref()
            .is_none_or(|reference| !reference.starts_with("interactive-npcs/v2/providers/"))));
    }

    #[test]
    fn product_provider_and_model_summaries_are_exact_canonical_catalog_adapters() {
        let canonical = bundled_provider_catalog();
        let provider_ids = provider_summaries(&FixedPresence)
            .into_iter()
            .map(|provider| provider.provider_id)
            .collect::<std::collections::BTreeSet<_>>();
        let canonical_provider_ids = canonical
            .content
            .providers
            .iter()
            .map(|provider| provider.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(provider_ids, canonical_provider_ids);
        assert!(provider_ids.contains("nemotron-local"));
        assert!(provider_ids.contains("chatterbox-local"));
        assert!(provider_ids.contains("qwen-tts-local"));
        assert!(provider_ids.contains("onnx-embedding-local"));

        let model_ids = model_summaries()
            .into_iter()
            .map(|model| model.id)
            .collect::<std::collections::BTreeSet<_>>();
        let canonical_model_ids = canonical
            .content
            .models
            .iter()
            .map(|model| model.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(model_ids, canonical_model_ids);
    }

    #[test]
    fn exactly_twenty_locked_profiles_are_declared() {
        let resources = ResourceCatalog::new(None);
        let summaries = resources.game_summaries();
        assert_eq!(summaries.len(), 20);
        assert!(!summaries.iter().any(|game| game.id == "eclipse-harbor"));
        assert_eq!(
            summaries
                .iter()
                .filter(|game| game.safety == ProfileSafety::OfflineOnly)
                .count(),
            3
        );

        let synthetic = resources
            .load_debug_synthetic_review_profile()
            .expect("hidden synthetic review profile");
        assert_eq!(synthetic.id, "eclipse-harbor");
        assert_eq!(
            synthetic.detection.processes[0].executable,
            "EclipseHarbor.exe"
        );
    }

    #[test]
    fn local_providers_are_credential_free() {
        let summaries = provider_summaries(&FixedPresence);
        let local = summaries
            .iter()
            .find(|provider| provider.provider_id == "llamacpp-local")
            .expect("local provider");
        assert_eq!(local.status, CredentialReferenceStatus::NotRequired);
        assert!(local.credential_reference.is_none());
    }

    #[test]
    fn nvidia_nim_uses_shared_native_credential_reference() {
        let summaries = provider_summaries(&FixedPresence);
        let nim = summaries
            .iter()
            .find(|provider| provider.provider_id == "nvidia-nim")
            .expect("NVIDIA NIM provider");
        assert_eq!(nim.status, CredentialReferenceStatus::Missing);
        assert_eq!(
            nim.credential_reference.as_deref(),
            Some("interactive-npcs/v2/providers/nvidia-nim")
        );
    }

    #[cfg(windows)]
    #[test]
    fn shell_save_is_visible_through_runtime_vault_namespace_without_exposure() {
        const CANARY: &str = "sk-canary-shared-vault-target";
        let reference = format!("tests/namespace-{}", uuid::Uuid::new_v4());
        let shell = SystemCredentialPresence::new().expect("shell vault");
        let runtime =
            interactive_npcs_credential_vault::WindowsCredentialVault::new("interactive-npcs/v2")
                .expect("runtime vault");
        let secret = SecretValue::new(CANARY.as_bytes().to_vec()).expect("secret");
        shell.save(&reference, &secret).expect("native save");
        drop(secret);

        let runtime_secret = runtime.get(&reference).expect("runtime target visibility");
        drop(runtime_secret);
        let response = serde_json::to_string(&shell.status(&reference)).expect("presence status");
        assert_eq!(shell.status(&reference), CredentialReferenceStatus::Present);
        assert!(!response.contains(CANARY));
        shell.delete(&reference).expect("cleanup fixture target");
        assert_eq!(shell.status(&reference), CredentialReferenceStatus::Missing);
    }

    #[cfg(windows)]
    #[test]
    fn review_shell_credential_is_invisible_to_production_namespace() {
        const CANARY: &str = "review-only-canary-secret";
        let reference = format!("tests/review-namespace-{}", uuid::Uuid::new_v4());
        let review = SystemCredentialPresence::new_for_application(
            interactive_npcs_credential_vault::REVIEW_APPLICATION_NAMESPACE,
        )
        .expect("review vault");
        let production = SystemCredentialPresence::new().expect("production vault");
        let secret = SecretValue::new(CANARY.as_bytes().to_vec()).expect("secret");
        review.save(&reference, &secret).expect("review save");
        drop(secret);

        assert_eq!(
            review.status(&reference),
            CredentialReferenceStatus::Present
        );
        assert_eq!(
            production.status(&reference),
            CredentialReferenceStatus::Missing
        );
        review.delete(&reference).expect("cleanup review fixture");
    }
}
