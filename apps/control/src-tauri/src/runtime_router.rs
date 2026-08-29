use crate::domain::{
    CancelOutcome, CancelSimulationResult, MeasurementBasis, ResponseStage, RuntimeBackend,
    RuntimeConnectionState, SimulationEvent, SimulationSnapshot, SimulationStatus,
    StartSimulationRequest, StartSimulationResult,
};
use crate::runtime_bridge::{SimulationController, StartError};
use crate::sidecar_protocol::{
    NativeDevLiveTtsRequest, NativeExecutionMode, NativeGenericGameSelection,
    NativeSimulationRequest, NativeSimulationResult, NativeSimulationSafetyContext,
};
use crate::sidecar_supervisor::{RuntimeSupervisor, SupervisorError};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

const ECLIPSE_HARBOR_PROFILE_ID: &str = "eclipse-harbor";
const GENERIC_GAME_ID: &str = "generic-game";
const ECLIPSE_HARBOR_CHARACTER: &str = "Mara Venn";
const ECLIPSE_HARBOR_EXECUTABLE: &str = "interactive-npcs-synthetic-target.exe";
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

#[derive(Debug)]
struct NativeActive {
    simulation_id: String,
    generation: u64,
    status: SimulationStatus,
    stage: Option<ResponseStage>,
}

#[derive(Debug, Default)]
struct NativeState {
    generation: u64,
    active: Option<NativeActive>,
}

#[derive(Clone, Debug)]
pub struct RuntimeRouter {
    supervisor: RuntimeSupervisor,
    fixture: SimulationController,
    native: Arc<Mutex<NativeState>>,
    trusted_safety_context: NativeSimulationSafetyContext,
}

impl RuntimeRouter {
    pub fn new(supervisor: RuntimeSupervisor) -> Self {
        Self::with_trusted_safety_context(supervisor, NativeSimulationSafetyContext::default())
    }

    pub(crate) fn with_trusted_safety_context(
        supervisor: RuntimeSupervisor,
        trusted_safety_context: NativeSimulationSafetyContext,
    ) -> Self {
        Self {
            supervisor,
            fixture: SimulationController::default(),
            native: Arc::new(Mutex::new(NativeState::default())),
            trusted_safety_context,
        }
    }

    pub fn supervisor(&self) -> &RuntimeSupervisor {
        &self.supervisor
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

    pub async fn start(
        &self,
        request: StartSimulationRequest,
        events: Channel<SimulationEvent>,
    ) -> Result<StartSimulationResult, RouterError> {
        request.validate().map_err(RouterError::InvalidRequest)?;
        validate_trusted_safety_context(self.trusted_safety_context)?;
        if self.supervisor.health().state == RuntimeConnectionState::DevelopmentFixture {
            return self
                .fixture
                .start(request, events)
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
            });
            (simulation_id, generation)
        };

        let native_request =
            native_request_for(&request, &simulation_id, self.trusted_safety_context);
        let _ = events.send(SimulationEvent::Started {
            simulation_id: simulation_id.clone(),
            generation,
            sequence: 1,
            measurement_basis: MeasurementBasis::TrustedRuntimeFixture,
        });
        let supervisor = self.supervisor.clone();
        let native_state = Arc::clone(&self.native);
        let task_id = simulation_id.clone();
        tauri::async_runtime::spawn(async move {
            match supervisor.simulate(native_request).await {
                Ok(result) => {
                    emit_native_result(&events, &task_id, generation, result, &native_state).await
                }
                Err(error) => {
                    emit_native_failure(&events, &task_id, generation, &error, &native_state)
                }
            }
            finish_native(&native_state, generation);
        });

