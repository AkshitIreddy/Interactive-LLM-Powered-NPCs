use crate::diagnostics_v2::DiagnosticsV2Manager;
use crate::domain::{
    CancelOutcome, CancelSimulationResult, MeasurementBasis, NativeAudioOutputSelectionMode,
    NativeDeliveryCommitState, NativeTurnDeliveryState, NativeTurnExecutionDegradation,
    NativeTurnExecutionEvidence, NativeVisualPresentationEvidence, ResponseStage, RuntimeBackend,
    RuntimeConnectionState, SimulationEvent, SimulationSnapshot, SimulationStatus,
    StartSimulationRequest, StartSimulationResult,
};
use crate::media_broker::{
    AudioOutputSelectionMode, AudioPlaybackLease, AudioPlaybackPoolRequest, MediaBrokerError,
    MediaBrokerSupervisor, PresentationPixelRect, TrustedCaptureBackend, TrustedCaptureScope,
    TrustedSubtitlePresentationContext, TrustedTargetColorSpace, MAX_PLAYBACK_LEASES_PER_TURN,
};
use crate::provider_loadouts::ProviderPrivateEvaluationAcknowledgementV1;
use crate::runtime_bridge::{SimulationController, StartError};
use crate::selected_stt::ConsumedSelectedStt;
use crate::sidecar_protocol::{
    NativeDevLiveTtsRequest, NativeExecutionMode, NativeGenericGameSelection,
    NativePushToTalkCaptureState, NativeSelectedRouteSnapshot, NativeSelectedSttTurnEvidenceV1,
    NativeSimulationRequest, NativeSimulationResult, NativeSimulationSafetyContext,
    NativeSubtitleContextProvenance, NativeSubtitlePresentationContext, NativeSubtitleRectPx,
    NativeSubtitleTargetColorSpace, NativeSubtitleTargetIdentity, NativeTurnDeliveryRequest,
    NativeTurnInputMode, NativeTurnInputSnapshot,
};
use crate::sidecar_supervisor::{RuntimeSupervisor, SupervisorError};
use crate::visual_runtime::VisualCoordinator;
use interactive_npcs_diagnostics::{DiagnosticStatus, Severity};
use npc_character_db::{
    CharacterDatabase, EncounterLifecycleEventV1, EncounterRecordV1, EncounterRegistryV1,
};
use npc_provider_loadouts::{
    ProviderRole, ResolvedProviderLoadoutV1, TurnRouteSnapshotV1, TurnRouteStateV1,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::ipc::Channel;

const GENERIC_GAME_ID: &str = "generic-game";
const DEFAULT_TRANSCRIPT: &str = "Did you ever make it to the old lighthouse?";
const FIXTURE_STAGE_TIMINGS: &[(ResponseStage, u64)] = &[
    (ResponseStage::Listening, 240),
    (ResponseStage::Transcribing, 220),
    (ResponseStage::Identifying, 180),
    (ResponseStage::Remembering, 220),
    (ResponseStage::Responding, 320),
    (ResponseStage::Voicing, 240),
    (ResponseStage::Animating, 180),
];

fn current_epoch_ms_i64() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn load_native_encounters(path: &Path) -> Result<NativeEncounterState, RouterError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(NativeEncounterState::default())
        }
        Err(error) => return Err(RouterError::EncounterPersistence(error.to_string())),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 8 * 1024 * 1024
    {
        return Err(RouterError::EncounterPersistence(
            "encounter store is linked, oversized, or not a regular file".into(),
        ));
    }
    let bytes =
        fs::read(path).map_err(|error| RouterError::EncounterPersistence(error.to_string()))?;
    let state: NativeEncounterState = serde_json::from_slice(&bytes)
        .map_err(|error| RouterError::EncounterPersistence(error.to_string()))?;
    if state.schema_version != 1
        || state
            .by_game
            .iter()
            .any(|(game, registry)| registry.game_profile_id != *game)
    {
        return Err(RouterError::EncounterPersistence(
            "encounter store schema or game namespace is invalid".into(),
        ));
    }
    Ok(state)
}

fn persist_native_encounters(path: &Path, state: &NativeEncounterState) -> Result<(), RouterError> {
    let parent = path.parent().ok_or_else(|| {
        RouterError::EncounterPersistence("encounter store has no parent directory".into())
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| RouterError::EncounterPersistence(error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| RouterError::EncounterPersistence(error.to_string()))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(RouterError::EncounterPersistence(
            "encounter store exceeds its hard bound".into(),
        ));
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| RouterError::EncounterPersistence(error.to_string()))?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| RouterError::EncounterPersistence(error.to_string()))?;
    temporary
        .persist(path)
        .map_err(|error| RouterError::EncounterPersistence(error.error.to_string()))?;
    Ok(())
}

#[derive(Debug)]
struct NativeActive {
    simulation_id: String,
    generation: u64,
    status: SimulationStatus,
    stage: Option<ResponseStage>,
    playback_pool_allocated: bool,
}

#[derive(Debug, Default)]
struct NativeState {
    generation: u64,
    active: Option<NativeActive>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeEncounterState {
    schema_version: u32,
    by_game: BTreeMap<String, EncounterRegistryV1>,
}

impl Default for NativeEncounterState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            by_game: BTreeMap::new(),
        }
    }
}

impl NativeEncounterState {
    fn remember(&mut self, encounter: EncounterRecordV1) -> Result<(), RouterError> {
        let registry = self
            .by_game
            .entry(encounter.game_profile_id.clone())
            .or_insert(
                EncounterRegistryV1::new(&encounter.game_profile_id)
                    .map_err(|error| RouterError::Character(error.to_string()))?,
            );
        if let Some(existing) = registry.record(encounter.encounter_id) {
            if existing.game_profile_id != encounter.game_profile_id
                || existing.archetype_character_id != encounter.archetype_character_id
                || existing.continuity_key_sha256 != encounter.continuity_key_sha256
            {
                return Err(RouterError::Character(
                    "trusted runtime encounter identity changed".into(),
                ));
            }
            registry
                .observe(encounter.encounter_id, encounter.last_seen_at_ms)
                .map_err(|error| RouterError::Character(error.to_string()))?;
        } else {
            registry
                .insert(encounter)
                .map_err(|error| RouterError::Character(error.to_string()))?;
        }
        registry.drain_events();
        Ok(())
    }

    fn correct(
        &mut self,
        database: &CharacterDatabase,
        encounter_id: uuid::Uuid,
        character_id: &str,
        explicitly_confirmed: bool,
    ) -> Result<EncounterLifecycleEventV1, RouterError> {
        let registry = self
            .by_game
            .get_mut(&database.profile().id)
            .ok_or_else(|| RouterError::Character("unknown trusted encounter game".into()))?;
        registry
            .correct_to_authored_character(
                database,
                encounter_id,
                character_id,
                explicitly_confirmed,
                current_epoch_ms_i64(),
            )
            .map_err(|error| RouterError::Character(error.to_string()))?;
        registry
            .drain_events()
            .into_iter()
            .last()
            .ok_or_else(|| RouterError::Character("encounter correction emitted no event".into()))
    }

