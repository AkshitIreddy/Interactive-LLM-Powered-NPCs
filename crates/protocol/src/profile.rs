use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreKind {
    Steam,
    Epic,
    Gog,
    MicrosoftStore,
    Standalone,
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Authored,
    ReplayVerified,
    LiveGameCertified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityLevel {
    Unsupported,
    Experimental,
    ReplayVerified,
    LiveGameCertified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMethod {
    WindowsGraphicsCapture,
    DesktopDuplication,
    AudioSubtitlesOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityEvidenceKind {
    ExplicitSelection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpoilerTier {
    None,
    EarlyGame,
    MidGame,
    LateGame,
    Ending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Blocking,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCheckKind {
    ProcessRunning,
    ProcessNotRunning,
    ExecutableHashAllowed,
    FileExists,
    StoreManifestFound,
    CaptureAvailable,
    OfflineModeConfirmed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreDetectionV2 {
    pub store: StoreKind,
    pub app_ids: Vec<String>,
    pub registry_keys: Vec<String>,
    pub common_install_subdirectories: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessDetectionV2 {
    pub executable_names: Vec<String>,
    pub required_product_names: Vec<String>,
    pub blocked_companion_processes: Vec<String>,
    pub window_title_hints: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildCompatibilityV2 {
    pub build_id: String,
    pub executable_sha256: Option<String>,
    pub file_version: Option<String>,
    pub status: VerificationStatus,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureRulesV2 {
    pub preferred_methods: Vec<CaptureMethod>,
    pub excluded_window_title_patterns: Vec<String>,
    pub exclude_overlays: bool,
    pub allow_desktop_duplication_fallback: bool,
    pub true_exclusive_fullscreen_fallback: CaptureMethod,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameSafetyPolicyV2 {
    pub single_player_only: bool,
    pub offline_only: bool,
    pub block_when_anti_cheat_detected: bool,
    pub anti_cheat_process_markers: Vec<String>,
    pub online_mode_process_or_window_markers: Vec<String>,
    pub user_confirmation_required: bool,
    pub safety_notice: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityMatrixV2 {
    pub conversation: CapabilityLevel,
    pub memory: CapabilityLevel,
    pub subtitles: CapabilityLevel,
    pub stable_identity: CapabilityLevel,
    pub screen_space_lip_sync: CapabilityLevel,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityEvidenceRuleV2 {
    pub kind: IdentityEvidenceKind,
    pub priority: u32,
    pub minimum_confidence: f32,
    pub stale_after_ms: u64,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityStrategyV2 {
    pub evidence: Vec<IdentityEvidenceRuleV2>,
    pub ambiguity_margin: f32,
    pub reacquisition_timeout_ms: u64,
    pub allow_manual_selection: bool,
    pub offscreen_behavior: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentProvenanceV2 {
    pub provenance_id: String,
    pub title: String,
    pub author: String,
    pub source_url: Option<String>,
    pub license: String,
    pub original_authored_content: bool,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoreSectionV2 {
    pub section_id: String,
    pub title: String,
    pub body: String,
    pub spoiler_tier: SpoilerTier,
    pub provenance_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalityV2 {
    pub traits: Vec<String>,
    pub speech_style: String,
    pub motivations: Vec<String>,
    pub boundaries: Vec<String>,
    pub emotional_baseline: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceRecommendationV2 {
    pub voice_profile_id: String,
    pub characteristics: Vec<String>,
    pub language: String,
    pub do_not_imitate_performer: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterProfileV2 {
    pub character_id: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub biography: String,
    pub personality: PersonalityV2,
    pub spoiler_tier: SpoilerTier,
    pub prompt_addendum: String,
    pub voice: VoiceRecommendationV2,
    pub provenance_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSetV2 {
    pub system: String,
    pub background_npc: String,
    pub offscreen_npc: String,
    pub memory_extraction: String,
    pub relationship_update: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDefaultsV2 {
    pub execution_mode: String,
    pub llm_model_profile: String,
    pub stt_model_profile: String,
    pub tts_model_profile: String,
    pub embedding_model_profile: String,
    pub background_voice_pool: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticRuleV2 {
    pub diagnostic_id: String,
    pub check: DiagnosticCheckKind,
    pub severity: DiagnosticSeverity,
    /// Declarative values consumed by a runtime-owned check implementation.
    pub parameters: BTreeMap<String, String>,
    pub success_message: String,
    pub failure_message: String,
    pub remediation_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TroubleshootingEntryV2 {
    pub remediation_id: String,
    pub title: String,
    pub symptoms: Vec<String>,
    pub steps: Vec<String>,
    pub fallback_behavior: String,
}

/// Versioned serde/wire representation of a game-integration summary.
///
/// This is deliberately not named `GameProfileV2`: the authoritative authored
/// profile model is `npc_game_profile::GameProfileV2`. Renaming this Rust type
/// does not change its field names or serialized representation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameProfileWireV2 {
    pub schema_version: u32,
    pub profile_id: String,
    pub display_name: String,
    pub publisher: String,
    pub verification_status: VerificationStatus,
    pub stores: Vec<StoreDetectionV2>,
    pub process_detection: ProcessDetectionV2,
    pub builds: Vec<BuildCompatibilityV2>,
    pub capture: CaptureRulesV2,
    pub safety: GameSafetyPolicyV2,
    pub capabilities: CapabilityMatrixV2,
    pub identity: IdentityStrategyV2,
    pub provenance: Vec<ContentProvenanceV2>,
    pub world_lore: Vec<LoreSectionV2>,
    pub characters: Vec<CharacterProfileV2>,
    pub prompts: PromptSetV2,
    pub defaults: ProviderDefaultsV2,
    pub diagnostics: Vec<DiagnosticRuleV2>,
    pub troubleshooting: Vec<TroubleshootingEntryV2>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameProfileWireCatalogV1 {
    pub schema_version: u32,
    pub catalog_revision: String,
    pub profiles: Vec<GameProfileWireV2>,
}

impl GameProfileWireCatalogV1 {
    pub const SCHEMA_VERSION: u32 = 1;
    pub const RELEASE_CANDIDATE_PROFILE_COUNT: usize = 20;

    pub fn validate(&self) -> Result<(), ProfileValidationError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(ProfileValidationError::UnsupportedCatalogSchema(
                self.schema_version,
            ));
        }
        if self.catalog_revision.trim().is_empty() || self.catalog_revision.len() > 128 {
            return Err(ProfileValidationError::InvalidCatalogRevision);
        }
        let mut ids = BTreeSet::new();
        for profile in &self.profiles {
            profile.validate()?;
            if !ids.insert(&profile.profile_id) {
                return Err(ProfileValidationError::DuplicateId("game profile"));
            }
        }
        Ok(())
    }

    /// Applies the hard release gate from the 2.0 plan: exactly twenty complete,
    /// deterministically replay-verified (or stronger) authored profiles.
    pub fn validate_release_candidate(&self) -> Result<(), ProfileValidationError> {
        self.validate()?;
        if self.profiles.len() != Self::RELEASE_CANDIDATE_PROFILE_COUNT {
            return Err(ProfileValidationError::InvalidReleaseCatalogSize {
                expected: Self::RELEASE_CANDIDATE_PROFILE_COUNT,
                actual: self.profiles.len(),
            });
        }
        if self
            .profiles
            .iter()
            .any(|profile| profile.verification_status == VerificationStatus::Authored)
        {
            return Err(ProfileValidationError::ProfileNotReplayVerified);
        }
        Ok(())
    }
}

impl GameProfileWireV2 {
    pub const SCHEMA_VERSION: u32 = 2;

    pub fn validate(&self) -> Result<(), ProfileValidationError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(ProfileValidationError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        validate_id(&self.profile_id)?;
        validate_authored_text(&self.display_name, 2, 128, "display name")?;
        validate_authored_text(&self.publisher, 2, 128, "publisher")?;
        self.validate_detection()?;
        self.validate_safety()?;
        self.validate_identity()?;
        self.validate_content()?;
        self.validate_external_only_capabilities()?;
        self.validate_diagnostics()?;
        Ok(())
    }

    fn validate_detection(&self) -> Result<(), ProfileValidationError> {
        if self.stores.is_empty() || self.process_detection.executable_names.is_empty() {
            return Err(ProfileValidationError::MissingDetection);
        }
        for store in &self.stores {
            if store.app_ids.is_empty()
                && store.registry_keys.is_empty()
                && store.common_install_subdirectories.is_empty()
            {
                return Err(ProfileValidationError::MissingDetection);
            }
        }
        for executable in &self.process_detection.executable_names {
            if executable.contains(['/', '\\'])
                || !executable.to_ascii_lowercase().ends_with(".exe")
                || executable.len() > 260
            {
                return Err(ProfileValidationError::UnsafeExecutableName(
                    executable.clone(),
                ));
            }
        }
        if self.capture.preferred_methods.is_empty() {
            return Err(ProfileValidationError::MissingCaptureRule);
        }
        if self.builds.iter().any(|build| {
            build.build_id.is_empty()
                || build
                    .executable_sha256
                    .as_ref()
                    .is_some_and(|value| !is_sha256(value))
        }) {
            return Err(ProfileValidationError::InvalidBuildRule);
        }
        Ok(())
    }

    fn validate_safety(&self) -> Result<(), ProfileValidationError> {
        if !self.safety.single_player_only
            || !self.safety.block_when_anti_cheat_detected
            || self.safety.safety_notice.trim().is_empty()
        {
            return Err(ProfileValidationError::UnsafePolicy);
        }
        Ok(())
    }

    fn validate_identity(&self) -> Result<(), ProfileValidationError> {
        if self.identity.evidence.len() != 1
            || self.identity.evidence[0].kind != IdentityEvidenceKind::ExplicitSelection
            || !self.identity.allow_manual_selection
        {
            return Err(ProfileValidationError::InvalidIdentityStrategy);
        }
        if !self.identity.ambiguity_margin.is_finite()
            || !(0.0..=1.0).contains(&self.identity.ambiguity_margin)
            || self.identity.reacquisition_timeout_ms == 0
        {
            return Err(ProfileValidationError::InvalidIdentityStrategy);
        }
        let mut priorities = BTreeSet::new();
        for evidence in &self.identity.evidence {
            if !priorities.insert(evidence.priority)
                || !evidence.minimum_confidence.is_finite()
                || !(0.0..=1.0).contains(&evidence.minimum_confidence)
                || evidence.stale_after_ms == 0
            {
                return Err(ProfileValidationError::InvalidIdentityStrategy);
            }
        }
        validate_authored_text(
            &self.identity.offscreen_behavior,
            40,
            2_048,
            "offscreen behavior",
        )
    }

    fn validate_content(&self) -> Result<(), ProfileValidationError> {
        if self.provenance.is_empty() || self.world_lore.len() < 2 || self.characters.len() < 2 {
            return Err(ProfileValidationError::IncompleteAuthoredContent);
        }
        let provenance_ids: BTreeSet<_> = self
            .provenance
            .iter()
            .map(|item| item.provenance_id.as_str())
            .collect();
        if provenance_ids.len() != self.provenance.len() {
            return Err(ProfileValidationError::DuplicateId("provenance"));
        }
        for source in &self.provenance {
            validate_id(&source.provenance_id)?;
            validate_authored_text(&source.title, 2, 256, "provenance title")?;
            validate_authored_text(&source.author, 2, 256, "provenance author")?;
            validate_authored_text(&source.license, 2, 256, "provenance license")?;
            if source
                .source_url
                .as_ref()
                .is_some_and(|url| !valid_https_url(url))
            {
                return Err(ProfileValidationError::InvalidProvenance);
            }
        }
        let mut lore_ids = BTreeSet::new();
        let mut total_lore_bytes = 0usize;
        for lore in &self.world_lore {
            validate_id(&lore.section_id)?;
            if !lore_ids.insert(&lore.section_id) {
                return Err(ProfileValidationError::DuplicateId("lore section"));
            }
            validate_authored_text(&lore.title, 2, 256, "lore title")?;
            validate_authored_text(&lore.body, 120, 32 * 1024, "lore body")?;
            validate_references(&lore.provenance_ids, &provenance_ids)?;
            total_lore_bytes = total_lore_bytes.saturating_add(lore.body.len());
        }
        if total_lore_bytes < 500 {
            return Err(ProfileValidationError::IncompleteAuthoredContent);
        }

        let mut character_ids = BTreeSet::new();
        for character in &self.characters {
            validate_id(&character.character_id)?;
            if !character_ids.insert(&character.character_id) {
                return Err(ProfileValidationError::DuplicateId("character"));
            }
            validate_authored_text(&character.display_name, 2, 128, "character name")?;
            validate_authored_text(&character.biography, 160, 32 * 1024, "character biography")?;
            validate_authored_text(
                &character.personality.speech_style,
                40,
                4_096,
                "speech style",
            )?;
            validate_authored_text(
                &character.personality.emotional_baseline,
                20,
                2_048,
                "emotional baseline",
            )?;
            if character.personality.traits.len() < 3
                || character.personality.motivations.is_empty()
                || character.personality.boundaries.is_empty()
            {
                return Err(ProfileValidationError::IncompleteCharacter(
                    character.character_id.clone(),
                ));
            }
            validate_authored_text(&character.prompt_addendum, 60, 8 * 1024, "character prompt")?;
            validate_id(&character.voice.voice_profile_id)?;
            if !character.voice.do_not_imitate_performer
                || character.voice.characteristics.len() < 2
                || character.voice.language.is_empty()
            {
                return Err(ProfileValidationError::UnsafeVoiceProfile(
                    character.character_id.clone(),
                ));
            }
            validate_references(&character.provenance_ids, &provenance_ids)?;
        }

        for (name, prompt) in [
            ("system prompt", self.prompts.system.as_str()),
            (
                "background NPC prompt",
                self.prompts.background_npc.as_str(),
            ),
            ("offscreen NPC prompt", self.prompts.offscreen_npc.as_str()),
            ("memory prompt", self.prompts.memory_extraction.as_str()),
            (
                "relationship prompt",
                self.prompts.relationship_update.as_str(),
            ),
        ] {
            validate_authored_text(prompt, 80, 32 * 1024, name)?;
        }
        if self.defaults.background_voice_pool.len() < 2 {
            return Err(ProfileValidationError::IncompleteAuthoredContent);
        }
        Ok(())
    }

    fn validate_external_only_capabilities(&self) -> Result<(), ProfileValidationError> {
        if self.capabilities.screen_space_lip_sync != CapabilityLevel::Experimental {
            return Err(ProfileValidationError::InvalidExternalCapability(
                "screen_space_lip_sync",
            ));
        }
        if !self
            .capture
            .preferred_methods
            .contains(&CaptureMethod::AudioSubtitlesOnly)
        {
            return Err(ProfileValidationError::MissingCaptureRule);
        }
        Ok(())
    }

    fn validate_diagnostics(&self) -> Result<(), ProfileValidationError> {
        if self.diagnostics.len() < 2 || self.troubleshooting.len() < 2 {
            return Err(ProfileValidationError::IncompleteDiagnostics);
        }
        let remediation_ids: BTreeSet<_> = self
            .troubleshooting
            .iter()
            .map(|item| item.remediation_id.as_str())
            .collect();
        if remediation_ids.len() != self.troubleshooting.len() {
            return Err(ProfileValidationError::DuplicateId("remediation"));
        }
        for entry in &self.troubleshooting {
            validate_id(&entry.remediation_id)?;
            validate_authored_text(&entry.title, 4, 256, "troubleshooting title")?;
            if entry.symptoms.is_empty() || entry.steps.is_empty() {
                return Err(ProfileValidationError::IncompleteDiagnostics);
            }
            for step in &entry.steps {
                validate_authored_text(step, 10, 2_048, "troubleshooting step")?;
            }
            validate_authored_text(&entry.fallback_behavior, 20, 2_048, "fallback behavior")?;
        }
        let mut diagnostic_ids = BTreeSet::new();
        for diagnostic in &self.diagnostics {
            validate_id(&diagnostic.diagnostic_id)?;
            if !diagnostic_ids.insert(&diagnostic.diagnostic_id) {
                return Err(ProfileValidationError::DuplicateId("diagnostic"));
            }
            if diagnostic.parameters.len() > 32
                || diagnostic
                    .parameters
                    .iter()
                    .any(|(key, value)| key.len() > 128 || value.len() > 2_048)
            {
                return Err(ProfileValidationError::UnsafeDiagnosticParameters);
            }
            if diagnostic
                .remediation_id
                .as_ref()
                .is_some_and(|id| !remediation_ids.contains(id.as_str()))
            {
                return Err(ProfileValidationError::UnknownReference("remediation"));
            }
        }
        Ok(())
    }
}

/// Experimental data-only behavior for capturable single-player games without an
/// authored profile. It uses manual target selection, external capture, audio,
/// subtitles, and optionally experimental screen-space animation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenericGameModeV1 {
    pub manual_game_name: String,
    pub executable_name: String,
    pub target_display_name: String,
    pub capture_method: CaptureMethod,
    pub enable_conversation: bool,
    pub enable_memory: bool,
    pub enable_audio: bool,
    pub enable_subtitles: bool,
    pub experimental_screen_identity: bool,
    pub experimental_screen_space_lip_sync: bool,
    pub safety: GameSafetyPolicyV2,
}

impl GenericGameModeV1 {
    pub fn validate(&self) -> Result<(), ProfileValidationError> {
        validate_authored_text(&self.manual_game_name, 2, 128, "manual game name")?;
        validate_authored_text(&self.target_display_name, 1, 128, "manual target name")?;
        if self.executable_name.contains(['/', '\\'])
            || !self.executable_name.to_ascii_lowercase().ends_with(".exe")
        {
            return Err(ProfileValidationError::UnsafeExecutableName(
                self.executable_name.clone(),
            ));
        }
        if !self.enable_conversation || !self.enable_audio || !self.enable_subtitles {
            return Err(ProfileValidationError::InvalidGenericMode);
        }
        if !self.safety.single_player_only || !self.safety.block_when_anti_cheat_detected {
            return Err(ProfileValidationError::UnsafePolicy);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Error)]
pub enum ProfileValidationError {
    #[error("unsupported game profile schema {0}")]
    UnsupportedSchema(u32),
    #[error("unsupported game profile catalog schema {0}")]
    UnsupportedCatalogSchema(u32),
    #[error("game profile catalog revision is missing or too large")]
    InvalidCatalogRevision,
    #[error("release catalog must contain {expected} profiles, found {actual}")]
    InvalidReleaseCatalogSize { expected: usize, actual: usize },
    #[error("every release profile must be replay-verified or live-game-certified")]
    ProfileNotReplayVerified,
    #[error("stable identifier is malformed")]
    InvalidId,
    #[error("{0} is missing, looks like a placeholder, or is outside its size bound")]
    InvalidAuthoredText(&'static str),
    #[error("game/store/process detection is incomplete")]
    MissingDetection,
    #[error("unsafe executable name: {0}")]
    UnsafeExecutableName(String),
    #[error("capture rule is missing")]
    MissingCaptureRule,
    #[error("build compatibility rule is invalid")]
    InvalidBuildRule,
    #[error("profile is not constrained to safe single-player behavior")]
    UnsafePolicy,
    #[error("identity strategy is incomplete or invalid")]
    InvalidIdentityStrategy,
    #[error("authored lore, character, prompt, or provenance content is incomplete")]
    IncompleteAuthoredContent,
    #[error("duplicate {0} identifier")]
    DuplicateId(&'static str),
    #[error("content provenance is invalid")]
    InvalidProvenance,
    #[error("unknown {0} reference")]
    UnknownReference(&'static str),
    #[error("character profile is incomplete: {0}")]
    IncompleteCharacter(String),
    #[error("character voice profile could enable performer imitation: {0}")]
    UnsafeVoiceProfile(String),
    #[error("capability {0} is outside the generic external-only contract")]
    InvalidExternalCapability(&'static str),
    #[error("diagnostics or troubleshooting are incomplete")]
    IncompleteDiagnostics,
    #[error("diagnostic parameters exceed the data-only contract")]
    UnsafeDiagnosticParameters,
    #[error("generic mode must remain conversation/audio/subtitles-only")]
    InvalidGenericMode,
}

fn validate_id(value: &str) -> Result<(), ProfileValidationError> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.' | b'_')
        })
    {
        Err(ProfileValidationError::InvalidId)
    } else {
        Ok(())
    }
}

fn validate_authored_text(
    value: &str,
    minimum: usize,
    maximum: usize,
    name: &'static str,
) -> Result<(), ProfileValidationError> {
    let normalized = value.trim().to_ascii_lowercase();
    const PLACEHOLDERS: [&str; 6] = [
        "todo",
        "tbd",
        "lorem ipsum",
        "placeholder",
        "fill this",
        "coming soon",
    ];
    if value.trim().len() < minimum
        || value.len() > maximum
        || PLACEHOLDERS
            .iter()
            .any(|placeholder| normalized.contains(placeholder))
    {
        Err(ProfileValidationError::InvalidAuthoredText(name))
    } else {
        Ok(())
    }
}

fn validate_references(
    references: &[String],
    known: &BTreeSet<&str>,
) -> Result<(), ProfileValidationError> {
    if references.is_empty() || references.iter().any(|id| !known.contains(id.as_str())) {
        Err(ProfileValidationError::UnknownReference("provenance"))
    } else {
        Ok(())
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_https_url(value: &str) -> bool {
    value.starts_with("https://") && value.len() <= 2_048 && !value.contains(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn authored(seed: &str, minimum: usize) -> String {
        let sentence = format!("{seed} is original profile material written for deterministic validation and safe contextual conversation. ");
        sentence.repeat((minimum / sentence.len()) + 2)
    }

    fn valid_profile() -> GameProfileWireV2 {
        let provenance = ContentProvenanceV2 {
            provenance_id: "original-profile".into(),
            title: "Original profile material".into(),
            author: "NPC 2.0 contributors".into(),
            source_url: None,
            license: "MIT".into(),
            original_authored_content: true,
            notes: "No copied wiki prose.".into(),
        };
        let character = |id: &str, name: &str| CharacterProfileV2 {
            character_id: id.into(),
            display_name: name.into(),
            aliases: vec![],
            biography: authored(name, 180),
            personality: PersonalityV2 {
                traits: vec!["observant".into(), "guarded".into(), "loyal".into()],
                speech_style: authored("Speech", 50),
                motivations: vec!["Protect allies".into()],
                boundaries: vec!["Does not know future events".into()],
                emotional_baseline: authored("Baseline", 24),
            },
            spoiler_tier: SpoilerTier::None,
            prompt_addendum: authored("Character prompt", 80),
            voice: VoiceRecommendationV2 {
                voice_profile_id: format!("{id}-voice"),
                characteristics: vec!["grounded".into(), "measured".into()],
                language: "en-US".into(),
                do_not_imitate_performer: true,
            },
            provenance_ids: vec!["original-profile".into()],
        };
        GameProfileWireV2 {
            schema_version: 2,
            profile_id: "fixture-game".into(),
            display_name: "Fixture Game".into(),
            publisher: "Fixture Studio".into(),
            verification_status: VerificationStatus::ReplayVerified,
            stores: vec![StoreDetectionV2 {
                store: StoreKind::Steam,
                app_ids: vec!["123".into()],
                registry_keys: vec![],
                common_install_subdirectories: vec!["Fixture Game".into()],
            }],
            process_detection: ProcessDetectionV2 {
                executable_names: vec!["fixture.exe".into()],
                required_product_names: vec![],
                blocked_companion_processes: vec![],
                window_title_hints: vec!["Fixture Game".into()],
            },
            builds: vec![BuildCompatibilityV2 {
                build_id: "replay-fixture".into(),
                executable_sha256: None,
                file_version: Some("1.0.0".into()),
                status: VerificationStatus::ReplayVerified,
                notes: "Data-only replay.".into(),
            }],
            capture: CaptureRulesV2 {
                preferred_methods: vec![
                    CaptureMethod::WindowsGraphicsCapture,
                    CaptureMethod::AudioSubtitlesOnly,
                ],
                excluded_window_title_patterns: vec![],
                exclude_overlays: true,
                allow_desktop_duplication_fallback: true,
                true_exclusive_fullscreen_fallback: CaptureMethod::AudioSubtitlesOnly,
            },
            safety: GameSafetyPolicyV2 {
                single_player_only: true,
                offline_only: false,
                block_when_anti_cheat_detected: true,
                anti_cheat_process_markers: vec![],
                online_mode_process_or_window_markers: vec![],
                user_confirmation_required: true,
                safety_notice: "Use only in unprotected single-player play.".into(),
            },
            capabilities: CapabilityMatrixV2 {
                conversation: CapabilityLevel::ReplayVerified,
                memory: CapabilityLevel::ReplayVerified,
                subtitles: CapabilityLevel::ReplayVerified,
                stable_identity: CapabilityLevel::ReplayVerified,
                screen_space_lip_sync: CapabilityLevel::Experimental,
            },
            identity: IdentityStrategyV2 {
                evidence: vec![IdentityEvidenceRuleV2 {
                    kind: IdentityEvidenceKind::ExplicitSelection,
                    priority: 1,
                    minimum_confidence: 1.0,
                    stale_after_ms: 60_000,
                    notes: "Manual selection is authoritative.".into(),
                }],
                ambiguity_margin: 0.1,
                reacquisition_timeout_ms: 1_000,
                allow_manual_selection: true,
                offscreen_behavior: authored("Offscreen", 50),
            },
            provenance: vec![provenance],
            world_lore: vec![
                LoreSectionV2 {
                    section_id: "world".into(),
                    title: "The world".into(),
                    body: authored("World", 300),
                    spoiler_tier: SpoilerTier::None,
                    provenance_ids: vec!["original-profile".into()],
                },
                LoreSectionV2 {
                    section_id: "factions".into(),
                    title: "Factions".into(),
                    body: authored("Factions", 300),
                    spoiler_tier: SpoilerTier::EarlyGame,
                    provenance_ids: vec!["original-profile".into()],
                },
            ],
            characters: vec![character("arden", "Arden"), character("mira", "Mira")],
            prompts: PromptSetV2 {
                system: authored("System", 100),
                background_npc: authored("Background", 100),
                offscreen_npc: authored("Offscreen", 100),
                memory_extraction: authored("Memory", 100),
                relationship_update: authored("Relationship", 100),
            },
            defaults: ProviderDefaultsV2 {
                execution_mode: "hybrid".into(),
                llm_model_profile: "balanced-chat".into(),
                stt_model_profile: "fast-english".into(),
                tts_model_profile: "local-natural".into(),
                embedding_model_profile: "local-small".into(),
                background_voice_pool: vec!["grounded-low".into(), "bright-mid".into()],
            },
            diagnostics: vec![
                DiagnosticRuleV2 {
                    diagnostic_id: "process-found".into(),
                    check: DiagnosticCheckKind::ProcessRunning,
                    severity: DiagnosticSeverity::Blocking,
                    parameters: BTreeMap::from([("process".into(), "fixture.exe".into())]),
                    success_message: "Game found.".into(),
                    failure_message: "Start the game.".into(),
                    remediation_id: Some("start-game".into()),
                },
                DiagnosticRuleV2 {
                    diagnostic_id: "capture-ready".into(),
                    check: DiagnosticCheckKind::CaptureAvailable,
                    severity: DiagnosticSeverity::Warning,
                    parameters: BTreeMap::new(),
                    success_message: "Capture ready.".into(),
                    failure_message: "Audio mode remains available.".into(),
                    remediation_id: Some("capture-help".into()),
                },
            ],
            troubleshooting: vec![
                TroubleshootingEntryV2 {
                    remediation_id: "start-game".into(),
                    title: "Game is not detected".into(),
                    symptoms: vec!["Start is unavailable".into()],
                    steps: vec!["Launch the single-player game, then run detection again.".into()],
                    fallback_behavior: "Manual executable selection remains available.".into(),
                },
                TroubleshootingEntryV2 {
                    remediation_id: "capture-help".into(),
                    title: "Capture is unavailable".into(),
                    symptoms: vec!["No game preview".into()],
                    steps: vec!["Use borderless windowed mode and retry capture.".into()],
                    fallback_behavior: "Conversation continues with audio and subtitles.".into(),
                },
            ],
        }
    }

    #[test]
    fn complete_profile_passes_deterministic_validation() {
        assert_eq!(valid_profile().validate(), Ok(()));
    }

    #[test]
    fn wire_profile_name_is_distinct_and_serialized_shape_is_stable() {
        let profile = valid_profile();
        assert!(std::any::type_name::<GameProfileWireV2>().ends_with("GameProfileWireV2"));

        let encoded = serde_json::to_value(&profile).expect("wire profile serializes");
        assert_eq!(encoded["schema_version"], 2);
        assert_eq!(encoded["profile_id"], "fixture-game");
        assert!(encoded.get("wire_type").is_none());

        let decoded: GameProfileWireV2 =
            serde_json::from_value(encoded).expect("unchanged wire shape deserializes");
        assert_eq!(decoded, profile);
    }

    #[test]
    fn placeholder_content_is_rejected() {
        let mut profile = valid_profile();
        profile.characters[0].biography = "TODO: write biography later".into();
        assert!(matches!(
            profile.validate(),
            Err(ProfileValidationError::InvalidAuthoredText(
                "character biography"
            ))
        ));
    }

    #[test]
    fn screen_space_lip_sync_cannot_claim_more_than_experimental() {
        let mut profile = valid_profile();
        profile.capabilities.screen_space_lip_sync = CapabilityLevel::ReplayVerified;
        assert_eq!(
            profile.validate(),
            Err(ProfileValidationError::InvalidExternalCapability(
                "screen_space_lip_sync"
            ))
        );
    }

    #[test]
    fn generic_mode_requires_conversation_audio_and_subtitles() {
        let generic = GenericGameModeV1 {
            manual_game_name: "Unknown RPG".into(),
            executable_name: "unknown.exe".into(),
            target_display_name: "Innkeeper".into(),
            capture_method: CaptureMethod::AudioSubtitlesOnly,
            enable_conversation: true,
            enable_memory: true,
            enable_audio: true,
            enable_subtitles: false,
            experimental_screen_identity: true,
            experimental_screen_space_lip_sync: false,
            safety: valid_profile().safety,
        };
        assert_eq!(
            generic.validate(),
            Err(ProfileValidationError::InvalidGenericMode)
        );
    }

    #[test]
    fn generic_mode_remains_a_standalone_valid_wire_contract() {
        let generic = GenericGameModeV1 {
            manual_game_name: "Unknown RPG".into(),
            executable_name: "unknown.exe".into(),
            target_display_name: "Innkeeper".into(),
            capture_method: CaptureMethod::WindowsGraphicsCapture,
            enable_conversation: true,
            enable_memory: true,
            enable_audio: true,
            enable_subtitles: true,
            experimental_screen_identity: false,
            experimental_screen_space_lip_sync: false,
            safety: valid_profile().safety,
        };

        assert_eq!(generic.validate(), Ok(()));
        let encoded = serde_json::to_string(&generic).expect("generic mode serializes");
        let decoded: GenericGameModeV1 =
            serde_json::from_str(&encoded).expect("generic mode deserializes");
        assert_eq!(decoded, generic);
    }

    proptest! {
        #[test]
        fn arbitrary_executable_paths_are_rejected(prefix in "[A-Za-z0-9]{1,20}") {
            let mut profile = valid_profile();
            profile.process_detection.executable_names = vec![format!("../{prefix}.exe")];
            prop_assert!(matches!(profile.validate(), Err(ProfileValidationError::UnsafeExecutableName(_))));
        }
    }
}
