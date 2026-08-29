use crate::domain::{
    CancelOutcome, CancelSimulationResult, MeasurementBasis, ResponseStage, RuntimeBackend,
    SimulationEvent, SimulationSnapshot, SimulationStatus, StartSimulationRequest,
    StartSimulationResult,
};
use std::fmt;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tauri::ipc::Channel;
use tokio::sync::watch;

const FIXTURE_STAGES: &[(ResponseStage, u64)] = &[
    (ResponseStage::Listening, 412),
    (ResponseStage::Transcribing, 286),
    (ResponseStage::Identifying, 41),
    (ResponseStage::Remembering, 53),
    (ResponseStage::Responding, 478),
    (ResponseStage::Voicing, 221),
    (ResponseStage::Animating, 77),
];
const FIXTURE_FIRST_AUDIO_MS: u64 = 1_310;
const FIXTURE_REPLY: &str =
    "The east pier is clear. If you are heading upriver, leave before the fog settles.";

#[derive(Debug)]
struct ActiveSimulation {
    id: String,
    generation: u64,
    status: SimulationStatus,
    active_stage: Option<ResponseStage>,
    cancel: watch::Sender<bool>,
}

#[derive(Debug, Default)]
struct CoordinatorState {
    generation: u64,
    active: Option<ActiveSimulation>,
}

#[derive(Clone)]
pub struct SimulationController {
    state: Arc<Mutex<CoordinatorState>>,
    bridge: Arc<dyn RuntimeBridge>,
}

impl fmt::Debug for SimulationController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SimulationController")
            .field("snapshot", &self.snapshot())
            .field("bridge", &self.bridge.backend())
            .finish()
    }
}

impl Default for SimulationController {
    fn default() -> Self {
        Self::new(Arc::new(DeterministicRuntimeBridge))
    }
}

impl SimulationController {
    fn new(bridge: Arc<dyn RuntimeBridge>) -> Self {
        Self {
            state: Arc::new(Mutex::new(CoordinatorState::default())),
            bridge,
        }
    }

    pub fn snapshot(&self) -> SimulationSnapshot {
        let Ok(state) = self.state.lock() else {
            return SimulationSnapshot {
                backend: self.bridge.backend(),
                ..SimulationSnapshot::default()
            };
        };
        match &state.active {
            Some(active) => SimulationSnapshot {
                status: active.status,
                simulation_id: Some(active.id.clone()),
                generation: active.generation,
                active_stage: active.active_stage,
                backend: self.bridge.backend(),
            },
            None => SimulationSnapshot {
                generation: state.generation,
                backend: self.bridge.backend(),
                ..SimulationSnapshot::default()
            },
        }
    }

    pub fn start(
        &self,
        request: StartSimulationRequest,
        events: Channel<SimulationEvent>,
    ) -> Result<StartSimulationResult, StartError> {
        request.validate().map_err(StartError::InvalidRequest)?;
        let (cancellation, cancellation_rx) = watch::channel(false);
        let (simulation_id, generation) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| StartError::StateUnavailable)?;
            if let Some(active) = &state.active {
                return Err(StartError::AlreadyActive(active.id.clone()));
            }
            state.generation = state.generation.saturating_add(1);
            let generation = state.generation;
            let simulation_id = format!("eclipse-harbor-{generation:06}");
            state.active = Some(ActiveSimulation {
                id: simulation_id.clone(),
                generation,
                status: SimulationStatus::Running,
                active_stage: None,
                cancel: cancellation,
            });
            (simulation_id, generation)
        };

        let weak_state = Arc::downgrade(&self.state);
        self.bridge.spawn_turn(TurnLaunch {
            simulation_id: simulation_id.clone(),
            generation,
            request,
            events,
            cancellation: cancellation_rx,
            observer: TurnObserver { state: weak_state },
        });

        Ok(StartSimulationResult {
            simulation_id,
            generation,
            backend: self.bridge.backend(),
            measurement_basis: MeasurementBasis::DeterministicFixture,
            runtime_fixture_only: true,
        })
    }

    pub fn cancel(&self) -> CancelSimulationResult {
        let Ok(mut state) = self.state.lock() else {
            return CancelSimulationResult {
                simulation_id: None,
                generation: 0,
                outcome: CancelOutcome::AlreadyIdle,
            };
        };
        let generation = state.generation;
        match state.active.as_mut() {
            Some(active) => {
                active.status = SimulationStatus::Cancelling;
                active.cancel.send_replace(true);
                CancelSimulationResult {
                    simulation_id: Some(active.id.clone()),
                    generation: active.generation,
                    outcome: CancelOutcome::CancellationRequested,
                }
            }
            None => CancelSimulationResult {
                simulation_id: None,
                generation,
                outcome: CancelOutcome::AlreadyIdle,
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("simulation request is invalid: {0}")]
    InvalidRequest(String),
    #[error("simulation `{0}` is already active")]
    AlreadyActive(String),
    #[error("simulation state is temporarily unavailable")]
    StateUnavailable,
}

struct TurnLaunch {
    simulation_id: String,
    generation: u64,
    request: StartSimulationRequest,
    events: Channel<SimulationEvent>,
    cancellation: watch::Receiver<bool>,
    observer: TurnObserver,
}

trait RuntimeBridge: Send + Sync {
    fn backend(&self) -> RuntimeBackend;
    fn spawn_turn(&self, launch: TurnLaunch);
}

#[derive(Debug)]
struct DeterministicRuntimeBridge;

impl RuntimeBridge for DeterministicRuntimeBridge {
    fn backend(&self) -> RuntimeBackend {
        RuntimeBackend::DeterministicFixture
    }

    fn spawn_turn(&self, launch: TurnLaunch) {
        tauri::async_runtime::spawn(run_fixture_turn(launch));
    }
}

#[derive(Clone)]
struct TurnObserver {
    state: Weak<Mutex<CoordinatorState>>,
}

impl TurnObserver {
    fn set_stage(&self, generation: u64, stage: ResponseStage) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let Ok(mut state) = state.lock() else { return };
        if let Some(active) = state.active.as_mut() {
            if active.generation == generation {
                active.active_stage = Some(stage);
            }
        }
    }

    fn finish(&self, generation: u64) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let Ok(mut state) = state.lock() else { return };
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.generation == generation)
        {
            state.active = None;
        }
    }
}