    fn merge(
        &mut self,
        game_profile_id: &str,
        source_encounter_id: uuid::Uuid,
        destination_encounter_id: uuid::Uuid,
        explicitly_confirmed: bool,
    ) -> Result<EncounterLifecycleEventV1, RouterError> {
        let registry = self
            .by_game
            .get_mut(game_profile_id)
            .ok_or_else(|| RouterError::Character("unknown trusted encounter game".into()))?;
        registry
            .merge_unknown_encounters(
                source_encounter_id,
                destination_encounter_id,
                explicitly_confirmed,
                current_epoch_ms_i64(),
            )
            .map_err(|error| RouterError::Character(error.to_string()))?;
        registry
            .drain_events()
            .into_iter()
            .last()
            .ok_or_else(|| RouterError::Character("encounter merge emitted no event".into()))
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeRouter {
    supervisor: RuntimeSupervisor,
    media_broker: MediaBrokerSupervisor,
    visual_coordinator: VisualCoordinator,
    fixture: SimulationController,
    native: Arc<Mutex<NativeState>>,
    encounters: Arc<Mutex<NativeEncounterState>>,
    encounter_store_path: Arc<PathBuf>,
    diagnostics: Arc<DiagnosticsV2Manager>,
}

impl RuntimeRouter {
    pub fn new(
        supervisor: RuntimeSupervisor,
        media_broker: MediaBrokerSupervisor,
        visual_coordinator: VisualCoordinator,
        encounter_store_path: PathBuf,
        diagnostics: Arc<DiagnosticsV2Manager>,
    ) -> Result<Self, RouterError> {
        let encounters = load_native_encounters(&encounter_store_path)?;
        Ok(Self {
            supervisor,
            media_broker,
            visual_coordinator,
            fixture: SimulationController::default(),
            native: Arc::new(Mutex::new(NativeState::default())),
            encounters: Arc::new(Mutex::new(encounters)),
            encounter_store_path: Arc::new(encounter_store_path),
            diagnostics,
        })
    }

    pub fn supervisor(&self) -> &RuntimeSupervisor {
        &self.supervisor
    }

    pub fn correct_encounter(
        &self,
        database: &CharacterDatabase,
        encounter_id: uuid::Uuid,
        character_id: &str,
        explicitly_confirmed: bool,
    ) -> Result<EncounterLifecycleEventV1, RouterError> {
        let mut state = self.encounters.lock().map_err(|_| RouterError::State)?;
        let mut staged = state.clone();
        let event = staged.correct(database, encounter_id, character_id, explicitly_confirmed)?;
        persist_native_encounters(&self.encounter_store_path, &staged)?;
        *state = staged;
        Ok(event)
    }

    pub fn merge_encounters(
        &self,
        game_profile_id: &str,
        source_encounter_id: uuid::Uuid,
        destination_encounter_id: uuid::Uuid,
        explicitly_confirmed: bool,
    ) -> Result<EncounterLifecycleEventV1, RouterError> {
        let mut state = self.encounters.lock().map_err(|_| RouterError::State)?;
        let mut staged = state.clone();
        let event = staged.merge(
            game_profile_id,
            source_encounter_id,
            destination_encounter_id,
            explicitly_confirmed,
        )?;
        persist_native_encounters(&self.encounter_store_path, &staged)?;
        *state = staged;
        Ok(event)
    }

    pub fn snapshot(&self) -> SimulationSnapshot {
        if let Ok(state) = self.native.lock() {
            if let Some(active) = &state.active {
                return SimulationSnapshot {
                    status: active.status,
                    simulation_id: Some(active.simulation_id.clone()),
                    generation: active.generation,
                    active_stage: active.stage,
                    backend: RuntimeBackend::NativeRuntime,
                };
            }
        }
        let fixture = self.fixture.snapshot();
        if fixture.status != SimulationStatus::Idle {
            return fixture;
        }
        let health = self.supervisor.health();
        SimulationSnapshot {
            backend: health.backend,
            ..fixture
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        &self,
        request: StartSimulationRequest,
        events: Channel<SimulationEvent>,
        resolved_routes: ResolvedProviderLoadoutV1,
        private_evaluation_acknowledgements: Vec<ProviderPrivateEvaluationAcknowledgementV1>,
        application_namespace: String,
        trusted_safety_context: NativeSimulationSafetyContext,
        selected_stt: Option<ConsumedSelectedStt>,
        subtitle_renderer_authority: npc_subtitle_engine::SubtitleRendererAuthorityV1,
    ) -> Result<StartSimulationResult, RouterError> {
        request.validate().map_err(RouterError::InvalidRequest)?;
        validate_trusted_safety_context(trusted_safety_context)?;
        validate_visual_route_safety(&resolved_routes, trusted_safety_context)?;
        validate_explicit_dev_route(&request, &resolved_routes)?;
        subtitle_renderer_authority
            .validate()
            .map_err(|error| RouterError::InvalidRequest(error.to_string()))?;
        if self.supervisor.health().state == RuntimeConnectionState::DevelopmentFixture {
            if selected_stt.is_some() {
                return Err(RouterError::InvalidRequest(
                    "selected_stt_fixture_unavailable: native PTT receipts cannot be consumed by the deterministic fixture".into(),
                ));
            }
            return self
                .fixture
                .start(request, events, resolved_routes)
                .map_err(RouterError::Fixture);
        }
        self.supervisor.ensure_ready().await?;

        let (simulation_id, generation) = {
            let mut state = self.native.lock().map_err(|_| RouterError::State)?;
            if let Some(active) = &state.active {
                return Err(RouterError::AlreadyActive(active.simulation_id.clone()));
            }
            state.generation = state.generation.saturating_add(1);
            let generation = state.generation;
            let simulation_id = format!("runtime-simulation-{generation:06}");
            state.active = Some(NativeActive {
                simulation_id: simulation_id.clone(),
                generation,
                status: SimulationStatus::Running,
                stage: None,
                playback_pool_allocated: false,
            });
            (simulation_id, generation)
        };

        let route_snapshot = resolved_routes.pin_turn_routes(generation);
        let playback_leases = match selected_playback_format(&request, &route_snapshot) {
            Some((sample_rate, channels)) => {
                let allocation = self
                    .media_broker
                    .allocate_playback_pool(playback_pool_request(
                        &simulation_id,
                        generation,
                        sample_rate,
                        channels,
                    ))
                    .await;
                let leases = match allocation {
                    Ok(leases) => leases,
                    Err(error) => {
                        finish_native(&self.native, generation);
                        return Err(RouterError::PlaybackBroker(error));
                    }
                };
                if leases.len() != MAX_PLAYBACK_LEASES_PER_TURN {
                    let cleanup = self.media_broker.cancel_playback().await;
                    finish_native(&self.native, generation);
                    if let Err(error) = cleanup {
                        return Err(RouterError::PlaybackBroker(error));
                    }
                    return Err(RouterError::PlaybackPoolIncomplete {
                        expected: MAX_PLAYBACK_LEASES_PER_TURN,
                        actual: leases.len(),
                    });
                }
                leases
            }
            None => Vec::new(),
        };
        let playback_pool_allocated = !playback_leases.is_empty();
        let playback_receipt_expectations = playback_leases
            .iter()
            .map(ExpectedPlaybackReceipt::from)
            .collect::<Vec<_>>();
        self.visual_coordinator.start_turn(&playback_leases).await;
        if playback_pool_allocated {
            let mut state = self.native.lock().map_err(|_| RouterError::State)?;
            let active = state
                .active
                .as_mut()
                .filter(|active| active.generation == generation)
                .ok_or(RouterError::State)?;
            active.playback_pool_allocated = true;
        }
        let mut native_request = native_request_for(
            &request,
            &simulation_id,
            trusted_safety_context,
            &route_snapshot,
            private_evaluation_acknowledgements,
            application_namespace,
            selected_stt.as_ref(),
            &subtitle_renderer_authority,
        );
        native_request.subtitle_presentation_context =
            Some(if trusted_safety_context.visuals_allowed {
                self.media_broker
                    .trusted_subtitle_presentation_context()
                    .await
                    .ok()
                    .and_then(|context| {
                        native_subtitle_context_from_broker(
                            &context,
                            trusted_safety_context,
                            subtitle_renderer_authority.clone(),
                        )
                    })
                    .unwrap_or_else(|| {
                        NativeSubtitlePresentationContext::console_unavailable(
                            subtitle_renderer_authority.clone(),
                        )
                    })
            } else {
                NativeSubtitlePresentationContext::console_unavailable(
                    subtitle_renderer_authority.clone(),
                )
            });
        native_request.audio_playback_leases = playback_leases;
        let (measurement_basis, runtime_fixture_only) = start_provenance(&request, &route_snapshot);
        let _ = self.diagnostics.record_native_turn_event(
            "runtime",
            "turn.dispatched",
            Severity::Info,
            DiagnosticStatus::Ok,
            None,
            &simulation_id,
        );
        let _ = events.send(SimulationEvent::Started {
            simulation_id: simulation_id.clone(),
            generation,
            sequence: 1,
            measurement_basis,
        });
        let supervisor = self.supervisor.clone();
        let native_state = Arc::clone(&self.native);
        let encounters = Arc::clone(&self.encounters);
        let encounter_store_path = Arc::clone(&self.encounter_store_path);
        let diagnostics = Arc::clone(&self.diagnostics);
        let media_broker = self.media_broker.clone();
        let visual_coordinator = self.visual_coordinator.clone();
        let task_id = simulation_id.clone();
        tauri::async_runtime::spawn(async move {
            let simulation = supervisor.simulate(native_request).await;
            visual_coordinator.stop_turn(Some(generation)).await;
            let visual_receipt = visual_coordinator.best_receipt_for_generation(generation);
            if playback_pool_allocated {
                let _ = diagnostics.record_native_turn_event(
                    "visual",
                    if visual_receipt
                        .as_ref()
                        .is_some_and(|receipt| receipt.presented)
                    {
                        "turn.presented"
                    } else {
                        "turn.unavailable"
                    },
                    if visual_receipt
                        .as_ref()
                        .is_some_and(|receipt| receipt.presented)
                    {
                        Severity::Info
                    } else {
                        Severity::Warn
                    },
                    if visual_receipt
                        .as_ref()
                        .is_some_and(|receipt| receipt.presented)
                    {
                        DiagnosticStatus::Ok
                    } else {
                        DiagnosticStatus::Skipped
                    },
                    (!visual_receipt
                        .as_ref()
                        .is_some_and(|receipt| receipt.presented))
                    .then_some("visual_actor_lock_or_admission_unavailable"),
                    &task_id,
                );
            }
            let playback_cleanup = if playback_pool_allocated {
                media_broker.cancel_playback().await
            } else {
                Ok(())
            };
            if playback_cleanup.is_err() {
                let _ = diagnostics.record_native_turn_event(
                    "audio-broker",
                    "playback.cancel_failed",
                    Severity::Error,
                    DiagnosticStatus::Failed,
                    Some("playback_cancel_failed"),
                    &task_id,
                );
                emit_playback_cleanup_failure(&events, &task_id, generation, &native_state);
                finish_native(&native_state, generation);
                return;
            }
            match simulation {
                Ok(result) => {
                    if let Some(encounter) = result
                        .character_context
                        .as_ref()
                        .and_then(|context| context.encounter.clone())
                    {
                        let ingestion = (|| -> Result<(), RouterError> {
                            let mut state = encounters.lock().map_err(|_| RouterError::State)?;
                            let mut staged = state.clone();
                            staged.remember(encounter)?;
                            persist_native_encounters(&encounter_store_path, &staged)?;
                            *state = staged;
                            Ok(())
                        })();
                        if let Err(error) = ingestion {
                            let _ = diagnostics.record_native_turn_event(
                                "character-db",
                                "encounter.persist_failed",
                                Severity::Error,
                                DiagnosticStatus::Failed,
                                Some("encounter_persist_failed"),
                                &task_id,
                            );
                            emit_native_encounter_failure(&events, &task_id, generation, &error);
                            finish_native(&native_state, generation);
                            return;
                        }
                    }
                    // A completed runtime result is authoritative after audio, subtitle, and
                    // memory side effects. Diagnostics are intentionally best effort here: a
                    // local log write failure must never suppress receipts or invite a duplicate
                    // retry of an already-delivered turn.
                    let _ = diagnostics.record_native_turn_event(
                        "runtime",
                        "turn.result_received",
                        Severity::Info,
                        DiagnosticStatus::Ok,
                        None,
                        &task_id,
                    );
                    emit_native_result(
                        &events,
                        &task_id,
                        generation,
                        result,
                        &native_state,
                        &playback_receipt_expectations,
                        visual_receipt.map(|receipt| NativeVisualPresentationEvidence {
                            schema_version: 1,
                            source_frame_sequence: receipt.source_frame_sequence,
                            residual_proposed: receipt.residual_proposed,
                            presented: receipt.presented,
                            degraded: receipt.degraded,
                            pixel_source: receipt.pixel_source.map(|source| format!("{source:?}")),
                            pixel_scope: receipt.pixel_scope.map(|scope| format!("{scope:?}")),
                            detail: receipt.detail,
                        }),
                    )
                    .await
                }
                Err(error) => {
                    let _ = diagnostics.record_native_turn_event(
                        "runtime",
                        "turn.failed",
                        Severity::Error,
                        DiagnosticStatus::Failed,
                        Some("runtime_turn_failed"),
                        &task_id,
                    );
                    emit_native_failure(&events, &task_id, generation, &error, &native_state)
                }
            }
            finish_native(&native_state, generation);
        });

        Ok(StartSimulationResult {
            simulation_id,
            generation,
            backend: RuntimeBackend::NativeRuntime,
            measurement_basis,
            runtime_fixture_only,
        })
    }

    pub async fn cancel(&self) -> CancelSimulationResult {
        let native = self.native.lock().ok().and_then(|mut state| {
            state.active.as_mut().map(|active| {
                active.status = SimulationStatus::Cancelling;
                (
                    active.simulation_id.clone(),
                    active.generation,
                    active.playback_pool_allocated,
                )
            })
        });
        if let Some((simulation_id, generation, playback_pool_allocated)) = native {
            let runtime_cancel = self.supervisor.cancel();
            let visual_cancel = self.visual_coordinator.stop_turn(Some(generation));
            let playback_cancel = async {
                if playback_pool_allocated {
                    self.media_broker.cancel_playback().await
                } else {
                    Ok(())
                }
            };
            let (runtime_result, playback_result, ()) =
                tokio::join!(runtime_cancel, playback_cancel, visual_cancel);
            // The spawned terminal path independently performs and verifies the
            // same broker-wide revocation, then emits a fail-closed event if it
            // cannot prove cleanup. The command result intentionally remains a
            // cancellation acknowledgement, never a false playback receipt.
            let _terminal_path_reports_playback_failure = playback_result.is_err();
            let outcome = if runtime_result.is_ok() {
                CancelOutcome::CancellationRequested
            } else {
                // The operation is already marked cancelling. The simulation
                // task will emit its terminal failure/cancellation and clean up.
                CancelOutcome::CancellationRequested
            };
            return CancelSimulationResult {
                simulation_id: Some(simulation_id),
                generation,
                outcome,
            };
        }
        self.fixture.cancel()
    }

    pub async fn shutdown(&self) {
        let _ = self.cancel().await;
        self.supervisor.shutdown().await;
    }
}

fn start_provenance(
    request: &StartSimulationRequest,
    route_snapshot: &TurnRouteSnapshotV1,
) -> (MeasurementBasis, bool) {
    if request.dev_live_tts.is_some() {
        return (MeasurementBasis::ControlledBenchmark, false);
    }
    let fixture_only = route_snapshot
        .roles
        .values()
        .filter_map(|role| role.primary.as_ref())
        .all(|route| route.provider_id.starts_with("mock-"));
    if fixture_only {
        (MeasurementBasis::TrustedRuntimeFixture, true)
    } else {
        // Route selection is known now, but provider success is not. Completion
        // evidence promotes or degrades this pending provenance without making
        // a speculative live-provider claim at turn start.
        (MeasurementBasis::PendingProviderEvidence, false)
    }
}

fn selected_playback_format(
    request: &StartSimulationRequest,
    snapshot: &TurnRouteSnapshotV1,
) -> Option<(u32, u16)> {
    if request.dev_live_tts.is_some() {
        return None;
    }
    let tts = snapshot.roles.get(&ProviderRole::Tts)?;
    if tts.state != TurnRouteStateV1::Ready {
        return None;
    }
    let route = tts.primary.as_ref()?;
    let credential_provider = route
        .credential
        .as_ref()
        .map(|credential| credential.provider_id.as_str());
    match route.provider_id.as_str() {
        "elevenlabs" if credential_provider == Some("elevenlabs") => Some((24_000, 1)),
        "nvidia-nim-magpie"
            if credential_provider == Some("nvidia-nim")
                && route.model_id == "magpie-tts-multilingual" =>
        {
            Some((44_100, 1))
        }
        _ => None,
    }
}

fn playback_pool_request(
    simulation_id: &str,
    generation: u64,
    sample_rate: u32,
    channels: u16,
) -> AudioPlaybackPoolRequest {
    AudioPlaybackPoolRequest {
        session_id: "response-console-simulation".into(),
        turn_id: simulation_id.into(),
        generation,
        sample_rate,
        channels,
        max_frames_per_lease: u64::from(sample_rate).saturating_mul(60),
        lease_count: MAX_PLAYBACK_LEASES_PER_TURN,
    }
}

async fn emit_native_result(
    channel: &Channel<SimulationEvent>,
    simulation_id: &str,
    generation: u64,
    mut result: NativeSimulationResult,
    state: &Arc<Mutex<NativeState>>,
    expected_playback: &[ExpectedPlaybackReceipt],
    visual_presentation: Option<NativeVisualPresentationEvidence>,
) {
    if result.schema_version != "1.0.0" {
        let _ = channel.send(SimulationEvent::Failed {
            simulation_id: simulation_id.into(),
            generation,
            sequence: 2,
            reason: "Runtime simulation returned an incompatible result contract.".into(),
            turn_execution: None,
            character_context: None,
        });
        return;
    }
    let mut sequence = 1_u64;
    let mut fixture_elapsed_ms = 0_u64;
    let mut seen = BTreeSet::new();
    for event in &result.events {
        if let Some(stage) = event_stage(event) {
            if seen.insert(stage) {
                if !native_turn_is_running(state, generation) {
                    emit_native_cancelled(channel, simulation_id, generation, sequence + 1);
                    return;
                }
                sequence += 1;
                set_native_stage(state, generation, stage);
                let fixture_stage_ms = fixture_stage_duration(result.fixture_only, stage);
                let _ = channel.send(SimulationEvent::StageStarted {
                    simulation_id: simulation_id.into(),
                    generation,
                    sequence,
                    stage,
                    estimated_duration_ms: fixture_stage_ms,
                });
                if fixture_stage_ms > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(fixture_stage_ms)).await;
                    fixture_elapsed_ms = fixture_elapsed_ms.saturating_add(fixture_stage_ms);
                }
                if !native_turn_is_running(state, generation) {
                    emit_native_cancelled(channel, simulation_id, generation, sequence + 1);
                    return;
                }
                sequence += 1;
                let _ = channel.send(SimulationEvent::StageCompleted {
                    simulation_id: simulation_id.into(),
                    generation,
                    sequence,
                    stage,
                    fixture_elapsed_ms,
                });
            }
        }
        if event.get("type").and_then(Value::as_str) == Some("sentence_ready") {
            if let Some(text) = event.get("text").and_then(Value::as_str) {
                sequence += 1;
                let _ = channel.send(SimulationEvent::SentenceReady {
                    simulation_id: simulation_id.into(),
                    generation,
                    sequence,
                    text: text.into(),
                });
            }
        }
    }
    let lifecycle = result
        .outcome
        .get("lifecycle")
        .and_then(Value::as_str)
        .unwrap_or("failed");
    if let Some(evidence) = result.turn_execution.as_deref_mut() {
        if !audio_receipts_match_expected(evidence, expected_playback) {
            fail_closed_invalid_audio_evidence(evidence);
        }
    }
    sequence += 1;
    let delivery_completed = match result.turn_execution.as_deref() {
        Some(evidence) => evidence.delivery_state == NativeTurnDeliveryState::Delivered,
        None => true,
    };
    if lifecycle == "completed" && delivery_completed {
        let _ = channel.send(SimulationEvent::Completed {
            simulation_id: simulation_id.into(),
            generation,
            sequence,
            // The runtime currently proves delivery with receipt-backed source
            // and device-frame submission, but does not expose a comparable
            // first-audible timestamp. Never turn that absence into a fake 0 ms.
            fixture_first_audio_ms: None,
            runtime_fixture_only: result.fixture_only,
            delivered_text: delivered_text(&result.outcome),
            turn_execution: result.turn_execution,
            character_context: result.character_context,
            visual_presentation,
        });
    } else if lifecycle == "cancelled" {
        let _ = channel.send(SimulationEvent::Cancelled {
            simulation_id: simulation_id.into(),
            generation,
            sequence,
            reason: "Runtime simulation was cancelled; undelivered dialogue was not committed."
                .into(),
            turn_execution: result.turn_execution,
            character_context: result.character_context,
        });
    } else {
        let reason = manual_retry_reason(result.turn_execution.as_deref())
            .unwrap_or("Runtime simulation ended without a completed delivery.");
        let _ = channel.send(SimulationEvent::Failed {
            simulation_id: simulation_id.into(),
            generation,
            sequence,
            reason: reason.into(),
            turn_execution: result.turn_execution,
            character_context: result.character_context,
        });
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExpectedPlaybackReceipt {
    schema_version: u32,
    stream_id: String,
    session_id: String,
    turn_id: String,
    generation: u64,
    output_selection_mode: NativeAudioOutputSelectionMode,
    output_endpoint_id: String,
    output_endpoint_generation: u64,
}

impl From<&AudioPlaybackLease> for ExpectedPlaybackReceipt {
    fn from(lease: &AudioPlaybackLease) -> Self {
        Self {
            schema_version: lease.schema_version,
            stream_id: lease.stream_id.clone(),
            session_id: lease.session_id.clone(),
            turn_id: lease.turn_id.clone(),
            generation: lease.generation,
            output_selection_mode: match lease.output_selection_mode {
                AudioOutputSelectionMode::SystemDefault => {
                    NativeAudioOutputSelectionMode::SystemDefault
                }
                AudioOutputSelectionMode::EndpointId => NativeAudioOutputSelectionMode::EndpointId,
            },
            output_endpoint_id: lease.output_endpoint_id.clone(),
            output_endpoint_generation: lease.output_endpoint_generation,
        }
    }
}

fn audio_receipts_match_expected(
    evidence: &NativeTurnExecutionEvidence,
    expected: &[ExpectedPlaybackReceipt],
) -> bool {
    if evidence.audio_receipts.is_empty() {
        // A turn may legitimately degrade to subtitles after Control has already
        // allocated a bounded playback-lease pool. Unused leases are revoked by
        // the router; they are not audio-delivery claims. Validate the empty
        // evidence on its own and reserve lease matching for actual receipts.
        return evidence.audio_receipts_valid();
    }
    if !evidence.audio_receipts_valid() || expected.is_empty() {
        return false;
    }
    let mut consumed_streams = BTreeSet::new();
    evidence.audio_receipts.iter().all(|receipt| {
        let Some(stream_id) = receipt.stream_id.as_deref() else {
            return false;
        };
        if !consumed_streams.insert(stream_id) {
            return false;
        }
        expected.iter().any(|lease| {
            lease.schema_version == 2
                && lease.stream_id == stream_id
                && receipt.transport_schema_version == Some(lease.schema_version)
                && receipt.session_id.as_deref() == Some(lease.session_id.as_str())
                && receipt.turn_id.as_deref() == Some(lease.turn_id.as_str())
                && receipt.generation == Some(lease.generation)
                && receipt.output_selection_mode == Some(lease.output_selection_mode)
                && receipt.output_endpoint_id.as_deref() == Some(lease.output_endpoint_id.as_str())
                && receipt.output_endpoint_generation == Some(lease.output_endpoint_generation)
        })
    })
}

fn fail_closed_invalid_audio_evidence(evidence: &mut NativeTurnExecutionEvidence) {
    evidence.success.audio_submitted = false;
    evidence.success.audio_drained = false;
    evidence.success.audio_receipt_count = 0;
    evidence.audio_receipts.clear();
    let reason = "Runtime audio delivery evidence did not match the authenticated transport-v2 playback lease; audio delivery was not accepted.".to_owned();
    if evidence.success.subtitle_delivered {
        evidence
            .degradations
            .push(NativeTurnExecutionDegradation::SubtitleOnly { reason });
    } else {
        evidence.delivery_state = NativeTurnDeliveryState::ManualRetryRequired;
        evidence.commit_state = NativeDeliveryCommitState::NotCommitted;
        evidence
            .degradations
            .push(NativeTurnExecutionDegradation::ManualRetryRequired {
                failed_role: "tts".into(),
                provider_id: evidence
                    .consumed_route
                    .tts
                    .as_ref()
                    .map(|route| route.provider_id.clone()),
                reason,
                retryable: true,
            });
    }
}

fn presentation_rect(rect: PresentationPixelRect) -> Option<NativeSubtitleRectPx> {
    let width = u32::try_from(i64::from(rect.right) - i64::from(rect.left)).ok()?;
    let height = u32::try_from(i64::from(rect.bottom) - i64::from(rect.top)).ok()?;
    (width > 0 && height > 0 && width <= 16_384 && height <= 16_384).then_some(
        NativeSubtitleRectPx {
            x: rect.left,
            y: rect.top,
            width,
            height,
        },
    )
}

fn native_subtitle_context_from_broker(
    evidence: &TrustedSubtitlePresentationContext,
    safety: NativeSimulationSafetyContext,
    renderer_authority: npc_subtitle_engine::SubtitleRendererAuthorityV1,
) -> Option<NativeSubtitlePresentationContext> {
    let viewport_px = presentation_rect(evidence.client_bounds_px)?;
    presentation_rect(evidence.window_bounds_px)?;
    presentation_rect(evidence.monitor_bounds_px)?;
    presentation_rect(evidence.monitor_work_area_px)?;
    let target_color_space = match evidence.target_color_space? {
        TrustedTargetColorSpace::SdrSrgb => NativeSubtitleTargetColorSpace::SdrSrgb,
        TrustedTargetColorSpace::SdrScRgb => NativeSubtitleTargetColorSpace::SdrScRgb,
        TrustedTargetColorSpace::Hdr10Pq => NativeSubtitleTargetColorSpace::Hdr10Pq,
        TrustedTargetColorSpace::HdrScRgb => NativeSubtitleTargetColorSpace::HdrScRgb,
    };
    let hdr_state_matches = match target_color_space {
        NativeSubtitleTargetColorSpace::SdrSrgb => !evidence.hdr_active,
        NativeSubtitleTargetColorSpace::SdrScRgb => {
            !evidence.hdr_active && evidence.advanced_color_active
        }
        NativeSubtitleTargetColorSpace::Hdr10Pq | NativeSubtitleTargetColorSpace::HdrScRgb => {
            evidence.hdr_supported
                && evidence.hdr_user_enabled
                && evidence.hdr_active
                && evidence.advanced_color_active
        }
        NativeSubtitleTargetColorSpace::Unknown => false,
    };
    let nits = evidence.sdr_white_level_nits as f32;
    let executable_valid = !evidence
        .selected_executable_name
        .contains(['/', '\\', '\0'])
        && evidence.selected_executable_name.len() <= 260
        && evidence
            .selected_executable_name
            .to_ascii_lowercase()
            .ends_with(".exe");
    let attestation_valid = evidence.source_frame_qpc <= evidence.attested_at_qpc
        && evidence
            .attested_at_qpc
            .saturating_sub(evidence.source_frame_qpc)
            <= evidence.qpc_frequency.saturating_mul(2);
    if evidence.schema_version != 1
        || !safety.visuals_allowed
        || evidence.selected_process_id == 0
        || evidence.selected_window == 0
        || !executable_valid
        || evidence.capture_device_generation == 0
        || evidence.geometry_epoch == 0
        || evidence.source_frame_sequence == 0
        || evidence.source_frame_qpc == 0
        || evidence.captured_width_px == 0
        || evidence.captured_height_px == 0
        || evidence.captured_width_px > 32_768
        || evidence.captured_height_px > 32_768
        || evidence.monitor_id.is_empty()
        || evidence.monitor_id.len() > 128
        || !evidence.dpi_available
        || !(48..=960).contains(&evidence.dpi_x)
        || !(48..=960).contains(&evidence.dpi_y)
        || !evidence.hdr_evidence_available
        || !evidence.color_encoding_available
        || !(6..=16).contains(&evidence.bits_per_color_channel)
        || !evidence.sdr_white_level_available
        || !nits.is_finite()
        || !(40.0..=1000.0).contains(&nits)
        || evidence.capture_backend != TrustedCaptureBackend::WindowsGraphicsCapture
        || evidence.capture_scope != TrustedCaptureScope::ExactGameHwndWgc
        || !evidence.overlay_capture_excluded
        || !evidence.overlay_visuals_allowed
        || !evidence.target_color_space_available
        || !hdr_state_matches
        || evidence.attested_at_qpc == 0
        || evidence.qpc_frequency == 0
        || evidence.attestation_id == 0
        || !attestation_valid
    {
        return None;
    }
    Some(NativeSubtitlePresentationContext {
        schema_version: 1,
        provenance: NativeSubtitleContextProvenance::TrustedNativeCapture,
        target: Some(NativeSubtitleTargetIdentity {
            process_id: evidence.selected_process_id,
            window_handle: evidence.selected_window,
            executable_name: evidence.selected_executable_name.clone(),
        }),
        viewport_px,
        dpi_x: evidence.dpi_x,
        dpi_y: evidence.dpi_y,
        target_color_space,
        sdr_white_level_nits: nits,
        capture_device_generation: Some(evidence.capture_device_generation),
        geometry_epoch: Some(evidence.geometry_epoch),
        capture_sequence: Some(evidence.source_frame_sequence),
        capture_qpc: Some(evidence.source_frame_qpc),
        // Presenter protocol v1 names this identity `graphics_generation`.
        graphics_generation: Some(evidence.capture_device_generation),
        attested_at_qpc: Some(evidence.attested_at_qpc),
        qpc_frequency: Some(evidence.qpc_frequency),
        attestation_id: Some(evidence.attestation_id),
        hud_exclusions_px: Vec::new(),
        renderer_authority,
    })
}

#[allow(clippy::too_many_arguments)]
fn native_request_for(
    request: &StartSimulationRequest,
    simulation_id: &str,
    trusted_safety_context: NativeSimulationSafetyContext,
    route_snapshot: &TurnRouteSnapshotV1,
    private_evaluation_acknowledgements: Vec<ProviderPrivateEvaluationAcknowledgementV1>,
    application_namespace: String,
    selected_stt: Option<&ConsumedSelectedStt>,
    subtitle_renderer_authority: &npc_subtitle_engine::SubtitleRendererAuthorityV1,
) -> NativeSimulationRequest {
    let is_generic = request.game_profile_id.is_none();
    let character_name = request
        .character_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Unknown character");
    NativeSimulationRequest {
        session_id: "response-console-simulation".into(),
        turn_id: simulation_id.into(),
        game_id: request
            .game_profile_id
            .clone()
            .unwrap_or_else(|| GENERIC_GAME_ID.into()),
        character_id: (!is_generic)
            .then(|| request.character_id.clone())
            .flatten(),
        // Identity-engine evidence may only come from a future trusted native
        // capture/identity pipeline. WebView turn requests cannot mint it.
        native_identity_decision: None,
        enabled_spoiler_tiers: if is_generic {
            Vec::new()
        } else {
            request.enabled_spoiler_tiers.clone()
        },
        generic_selection: is_generic.then(|| NativeGenericGameSelection {
            game_name: "Unprofiled game".into(),
            executable_name: "unknown-game.exe".into(),
            character_name: character_name.into(),
            protected_online_detected: trusted_safety_context.protected_online_detected,
            anti_cheat_detected: trusted_safety_context.anti_cheat_detected,
        }),
        safety_context: trusted_safety_context,
        transcript: request
            .transcript
            .clone()
            .unwrap_or_else(|| DEFAULT_TRANSCRIPT.into()),
        locale: "en-US".into(),
        route_snapshot: NativeSelectedRouteSnapshot::from(route_snapshot),
        input: NativeTurnInputSnapshot {
            mode: if selected_stt.is_some() {
                NativeTurnInputMode::PushToTalk
            } else {
                NativeTurnInputMode::Typed
            },
            push_to_talk_state: if selected_stt.is_some() {
                NativePushToTalkCaptureState::TranscriptReady
            } else {
                NativePushToTalkCaptureState::NotRequested
            },
            selected_stt_receipt: selected_stt.map(|receipt| NativeSelectedSttTurnEvidenceV1 {
                schema_version: 1,
                receipt_id: receipt.receipt_id.clone(),
                receipt_sha256: receipt.receipt_sha256.clone(),
                capture_session_id: receipt.capture_session_id.clone(),
                capture_turn_id: receipt.capture_turn_id.clone(),
                capture_generation: receipt.generation,
                game_id: receipt.game_id.clone(),
                character_id: receipt.character_id.clone(),
                source_loadout_id: receipt.source_loadout_id.clone(),
                route: receipt.route.clone(),
                chunks_sent: receipt.chunks_sent,
                pcm_bytes_sent: receipt.pcm_bytes_sent,
                partial_events: receipt.partial_events,
            }),
        },
        delivery: NativeTurnDeliveryRequest {
            audio: true,
            subtitles: true,
        },
        audio_playback_leases: Vec::new(),
        private_evaluation_acknowledgements,
        application_namespace,
        // No trusted capture/presenter geometry is currently bound to ordinary
        // turns. Send the explicit console-unavailable sentinel; measured
        // viewport/DPI/HDR values may only be added by native capture state.
        subtitle_presentation_context: Some(
            crate::sidecar_protocol::NativeSubtitlePresentationContext::console_unavailable(
                subtitle_renderer_authority.clone(),
            ),
        ),
        execution_mode: request
            .dev_live_tts
            .as_ref()
            .map(|_| match request.execution_mode {
                crate::domain::ExecutionMode::Cloud => NativeExecutionMode::Cloud,
                crate::domain::ExecutionMode::Hybrid => NativeExecutionMode::Hybrid,
                crate::domain::ExecutionMode::Local => NativeExecutionMode::Local,
            }),
        dev_live_tts: request
            .dev_live_tts
            .as_ref()
            .map(|route| NativeDevLiveTtsRequest {
                provider_id: route.provider_id.clone(),
                model_id: route.model_id.clone(),
                voice_id: route.voice_id.clone(),
                explicit_user_authorization: route.explicit_user_authorization,
            }),
    }
}

fn validate_explicit_dev_route(
    request: &StartSimulationRequest,
    resolved: &ResolvedProviderLoadoutV1,
) -> Result<(), RouterError> {
    let Some(dev_live_tts) = &request.dev_live_tts else {
        return Ok(());
    };
    let Some(tts) = resolved.roles.get(&ProviderRole::Tts) else {
        return Err(RouterError::InvalidRequest(
            "provider_loadout_incomplete: a TTS route is required".into(),
        ));
    };
    let selected = &tts.primary;
    if selected.provider_id != dev_live_tts.provider_id
        || selected.model_id != dev_live_tts.model_id
        || selected.voice_id.as_deref() != Some(dev_live_tts.voice_id.as_str())
    {
        return Err(RouterError::InvalidRequest(
            "provider_route_mismatch: the explicitly authorized live TTS route does not match the pinned loadout".into(),
        ));
    }
    Ok(())
}

fn validate_trusted_safety_context(
    context: NativeSimulationSafetyContext,
) -> Result<(), RouterError> {
    context
        .validate_admitted()
        .map_err(|error| RouterError::InvalidRequest(error.to_string()))
}

fn validate_visual_route_safety(
    routes: &ResolvedProviderLoadoutV1,
    context: NativeSimulationSafetyContext,
) -> Result<(), RouterError> {
    let visual_route_selected = routes.roles.contains_key(&ProviderRole::Vision)
        || routes.roles.contains_key(&ProviderRole::Lipsync);
    if visual_route_selected && !context.visuals_allowed {
        return Err(RouterError::InvalidRequest(
            "trusted target evidence does not authorize visual capture or lip-sync".into(),
        ));
    }
    Ok(())
}

fn fixture_stage_duration(fixture_only: bool, stage: ResponseStage) -> u64 {
    if !fixture_only {
        return 0;
    }
    FIXTURE_STAGE_TIMINGS
        .iter()
        .find_map(|(candidate, duration)| (*candidate == stage).then_some(*duration))
        .unwrap_or(0)
}

fn native_turn_is_running(state: &Arc<Mutex<NativeState>>, generation: u64) -> bool {
    state
        .lock()
        .ok()
        .and_then(|state| {
            state.active.as_ref().map(|active| {
                active.generation == generation && active.status == SimulationStatus::Running
            })
        })
        .unwrap_or(false)
}

fn emit_native_cancelled(
    channel: &Channel<SimulationEvent>,
    simulation_id: &str,
    generation: u64,
    sequence: u64,
) {
    let _ = channel.send(SimulationEvent::Cancelled {
        simulation_id: simulation_id.into(),
        generation,
        sequence,
        reason: "Runtime simulation was cancelled; undelivered dialogue was not committed.".into(),
        turn_execution: None,
        character_context: None,
    });
}

fn emit_native_failure(
    channel: &Channel<SimulationEvent>,
    simulation_id: &str,
    generation: u64,
    error: &SupervisorError,
    state: &Arc<Mutex<NativeState>>,
) {
    let cancelling = state
        .lock()
        .ok()
        .and_then(|state| state.active.as_ref().map(|active| active.status))
        == Some(SimulationStatus::Cancelling);
    let reason = if cancelling {
        "Runtime simulation was cancelled; undelivered dialogue was not committed.".to_owned()
    } else {
        format!("Runtime simulation stopped safely: {error}")
    };
    let event = if cancelling {
        SimulationEvent::Cancelled {
            simulation_id: simulation_id.into(),
            generation,
            sequence: 2,
            reason,
            turn_execution: None,
            character_context: None,
        }
    } else {
        SimulationEvent::Failed {
            simulation_id: simulation_id.into(),
            generation,
            sequence: 2,
            reason,
            turn_execution: None,
            character_context: None,
        }
    };
    let _ = channel.send(event);
}

fn emit_native_encounter_failure(
    channel: &Channel<SimulationEvent>,
    simulation_id: &str,
    generation: u64,
    error: &RouterError,
) {
    let _ = channel.send(SimulationEvent::Failed {
        simulation_id: simulation_id.into(),
        generation,
        sequence: 2,
        reason: format!(
            "The runtime turn returned, but its trusted encounter record could not be stored; encounter correction is disabled for this turn: {error}"
        ),
        turn_execution: None,
        character_context: None,
    });
}

fn emit_playback_cleanup_failure(
    channel: &Channel<SimulationEvent>,
    simulation_id: &str,
    generation: u64,
    state: &Arc<Mutex<NativeState>>,
) {
    let cancelling = state
        .lock()
        .ok()
        .and_then(|state| state.active.as_ref().map(|active| active.status))
        == Some(SimulationStatus::Cancelling);
    let event = if cancelling {
        SimulationEvent::Cancelled {
            simulation_id: simulation_id.into(),
            generation,
            sequence: 2,
            reason: "Runtime playback was cancelled; undelivered dialogue was not committed."
                .into(),
            turn_execution: None,
            character_context: None,
        }
    } else {
        SimulationEvent::Failed {
            simulation_id: simulation_id.into(),
            generation,
            sequence: 2,
            reason: "The native playback broker could not revoke all remaining one-time leases; the turn failed closed. Restart the runtime before retrying."
                .into(),
            turn_execution: None,
            character_context: None,
        }
    };
    let _ = channel.send(event);
}

fn manual_retry_reason(
    evidence: Option<&crate::domain::NativeTurnExecutionEvidence>,
) -> Option<&str> {
    evidence?
        .degradations
        .iter()
        .find_map(|degradation| match degradation {
            crate::domain::NativeTurnExecutionDegradation::ManualRetryRequired {
                reason, ..
            } => Some(reason.as_str()),
            _ => None,
        })
}

fn event_stage(value: &Value) -> Option<ResponseStage> {
    if value.get("type").and_then(Value::as_str) != Some("lifecycle") {
        return None;
    }
    match value.get("stage").and_then(Value::as_str)? {
        "listening" => Some(ResponseStage::Listening),
        "transcribing" => Some(ResponseStage::Transcribing),
        "identifying" => Some(ResponseStage::Identifying),
        "remembering" => Some(ResponseStage::Remembering),
        "responding" => Some(ResponseStage::Responding),
        "voicing" => Some(ResponseStage::Voicing),
        "animating" => Some(ResponseStage::Animating),
        _ => None,
    }
}

fn delivered_text(outcome: &Value) -> String {
    outcome
        .get("delivered")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|sentence| sentence.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn set_native_stage(state: &Arc<Mutex<NativeState>>, generation: u64, stage: ResponseStage) {
    let Ok(mut state) = state.lock() else { return };
    if let Some(active) = state.active.as_mut() {
        if active.generation == generation {
            active.stage = Some(stage);
        }
    }
}

fn finish_native(state: &Arc<Mutex<NativeState>>, generation: u64) {
    let Ok(mut state) = state.lock() else { return };
    if state
        .active
        .as_ref()
        .is_some_and(|active| active.generation == generation)
    {
        state.active = None;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    #[error("simulation request is invalid: {0}")]
    InvalidRequest(String),
    #[error("simulation `{0}` is already active")]
    AlreadyActive(String),
    #[error("simulation state is unavailable")]
    State,
    #[error("character encounter request is invalid: {0}")]
    Character(String),
    #[error("native encounter registry persistence failed: {0}")]
    EncounterPersistence(String),
    #[error(transparent)]
    Fixture(#[from] StartError),
    #[error(transparent)]
    Supervisor(#[from] SupervisorError),
    #[error("authenticated playback lease allocation failed: {0}")]
    PlaybackBroker(#[from] MediaBrokerError),
    #[error(
        "authenticated playback broker returned an incomplete lease pool ({actual}/{expected})"
    )]
    PlaybackPoolIncomplete { expected: usize, actual: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use npc_character_db::{
        EncounterCorrectionSourceV1, EncounterStatus, VoiceCandidateV1, CHARACTER_DB_SCHEMA_VERSION,
    };
    use npc_provider_loadouts::{LoadoutContextV1, ValidationContextV1};
    use serde_json::json;

    fn test_renderer_authority() -> npc_subtitle_engine::SubtitleRendererAuthorityV1 {
        npc_subtitle_engine::bundled_default_renderer_authority()
            .expect("bundled renderer authority")
    }

    fn route_snapshot(generation: u64) -> TurnRouteSnapshotV1 {
        crate::provider_loadouts::starter_document()
            .resolve(&LoadoutContextV1::global(), &ValidationContextV1::online())
            .expect("starter routes resolve")
            .pin_turn_routes(generation)
    }

    fn trusted_subtitle_evidence() -> TrustedSubtitlePresentationContext {
        TrustedSubtitlePresentationContext {
            schema_version: 1,
            selected_process_id: 4242,
            selected_window: 0x1234,
            selected_executable_name: "game.exe".into(),
            capture_device_generation: 8,
            geometry_epoch: 13,
            source_frame_sequence: 21,
            source_frame_qpc: 1_000_000_000,
            window_bounds_px: PresentationPixelRect {
                left: -1920,
                top: 0,
                right: 0,
                bottom: 1080,
            },
            client_bounds_px: PresentationPixelRect {
                left: -1912,
                top: 31,
                right: -8,
                bottom: 1072,
            },
            captured_width_px: 1920,
            captured_height_px: 1080,
            monitor_id: r"\\.\DISPLAY2".into(),
            monitor_bounds_px: PresentationPixelRect {
                left: -1920,
                top: 0,
                right: 0,
                bottom: 1080,
            },
            monitor_work_area_px: PresentationPixelRect {
                left: -1920,
                top: 0,
                right: 0,
                bottom: 1040,
            },
            dpi_available: true,
            dpi_x: 144,
            dpi_y: 144,
            hdr_evidence_available: true,
            hdr_supported: true,
            hdr_user_enabled: true,
            hdr_active: true,
            advanced_color_active: true,
            active_color_mode: 2,
            color_encoding_available: true,
            color_encoding: 0,
            bits_per_color_channel: 10,
            sdr_white_level_available: true,
            sdr_white_level_nits: 203.2,
            capture_backend: TrustedCaptureBackend::WindowsGraphicsCapture,
            capture_scope: TrustedCaptureScope::ExactGameHwndWgc,
            overlay_capture_excluded: true,
            overlay_visuals_allowed: true,
            target_color_space_available: true,
            target_color_space: Some(TrustedTargetColorSpace::HdrScRgb),
            attested_at_qpc: 1_020_000_000,
            qpc_frequency: 10_000_000,
            attestation_id: 89,
        }
    }

    #[test]
    fn command_23_context_maps_exact_native_evidence_and_rejects_unavailable_or_weak_fields() {
        let safety = NativeSimulationSafetyContext::verified_synthetic_fixture();
        let evidence = trusted_subtitle_evidence();
        let mapped =
            native_subtitle_context_from_broker(&evidence, safety, test_renderer_authority())
                .expect("exact trusted command 23 context");
        assert_eq!(
            mapped.provenance,
            NativeSubtitleContextProvenance::TrustedNativeCapture
        );
        assert_eq!(mapped.target.as_ref().expect("target").process_id, 4242);
        assert_eq!(mapped.viewport_px.x, -1912);
        assert_eq!(mapped.viewport_px.width, 1904);
        assert_eq!(mapped.capture_device_generation, Some(8));
        assert_eq!(mapped.graphics_generation, Some(8));
        assert_eq!(mapped.capture_sequence, Some(21));
        assert_eq!(mapped.attestation_id, Some(89));

        let mut rejected = evidence.clone();
        rejected.dpi_available = false;
        rejected.dpi_x = 0;
        rejected.dpi_y = 0;
        assert!(
            native_subtitle_context_from_broker(&rejected, safety, test_renderer_authority())
                .is_none()
        );
        rejected = evidence.clone();
        rejected.capture_scope = TrustedCaptureScope::MonitorRegionCrop;
        assert!(
            native_subtitle_context_from_broker(&rejected, safety, test_renderer_authority())
                .is_none()
        );
        rejected = evidence.clone();
        rejected.target_color_space_available = false;
        rejected.target_color_space = None;
        assert!(
            native_subtitle_context_from_broker(&rejected, safety, test_renderer_authority())
                .is_none()
        );
        rejected = evidence.clone();
        rejected.source_frame_qpc = rejected.attested_at_qpc - 2 * rejected.qpc_frequency - 1;
        assert!(
            native_subtitle_context_from_broker(&rejected, safety, test_renderer_authority())
                .is_none()
        );
        assert!(native_subtitle_context_from_broker(
            &evidence,
            NativeSimulationSafetyContext::verified_safe(),
            test_renderer_authority(),
        )
        .is_none());
    }

    fn encounter(game_profile_id: &str, encounter_id: uuid::Uuid) -> EncounterRecordV1 {
        let now = current_epoch_ms_i64();
        EncounterRecordV1 {
            schema_version: CHARACTER_DB_SCHEMA_VERSION.into(),
            encounter_id,
            game_profile_id: game_profile_id.into(),
            archetype_character_id: "background-citizen".into(),
            continuity_key_sha256: "a".repeat(64),
            selected_voice: VoiceCandidateV1 {
                binding_id: "fixture-voice".into(),
                adapter_id: "fixture-adapter".into(),
                provider_voice_id: "fixture-provider-voice".into(),
                locale: "en-US".into(),
                traits: Vec::new(),
                catalog_version: None,
                license: None,
            },
            created_at_ms: now.saturating_sub(1_000),
            last_seen_at_ms: now.saturating_sub(500),
            expires_at_ms: now.saturating_add(60_000),
            status: EncounterStatus::Active,
        }
    }

    fn test_router(root: &Path, encounter_store_path: PathBuf) -> RuntimeRouter {
        let executable = std::env::current_exe().expect("current test executable");
        let supervisor = crate::sidecar_supervisor::RuntimeSupervisor::try_new(
            crate::sidecar_supervisor::RuntimeLaunchConfig {
                executable,
                resource_root: root.to_path_buf(),
                app_data: root.join("runtime-data"),
                development_fixture_allowed: false,
            },
        )
        .expect("test runtime supervisor");
        let broker = crate::media_broker::MediaBrokerSupervisor::new(
            crate::media_broker::MediaBrokerLaunchConfig::from_application(false, root)
                .expect("test broker config"),
            supervisor.clone(),
        );
        let diagnostics_root = root.join(format!("diagnostics-{}", uuid::Uuid::new_v4()));
        let diagnostics = Arc::new(
            DiagnosticsV2Manager::new(&diagnostics_root).expect("test diagnostics manager"),
        );
        RuntimeRouter::new(
            supervisor,
            broker,
            VisualCoordinator::unavailable_for_tests(),
            encounter_store_path,
            diagnostics,
        )
        .expect("test runtime router")
    }

    #[test]
    fn native_encounter_correction_is_same_game_explicit_and_requires_migration() {
        let resources = crate::catalog::ResourceCatalog::new(None);
        let profile = resources
            .load_game_profile("skyrim-special-edition")
            .expect("authored profile");
        let database = CharacterDatabase::new(profile).expect("character database");
        let encounter_id = uuid::Uuid::new_v4();
        let mut state = NativeEncounterState::default();
        state
            .remember(encounter("skyrim-special-edition", encounter_id))
            .expect("trusted runtime encounter");
        assert!(state
            .correct(&database, encounter_id, "lydia", false)
            .is_err());
        let event = state
            .correct(&database, encounter_id, "lydia", true)
            .expect("explicit correction");
        assert!(matches!(
            event,
            EncounterLifecycleEventV1::CorrectedToAuthoredCharacter {
                source: EncounterCorrectionSourceV1::ManualExplicit,
                memory_migration_required: true,
                ..
            }
        ));
        let directory = tempfile::tempdir().expect("encounter store directory");
        let path = directory.path().join("encounters.json");
        persist_native_encounters(&path, &state).expect("persist corrected registry");
        let reopened = load_native_encounters(&path).expect("reopen corrected registry");
        assert_eq!(
            reopened.by_game["skyrim-special-edition"]
                .record(encounter_id)
                .expect("persisted encounter")
                .status,
            EncounterStatus::Expired
        );
        assert!(state
            .correct(&database, encounter_id, "lydia", true)
            .is_err());
    }

    #[test]
    fn native_encounter_merge_rejects_cross_game_and_expires_only_source() {
        let source = uuid::Uuid::new_v4();
        let destination = uuid::Uuid::new_v4();
        let mut state = NativeEncounterState::default();
        state
            .remember(encounter("skyrim-special-edition", source))
            .expect("source");
        state
            .remember(encounter("skyrim-special-edition", destination))
            .expect("destination");
        assert!(state.merge("fallout-4", source, destination, true).is_err());
        let event = state
            .merge("skyrim-special-edition", source, destination, true)
            .expect("same-game explicit merge");
        assert!(matches!(
            event,
            EncounterLifecycleEventV1::MergedIntoEncounter {
                source: EncounterCorrectionSourceV1::ManualExplicit,
                memory_migration_required: true,
                ..
            }
        ));
        let registry = &state.by_game["skyrim-special-edition"];
        assert_eq!(
            registry.record(destination).expect("destination").status,
            EncounterStatus::Active
        );
    }

    #[test]
    fn correction_and_merge_survive_actual_runtime_router_reconstruction() {
        let directory = tempfile::tempdir().expect("router persistence directory");
        let path = directory.path().join("encounter-registry-v1.json");
        let correction = uuid::Uuid::new_v4();
        let source = uuid::Uuid::new_v4();
        let destination = uuid::Uuid::new_v4();
        let mut seeded = NativeEncounterState::default();
        for id in [correction, source, destination] {
            seeded
                .remember(encounter("skyrim-special-edition", id))
                .expect("seed authenticated encounter");
        }
        persist_native_encounters(&path, &seeded).expect("persist seeded registry");

        let resources = crate::catalog::ResourceCatalog::new(None);
        let profile = resources
            .load_game_profile("skyrim-special-edition")
            .expect("authored profile");
        let database = CharacterDatabase::new(profile).expect("character database");
        let router = test_router(directory.path(), path.clone());
        router
            .correct_encounter(&database, correction, "lydia", true)
            .expect("persist explicit correction through router");
        router
            .merge_encounters("skyrim-special-edition", source, destination, true)
            .expect("persist explicit merge through router");
        drop(router);

        let reopened = test_router(directory.path(), path);
        let state = reopened
            .encounters
            .lock()
            .expect("reopened encounter state");
        let registry = &state.by_game["skyrim-special-edition"];
        assert_eq!(
            registry
                .record(correction)
                .expect("corrected encounter")
                .status,
            EncounterStatus::Expired
        );
        let redirected = registry.record(source).expect("merged source redirect");
        assert_eq!(redirected.encounter_id, destination);
        assert_eq!(redirected.status, EncounterStatus::Active);
    }

    #[test]
    fn rejected_authenticated_observation_does_not_mutate_staged_registry() {
        let resources = crate::catalog::ResourceCatalog::new(None);
        let profile = resources
            .load_game_profile("skyrim-special-edition")
            .expect("authored profile");
        let database = CharacterDatabase::new(profile).expect("character database");
        let encounter_id = uuid::Uuid::new_v4();
        let mut state = NativeEncounterState::default();
        state
            .remember(encounter("skyrim-special-edition", encounter_id))
            .expect("seed encounter");
        state
            .correct(&database, encounter_id, "lydia", true)
            .expect("expire via explicit correction");
        let before = serde_json::to_vec(&state).expect("serialize before rejection");
        let mut staged = state.clone();
        assert!(staged
            .remember(encounter("skyrim-special-edition", encounter_id))
            .is_err());
        assert_eq!(
            serde_json::to_vec(&staged).expect("serialize after rejection"),
            before
        );
    }

    #[test]
    fn maps_only_response_spine_lifecycle_stages() {
        assert_eq!(
            event_stage(&json!({"type":"lifecycle","stage":"remembering"})),
            Some(ResponseStage::Remembering)
        );
        assert_eq!(
            event_stage(&json!({"type":"timing","stage":"remembering"})),
            None
        );
        assert_eq!(
            event_stage(&json!({"type":"lifecycle","stage":"committing"})),
            None
        );
    }

    #[test]
    fn delivered_text_uses_only_delivered_sentences() {
        let outcome = json!({
            "delivered": [{"text":"First."},{"text":"Second."}],
            "generatedButNotDelivered": "must not appear"
        });
        assert_eq!(delivered_text(&outcome), "First. Second.");
    }

    #[test]
    fn live_llm_with_subtitle_only_tts_starts_with_pending_provider_evidence() {
        let mut routes = route_snapshot(3);
        routes
            .roles
            .get_mut(&ProviderRole::Tts)
            .and_then(|role| role.primary.as_mut())
            .expect("TTS route")
            .provider_id = "mock-subtitle-only".into();
        let (basis, fixture_only) = start_provenance(&StartSimulationRequest::default(), &routes);
        assert_eq!(basis, MeasurementBasis::PendingProviderEvidence);
        assert!(
            !fixture_only,
            "the selected live LLM route is not a fixture"
        );

        for role in routes.roles.values_mut() {
            if let Some(primary) = role.primary.as_mut() {
                primary.provider_id = format!("mock-{}", primary.provider_id);
            }
        }
        let (basis, fixture_only) = start_provenance(&StartSimulationRequest::default(), &routes);
        assert_eq!(basis, MeasurementBasis::TrustedRuntimeFixture);
        assert!(fixture_only);
    }

    #[test]
    fn ordinary_hosted_tts_gets_exact_broker_pcm_format_but_debug_wasapi_does_not() {
        let mut routes = route_snapshot(9);
        assert_eq!(
            selected_playback_format(&StartSimulationRequest::default(), &routes),
            Some((24_000, 1))
        );

        let tts = routes
            .roles
            .get_mut(&ProviderRole::Tts)
            .and_then(|role| role.primary.as_mut())
            .expect("TTS route");
        tts.provider_id = "nvidia-nim-magpie".into();
        tts.model_id = "magpie-tts-multilingual".into();
        tts.credential.as_mut().expect("credential").provider_id = "nvidia-nim".into();
        assert_eq!(
            selected_playback_format(&StartSimulationRequest::default(), &routes),
            Some((44_100, 1))
        );

        let debug = StartSimulationRequest {
            dev_live_tts: Some(crate::domain::DevLiveTtsRequest {
                provider_id: "elevenlabs".into(),
                model_id: "eleven_flash_v2_5".into(),
                voice_id: "EXAVITQu4vr4xnSDxMaL".into(),
                explicit_user_authorization: true,
            }),
            ..StartSimulationRequest::default()
        };
        assert_eq!(selected_playback_format(&debug, &routes), None);
    }

    #[test]
    fn broker_pool_request_is_exactly_bound_to_the_admitted_runtime_turn() {
        for (sample_rate, channels) in [(24_000, 1), (44_100, 1)] {
            let request =
                playback_pool_request("runtime-simulation-000009", 9, sample_rate, channels);
            assert_eq!(request.session_id, "response-console-simulation");
            assert_eq!(request.turn_id, "runtime-simulation-000009");
            assert_eq!(request.generation, 9);
            assert_eq!(request.sample_rate, sample_rate);
            assert_eq!(request.channels, channels);
            assert_eq!(request.max_frames_per_lease, u64::from(sample_rate) * 60);
            assert_eq!(request.lease_count, MAX_PLAYBACK_LEASES_PER_TURN);
        }
    }

    #[test]
    fn eclipse_harbor_uses_exact_authored_profile_and_character_authority() {
        let request = StartSimulationRequest {
            transcript: Some(DEFAULT_TRANSCRIPT.into()),
            enabled_spoiler_tiers: vec!["act-1".into()],
            ..StartSimulationRequest::default()
        };
        let native = native_request_for(
            &request,
            "fixture-turn-1",
            NativeSimulationSafetyContext::default(),
            &route_snapshot(1),
            Vec::new(),
            crate::provider_loadouts::PRODUCTION_APPLICATION_NAMESPACE.into(),
            None,
            &test_renderer_authority(),
        );
        assert_eq!(native.game_id, "eclipse-harbor");
        assert_eq!(native.character_id.as_deref(), Some("mara-venn"));
        assert_eq!(native.transcript, DEFAULT_TRANSCRIPT);
        assert_eq!(native.enabled_spoiler_tiers, vec!["act-1"]);
        assert_eq!(
            native.application_namespace,
            crate::provider_loadouts::PRODUCTION_APPLICATION_NAMESPACE
        );
        assert!(native.generic_selection.is_none());
        let subtitle = native
            .subtitle_presentation_context
            .expect("native console fallback context");
        assert_eq!(
            subtitle.provenance,
            crate::sidecar_protocol::NativeSubtitleContextProvenance::ConsoleBottomCenterUnavailable
        );
        assert!(subtitle.target.is_none());
        assert_eq!(subtitle.viewport_px.width, 0);
        assert_eq!(subtitle.dpi_x, 0);
        assert!(subtitle.geometry_epoch.is_none());
    }

    #[test]
    fn explicitly_unprofiled_turn_is_generic_without_identity_claim() {
        let request = StartSimulationRequest {
            game_profile_id: None,
            character_id: None,
            character_name: Some("Innkeeper".into()),
            enabled_spoiler_tiers: vec!["must-not-cross".into()],
            ..StartSimulationRequest::default()
        };
        let native = native_request_for(
            &request,
            "generic-turn-1",
            NativeSimulationSafetyContext::default(),
            &route_snapshot(1),
            Vec::new(),
            crate::provider_loadouts::PRODUCTION_APPLICATION_NAMESPACE.into(),
            None,
            &test_renderer_authority(),
        );
        let generic = native.generic_selection.expect("generic manual selection");
        assert_eq!(native.game_id, GENERIC_GAME_ID);
        assert!(native.character_id.is_none());
        assert!(native.native_identity_decision.is_none());
        assert!(native.enabled_spoiler_tiers.is_empty());
        assert_eq!(generic.character_name, "Innkeeper");
    }

    #[test]
    fn authorized_dev_live_tts_is_shaped_without_credentials() {
        let request = StartSimulationRequest {
            dev_live_tts: Some(crate::domain::DevLiveTtsRequest {
                provider_id: "elevenlabs".into(),
                model_id: "eleven_flash_v2_5".into(),
                voice_id: "EXAVITQu4vr4xnSDxMaL".into(),
                explicit_user_authorization: true,
            }),
            ..StartSimulationRequest::default()
        };
        let native = native_request_for(
            &request,
            "dev-live-tts-turn",
            NativeSimulationSafetyContext::default(),
            &route_snapshot(1),
            Vec::new(),
            crate::provider_loadouts::PRODUCTION_APPLICATION_NAMESPACE.into(),
            None,
            &test_renderer_authority(),
        );
        let route = native.dev_live_tts.as_ref().expect("authorized route");

        assert_eq!(route.provider_id, "elevenlabs");
        assert_eq!(route.model_id, "eleven_flash_v2_5");
        assert_eq!(route.voice_id, "EXAVITQu4vr4xnSDxMaL");
        assert!(route.explicit_user_authorization);
        assert_eq!(native.route_snapshot.generation, 1);
        assert_eq!(
            native.route_snapshot.roles.tts.primary.as_ref(),
            Some(&crate::sidecar_protocol::NativeSelectedProviderRoute {
                provider_id: "elevenlabs".into(),
                model_id: "eleven_flash_v2_5".into(),
                voice_id: Some("EXAVITQu4vr4xnSDxMaL".into()),
                execution: crate::sidecar_protocol::NativeRouteExecution::Cloud,
                egress: "providerCloud".into(),
                credential_reference: Some("providers/elevenlabs".into()),
            })
        );
        assert!(matches!(
            native.execution_mode,
            Some(NativeExecutionMode::Hybrid)
        ));
    }

    #[test]
    fn explicit_live_tts_must_match_the_resolved_immutable_route() {
        let resolved = crate::provider_loadouts::starter_document()
            .resolve(&LoadoutContextV1::global(), &ValidationContextV1::online())
            .expect("starter routes resolve");
        let matching = StartSimulationRequest {
            dev_live_tts: Some(crate::domain::DevLiveTtsRequest {
                provider_id: "elevenlabs".into(),
                model_id: "eleven_flash_v2_5".into(),
                voice_id: "EXAVITQu4vr4xnSDxMaL".into(),
                explicit_user_authorization: true,
            }),
            ..StartSimulationRequest::default()
        };
        validate_explicit_dev_route(&matching, &resolved).expect("matching route");

        let mismatched = StartSimulationRequest {
            dev_live_tts: Some(crate::domain::DevLiveTtsRequest {
                voice_id: "different-stock-voice".into(),
                ..matching.dev_live_tts.expect("matching dev route")
            }),
            ..StartSimulationRequest::default()
        };
        let error = validate_explicit_dev_route(&mismatched, &resolved)
            .expect_err("mismatched voice must be rejected");
        assert_eq!(
            error.to_string(),
            "simulation request is invalid: provider_route_mismatch: the explicitly authorized live TTS route does not match the pinned loadout"
        );
    }

    #[test]
    fn authored_profile_safety_is_trusted_router_state_not_webview_input() {
        let request = StartSimulationRequest {
            game_profile_id: Some("cyberpunk-2077".into()),
            ..StartSimulationRequest::default()
        };
        let webview_wire = serde_json::to_value(&request).expect("serialize WebView request");
        assert!(webview_wire.get("safetyContext").is_none());
        assert!(webview_wire.get("applicationNamespace").is_none());

        let trusted = NativeSimulationSafetyContext {
            evidence_state: crate::sidecar_protocol::NativeSafetyEvidenceState::Blocked,
            profile_policy: crate::sidecar_protocol::NativeProfileSafetyPolicy::SinglePlayerOnly,
            visuals_allowed: false,
            protected_online_detected: false,
            anti_cheat_detected: true,
        };
        let native = native_request_for(
            &request,
            "unsafe-authored-turn",
            trusted,
            &route_snapshot(1),
            Vec::new(),
            crate::provider_loadouts::PRODUCTION_APPLICATION_NAMESPACE.into(),
            None,
            &test_renderer_authority(),
        );
        assert_eq!(native.safety_context, trusted);
        assert!(validate_trusted_safety_context(native.safety_context).is_err());
    }

    #[test]
    fn unknown_and_detected_target_states_fail_closed_and_block_visual_routes() {
        use crate::sidecar_protocol::{NativeProfileSafetyPolicy, NativeSafetyEvidenceState};

        let unknown = NativeSimulationSafetyContext {
            evidence_state: NativeSafetyEvidenceState::Unknown,
            profile_policy: NativeProfileSafetyPolicy::SinglePlayerOnly,
            visuals_allowed: false,
            protected_online_detected: false,
            anti_cheat_detected: false,
        };
        assert!(validate_trusted_safety_context(unknown).is_err());

        let console_isolated = NativeSimulationSafetyContext {
            evidence_state: NativeSafetyEvidenceState::ConsoleIsolated,
            profile_policy: NativeProfileSafetyPolicy::ConsoleIsolatedNoGameInteraction,
            visuals_allowed: false,
            protected_online_detected: false,
            anti_cheat_detected: false,
        };
        validate_trusted_safety_context(console_isolated)
            .expect("profile-authored console conversation");
        assert!(
            validate_trusted_safety_context(NativeSimulationSafetyContext {
                visuals_allowed: true,
                ..console_isolated
            })
            .is_err()
        );

        for detected in [
            NativeSimulationSafetyContext {
                evidence_state: NativeSafetyEvidenceState::Blocked,
                protected_online_detected: true,
                ..unknown
            },
            NativeSimulationSafetyContext {
                evidence_state: NativeSafetyEvidenceState::Blocked,
                anti_cheat_detected: true,
                ..unknown
            },
        ] {
            assert!(validate_trusted_safety_context(detected).is_err());
        }

        let mut routes = crate::provider_loadouts::starter_document()
            .resolve(&LoadoutContextV1::global(), &ValidationContextV1::online())
            .expect("starter routes");
        let llm_route = routes.roles[&ProviderRole::Llm].clone();
        routes.roles.insert(ProviderRole::Vision, llm_route);
        let verified_audio_only = NativeSimulationSafetyContext {
            evidence_state: NativeSafetyEvidenceState::VerifiedSafe,
            visuals_allowed: false,
            ..unknown
        };
        validate_trusted_safety_context(verified_audio_only).expect("verified target");
        assert!(validate_visual_route_safety(&routes, verified_audio_only).is_err());
    }

    #[test]
    fn only_fixture_results_receive_visible_stage_pacing() {
        for (stage, expected_duration) in FIXTURE_STAGE_TIMINGS {
            assert_eq!(
                fixture_stage_duration(true, *stage),
                *expected_duration,
                "fixture stage {stage:?}"
            );
            assert_eq!(
                fixture_stage_duration(false, *stage),
                0,
                "non-fixture stage {stage:?}"
            );
        }
        assert!(FIXTURE_STAGE_TIMINGS
            .iter()
            .all(|(_, duration)| *duration >= 180));
    }

    fn broker_audio_evidence() -> NativeTurnExecutionEvidence {
        serde_json::from_value(json!({
            "consumedRoute": {"schemaVersion":1,"sourceLoadoutId":"test","generation":7,"sha256":"abc","llm":null,"tts":{"providerId":"elevenlabs","modelId":"eleven_flash_v2_5","voiceId":"stock"}},
            "input":{"mode":"typed","pushToTalkState":"notRequested"},
            "deliveryState":"delivered",
            "commitState":"committed",
            "subtitles":[{"sentenceId":1,"textStartBytes":0,"textEndBytes":5,"speaker":"Mara","text":"Hello"}],
            "subtitlePresentationReceipts":[],
            "audioReceipts":[{"receiptId":"receipt-001","sentenceId":1,"sink":"nativeBrokerSubmission","transportSchemaVersion":2,"streamId":"pcm-001","sessionId":"response-console-simulation","turnId":"runtime-simulation-000007","generation":7,"sourceFrames":2400,"deviceFrames":2400,"durationMs":100,"peak":0.5,"rms":0.1,"outputSelectionMode":"systemDefault","outputEndpointId":"{0.0.0.00000000}.fixture-output","outputEndpointGeneration":17,"cancelled":false,"submitted":true,"drained":true,"completed":true}],
            "degradations":[],
            "success":{"llmProviderLive":true,"ttsProviderLive":true,"sttSkipped":true,"subtitleDelivered":false,"subtitleReceiptCount":0,"audioSubmitted":true,"audioDrained":true,"audioReceiptCount":1}
        }))
        .expect("broker audio evidence")
    }

    fn expected_broker_audio() -> ExpectedPlaybackReceipt {
        ExpectedPlaybackReceipt {
            schema_version: 2,
            stream_id: "pcm-001".into(),
            session_id: "response-console-simulation".into(),
            turn_id: "runtime-simulation-000007".into(),
            generation: 7,
            output_selection_mode: NativeAudioOutputSelectionMode::SystemDefault,
            output_endpoint_id: "{0.0.0.00000000}.fixture-output".into(),
            output_endpoint_generation: 17,
        }
    }

    #[test]
    fn broker_audio_claim_requires_exact_allocated_v2_lease_and_fails_closed() {
        let evidence = broker_audio_evidence();
        assert!(evidence.audio_receipts_valid());
        assert!(audio_receipts_match_expected(
            &evidence,
            &[expected_broker_audio()]
        ));

        let mut stale = expected_broker_audio();
        stale.output_endpoint_generation = 18;
        assert!(!audio_receipts_match_expected(&evidence, &[stale]));

        let mut sanitized = evidence;
        fail_closed_invalid_audio_evidence(&mut sanitized);
        assert!(!sanitized.success.audio_submitted);
        assert!(!sanitized.success.audio_drained);
        assert_eq!(sanitized.success.audio_receipt_count, 0);
        assert!(sanitized.audio_receipts.is_empty());
        assert_eq!(
            sanitized.delivery_state,
            NativeTurnDeliveryState::ManualRetryRequired
        );
        assert_eq!(
            sanitized.commit_state,
            NativeDeliveryCommitState::NotCommitted
        );
        assert!(matches!(
            sanitized.degradations.last(),
            Some(NativeTurnExecutionDegradation::ManualRetryRequired { reason, .. })
                if reason.contains("transport-v2")
        ));
    }

    #[test]
    fn unused_allocated_audio_leases_do_not_create_an_audio_claim() {
        let mut evidence = broker_audio_evidence();
        evidence.audio_receipts.clear();
        evidence.success.audio_submitted = false;
        evidence.success.audio_drained = false;
        evidence.success.audio_receipt_count = 0;
        evidence.delivery_state = NativeTurnDeliveryState::Delivered;
        evidence.commit_state = NativeDeliveryCommitState::Committed;
        evidence.success.subtitle_delivered = true;
        evidence.success.subtitle_receipt_count = 1;

        assert!(evidence.audio_receipts_valid());
        assert!(audio_receipts_match_expected(
            &evidence,
            &[expected_broker_audio()]
        ));
    }
}
