use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use npc_memory::{
    MemoryInput, MemoryNamespace, MemoryScope, MemoryStore as SqliteMemoryStore, Provenance,
    RetrievalFilter, RetrievalQuery, Visibility,
};
use npc_runtime_core::{
    AudioChunk, AudioSink, CharacterIdentity, DeliveredSentence, EffectsProvider, EffectsRequest,
    ExecutionMode, GenerationRequest, IdentityResolver, LanguageModelProvider, LlmDelta, LlmStream,
    MemoryContext, MemoryStore, NetworkPolicy, NpcEffectsV1, PlaybackReceipt, ProviderDescriptor,
    ProviderError, ProviderLocation, ProviderModality, ProviderPool, RuntimeDependencies,
    RuntimeDependencyError, SpeechRequest, SpeechStream, SpeechStreamItem, SupervisorConfig,
    TtsProvider, TtsSession, TurnEvent, TurnIdentity, TurnOutcome, TurnRequest, TurnSupervisor,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{
    profiles::{GenericGameError, GenericGameSelection, ProfileCorpus, GENERIC_GAME_ID},
    HostState,
};

const DEV_LIVE_TTS_PROVIDER_ID: &str = "elevenlabs";
const DEV_LIVE_TTS_MODEL_ID: &str = "eleven_flash_v2_5";
const DEV_LIVE_TTS_STOCK_VOICE_IDS: &[&str] = &["EXAVITQu4vr4xnSDxMaL"];

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SimulationRequest {
    pub session_id: String,
    pub turn_id: String,
    pub game_id: String,
    pub character_id: Option<String>,
    #[serde(default)]
    pub generic_selection: Option<GenericGameSelection>,
    #[serde(default)]
    pub safety_context: SimulationSafetyContext,
    pub transcript: String,
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default)]
    pub execution_mode: Option<SimulationExecutionMode>,
    #[serde(default)]
    pub dev_live_tts: Option<DevLiveTtsRequest>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DevLiveTtsRequest {
    pub provider_id: String,
    pub model_id: String,
    pub voice_id: String,
    pub explicit_user_authorization: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SimulationExecutionMode {
    Cloud,
    Hybrid,
    Local,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SimulationSafetyContext {
    #[serde(default)]
    pub protected_online_detected: bool,
    #[serde(default)]
    pub anti_cheat_detected: bool,
}

fn default_locale() -> String {
    "en-US".to_owned()
}

impl SimulationRequest {
    pub fn validate(&self) -> Result<(), SimulationError> {
        if self.session_id.trim().is_empty()
            || self.turn_id.trim().is_empty()
            || self.game_id.trim().is_empty()
            || self.transcript.trim().is_empty()
            || self.transcript.len() > 64 * 1024
        {
            return Err(SimulationError::InvalidRequest);
        }
        if let Some(route) = &self.dev_live_tts {
            if !cfg!(debug_assertions)
                || !route.explicit_user_authorization
                || !matches!(
                    self.execution_mode,
                    Some(SimulationExecutionMode::Cloud | SimulationExecutionMode::Hybrid)
                )
                || route.provider_id != DEV_LIVE_TTS_PROVIDER_ID
                || route.model_id != DEV_LIVE_TTS_MODEL_ID
                || !DEV_LIVE_TTS_STOCK_VOICE_IDS.contains(&route.voice_id.as_str())
            {
                return Err(SimulationError::InvalidRequest);
            }
            if self.safety_context.protected_online_detected
                || self.safety_context.anti_cheat_detected
                || self.generic_selection.as_ref().is_some_and(|selection| {
                    selection.protected_online_detected || selection.anti_cheat_detected
                })
            {
                return Err(SimulationError::InvalidRequest);
            }
        } else if self.execution_mode.is_some() {
            return Err(SimulationError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationResult {
    pub schema_version: String,
    pub fixture_only: bool,
    pub integration_mode: &'static str,
    pub capability_notices: Vec<String>,
    pub events: Vec<TurnEvent>,
    pub outcome: TurnOutcome,
}

impl HostState {
    pub async fn simulate_turn(
        &self,
        request: SimulationRequest,
    ) -> Result<SimulationResult, SimulationError> {
        self.simulate_turn_cancellable(request, CancellationToken::new())
            .await
    }

    pub(crate) async fn simulate_turn_cancellable(
        &self,
        request: SimulationRequest,
        cancellation: CancellationToken,
    ) -> Result<SimulationResult, SimulationError> {
        request.validate()?;
        if request.safety_context.protected_online_detected {
            return Err(SimulationError::ProtectedOnlineBlocked);
        }
        if request.safety_context.anti_cheat_detected {
            return Err(SimulationError::AntiCheatBlocked);
        }
        let dev_live_tts_accepted = request.dev_live_tts.is_some();
        let (game_id, character_id, display_name, generic_mode) =
            if request.game_id == GENERIC_GAME_ID {
                let selection = request
                    .generic_selection
                    .as_ref()
                    .ok_or(SimulationError::GenericSelectionRequired)?;
                selection.validate()?;
                if request.character_id.is_some() {
                    return Err(SimulationError::InvalidRequest);
                }
                (
                    generic_memory_scope(selection),
                    "manual-character".to_owned(),
                    selection.character_name.trim().to_owned(),
                    true,
                )
            } else {
                if request.generic_selection.is_some() {
                    return Err(SimulationError::InvalidRequest);
                }
                let profile = self
                    .profiles
                    .profile(&request.game_id)
                    .ok_or(SimulationError::UnknownGame)?;
                let selected = match request.character_id.as_deref() {
                    Some(id) => profile
                        .characters
                        .iter()
                        .find(|character| character.id == id)
                        .ok_or(SimulationError::UnknownCharacter)?,
                    None => profile
                        .characters
                        .iter()
                        .find(|character| character.id == profile.defaults.character_id)
                        .ok_or(SimulationError::UnknownCharacter)?,
                };
                (
                    profile.id.clone(),
                    selected.id.clone(),
                    selected.display_name.clone(),
                    false,
                )
            };

        let identity = Arc::new(FixtureIdentity {
            character_id: character_id.clone(),
            display_name: display_name.clone(),
        });
        let memory = Arc::new(RuntimeMemory {
            store: self.memory.clone(),
            game_id: game_id.clone(),
            character_id: character_id.clone(),
        });
        let response = fixture_response(&request, &display_name);
        let llm = Arc::new(FixtureLlm {
            descriptor: local_descriptor("fixture-llm", ProviderModality::LanguageModel),
            response,
        });
        let effects = Arc::new(FixtureEffects {
            descriptor: local_descriptor("fixture-effects", ProviderModality::Effects),
        });
        let tts = Arc::new(FixtureTts {
            descriptor: local_descriptor("fixture-tts", ProviderModality::Speech),
        });
        // Game profiles provide dialogue/lore data only. The runtime never
        // turns model output into game actions or an executable integration
        // route, regardless of which authored profile is selected.
        let supervisor = TurnSupervisor::new(
            SupervisorConfig {
                allowed_actions: Vec::new(),
                ..SupervisorConfig::default()
            },
            RuntimeDependencies {
                providers: ProviderPool {
                    recognizers: Vec::new(),
                    language_models: vec![llm],
                    effects: vec![effects],
                    speech: vec![tts],
                },
                identity,
                memory,
                audio: Arc::new(FixtureAudio),
            },
        );
        let turn = TurnRequest {
            session_id: request.session_id,
            turn_id: request.turn_id,
            transcript: request.transcript,
            character_hint: Some(character_id),
            game_id,
            locale: request.locale,
            execution_mode: ExecutionMode::FullyLocal,
            network_policy: NetworkPolicy::Offline,
            authorized_cloud_providers: Vec::new(),
            allow_provider_fallback: false,
            allow_local_to_cloud_fallback: false,
            allow_retaining_providers: false,
            metadata: BTreeMap::new(),
        };
        let mut handle = supervisor
            .start_turn(turn)
            .await
            .map_err(|_| SimulationError::Runtime)?;
        let mut events = Vec::new();
        let mut cancellation_observed = false;
        loop {
            tokio::select! {
                _ = cancellation.cancelled(), if !cancellation_observed => {
                    cancellation_observed = true;
                    handle.cancel();
                }
                event = handle.next_event() => {
                    let Some(event) = event else { break };
                    let terminal = matches!(event, TurnEvent::Terminal { .. });
                    events.push(event);
                    if terminal {
                        break;
                    }
                }
            }
        }
        let outcome = handle
            .outcome()
            .await
            .map_err(|_| SimulationError::Runtime)?;
        supervisor.shutdown().await;
        Ok(SimulationResult {
            schema_version: "1.0.0".to_owned(),
            fixture_only: !dev_live_tts_accepted,
            integration_mode: if dev_live_tts_accepted {
                "developer_live_tts"
            } else if generic_mode {
                "generic_experimental"
            } else {
                "authored_profile"
            },
            capability_notices: if dev_live_tts_accepted {
                vec![
                    "Developer live TTS route accepted for private qualification; IPC contains provider, model, and allowlisted stock-voice identifiers only, never credential values.".into(),
                    "Live transport and playback are not exercised by this request-shaping layer; lip-sync remains unavailable.".into(),
                    "Executable adapters and action proposals are disabled.".into(),
                ]
            } else if generic_mode {
                vec![
                    "Identity is manual and experimental; no visual identity claim is made.".into(),
                    "Screen-space lip-sync is experimental and is not exercised by this simulation.".into(),
                    "Executable adapters and action proposals are disabled.".into(),
                ]
            } else {
                vec![
                    "Game integration is external capture only; profiles cannot load modules, inject code, install hooks, or execute adapters.".into(),
                    "Model action proposals are disabled; dialogue is delivered through audio and subtitles.".into(),
                ]
            },
            events,
            outcome,
        })
    }
}

fn local_descriptor(id: &str, modality: ProviderModality) -> ProviderDescriptor {
    ProviderDescriptor {
        id: id.to_owned(),
        display_name: id.to_owned(),
        modality,
        location: ProviderLocation::Local,
        may_retain_data: false,
        transmitted_data: Vec::new(),
        capabilities: BTreeMap::from([
            ("fixture_only".to_owned(), "true".to_owned()),
            ("network_access".to_owned(), "false".to_owned()),
        ]),
    }
}

struct FixtureIdentity {
    character_id: String,
    display_name: String,
}

#[async_trait]
impl IdentityResolver for FixtureIdentity {
    async fn resolve(
        &self,
        _request: &TurnRequest,
        cancellation: CancellationToken,
    ) -> Result<CharacterIdentity, RuntimeDependencyError> {
        if cancellation.is_cancelled() {
            return Err(RuntimeDependencyError::Cancelled);
        }
        Ok(CharacterIdentity {
            character_id: Some(self.character_id.clone()),
            display_name: self.display_name.clone(),
            confidence: 1.0,
            evidence: vec!["explicit_selection".to_owned()],
            explicit_selection: true,
        })
    }
}

struct RuntimeMemory {
    store: SqliteMemoryStore,
    game_id: String,
    character_id: String,
}

#[async_trait]
impl MemoryStore for RuntimeMemory {
    async fn retrieve(
        &self,
        request: &TurnRequest,
        cancellation: CancellationToken,
    ) -> Result<MemoryContext, RuntimeDependencyError> {
        if cancellation.is_cancelled() {
            return Err(RuntimeDependencyError::Cancelled);
        }
        let hits = self
            .store
            .retrieve(RetrievalQuery {
                text: None,
                embedding: None,
                embedding_generation: None,
                filter: RetrievalFilter {
                    game_id: Some(self.game_id.clone()),
                    character_id: Some(self.character_id.clone()),
                    session_id: Some(request.session_id.clone()),
                    ..RetrievalFilter::default()
                },
                limit: 8,
                candidate_limit: 32,
                now_ms: unix_time_ms(),
            })
            .await
            .map_err(|_| RuntimeDependencyError::Unavailable("memory retrieval failed".into()))?;
        let mut context = MemoryContext::default();
        for hit in hits {
            match hit.record.namespace {
                MemoryNamespace::Lore | MemoryNamespace::Fact => {
                    context.canon_facts.push(hit.record.content)
                }
                MemoryNamespace::Relationship => {
                    context.relationship_summary = Some(hit.record.content)
                }
                MemoryNamespace::Event | MemoryNamespace::Episode => {
                    context.episodic_memories.push(hit.record.content)
                }
                MemoryNamespace::Summary | MemoryNamespace::Profile => {
                    context.working_context.push(hit.record.content)
                }
            }
        }
        Ok(context)
    }

    async fn commit_delivered(
        &self,
        identity: &TurnIdentity,
        transcript: &str,
        delivered: &[DeliveredSentence],
        cancellation: CancellationToken,
    ) -> Result<(), RuntimeDependencyError> {
        if cancellation.is_cancelled() {
            return Err(RuntimeDependencyError::Cancelled);
        }
        if delivered.is_empty() {
            return Ok(());
        }
        let now = unix_time_ms();
        let scope = MemoryScope {
            game_id: Some(self.game_id.clone()),
            character_id: Some(self.character_id.clone()),
            session_id: Some(identity.session_id.clone()),
            ..MemoryScope::default()
        };
        self.store
            .upsert(MemoryInput {
                id: None,
                namespace: MemoryNamespace::Event,
                scope: scope.clone(),
                visibility: Visibility::Private,
                content: transcript.to_owned(),
                provenance: Provenance {
                    source_kind: "delivered_turn_player".to_owned(),
                    ..Provenance::default()
                },
                confidence: 1.0,
                importance: 0.5,
                observed_at_ms: now,
                expires_at_ms: None,
                expected_revision: None,
            })
            .await
            .map_err(|_| RuntimeDependencyError::Unavailable("memory commit failed".into()))?;
        for sentence in delivered {
            self.store
                .upsert(MemoryInput {
                    id: None,
                    namespace: MemoryNamespace::Event,
                    scope: scope.clone(),
                    visibility: Visibility::Private,
                    content: sentence.text.clone(),
                    provenance: Provenance {
                        source_kind: "delivered_turn_npc".to_owned(),
                        ..Provenance::default()
                    },
                    confidence: 1.0,
                    importance: 0.5,
                    observed_at_ms: now,
                    expires_at_ms: None,
                    expected_revision: None,
                })
                .await
                .map_err(|_| RuntimeDependencyError::Unavailable("memory commit failed".into()))?;
        }
        Ok(())
    }
}

struct FixtureLlm {
    descriptor: ProviderDescriptor,
    response: String,
}

#[async_trait]
impl LanguageModelProvider for FixtureLlm {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    async fn stream_response(
        &self,
        _request: GenerationRequest,
        cancellation: CancellationToken,
    ) -> Result<LlmStream, ProviderError> {
        if cancellation.is_cancelled() {
            return Err(ProviderError::cancelled(&self.descriptor.id));
        }
        let midpoint = self.response.len() / 2;
        let boundary = self.response[..midpoint]
            .rfind(char::is_whitespace)
            .unwrap_or(midpoint);
        let parts = vec![
            self.response[..boundary].to_owned(),
            self.response[boundary..].to_owned(),
        ];
        Ok(Box::pin(stream::iter(parts.into_iter().enumerate().map(
            |(index, text)| {
                Ok(LlmDelta {
                    text,
                    sequence: index as u64 + 1,
                })
            },
        ))))
    }
}

struct FixtureEffects {
    descriptor: ProviderDescriptor,
}

#[async_trait]
impl EffectsProvider for FixtureEffects {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    async fn derive_effects(
        &self,
        _request: EffectsRequest,
        cancellation: CancellationToken,
    ) -> Result<NpcEffectsV1, ProviderError> {
        if cancellation.is_cancelled() {
            return Err(ProviderError::cancelled(&self.descriptor.id));
        }
        Ok(NpcEffectsV1 {
            emotion: Some("attentive".to_owned()),
            valence: Some(0.2),
            arousal: Some(0.25),
            intensity: Some(0.3),
            voice_style: Some("grounded".to_owned()),
            ..NpcEffectsV1::neutral()
        })
    }
}

struct FixtureTts {
    descriptor: ProviderDescriptor,
}

struct FixtureTtsSession {
    descriptor: ProviderDescriptor,
}

#[async_trait]
impl TtsProvider for FixtureTts {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    async fn start_session(
        &self,
        _identity: &TurnIdentity,
        _locale: &str,
        cancellation: CancellationToken,
    ) -> Result<Box<dyn TtsSession>, ProviderError> {
        if cancellation.is_cancelled() {
            return Err(ProviderError::cancelled(&self.descriptor.id));
        }
        Ok(Box::new(FixtureTtsSession {
            descriptor: self.descriptor.clone(),
        }))
    }
}

#[async_trait]
impl TtsSession for FixtureTtsSession {
    fn provider(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    async fn synthesize(
        &mut self,
        request: SpeechRequest,
        cancellation: CancellationToken,
    ) -> Result<SpeechStream, ProviderError> {
        if cancellation.is_cancelled() {
            return Err(ProviderError::cancelled(&self.descriptor.id));
        }
        // Silent deterministic PCM: this exercises ordering and delivery without
        // claiming speech quality or touching an audio device in CI.
        let frames = (request.text.chars().count().max(1) * 120).min(24_000);
        Ok(Box::pin(stream::iter(vec![Ok(SpeechStreamItem::Audio(
            AudioChunk {
                sequence: 1,
                sample_rate_hz: 24_000,
                channels: 1,
                pcm_s16le: vec![0; frames * 2],
                end_of_stream: true,
            },
        ))])))
    }
}

struct FixtureAudio;

#[async_trait]
impl AudioSink for FixtureAudio {
    async fn play(
        &self,
        _identity: &TurnIdentity,
        _sentence_id: u64,
        mut stream: SpeechStream,
        cancellation: CancellationToken,
    ) -> Result<PlaybackReceipt, RuntimeDependencyError> {
        let mut frames = 0_u64;
        while let Some(item) = stream.next().await {
            if cancellation.is_cancelled() {
                return Err(RuntimeDependencyError::Cancelled);
            }
            if let SpeechStreamItem::Audio(chunk) = item
                .map_err(|_| RuntimeDependencyError::Unavailable("fixture speech failed".into()))?
            {
                frames = frames.saturating_add((chunk.pcm_s16le.len() / 2) as u64);
            }
        }
        Ok(PlaybackReceipt {
            audible_frames: frames,
            duration: Duration::from_secs_f64(frames as f64 / 24_000.0),
            completed: true,
        })
    }

    async fn stop(&self, _identity: &TurnIdentity) -> Result<(), RuntimeDependencyError> {
        Ok(())
    }
}

fn unix_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn generic_memory_scope(selection: &GenericGameSelection) -> String {
    let executable = selection
        .executable_name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    format!("generic-game:{executable}")
}

fn fixture_response(request: &SimulationRequest, display_name: &str) -> String {
    let eclipse_harbor_turn = request.generic_selection.as_ref().is_some_and(|selection| {
        selection.game_name.trim() == "Eclipse Harbor"
            && selection.character_name.trim() == "Mara Venn"
    }) && request.transcript.trim()
        == "Did you ever make it to the old lighthouse?";
    if eclipse_harbor_turn {
        return "I made it as far as the eastern lock. It jammed again, but I remembered your service-tunnel route. If the tide stays low, I can reach the old lighthouse before dark.".into();
    }
    format!(
        "I hear you. We can continue as {display_name} while keeping this local simulation deterministic."
    )
}

#[derive(Debug, thiserror::Error)]
pub enum SimulationError {
    #[error("simulation request is invalid")]
    InvalidRequest,
    #[error("simulation game profile is unknown")]
    UnknownGame,
    #[error("simulation character is unknown")]
    UnknownCharacter,
    #[error("generic simulation requires manual game, executable, and character selection")]
    GenericSelectionRequired,
    #[error(transparent)]
    Generic(#[from] GenericGameError),
    #[error("simulation runtime failed")]
    Runtime,
    #[error("simulation is blocked for protected online play")]
    ProtectedOnlineBlocked,
    #[error("simulation is blocked when anti-cheat is detected")]
    AntiCheatBlocked,
}

#[allow(dead_code)]
fn _assert_profile_corpus(_: &ProfileCorpus) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_with(route: Option<DevLiveTtsRequest>) -> SimulationRequest {
        SimulationRequest {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            game_id: "skyrim-special-edition".into(),
            character_id: None,
            generic_selection: None,
            safety_context: SimulationSafetyContext::default(),
            transcript: "Can you hear me?".into(),
            locale: "en-US".into(),
            execution_mode: route.as_ref().map(|_| SimulationExecutionMode::Hybrid),
            dev_live_tts: route,
        }
    }

    fn allowed_route() -> DevLiveTtsRequest {
        DevLiveTtsRequest {
            provider_id: DEV_LIVE_TTS_PROVIDER_ID.into(),
            model_id: DEV_LIVE_TTS_MODEL_ID.into(),
            voice_id: DEV_LIVE_TTS_STOCK_VOICE_IDS[0].into(),
            explicit_user_authorization: true,
        }
    }

    #[test]
    fn legacy_request_without_dev_route_remains_valid() {
        let wire = serde_json::json!({
            "sessionId": "session-1",
            "turnId": "turn-1",
            "gameId": "skyrim-special-edition",
            "characterId": null,
            "transcript": "Can you hear me?",
            "locale": "en-US"
        });
        let request: SimulationRequest =
            serde_json::from_value(wire).expect("deserialize legacy request");
        assert!(request.dev_live_tts.is_none());
        assert!(request.validate().is_ok());
    }

    #[test]
    fn dev_live_tts_rejects_unauthorized_local_or_unknown_routes() {
        assert!(request_with(Some(allowed_route())).validate().is_ok());

        let mutations: Vec<Box<dyn Fn(&mut DevLiveTtsRequest)>> = vec![
            Box::new(|route| route.explicit_user_authorization = false),
            Box::new(|route| route.provider_id = "unknown-provider".into()),
            Box::new(|route| route.model_id = "unknown-model".into()),
            Box::new(|route| route.voice_id = "custom-or-cloned-voice".into()),
        ];
        for mutate in mutations {
            let mut route = allowed_route();
            mutate(&mut route);
            assert!(matches!(
                request_with(Some(route)).validate(),
                Err(SimulationError::InvalidRequest)
            ));
        }
        let mut local = request_with(Some(allowed_route()));
        local.execution_mode = Some(SimulationExecutionMode::Local);
        assert!(matches!(
            local.validate(),
            Err(SimulationError::InvalidRequest)
        ));
    }

    #[test]
    fn dev_live_tts_rejects_detected_risk_and_secret_fields() {
        let mut unsafe_request = request_with(Some(allowed_route()));
        unsafe_request.safety_context.protected_online_detected = true;
        assert!(matches!(
            unsafe_request.validate(),
            Err(SimulationError::InvalidRequest)
        ));

        let with_secret = serde_json::json!({
            "sessionId": "session-1",
            "turnId": "turn-1",
            "gameId": "skyrim-special-edition",
            "characterId": null,
            "transcript": "Can you hear me?",
            "locale": "en-US",
            "executionMode": "hybrid",
            "devLiveTts": {
                "providerId": "elevenlabs",
                "modelId": "eleven_flash_v2_5",
                "voiceId": "EXAVITQu4vr4xnSDxMaL",
                "explicitUserAuthorization": true,
                "apiKey": "must-never-cross-ipc"
            }
        });
        assert!(serde_json::from_value::<SimulationRequest>(with_secret).is_err());
    }
}
