use serde::{Deserialize, Serialize};

pub const GAME_PROFILE_V2_VERSION: &str = "2.0.0";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameProfileV2 {
    pub schema_version: String,
    pub id: String,
    pub display_name: String,
    pub game: GameMetadata,
    pub detection: DetectionMetadata,
    pub safety: SafetyPolicy,
    pub capabilities: CapabilitySet,
    pub content: ContentBundle,
    pub characters: Vec<CharacterProfile>,
    pub defaults: ProfileDefaults,
    pub diagnostics: Vec<DiagnosticContract>,
    pub troubleshooting: Vec<TroubleshootingEntry>,
    pub prompts: GlobalPrompts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameMetadata {
    pub publisher: String,
    pub release_year: u16,
    pub genres: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub official_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectionMetadata {
    /// Executable names and store metadata are used only to locate an eligible
    /// single-player game window. Profiles never describe in-process code.
    pub processes: Vec<ProcessDetection>,
    pub stores: Vec<StoreDetection>,
    pub builds: Vec<BuildCompatibility>,
    pub capture: CaptureContract,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureContract {
    pub preferred_methods: Vec<CaptureMethod>,
    #[serde(default)]
    pub excluded_window_title_regexes: Vec<String>,
    pub allow_exclusive_fullscreen: bool,
    pub fallback: CaptureFallback,
    #[serde(default)]
    pub ui_regions: Vec<UiRegion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiRegion {
    pub id: String,
    pub purpose: UiRegionPurpose,
    pub rect: NormalizedRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiRegionPurpose {
    ExcludeIdentity,
    ExcludeCompositing,
    SubtitleSafeZone,
    DialogueIndicator,
    NameplateHint,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMethod {
    WindowsGraphicsCapture,
    DesktopDuplication,
    AudioSubtitles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureFallback {
    AudioSubtitles,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessDetection {
    pub executable: String,
    /// Primary process candidates are OR alternatives: a match on any entry
    /// with `required = true` satisfies process detection. False entries are
    /// auxiliary launcher/telemetry hints and cannot start a session alone.
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_title_regex: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreDetection {
    pub store: StoreKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default)]
    pub install_directory_hints: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreKind {
    Steam,
    Epic,
    Gog,
    Microsoft,
    Standalone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildCompatibility {
    pub build_id: String,
    pub status: BuildStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildStatus {
    Supported,
    Experimental,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyPolicy {
    pub single_player_only: bool,
    pub online_policy: OnlinePolicy,
    pub anti_cheat_policy: AntiCheatPolicy,
    pub declarative_only: bool,
    #[serde(default)]
    pub risk_notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnlinePolicy {
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiCheatPolicy {
    BlockWhenDetected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilitySet {
    pub conversation: CapabilityClaim,
    pub identity: CapabilityClaim,
    pub capture: CapabilityClaim,
    pub subtitles: CapabilityClaim,
    pub memory: CapabilityClaim,
    pub screen_space_lip_sync: CapabilityClaim,
}

impl CapabilitySet {
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &CapabilityClaim)> {
        [
            ("conversation", &self.conversation),
            ("identity", &self.identity),
            ("capture", &self.capture),
            ("subtitles", &self.subtitles),
            ("memory", &self.memory),
            ("screen_space_lip_sync", &self.screen_space_lip_sync),
        ]
        .into_iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityClaim {
    pub tier: CapabilityTier,
    pub evidence: Vec<CapabilityEvidence>,
    pub fallback: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityEvidence {
    pub kind: CapabilityEvidenceKind,
    pub reference: String,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityEvidenceKind {
    DeterministicReplay,
    DeclarativeContract,
    NotVerified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityTier {
    Unsupported,
    Experimental,
    ReplayVerified,
    LiveCertified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentBundle {
    pub world_lore: String,
    pub spoiler_policy: String,
    pub spoiler_tiers: Vec<SpoilerTier>,
    pub background_npc_rules: Vec<String>,
    pub provenance: Vec<ProvenanceRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpoilerTier {
    pub id: String,
    pub description: String,
    pub default_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvenanceRecord {
    pub id: String,
    pub title: String,
    pub kind: ProvenanceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceKind {
    Original,
    Official,
    Licensed,
    LegacyImport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterProfile {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub biography: String,
    pub personality: String,
    pub dialogue_style: String,
    #[serde(default)]
    pub background_npc: bool,
    pub prompt: CharacterPrompt,
    pub voice: VoiceDefaults,
    pub identity: IdentityContract,
    pub model: ModelDefaults,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterPrompt {
    pub role: String,
    pub objectives: Vec<String>,
    pub constraints: Vec<String>,
    #[serde(default)]
    pub knowledge_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceDefaults {
    pub description: String,
    pub locale: String,
    #[serde(default)]
    pub style_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_voice_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityContract {
    pub strategy: IdentityStrategy,
    #[serde(default)]
    pub evidence: Vec<IdentityEvidence>,
    pub fallback: IdentityFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityStrategy {
    ExplicitSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityEvidence {
    ExplicitSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityFallback {
    ExplicitSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDefaults {
    pub quality_tier: ModelQualityTier,
    pub context_budget: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelQualityTier {
    Fast,
    Balanced,
    Quality,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDefaults {
    pub character_id: String,
    pub model: ModelDefaults,
    pub voice: VoiceDefaults,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticContract {
    pub id: String,
    pub severity: DiagnosticSeverity,
    pub check: String,
    pub remediation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TroubleshootingEntry {
    pub symptom: String,
    pub cause: String,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalPrompts {
    pub system_preamble: String,
    pub safety_rules: Vec<String>,
    pub background_npc_template: String,
}
