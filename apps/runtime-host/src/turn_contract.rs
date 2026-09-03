use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const TURN_ROUTE_SCHEMA_VERSION: u32 = 1;
pub const PRIVATE_EVALUATION_ACKNOWLEDGEMENT_SCHEMA_VERSION: u32 = 1;
pub const NVIDIA_PRIVATE_EVALUATION_TERMS_REVISION: &str =
    "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1";
/// Compatibility symbol for callers compiled against the earlier Magpie-only
/// name. Its value is provider-wide; persisted Magpie-only acknowledgements are invalid.
pub const NVIDIA_MAGPIE_PRIVATE_EVALUATION_TERMS_REVISION: &str =
    NVIDIA_PRIVATE_EVALUATION_TERMS_REVISION;
pub const PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE: &str =
    "io.github.akshitireddy.interactive-npcs.review";
pub const PRIVATE_EVALUATION_DEBUG_APPLICATION_NAMESPACE: &str =
    "io.github.akshitireddy.interactive-npcs.debug";
pub const PRODUCTION_APPLICATION_NAMESPACE: &str = "io.github.akshitireddy.interactive-npcs";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PrivateEvaluationModeV1 {
    PrivateEvaluationOnly,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderPrivateEvaluationAcknowledgementV1 {
    pub schema_version: u32,
    pub provider_id: String,
    pub application_namespace: String,
    pub mode: PrivateEvaluationModeV1,
    pub terms_revision: String,
    pub catalog_revision: u64,
    pub acknowledged_at_epoch_ms: u64,
    pub promotion_supported: bool,
    pub publication_supported: bool,
}

impl ProviderPrivateEvaluationAcknowledgementV1 {
    pub fn validate_shape(&self) -> Result<(), &'static str> {
        if self.schema_version != PRIVATE_EVALUATION_ACKNOWLEDGEMENT_SCHEMA_VERSION
            || self.provider_id != "nvidia-nim"
            || !matches!(
                self.application_namespace.as_str(),
                PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE
                    | PRIVATE_EVALUATION_DEBUG_APPLICATION_NAMESPACE
            )
            || self.mode != PrivateEvaluationModeV1::PrivateEvaluationOnly
            || self.terms_revision != NVIDIA_PRIVATE_EVALUATION_TERMS_REVISION
            || self.catalog_revision == 0
            || self.acknowledged_at_epoch_ms == 0
            || self.promotion_supported
            || self.publication_supported
        {
            return Err("invalid_private_evaluation_acknowledgement");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedRouteSnapshot {
    pub schema_version: u32,
    pub source_loadout_id: String,
    #[serde(default)]
    pub inheritance_chain: Vec<String>,
    pub generation: u64,
    pub roles: SelectedRouteRoles,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedRouteRoles {
    pub llm: SelectedRoleRoute,
    pub stt: SelectedRoleRoute,
    pub tts: SelectedRoleRoute,
    pub embeddings: SelectedRoleRoute,
    pub vision: SelectedRoleRoute,
    #[serde(rename = "lipsync")]
    pub lip_sync: SelectedRoleRoute,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectedRouteState {
    Ready,
    Disabled,
    Degraded,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedRoleRoute {
    pub state: SelectedRouteState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<SelectedProviderRoute>,
    #[serde(default)]
    pub fallbacks: Vec<ManualFallbackRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degradation: Option<RouteDegradation>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RouteExecution {
    Cloud,
    Local,
    Off,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedProviderRoute {
    pub provider_id: String,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
    pub execution: RouteExecution,
    pub egress: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_reference: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManualFallbackRoute {
    #[serde(flatten)]
    pub route: SelectedProviderRoute,
    pub activation: ManualFallbackActivation,
    pub user_authorized: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ManualFallbackActivation {
    ManualOnly,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteDegradation {
    pub code: String,
    pub detail: String,
    pub retryable: bool,
}

impl SelectedRouteSnapshot {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != TURN_ROUTE_SCHEMA_VERSION
            || self.generation == 0
            || !valid_identifier(&self.source_loadout_id)
            || self.inheritance_chain.len() > 16
            || self
                .inheritance_chain
                .iter()
                .any(|identifier| !valid_identifier(identifier))
        {
            return Err("invalid_route_snapshot");
        }
        for role in self.roles.all() {
            role.validate()?;
        }
        Ok(())
    }
}

impl SelectedRouteRoles {
    fn all(&self) -> [&SelectedRoleRoute; 6] {
        [
            &self.llm,
            &self.stt,
            &self.tts,
            &self.embeddings,
            &self.vision,
            &self.lip_sync,
        ]
    }
}

impl SelectedRoleRoute {
    fn validate(&self) -> Result<(), &'static str> {
        if self.fallbacks.len() > 8
            || self.primary.as_ref().is_some_and(|route| !route.is_valid())
            || self.fallbacks.iter().any(|fallback| {
                !fallback.route.is_valid()
                    || fallback.activation != ManualFallbackActivation::ManualOnly
                    || !fallback.user_authorized
            })
            || self.degradation.as_ref().is_some_and(|degradation| {
                !valid_code(&degradation.code)
                    || degradation.detail.trim().is_empty()
                    || degradation.detail.len() > 512
            })
        {
            return Err("invalid_role_route");
        }
        if self.state == SelectedRouteState::Ready && self.primary.is_none() {
            return Err("ready_role_missing_primary");
        }
        if self.state == SelectedRouteState::Disabled && self.primary.is_some() {
            return Err("disabled_role_has_primary");
        }
        Ok(())
    }
}

impl SelectedProviderRoute {
    fn is_valid(&self) -> bool {
        valid_identifier(&self.provider_id)
            && valid_route_value(&self.model_id, 256)
            && self
                .voice_id
                .as_ref()
                .is_none_or(|voice| valid_route_value(voice, 256))
            && !self.egress.trim().is_empty()
            && self.egress.len() <= 128
            && self.egress.chars().all(|character| !character.is_control())
            && self
                .credential_reference
                .as_ref()
                .is_none_or(|reference| valid_credential_reference(reference))
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'.' | b'_' | b':' | b'/')
        })
        && !value.contains("..")
}

fn valid_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_route_value(value: &str, maximum_bytes: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= maximum_bytes
        && value.chars().all(|character| !character.is_control())
}

fn valid_credential_reference(value: &str) -> bool {
    value.starts_with("providers/") && valid_identifier(value)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnInputMode {
    #[default]
    Typed,
    PushToTalk,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PushToTalkCaptureState {
    #[default]
    NotRequested,
    TranscriptReady,
    CaptureUnavailable,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnInputSnapshot {
    #[serde(default)]
    pub mode: TurnInputMode,
    #[serde(default)]
    pub push_to_talk_state: PushToTalkCaptureState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_stt_receipt: Option<SelectedSttTurnEvidenceV1>,
}

/// Non-secret, authenticated evidence for a one-time native push-to-talk
/// transcript. The transcript remains on `SimulationRequest`; this receipt
/// binds it to the native capture result without copying audio or credentials.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedSttTurnEvidenceV1 {
    pub schema_version: u32,
    pub receipt_id: String,
    pub receipt_sha256: String,
    pub capture_session_id: String,
    pub capture_turn_id: String,
    pub capture_generation: u64,
    pub game_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_id: Option<String>,
    pub source_loadout_id: String,
    pub route: SelectedSttRouteReceiptV1,
    pub chunks_sent: u64,
    pub pcm_bytes_sent: u64,
    pub partial_events: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedSttRouteReceiptV1 {
    pub provider_id: String,
    pub model_id: String,
    pub credential_reference: String,
    pub egress: String,
    pub generation: u64,
    pub input_endpoint_id: String,
    pub input_endpoint_generation: u64,
    pub manual_retry: bool,
    pub automatic_fallback: bool,
    pub captured_frames: u64,
    pub ptt_virtual_key: u32,
    pub ptt_press_transition_sequence: u64,
    pub ptt_pressed_qpc: u64,
    pub ptt_release_transition_sequence: u64,
    pub ptt_released_qpc: u64,
}

impl SelectedSttTurnEvidenceV1 {
    pub fn validate_shape_and_digest(&self, transcript: &str) -> Result<(), &'static str> {
        if self.schema_version != 1
            || !canonical_non_nil_uuid(&self.receipt_id)
            || !canonical_non_nil_uuid(&self.capture_session_id)
            || !canonical_non_nil_uuid(&self.capture_turn_id)
            || self.capture_generation == 0
            || !valid_route_value(&self.game_id, 128)
            || self
                .character_id
                .as_ref()
                .is_some_and(|value| !valid_route_value(value, 128))
            || !valid_identifier(&self.source_loadout_id)
            || self.chunks_sent == 0
            || self.pcm_bytes_sent == 0
            || !self.route.validate_shape()
            || !lowercase_sha256(&self.receipt_sha256)
            || self.canonical_sha256(transcript)? != self.receipt_sha256
        {
            return Err("invalid_selected_stt_receipt");
        }
        Ok(())
    }

    pub fn canonical_sha256(&self, transcript: &str) -> Result<String, &'static str> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct CanonicalResult<'a> {
            transcript: &'a str,
            route: &'a SelectedSttRouteReceiptV1,
            chunks_sent: u64,
            pcm_bytes_sent: u64,
            partial_events: u64,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct CanonicalControlResult<'a> {
            schema_version: u32,
            source_loadout_id: &'a str,
            route_snapshot_generation: u64,
            result: CanonicalResult<'a>,
        }

        let canonical = CanonicalControlResult {
            schema_version: 1,
            source_loadout_id: &self.source_loadout_id,
            route_snapshot_generation: self.capture_generation,
            result: CanonicalResult {
                transcript,
                route: &self.route,
                chunks_sent: self.chunks_sent,
                pcm_bytes_sent: self.pcm_bytes_sent,
                partial_events: self.partial_events,
            },
        };
        let bytes = serde_json::to_vec(&canonical).map_err(|_| "invalid_selected_stt_receipt")?;
        Ok(Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
}

impl SelectedSttRouteReceiptV1 {
    fn validate_shape(&self) -> bool {
        self.provider_id == "assemblyai"
            && self.model_id == "u3-rt-pro"
            && self.credential_reference == "providers/assemblyai"
            && self.egress == "microphone_audio_and_optional_non_secret_context"
            && self.generation > 0
            && !self.input_endpoint_id.is_empty()
            && self.input_endpoint_id.len() <= 1024
            && self
                .input_endpoint_id
                .chars()
                .all(|character| !character.is_control())
            && self.input_endpoint_generation > 0
            && !self.automatic_fallback
            && self.captured_frames > 0
            && (1..=255).contains(&self.ptt_virtual_key)
            && self.ptt_press_transition_sequence > 0
            && self.ptt_pressed_qpc > 0
            && self.ptt_release_transition_sequence > self.ptt_press_transition_sequence
            && self.ptt_released_qpc >= self.ptt_pressed_qpc
    }
}

fn canonical_non_nil_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|parsed| !parsed.is_nil() && parsed.to_string() == value)
}

fn lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnDeliveryRequest {
    pub audio: bool,
    pub subtitles: bool,
}

impl Default for TurnDeliveryRequest {
    fn default() -> Self {
        Self {
            audio: true,
            subtitles: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConsumedRouteSnapshot {
    pub schema_version: u32,
    pub source_loadout_id: String,
    pub generation: u64,
    pub sha256: String,
    pub llm: Option<ConsumedProviderRoute>,
    pub tts: Option<ConsumedProviderRoute>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_evaluation_acknowledgement: Option<ConsumedPrivateEvaluationAcknowledgementV1>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConsumedPrivateEvaluationAcknowledgementV1 {
    pub provider_id: String,
    pub application_namespace: String,
    pub modalities: Vec<PrivateEvaluationModalityV1>,
    pub terms_revision: String,
    pub catalog_revision: u64,
    pub acknowledgement_sha256: String,
    pub promotion_supported: bool,
    pub publication_supported: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum PrivateEvaluationModalityV1 {
    Llm,
    Embeddings,
    Tts,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConsumedProviderRoute {
    pub provider_id: String,
    pub model_id: String,
    pub voice_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryCommitState {
    Committed,
    NotCommitted,
    CommitDeferred,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnDeliveryState {
    Delivered,
    Cancelled,
    ManualRetryRequired,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleCue {
    pub sentence_id: u64,
    pub text_start_bytes: usize,
    pub text_end_bytes: usize,
    pub speaker: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioReceiptSummary {
    pub receipt_id: String,
    pub sentence_id: u64,
    pub sink: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport_schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    pub source_frames: u64,
    pub device_frames: u64,
    pub duration_ms: u64,
    pub peak: Option<f32>,
    pub rms: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_selection_mode: Option<AudioOutputSelectionModeEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_endpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_endpoint_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<bool>,
    pub submitted: bool,
    pub drained: bool,
    pub completed: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AudioOutputSelectionModeEvidence {
    SystemDefault,
    EndpointId,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleDirection {
    LeftToRight,
    RightToLeft,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleColorTreatment {
    SdrPremultipliedSourceOver,
    /// Decode-only compatibility value from pre-v1 review presenters.
    ScrgbLinearSourceOver,
    /// Decode-only compatibility value from pre-v1 review presenters.
    Hdr10ToneMappedSourceOver,
    WindowsCompositorSdrWhiteMapping,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubtitlePresentationProvenance {
    TrustedNativeCapture,
    ConsoleBottomCenterUnavailable,
    DeterministicFixture,
}

/// Receipt-backed evidence from the native subtitle surface. It is created
/// only after the swap chain commits; a cue or render enqueue is insufficient.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitlePresentationReceiptSummary {
    pub receipt_id: String,
    pub sentence_id: u64,
    pub provenance: SubtitlePresentationProvenance,
    pub presentation_id: u64,
    pub target_geometry_epoch: u64,
    pub capture_sequence: u64,
    pub graphics_generation: u64,
    pub layer_hash_hex: String,
    pub presented_qpc_ticks: String,
    pub desktop_x_px: i32,
    pub desktop_y_px: i32,
    pub width_px: u32,
    pub height_px: u32,
    pub dpi_x: u32,
    pub dpi_y: u32,
    pub direction: SubtitleDirection,
    pub bidi_shaping_applied: bool,
    pub grapheme_clusters_preserved: bool,
    pub used_bottom_center_fallback: bool,
    pub color_treatment: SubtitleColorTreatment,
    pub renderer_authority_revision: u64,
    pub renderer_authority_sha256: String,
    pub renderer_authority_sources: npc_subtitle_engine::SubtitleRendererAuthoritySourcesV1,
    pub renderer_style_id: String,
    pub renderer_safe_area_dp: f32,
    pub renderer_text_scale: f32,
    pub renderer_backplate_enabled: bool,
    pub renderer_opacity: f32,
    pub committed: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnExecutionSuccessMetadata {
    pub llm_provider_live: bool,
    pub tts_provider_live: bool,
    pub stt_skipped: bool,
    pub subtitle_delivered: bool,
    pub subtitle_receipt_count: usize,
    pub audio_submitted: bool,
    pub audio_drained: bool,
    pub audio_receipt_count: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeClockDomainV1 {
    WindowsQpc,
    PortableMonotonic,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeClockStampV1 {
    pub clock_domain: RuntimeClockDomainV1,
    pub qpc_frequency_hz: u64,
    pub qpc_ticks: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeProviderRouteBindingV1 {
    pub provider_id: String,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
    pub egress: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeTurnCancellationReceiptV1 {
    pub requested: RuntimeClockStampV1,
    pub terminal: RuntimeClockStampV1,
}

/// Strict producer-measured receipt for one selected live-provider turn. It is
/// absent unless exact provider usage, structured validation, decoded PCM, and
/// a single shared clock domain are all available.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeTurnTimingReceiptV1 {
    pub schema_version: u32,
    pub receipt_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub source_loadout_id: String,
    pub route_snapshot_generation: u64,
    pub route_snapshot_sha256: String,
    pub llm: RuntimeProviderRouteBindingV1,
    pub tts: RuntimeProviderRouteBindingV1,
    pub input_finalized: RuntimeClockStampV1,
    pub identity_started: RuntimeClockStampV1,
    pub identity_completed: RuntimeClockStampV1,
    pub llm_requested: RuntimeClockStampV1,
    pub llm_first_token: RuntimeClockStampV1,
    pub llm_provider_terminal: RuntimeClockStampV1,
    pub structured_response_validated: RuntimeClockStampV1,
    pub output_tokens: u32,
    pub tts_requested: RuntimeClockStampV1,
    pub tts_first_decoded_pcm: RuntimeClockStampV1,
    pub tts_final_decoded_pcm: RuntimeClockStampV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation: Option<RuntimeTurnCancellationReceiptV1>,
    pub live_provider_receipts: bool,
}

impl RuntimeTurnTimingReceiptV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        let stamps = [
            &self.input_finalized,
            &self.identity_started,
            &self.identity_completed,
            &self.llm_requested,
            &self.llm_first_token,
            &self.llm_provider_terminal,
            &self.structured_response_validated,
            &self.tts_requested,
            &self.tts_first_decoded_pcm,
            &self.tts_final_decoded_pcm,
        ];
        let first = stamps[0];
        if self.schema_version != 1
            || !canonical_non_nil_uuid(&self.receipt_id)
            || !valid_route_value(&self.session_id, 128)
            || !valid_route_value(&self.turn_id, 128)
            || !valid_identifier(&self.source_loadout_id)
            || self.route_snapshot_generation == 0
            || !lowercase_sha256(&self.route_snapshot_sha256)
            || !self.llm.validate(false)
            || !self.tts.validate(true)
            || self.output_tokens == 0
            || !self.live_provider_receipts
            || stamps.iter().any(|stamp| {
                stamp.qpc_ticks == 0
                    || stamp.qpc_frequency_hz == 0
                    || stamp.clock_domain != first.clock_domain
                    || stamp.qpc_frequency_hz != first.qpc_frequency_hz
            })
            || !ordered(&[
                self.input_finalized.qpc_ticks,
                self.identity_started.qpc_ticks,
                self.identity_completed.qpc_ticks,
                self.llm_requested.qpc_ticks,
                self.llm_first_token.qpc_ticks,
                self.llm_provider_terminal.qpc_ticks,
                self.structured_response_validated.qpc_ticks,
            ])
            // The speech lane consumes complete sentences while the LLM is
            // still streaming. Requiring provider completion before TTS
            // submission would reject the product's intended low-latency
            // execution. Preserve the real cross-lane dependency (a spoken
            // sentence cannot precede the first non-empty model delta) and
            // validate the TTS lane's own producer order independently.
            || self.tts_requested.qpc_ticks < self.llm_first_token.qpc_ticks
            || !ordered(&[
                self.tts_requested.qpc_ticks,
                self.tts_first_decoded_pcm.qpc_ticks,
                self.tts_final_decoded_pcm.qpc_ticks,
            ])
        {
            return Err("invalid_runtime_turn_timing_receipt");
        }
        if let Some(cancellation) = &self.cancellation {
            for stamp in [&cancellation.requested, &cancellation.terminal] {
                if stamp.qpc_ticks == 0
                    || stamp.clock_domain != first.clock_domain
                    || stamp.qpc_frequency_hz != first.qpc_frequency_hz
                {
                    return Err("invalid_runtime_turn_timing_receipt");
                }
            }
            if cancellation.requested.qpc_ticks < self.input_finalized.qpc_ticks
                || cancellation.terminal.qpc_ticks < cancellation.requested.qpc_ticks
            {
                return Err("invalid_runtime_turn_timing_receipt");
            }
        }
        Ok(())
    }
}

impl RuntimeProviderRouteBindingV1 {
    fn validate(&self, voice_required: bool) -> bool {
        valid_identifier(&self.provider_id)
            && valid_route_value(&self.model_id, 256)
            && !self.egress.trim().is_empty()
            && self.egress.len() <= 192
            && self.egress.chars().all(|character| !character.is_control())
            && if voice_required {
                self.voice_id
                    .as_ref()
                    .is_some_and(|voice| valid_route_value(voice, 256))
            } else {
                self.voice_id.is_none()
            }
    }
}

fn ordered(values: &[u64]) -> bool {
    values.windows(2).all(|pair| pair[1] >= pair[0])
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TurnExecutionDegradation {
    TypedInput {
        reason: String,
    },
    AudioOnly {
        reason: String,
    },
    SubtitleOnly {
        reason: String,
    },
    ManualRetryRequired {
        failed_role: String,
        provider_id: Option<String>,
        reason: String,
        retryable: bool,
    },
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnExecutionEvidence {
    pub consumed_route: ConsumedRouteSnapshot,
    pub input: TurnInputSnapshot,
    pub delivery_state: TurnDeliveryState,
    pub commit_state: DeliveryCommitState,
    pub subtitles: Vec<SubtitleCue>,
    pub subtitle_presentation_receipts: Vec<SubtitlePresentationReceiptSummary>,
    pub audio_receipts: Vec<AudioReceiptSummary>,
    pub degradations: Vec<TurnExecutionDegradation>,
    pub success: TurnExecutionSuccessMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_timing_receipt: Option<RuntimeTurnTimingReceiptV1>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ready_route_without_primary_and_non_manual_fallbacks_are_unrepresentable() {
        let snapshot = SelectedRouteSnapshot {
            schema_version: TURN_ROUTE_SCHEMA_VERSION,
            source_loadout_id: "loadout-1".into(),
            inheritance_chain: Vec::new(),
            generation: 1,
            roles: SelectedRouteRoles {
                llm: SelectedRoleRoute {
                    state: SelectedRouteState::Ready,
                    primary: None,
                    fallbacks: Vec::new(),
                    degradation: None,
                },
                stt: disabled(),
                tts: disabled(),
                embeddings: disabled(),
                vision: disabled(),
                lip_sync: disabled(),
            },
        };
        assert_eq!(snapshot.validate(), Err("ready_role_missing_primary"));
    }

    #[test]
    fn accepts_the_authenticated_camel_case_wire_contract_and_rejects_generation_zero() {
        let wire = serde_json::json!({
            "schemaVersion": 1,
            "sourceLoadoutId": "player-loadout",
            "inheritanceChain": ["base-loadout", "player-loadout"],
            "generation": 9,
            "roles": {
                "llm": {
                    "state": "ready",
                    "primary": {
                        "providerId": "openai",
                        "modelId": "gpt-5-mini",
                        "execution": "cloud",
                        "egress": "conversation_text",
                        "credentialReference": "providers/openai"
                    },
                    "fallbacks": []
                },
                "stt": { "state": "disabled", "fallbacks": [] },
                "tts": {
                    "state": "ready",
                    "primary": {
                        "providerId": "elevenlabs",
                        "modelId": "eleven_flash_v2_5",
                        "voiceId": "EXAVITQu4vr4xnSDxMaL",
                        "execution": "cloud",
                        "egress": "conversation_audio",
                        "credentialReference": "providers/elevenlabs"
                    },
                    "fallbacks": []
                },
                "embeddings": { "state": "disabled", "fallbacks": [] },
                "vision": { "state": "disabled", "fallbacks": [] },
                "lipsync": { "state": "disabled", "fallbacks": [] }
            }
        });
        let mut snapshot: SelectedRouteSnapshot =
            serde_json::from_value(wire).expect("deserialize authenticated route snapshot");
        assert!(snapshot.validate().is_ok());
        snapshot.generation = 0;
        assert_eq!(snapshot.validate(), Err("invalid_route_snapshot"));
    }

    #[test]
    fn refuses_a_fallback_without_explicit_user_authorization() {
        let mut snapshot = SelectedRouteSnapshot {
            schema_version: TURN_ROUTE_SCHEMA_VERSION,
            source_loadout_id: "loadout-1".into(),
            inheritance_chain: Vec::new(),
            generation: 1,
            roles: SelectedRouteRoles {
                llm: SelectedRoleRoute {
                    state: SelectedRouteState::Ready,
                    primary: Some(local_route("mock-llm", "mock-stream-v1")),
                    fallbacks: vec![ManualFallbackRoute {
                        route: local_route("mock-llm-fallback", "mock-stream-v1"),
                        activation: ManualFallbackActivation::ManualOnly,
                        user_authorized: false,
                    }],
                    degradation: None,
                },
                stt: disabled(),
                tts: disabled(),
                embeddings: disabled(),
                vision: disabled(),
                lip_sync: disabled(),
            },
        };
        assert_eq!(snapshot.validate(), Err("invalid_role_route"));
        snapshot.roles.llm.fallbacks[0].user_authorized = true;
        assert!(snapshot.validate().is_ok());
    }

    fn local_route(provider_id: &str, model_id: &str) -> SelectedProviderRoute {
        SelectedProviderRoute {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            voice_id: None,
            execution: RouteExecution::Local,
            egress: "conversation_text".into(),
            credential_reference: None,
        }
    }

    fn disabled() -> SelectedRoleRoute {
        SelectedRoleRoute {
            state: SelectedRouteState::Disabled,
            primary: None,
            fallbacks: Vec::new(),
            degradation: None,
        }
    }
}
