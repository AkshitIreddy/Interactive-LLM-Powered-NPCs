use crate::domain::{
    CatalogState, CredentialReferenceStatus, GameProfileSummary, ModelInstallation, ModelSummary,
    ProfileSafety, ProviderCredentialSummary,
};
use interactive_npcs_credential_vault::{CredentialVault, SecretValue, VaultError};
use std::path::{Path, PathBuf};

/// Shared with `apps/runtime-host/src/bootstrap.rs`. Both processes must resolve
/// the identical Windows Credential Manager target namespace.
pub(crate) const CREDENTIAL_NAMESPACE: &str = "interactive-npcs/v2";

#[derive(Debug, Clone, Copy)]
struct ProviderDeclaration {
    id: &'static str,
    name: &'static str,
    credential_reference: Option<&'static str>,
}

const PROVIDERS: &[ProviderDeclaration] = &[
    ProviderDeclaration {
        id: "openai",
        name: "OpenAI",
        credential_reference: Some("providers/openai"),
    },
    ProviderDeclaration {
        id: "gemini",
        name: "Google Gemini",
        credential_reference: Some("providers/gemini"),
    },
    ProviderDeclaration {
        id: "anthropic",
        name: "Anthropic",
        credential_reference: Some("providers/anthropic"),
    },
    ProviderDeclaration {
        id: "groq",
        name: "Groq",
        credential_reference: Some("providers/groq"),
    },
    ProviderDeclaration {
        id: "cohere",
        name: "Cohere",
        credential_reference: Some("providers/cohere"),
    },
    ProviderDeclaration {
        id: "nvidia-nim",
        name: "NVIDIA NIM",
        credential_reference: Some("providers/nvidia-nim"),
    },
    ProviderDeclaration {
        id: "openai-compatible",
        name: "OpenAI-compatible endpoint",
        credential_reference: Some("providers/openai-compatible"),
    },
    ProviderDeclaration {
        id: "deepgram",
        name: "Deepgram",
        credential_reference: Some("providers/deepgram"),
    },
    ProviderDeclaration {
        id: "assemblyai",
        name: "AssemblyAI",
        credential_reference: Some("providers/assemblyai"),
    },
    ProviderDeclaration {
        id: "elevenlabs",
        name: "ElevenLabs",
        credential_reference: Some("providers/elevenlabs"),
    },
    ProviderDeclaration {
        id: "cartesia",
        name: "Cartesia",
        credential_reference: Some("providers/cartesia"),
    },
    ProviderDeclaration {
        id: "inworld",
        name: "Inworld",
        credential_reference: Some("providers/inworld"),
    },
    ProviderDeclaration {
        id: "llamacpp-local",
        name: "llama.cpp local",
        credential_reference: None,
    },
    ProviderDeclaration {
        id: "moonshine-local",
        name: "Moonshine local",
        credential_reference: None,
    },
    ProviderDeclaration {
        id: "whispercpp-local",
        name: "whisper.cpp local",
        credential_reference: None,
    },
    ProviderDeclaration {
        id: "kokoro-local",
        name: "Kokoro local",
        credential_reference: None,
    },
];

pub trait CredentialPresence: Send + Sync + std::fmt::Debug {
    fn status(&self, reference: &str) -> CredentialReferenceStatus;
    fn save(&self, reference: &str, secret: &SecretValue) -> Result<(), CredentialMutationError>;
    fn delete(&self, reference: &str) -> Result<(), CredentialMutationError>;
    fn availability_detail(&self) -> &'static str;
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
}

