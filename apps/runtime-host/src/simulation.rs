use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

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

#[cfg(any(test, all(windows, debug_assertions, feature = "dev-wasapi-audio")))]
#[allow(dead_code)]
#[path = "audio_output/mod.rs"]
mod dev_audio_output;

use crate::{
    profiles::{GenericGameError, GenericGameSelection, ProfileCorpus, GENERIC_GAME_ID},
    HostState,
};

#[cfg(all(windows, debug_assertions, feature = "dev-wasapi-audio"))]
use crate::tts_bridge::{RuntimeTtsBridge, RuntimeTtsBridgeConfig, VaultTtsCredentialResolver};
#[cfg(all(windows, debug_assertions, feature = "dev-wasapi-audio"))]
use dev_audio_output::{DevSubmittedPlaybackReceipt, DevWasapiAudioSink, DevWasapiConfig};
#[cfg(all(windows, debug_assertions, feature = "dev-wasapi-audio"))]
use npc_providers_tts::{
    ElevenLabsConfig, ElevenLabsProvider, ElevenLabsWebSocketTransport, HostedTtsProviderId,
    StreamingTtsProvider, VoiceBinding, VoiceBindings,
};
#[cfg(all(windows, debug_assertions, feature = "dev-wasapi-audio"))]
use npc_runtime_core::DataClass;

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
        } else if self.execution_mode.is_some() {
            return Err(SimulationError::InvalidRequest);
        }
        Ok(())
    }

    fn protected_online_detected(&self) -> bool {
        self.safety_context.protected_online_detected
            || self
                .generic_selection
                .as_ref()
                .is_some_and(|selection| selection.protected_online_detected)
    }

    fn anti_cheat_detected(&self) -> bool {
        self.safety_context.anti_cheat_detected
            || self
                .generic_selection
                .as_ref()
                .is_some_and(|selection| selection.anti_cheat_detected)
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct LivePlaybackEvidence {
    sentence_id: u64,
    source_frames_submitted: u64,
    device_frames_submitted: u64,
    source_duration: Duration,
    source_submission_complete: bool,
    endpoint_drain_complete: bool,
    cancelled: bool,
}

#[derive(Default)]
struct LivePlaybackLedger {
    receipts: Mutex<Vec<LivePlaybackEvidence>>,
}

impl LivePlaybackLedger {
    #[cfg(all(windows, debug_assertions, feature = "dev-wasapi-audio"))]
    fn record(&self, receipt: LivePlaybackEvidence) -> Result<(), RuntimeDependencyError> {
        self.receipts
            .lock()
            .map_err(|_| RuntimeDependencyError::Internal("live playback ledger poisoned".into()))?
            .push(receipt);
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<LivePlaybackEvidence>, SimulationError> {
        self.receipts
            .lock()
            .map(|receipts| receipts.clone())
            .map_err(|_| SimulationError::Runtime)
    }
}

struct DevLiveDependencies {
    tts: Arc<dyn TtsProvider>,
    audio: Arc<dyn AudioSink>,
    ledger: Arc<LivePlaybackLedger>,
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
        if request.protected_online_detected() {
            return Err(SimulationError::ProtectedOnlineBlocked);
        }
        if request.anti_cheat_detected() {
            return Err(SimulationError::AntiCheatBlocked);
        }
        let dev_live_tts = request.dev_live_tts.clone();
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
        let live_dependencies = match dev_live_tts.as_ref() {
            Some(route) => Some(build_dev_live_dependencies(self, route)?),
            None => None,
        };
        let (tts, audio, live_ledger): (
            Arc<dyn TtsProvider>,
            Arc<dyn AudioSink>,
            Option<Arc<LivePlaybackLedger>>,
        ) = match live_dependencies {
            Some(dependencies) => (
                dependencies.tts,
                dependencies.audio,
                Some(dependencies.ledger),
            ),
            None => (
                Arc::new(FixtureTts {
                    descriptor: local_descriptor("fixture-tts", ProviderModality::Speech),
                }),
                Arc::new(FixtureAudio),
                None,
            ),
        };
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
                audio,
            },
        );
        let is_dev_live_tts = dev_live_tts.is_some();
        let turn = TurnRequest {
            session_id: request.session_id,
            turn_id: request.turn_id,
            transcript: request.transcript,
            character_hint: Some(character_id),
            game_id,
            locale: request.locale,
            execution_mode: match request.execution_mode {
                Some(SimulationExecutionMode::Cloud) => ExecutionMode::Cloud,
                Some(SimulationExecutionMode::Hybrid) => ExecutionMode::Hybrid,
                Some(SimulationExecutionMode::Local) | None => ExecutionMode::FullyLocal,
            },
            network_policy: if is_dev_live_tts {
                NetworkPolicy::Online
            } else {
                NetworkPolicy::Offline
            },
            authorized_cloud_providers: if is_dev_live_tts {
                vec![DEV_LIVE_TTS_PROVIDER_ID.to_owned()]
            } else {
                Vec::new()
            },
            allow_provider_fallback: false,
            allow_local_to_cloud_fallback: false,
            // The bridge deliberately describes the ordinary hosted route
            // conservatively as retaining. Reaching this branch requires the
            // request's explicit, per-turn developer authorization.
            allow_retaining_providers: is_dev_live_tts,
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
        let live_receipts = match live_ledger {
            Some(ledger) => {
                let receipts = ledger.snapshot()?;
                validate_live_delivery(&outcome, &receipts)?;
                receipts
            }
            None => Vec::new(),
        };
        let submitted_source_frames = live_receipts
            .iter()
            .map(|receipt| receipt.source_frames_submitted)
            .sum::<u64>();
        let submitted_device_frames = live_receipts
            .iter()
            .map(|receipt| receipt.device_frames_submitted)
            .sum::<u64>();
        let (fixture_only, integration_mode) = result_mode(is_dev_live_tts, generic_mode);
        Ok(SimulationResult {
            schema_version: "1.0.0".to_owned(),
            fixture_only,
            integration_mode,
            capability_notices: if is_dev_live_tts {
                vec![
                    "Developer-only ElevenLabs stock-voice synthesis completed through the trusted credential resolver; IPC contained provider, model, and allowlisted stock-voice identifiers only, never credential values.".into(),
                    format!(
                        "The developer WASAPI sink accepted {submitted_source_frames} source frames and submitted {submitted_device_frames} device frames with bounded endpoint drain receipts. These are operating-system callback submission measurements, not proof of physical audibility."
                    ),
                    "lip_sync_unavailable: this live-audio qualification path does not claim or drive mouth animation.".into(),
                    "The response text and effects remain deterministic fixtures; only hosted TTS synthesis and developer WASAPI submission are live in this mixed qualification route.".into(),
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

fn result_mode(is_dev_live_tts: bool, generic_mode: bool) -> (bool, &'static str) {
    if is_dev_live_tts {
        (false, "debug_hosted_tts_wasapi_submission")
    } else if generic_mode {
        (true, "generic_experimental")
    } else {
        (true, "authored_profile")
    }
}

#[cfg(all(windows, debug_assertions, feature = "dev-wasapi-audio"))]
fn build_dev_live_dependencies(
    state: &HostState,
    route: &DevLiveTtsRequest,
) -> Result<DevLiveDependencies, SimulationError> {
    let bindings = VoiceBindings::new([VoiceBinding {
        intent_id: "dev.elevenlabs.stock".to_owned(),
        provider_id: HostedTtsProviderId::ElevenLabs,
        voice_id: route.voice_id.clone(),
        model_id: route.model_id.clone(),
        provider_options: BTreeMap::new(),
    }])
    .map_err(|_| SimulationError::DevLiveTtsUnavailable)?;
    let transport = Arc::new(ElevenLabsWebSocketTransport::default());
    let credentials = Arc::new(VaultTtsCredentialResolver::new(Arc::clone(&state.vault)));
    let upstream: Arc<dyn StreamingTtsProvider> = Arc::new(ElevenLabsProvider::new(
        transport,
        credentials,
        bindings,
        ElevenLabsConfig::default(),
    ));
    let bridge = RuntimeTtsBridge::new(
        upstream,
        RuntimeTtsBridgeConfig::dev_elevenlabs_stock(dev_elevenlabs_descriptor()),
    )
    .map_err(|_| SimulationError::DevLiveTtsUnavailable)?;

    let ledger = Arc::new(LivePlaybackLedger::default());
    let sink = DevWasapiAudioSink::new(DevWasapiConfig::default())
        .map_err(|_| SimulationError::DevLiveTtsUnavailable)?;
    Ok(DevLiveDependencies {
        tts: Arc::new(bridge),
        audio: Arc::new(ReceiptCheckedDevAudio {
            sink,
            ledger: Arc::clone(&ledger),
        }),
        ledger,
    })
}

#[cfg(all(windows, debug_assertions, feature = "dev-wasapi-audio"))]
fn dev_elevenlabs_descriptor() -> ProviderDescriptor {
    ProviderDescriptor {
        id: DEV_LIVE_TTS_PROVIDER_ID.to_owned(),
        display_name: "ElevenLabs".to_owned(),
        modality: ProviderModality::Speech,
        location: ProviderLocation::Cloud {
            service: DEV_LIVE_TTS_PROVIDER_ID.to_owned(),
        },
        may_retain_data: true,
        transmitted_data: vec![DataClass::Transcript],
        capabilities: BTreeMap::new(),
    }
}

#[cfg(all(windows, debug_assertions, feature = "dev-wasapi-audio"))]
struct ReceiptCheckedDevAudio {
    sink: DevWasapiAudioSink,
    ledger: Arc<LivePlaybackLedger>,
}

#[cfg(all(windows, debug_assertions, feature = "dev-wasapi-audio"))]
#[async_trait]
impl AudioSink for ReceiptCheckedDevAudio {
    async fn play(
        &self,
        identity: &TurnIdentity,
        sentence_id: u64,
        stream: SpeechStream,
        cancellation: CancellationToken,
    ) -> Result<PlaybackReceipt, RuntimeDependencyError> {
        let receipt = self
            .sink
            .play_submitted(identity, sentence_id, stream, cancellation)
            .await?;
        self.ledger
            .record(live_playback_evidence(sentence_id, &receipt))?;

        if receipt.cancelled {
            return Ok(PlaybackReceipt {
                audible_frames: receipt.source_frames_submitted,
                duration: receipt.source_duration,
                completed: false,
            });
        }
        if receipt.source_frames_submitted == 0
            || receipt.device_frames_submitted == 0
            || receipt.source_duration.is_zero()
            || !receipt.source_submission_complete
            || !receipt.endpoint_drain_complete
            || receipt.detached_cleanup_pending
        {
            return Err(RuntimeDependencyError::Unavailable(
                "developer WASAPI sink did not return a complete nonzero submission receipt".into(),
            ));
        }

        // Runtime Core's historical field is named `audible_frames`, but this
        // developer sink can prove only frames accepted and submitted through
        // WASAPI callbacks. The result notice preserves that narrower claim.
        Ok(PlaybackReceipt {
            audible_frames: receipt.source_frames_submitted,
            duration: receipt.source_duration,
            completed: true,
        })
    }

    async fn stop(&self, identity: &TurnIdentity) -> Result<(), RuntimeDependencyError> {
        self.sink.stop(identity).await
    }
}

#[cfg(all(windows, debug_assertions, feature = "dev-wasapi-audio"))]
fn live_playback_evidence(
    sentence_id: u64,
    receipt: &DevSubmittedPlaybackReceipt,
) -> LivePlaybackEvidence {
    LivePlaybackEvidence {
        sentence_id,
        source_frames_submitted: receipt.source_frames_submitted,
        device_frames_submitted: receipt.device_frames_submitted,
        source_duration: receipt.source_duration,
        source_submission_complete: receipt.source_submission_complete,
        endpoint_drain_complete: receipt.endpoint_drain_complete,
        cancelled: receipt.cancelled,
    }
}

#[cfg(not(all(windows, debug_assertions, feature = "dev-wasapi-audio")))]
fn build_dev_live_dependencies(
    _state: &HostState,
    _route: &DevLiveTtsRequest,
) -> Result<DevLiveDependencies, SimulationError> {
    Err(SimulationError::DevLiveTtsUnavailable)
}

fn validate_live_delivery(
    outcome: &TurnOutcome,
    receipts: &[LivePlaybackEvidence],
) -> Result<(), SimulationError> {
    if outcome.lifecycle != npc_runtime_core::TurnLifecycle::Completed
        || outcome.delivered.is_empty()
        || outcome.selected_tts_providers.as_slice() != [DEV_LIVE_TTS_PROVIDER_ID]
        || outcome.delivered.len() != receipts.len()
    {
        return Err(SimulationError::DevLiveTtsDeliveryFailed);
    }

    for delivered in &outcome.delivered {
        if delivered.delivery != npc_runtime_core::DeliveryMode::Audio
            || delivered.audible_frames == 0
            || delivered.duration.is_zero()
        {
            return Err(SimulationError::DevLiveTtsDeliveryFailed);
        }
        let Some(receipt) = receipts
            .iter()
            .find(|receipt| receipt.sentence_id == delivered.sentence_id)
        else {
            return Err(SimulationError::DevLiveTtsDeliveryFailed);
        };
        if receipt.source_frames_submitted != delivered.audible_frames
            || receipt.source_duration != delivered.duration
            || receipt.source_frames_submitted == 0
            || receipt.device_frames_submitted == 0
            || !receipt.source_submission_complete
            || !receipt.endpoint_drain_complete
            || receipt.cancelled
        {
            return Err(SimulationError::DevLiveTtsDeliveryFailed);
        }
    }

    let mut sentence_ids = receipts
        .iter()
        .map(|receipt| receipt.sentence_id)
        .collect::<Vec<_>>();
    sentence_ids.sort_unstable();
    sentence_ids.dedup();
    if sentence_ids.len() != receipts.len() {
        return Err(SimulationError::DevLiveTtsDeliveryFailed);
    }
    Ok(())
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
    #[error(
        "developer live TTS is available only in a Windows debug build with dev-wasapi-audio enabled"
    )]
    DevLiveTtsUnavailable,
    #[error("developer live TTS did not produce receipt-backed nonzero WASAPI delivery")]
    DevLiveTtsDeliveryFailed,
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

    fn completed_live_outcome() -> TurnOutcome {
        TurnOutcome {
            identity: TurnIdentity {
                session_id: "receipt-session".into(),
                turn_id: "receipt-turn".into(),
                cancellation_generation: 0,
            },
            lifecycle: npc_runtime_core::TurnLifecycle::Completed,
            full_response: "Receipt-backed speech.".into(),
            effects: NpcEffectsV1::neutral(),
            delivered: vec![DeliveredSentence {
                sentence_id: 1,
                text: "Receipt-backed speech.".into(),
                delivery: npc_runtime_core::DeliveryMode::Audio,
                audible_frames: 4_800,
                duration: Duration::from_millis(200),
            }],
            degradations: Vec::new(),
            selected_llm_provider: Some("fixture-llm".into()),
            selected_tts_providers: vec![DEV_LIVE_TTS_PROVIDER_ID.into()],
            error: None,
        }
    }

    fn completed_sink_receipt() -> LivePlaybackEvidence {
        LivePlaybackEvidence {
            sentence_id: 1,
            source_frames_submitted: 4_800,
            device_frames_submitted: 9_600,
            source_duration: Duration::from_millis(200),
            source_submission_complete: true,
            endpoint_drain_complete: true,
            cancelled: false,
        }
    }

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
        assert!(unsafe_request.validate().is_ok());
        assert!(unsafe_request.protected_online_detected());
        assert!(!unsafe_request.anti_cheat_detected());

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

    #[test]
    fn live_route_rejects_fixture_or_silent_delivery() {
        let mut fixture = completed_live_outcome();
        fixture.selected_tts_providers = vec!["fixture-tts".into()];
        assert!(matches!(
            validate_live_delivery(&fixture, &[completed_sink_receipt()]),
            Err(SimulationError::DevLiveTtsDeliveryFailed)
        ));

        let mut silent = completed_live_outcome();
        silent.delivered[0].audible_frames = 0;
        silent.delivered[0].duration = Duration::ZERO;
        let mut silent_receipt = completed_sink_receipt();
        silent_receipt.source_frames_submitted = 0;
        silent_receipt.source_duration = Duration::ZERO;
        assert!(matches!(
            validate_live_delivery(&silent, &[silent_receipt]),
            Err(SimulationError::DevLiveTtsDeliveryFailed)
        ));
    }

    #[test]
    fn live_route_cannot_claim_audio_without_matching_sink_receipt() {
        let outcome = completed_live_outcome();
        assert!(matches!(
            validate_live_delivery(&outcome, &[]),
            Err(SimulationError::DevLiveTtsDeliveryFailed)
        ));

        let mut incomplete = completed_sink_receipt();
        incomplete.endpoint_drain_complete = false;
        assert!(matches!(
            validate_live_delivery(&outcome, &[incomplete]),
            Err(SimulationError::DevLiveTtsDeliveryFailed)
        ));
    }

    #[test]
    fn live_route_accepts_only_matching_nonzero_completed_receipt() {
        assert!(
            validate_live_delivery(&completed_live_outcome(), &[completed_sink_receipt()]).is_ok()
        );
    }

    #[test]
    fn live_route_result_metadata_cannot_fall_back_to_fixture_only() {
        let (fixture_only, integration_mode) = result_mode(true, false);
        assert!(!fixture_only);
        assert_eq!(integration_mode, "debug_hosted_tts_wasapi_submission");

        assert_eq!(result_mode(false, false), (true, "authored_profile"));
        assert_eq!(result_mode(false, true), (true, "generic_experimental"));
    }
}