async fn run_fixture_turn(mut launch: TurnLaunch) {
    let mut sequence = 1_u64;
    if !send(
        &launch.events,
        SimulationEvent::Started {
            simulation_id: launch.simulation_id.clone(),
            generation: launch.generation,
            sequence,
            measurement_basis: MeasurementBasis::DeterministicFixture,
        },
    ) {
        launch.observer.finish(launch.generation);
        return;
    }

    let mut fixture_elapsed_ms = 0_u64;
    for (stage, duration_ms) in FIXTURE_STAGES {
        if *launch.cancellation.borrow() {
            emit_cancelled(&launch, sequence + 1);
            launch.observer.finish(launch.generation);
            return;
        }
        sequence += 1;
        launch.observer.set_stage(launch.generation, *stage);
        if !send(
            &launch.events,
            SimulationEvent::StageStarted {
                simulation_id: launch.simulation_id.clone(),
                generation: launch.generation,
                sequence,
                stage: *stage,
                estimated_duration_ms: *duration_ms,
            },
        ) {
            launch.observer.finish(launch.generation);
            return;
        }

        let sleep = tokio::time::sleep(Duration::from_millis(*duration_ms));
        tokio::pin!(sleep);
        tokio::select! {
            _ = &mut sleep => {}
            changed = launch.cancellation.changed() => {
                if changed.is_ok() && *launch.cancellation.borrow() {
                    emit_cancelled(&launch, sequence + 1);
                    launch.observer.finish(launch.generation);
                    return;
                }
            }
        }

        fixture_elapsed_ms = fixture_elapsed_ms.saturating_add(*duration_ms);
        sequence += 1;
        if !send(
            &launch.events,
            SimulationEvent::StageCompleted {
                simulation_id: launch.simulation_id.clone(),
                generation: launch.generation,
                sequence,
                stage: *stage,
                fixture_elapsed_ms,
            },
        ) {
            launch.observer.finish(launch.generation);
            return;
        }

        if *stage == ResponseStage::Responding {
            sequence += 1;
            if !send(
                &launch.events,
                SimulationEvent::SentenceReady {
                    simulation_id: launch.simulation_id.clone(),
                    generation: launch.generation,
                    sequence,
                    text: fixture_reply_for(&launch.request),
                },
            ) {
                launch.observer.finish(launch.generation);
                return;
            }
        }
    }

    sequence += 1;
    let _ = send(
        &launch.events,
        SimulationEvent::Completed {
            simulation_id: launch.simulation_id.clone(),
            generation: launch.generation,
            sequence,
            fixture_first_audio_ms: Some(FIXTURE_FIRST_AUDIO_MS),
            runtime_fixture_only: true,
            delivered_text: fixture_reply_for(&launch.request),
        },
    );
    launch.observer.finish(launch.generation);
}

fn emit_cancelled(launch: &TurnLaunch, sequence: u64) {
    let _ = send(
        &launch.events,
        SimulationEvent::Cancelled {
            simulation_id: launch.simulation_id.clone(),
            generation: launch.generation,
            sequence,
            reason: "Cancelled by the user; fixture dialogue was not committed.".into(),
        },
    );
}

fn send(channel: &Channel<SimulationEvent>, event: SimulationEvent) -> bool {
    channel.send(event).is_ok()
}

fn fixture_reply_for(request: &StartSimulationRequest) -> String {
    match request
        .character_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(name) => format!("{name}: {FIXTURE_REPLY}"),
        None => FIXTURE_REPLY.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_is_idle_before_start() {
        let controller = SimulationController::default();
        let snapshot = controller.snapshot();
        assert_eq!(snapshot.status, SimulationStatus::Idle);
        assert_eq!(snapshot.backend, RuntimeBackend::DeterministicFixture);
    }

    #[test]
    fn cancelling_idle_controller_is_idempotent() {
        let controller = SimulationController::default();
        assert_eq!(controller.cancel().outcome, CancelOutcome::AlreadyIdle);
        assert_eq!(controller.cancel().outcome, CancelOutcome::AlreadyIdle);
    }

    #[test]
    fn fixture_reply_is_deterministic_and_has_no_machine_claim() {
        let request = StartSimulationRequest::default();
        let first = fixture_reply_for(&request);
        let second = fixture_reply_for(&request);
        assert_eq!(first, second);
        assert!(!first.contains("FPS"));
        assert!(!first.contains("VRAM"));
    }

    #[test]
    fn fixture_stages_match_response_spine_order() {
        assert_eq!(FIXTURE_STAGES.len(), 7);
        assert_eq!(
            FIXTURE_STAGES.first().map(|stage| stage.0),
            Some(ResponseStage::Listening)
        );
        assert_eq!(
            FIXTURE_STAGES.last().map(|stage| stage.0),
            Some(ResponseStage::Animating)
        );
    }
}