#[cfg(windows)]
impl SystemCredentialPresence {
    pub fn new() -> Result<Self, VaultError> {
        Ok(Self {
            vault: interactive_npcs_credential_vault::WindowsCredentialVault::new(
                CREDENTIAL_NAMESPACE,
            )?,
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
}

#[cfg(not(windows))]
#[derive(Debug, Default)]
pub struct SystemCredentialPresence;

#[cfg(not(windows))]
impl SystemCredentialPresence {
    pub fn new() -> Result<Self, VaultError> {
        Ok(Self)
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
}

pub fn provider_summaries(presence: &dyn CredentialPresence) -> Vec<ProviderCredentialSummary> {
    PROVIDERS
        .iter()
        .map(|provider| match provider.credential_reference {
            Some(reference) => {
                let status = presence.status(reference);
                let detail = match status {
                    CredentialReferenceStatus::Present => "A credential reference is available.",
                    CredentialReferenceStatus::Missing => {
                        "No credential is saved for this provider."
                    }
                    CredentialReferenceStatus::Unavailable => presence.availability_detail(),
                    CredentialReferenceStatus::NotRequired => "No credential is required.",
                };
                ProviderCredentialSummary {
                    provider_id: provider.id.into(),
                    display_name: provider.name.into(),
                    credential_reference: Some(format!("{CREDENTIAL_NAMESPACE}/{reference}")),
                    status,
                    detail: detail.into(),
                }
            }
            None => ProviderCredentialSummary {
                provider_id: provider.id.into(),
                display_name: provider.name.into(),
                credential_reference: None,
                status: CredentialReferenceStatus::NotRequired,
                detail: "This local provider does not require a cloud credential.".into(),
            },
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
}

impl ResourceCatalog {
    pub fn new(installed_resource_root: Option<PathBuf>) -> Self {
        Self {
            installed_resource_root,
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

    fn profile_exists(&self, id: &str) -> bool {
        let installed = self.installed_resource_root.as_ref().map(|root| {
            root.join("profiles")
                .join("games")
                .join(id)
                .join("profile.json")
        });
        let development = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../profiles/games")
            .join(id)
            .join("profile.json");
        installed.as_ref().is_some_and(|path| path.is_file()) || development.is_file()
    }
}

pub fn model_summaries() -> Vec<ModelSummary> {
    [
        model(
            "moonshine.stt.medium",
            "Moonshine Medium",
            "Speech in",
            "local",
            "qualificationRequired",
            ModelInstallation::CatalogOnly,
            Some("Windows CPU latency, WER, and exact artifact license must pass qualification."),
        ),
        model(
            "whispercpp.stt.user-selected",
            "Imported whisper.cpp model",
            "Speech in",
            "local",
            "qualificationRequired",
            ModelInstallation::UserImportRequired,
            Some("A validated pack manifest and runtime self-test are required."),
        ),
        model(
            "local.llm.custom-gguf",
            "Imported llama.cpp GGUF",
            "Thinking",
            "local",
            "stable",
            ModelInstallation::UserImportRequired,
            Some("Each model's license, hash, and measured resource envelope must be reviewed."),
        ),
        model(
            "kokoro.tts.onnx",
            "Kokoro ONNX",
            "Voice out",
            "local",
            "qualificationRequired",
            ModelInstallation::CatalogOnly,
            Some(
                "Windows CPU latency, quality, and exact artifact license must pass qualification.",
            ),
        ),
        model(
            "local.embedding.custom-onnx-int8",
            "Imported INT8 ONNX embedding model",
            "Memory",
            "local",
            "stable",
            ModelInstallation::UserImportRequired,
            Some("A validated pack manifest and runtime self-test are required."),
        ),
        model(
            "musetalk.presence.candidate",
            "MuseTalk screen-space candidate",
            "Presence",
            "local",
            "experimental",
            ModelInstallation::CatalogOnly,
            Some("It remains experimental until visual, latency, and 12 GB contention gates pass."),
        ),
    ]
    .into_iter()
    .collect()
}

fn model(
    id: &'static str,
    display_name: &'static str,
    purpose: &'static str,
    execution: &'static str,
    lifecycle: &'static str,
    installation: ModelInstallation,
    qualification_note: Option<&'static str>,
) -> ModelSummary {
    ModelSummary {
        id: id.into(),
        display_name: display_name.into(),
        purpose: purpose.into(),
        execution: execution.into(),
        lifecycle: lifecycle.into(),
        installation,
        qualification_note: qualification_note.map(str::to_owned),
    }
}

pub fn is_known_provider(id: &str) -> bool {
    PROVIDERS.iter().any(|provider| provider.id == id)
}

pub fn credential_reference_for(provider_id: &str) -> Option<&'static str> {
    PROVIDERS
        .iter()
        .find(|provider| provider.id == provider_id)
        .and_then(|provider| provider.credential_reference)
}

pub fn credential_provider_name(provider_id: &str) -> Option<&'static str> {
    PROVIDERS
        .iter()
        .find(|provider| provider.id == provider_id && provider.credential_reference.is_some())
        .map(|provider| provider.name)
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
    fn exactly_twenty_locked_profiles_are_declared() {
        let summaries = ResourceCatalog::new(None).game_summaries();
        assert_eq!(summaries.len(), 20);
        assert_eq!(
            summaries
                .iter()
                .filter(|game| game.safety == ProfileSafety::OfflineOnly)
                .count(),
            3
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
}