        Ok(StartSimulationResult {
            simulation_id,
            generation,
            backend: RuntimeBackend::NativeRuntime,
            measurement_basis: MeasurementBasis::TrustedRuntimeFixture,
            runtime_fixture_only: true,
        })
    }

    pub async fn cancel(&self) -> CancelSimulationResult {
        let native = self.native.lock().ok().and_then(|mut state| {
            state.active.as_mut().map(|active| {
                active.status = SimulationStatus::Cancelling;
                (active.simulation_id.clone(), active.generation)
            })
        });
        if let Some((simulation_id, generation)) = native {
            let outcome = if self.supervisor.cancel().await.is_ok() {
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

async fn emit_native_result(
    channel: &Channel<SimulationEvent>,
    simulation_id: &str,
    generation: u64,
    result: NativeSimulationResult,
    state: &Arc<Mutex<NativeState>>,
) {
    if result.schema_version != "1.0.0" {
        let _ = channel.send(SimulationEvent::Cancelled {
            simulation_id: simulation_id.into(),
            generation,
            sequence: 2,
            reason: "Runtime simulation returned an incompatible result contract.".into(),
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
    sequence += 1;
    if lifecycle == "completed" {
        let _ = channel.send(SimulationEvent::Completed {
            simulation_id: simulation_id.into(),
            generation,
            sequence,
            fixture_first_audio_ms: 0,
            runtime_fixture_only: result.fixture_only,
            delivered_text: delivered_text(&result.outcome),
        });
    } else {
        let reason = if lifecycle == "cancelled" {
            "Runtime simulation was cancelled; undelivered dialogue was not committed."
        } else {
            "Runtime simulation ended without a completed delivery."
        };
        let _ = channel.send(SimulationEvent::Cancelled {
            simulation_id: simulation_id.into(),
            generation,
            sequence,
            reason: reason.into(),
        });
    }
}

fn native_request_for(
    request: &StartSimulationRequest,
    simulation_id: &str,
    trusted_safety_context: NativeSimulationSafetyContext,
) -> NativeSimulationRequest {
    let is_eclipse_harbor = request.game_profile_id.as_deref() == Some(ECLIPSE_HARBOR_PROFILE_ID);
    let character_name = request
        .character_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(ECLIPSE_HARBOR_CHARACTER);
    NativeSimulationRequest {
        session_id: "response-console-simulation".into(),
        turn_id: simulation_id.into(),
        game_id: if is_eclipse_harbor {
            GENERIC_GAME_ID.into()
        } else {
            request
                .game_profile_id
                .clone()
                .unwrap_or_else(|| "skyrim-special-edition".into())
        },
        character_id: if is_eclipse_harbor {
            None
        } else {
            request.character_id.clone()
        },
        generic_selection: is_eclipse_harbor.then(|| NativeGenericGameSelection {
            game_name: "Eclipse Harbor".into(),
            executable_name: ECLIPSE_HARBOR_EXECUTABLE.into(),
            character_name: character_name.into(),
            protected_online_detected: false,
            anti_cheat_detected: false,
        }),
        safety_context: trusted_safety_context,
        transcript: request
            .transcript
            .clone()
            .unwrap_or_else(|| DEFAULT_TRANSCRIPT.into()),
        locale: "en-US".into(),
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

fn validate_trusted_safety_context(
    context: NativeSimulationSafetyContext,
) -> Result<(), RouterError> {
    if context.protected_online_detected {
        return Err(RouterError::InvalidRequest(
            "trusted game-state evidence detected protected online play".into(),
        ));
    }
    if context.anti_cheat_detected {
        return Err(RouterError::InvalidRequest(
            "trusted game-state evidence detected anti-cheat".into(),
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
    let _ = channel.send(SimulationEvent::Cancelled {
        simulation_id: simulation_id.into(),
        generation,
        sequence: 2,
        reason,
    });
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
    #[error(transparent)]
    Fixture(#[from] StartError),
    #[error(transparent)]
    Supervisor(#[from] SupervisorError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn eclipse_harbor_uses_a_truthful_generic_fixture_selection() {
        let request = StartSimulationRequest {
            transcript: Some(DEFAULT_TRANSCRIPT.into()),
            ..StartSimulationRequest::default()
        };
        let native = native_request_for(
            &request,
            "fixture-turn-1",
            NativeSimulationSafetyContext::default(),
        );
        let selection = native
            .generic_selection
            .expect("Eclipse Harbor must use explicit generic selection");

        assert_eq!(native.game_id, GENERIC_GAME_ID);
        assert_eq!(native.character_id, None);
        assert_eq!(native.transcript, DEFAULT_TRANSCRIPT);
        assert_eq!(selection.game_name, "Eclipse Harbor");
        assert_eq!(selection.character_name, ECLIPSE_HARBOR_CHARACTER);
        assert_eq!(selection.executable_name, ECLIPSE_HARBOR_EXECUTABLE);
        assert!(!selection.protected_online_detected);
        assert!(!selection.anti_cheat_detected);
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
        );
        let route = native.dev_live_tts.as_ref().expect("authorized route");

        assert_eq!(route.provider_id, "elevenlabs");
        assert_eq!(route.model_id, "eleven_flash_v2_5");
        assert_eq!(route.voice_id, "EXAVITQu4vr4xnSDxMaL");
        assert!(route.explicit_user_authorization);
        assert!(matches!(
            native.execution_mode,
            Some(NativeExecutionMode::Hybrid)
        ));
    }

    #[test]
    fn authored_profile_safety_is_trusted_router_state_not_webview_input() {
        let request = StartSimulationRequest {
            game_profile_id: Some("cyberpunk-2077".into()),
            ..StartSimulationRequest::default()
        };
        let webview_wire = serde_json::to_value(&request).expect("serialize WebView request");
        assert!(webview_wire.get("safetyContext").is_none());

        let trusted = NativeSimulationSafetyContext {
            protected_online_detected: false,
            anti_cheat_detected: true,
        };
        let native = native_request_for(&request, "unsafe-authored-turn", trusted);
        assert_eq!(native.safety_context, trusted);
        assert!(validate_trusted_safety_context(native.safety_context).is_err());
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
}
