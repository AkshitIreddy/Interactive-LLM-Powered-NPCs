use crate::domain::{
    CancelOutcome, CancelSimulationResult, MeasurementBasis, ResponseStage, RuntimeBackend,
    RuntimeConnectionState, SimulationEvent, SimulationSnapshot, SimulationStatus,
    StartSimulationRequest, StartSimulationResult,
};
use crate::runtime_bridge::{SimulationController, StartError};
use crate::sidecar_protocol::{NativeSimulationRequest, NativeSimulationResult};
use crate::sidecar_supervisor::{RuntimeSupervisor, SupervisorError};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

const DEFAULT_TRANSCRIPT: &str = "Can you help me understand what is happening nearby?";

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
}

impl RuntimeRouter {
    pub fn new(supervisor: RuntimeSupervisor) -> Self {
        Self {
            supervisor,
            fixture: SimulationController::default(),
            native: Arc::new(Mutex::new(NativeState::default())),
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

        let game_id = request
            .game_profile_id
            .as_deref()
            .filter(|id| *id != "eclipse-harbor")
            .unwrap_or("skyrim-special-edition")
            .to_owned();
        let native_request = NativeSimulationRequest {
            session_id: "response-console-simulation".into(),
            turn_id: simulation_id.clone(),
            game_id,
            character_id: request.character_id.clone(),
            transcript: request
                .transcript
                .clone()
                .unwrap_or_else(|| DEFAULT_TRANSCRIPT.into()),
            locale: "en-US".into(),
        };
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
                    emit_native_result(&events, &task_id, generation, result, &native_state)
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

fn emit_native_result(
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
    let mut seen = BTreeSet::new();
    for event in &result.events {
        if let Some(stage) = event_stage(event) {
            if seen.insert(stage) {
                sequence += 1;
                set_native_stage(state, generation, stage);
                let _ = channel.send(SimulationEvent::StageStarted {
                    simulation_id: simulation_id.into(),
                    generation,
                    sequence,
                    stage,
                    estimated_duration_ms: 0,
                });
                sequence += 1;
                let _ = channel.send(SimulationEvent::StageCompleted {
                    simulation_id: simulation_id.into(),
                    generation,
                    sequence,
                    stage,
                    fixture_elapsed_ms: 0,
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
}
