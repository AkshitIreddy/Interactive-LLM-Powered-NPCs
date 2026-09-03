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
    /// Provider-agnostic product recommendations. Provider/model IDs remain in
    /// independently swappable loadout catalogs, never in game profiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendations: Option<RecommendationPolicy>,
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
    /// Canonical, authored knowledge records. Runtime observations and summaries
    /// remain in `npc-memory`; they must never be written back into a profile.
    #[serde(default)]
    pub knowledge: Vec<KnowledgeRecord>,
    /// Controls how independently authorized context lanes are budgeted.
    #[serde(default)]
    pub retrieval: RetrievalPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_data_readiness: Option<CharacterDataReadiness>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_data_readiness_notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterDataReadiness {
    Curated,
    Partial,
    Unavailable,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_status: Option<ReviewStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceKind {
    Original,
    Official,
    Licensed,
    LegacyImport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeAuthority {
    CoreCanon,
    GamePublic,
    CharacterAuthored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeRecord {
    pub id: String,
    pub authority: KnowledgeAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_character_id: Option<String>,
    pub text: String,
    #[serde(default)]
    pub topic_tags: Vec<String>,
    pub spoiler_tier: String,
    pub provenance_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptAuthority {
    CoreCanon,
    CharacterProfile,
    GamePublic,
    CharacterAuthored,
    /// Provenance-bearing, spoiler-filtered derived memories retrieved from
    /// the exact active authority scope. These records are never canon merely
    /// because they were remembered.
    RetrievedMemory,
    SessionSummary,
    RecentDeliveredTurns,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalPolicy {
    pub query_window_delivered_turns: u16,
    pub max_core_records: u16,
    pub max_public_records: u16,
    pub max_character_records: u16,
    pub max_memory_records: u16,
    pub authority_order: Vec<PromptAuthority>,
}

impl Default for RetrievalPolicy {
    fn default() -> Self {
        Self {
            query_window_delivered_turns: 5,
            max_core_records: 16,
            max_public_records: 4,
            max_character_records: 4,
            max_memory_records: 8,
            authority_order: vec![
                PromptAuthority::CoreCanon,
                PromptAuthority::CharacterProfile,
                PromptAuthority::CharacterAuthored,
                PromptAuthority::GamePublic,
                PromptAuthority::SessionSummary,
                PromptAuthority::RetrievedMemory,
                PromptAuthority::RecentDeliveredTurns,
            ],
        }
    }
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
    /// Curated examples are data, never executable profile hooks. Stable IDs and
    /// integer weights make deterministic selection and replay straightforward.
    #[serde(default)]
    pub style_examples: Vec<StyleExample>,
    /// Profile-authored greetings are not durable conversation history.
    #[serde(default)]
    pub opening_lines: Vec<String>,
    #[serde(default)]
    pub background_npc: bool,
    pub prompt: CharacterPrompt,
    pub voice: VoiceDefaults,
    pub identity: IdentityContract,
    pub model: ModelDefaults,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StyleExample {
    pub id: String,
    pub speaker: String,
    pub text: String,
    #[serde(default)]
    pub situation_tags: Vec<String>,
    #[serde(default)]
    pub tone_tags: Vec<String>,
    /// Relative weight in the inclusive 0..=1000 range.
    pub weight_millis: u16,
    pub provenance_id: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default)]
    pub user_override_allowed: bool,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecommendationPolicy {
    pub integration_mode: IntegrationMode,
    pub provider_strategy: ProviderStrategy,
    pub local_activation_policy: LocalActivationPolicy,
    pub game_resource_reserve_required: bool,
    pub screen_space_lip_sync: ScreenSpaceLipSyncRecommendation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationMode {
    ExternalOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStrategy {
    ApiFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalActivationPolicy {
    MeasuredWholeLoadoutFitRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenSpaceLipSyncRecommendation {
    ExperimentalOptInAfterExactTargetAndAdvancingFrameQualification,
}
