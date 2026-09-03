use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use futures_util::StreamExt;
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, info, warn};

use crate::{
    cancellation::{CancellationGeneration, GenerationToken},
    circuit_breaker::{CircuitBreakerConfig, CircuitRegistry},
    policy::{PrivacyDecision, RoutingPlan},
    provider::{
        EffectsProvider, LanguageModelProvider, ProviderError, RuntimeDependencies,
        RuntimeDependencyError, TtsProvider, TtsSession,
    },
    sentence::{SentenceSegmenterConfig, SentenceSpan},
    structured_response::{
        NpcResponseEnvelopeV1, ResponseValidationPolicy, StreamingResponseAdapterV1,
        StreamingResponseError, StreamingResponseFormatV1,
    },
    timing::{SpanOutcome, TimingCollector},
    types::{
        CharacterIdentity, Degradation, DeliveredSentence, DeliveryMode, EffectsRequest,
        GenerationRequest, LaneLifecycle, MemoryContext, NpcEffectsV1, PlaybackReceipt,
        ProviderDescriptor, ProviderModality, SpeechRequest, TurnEvent, TurnFailure, TurnIdentity,
        TurnLane, TurnLifecycle, TurnOutcome, TurnRequest,
    },
};

#[derive(Clone, Debug)]
pub struct SupervisorConfig {
    pub event_channel_capacity: usize,
    pub sentence_channel_capacity: usize,
    pub sentence_segmentation: SentenceSegmenterConfig,
    pub circuit_breaker: CircuitBreakerConfig,
    pub allowed_actions: Vec<String>,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            event_channel_capacity: 256,
            sentence_channel_capacity: 8,
            sentence_segmentation: SentenceSegmenterConfig::default(),
            circuit_breaker: CircuitBreakerConfig::default(),
            allowed_actions: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StartTurnError {
    #[error(transparent)]
    InvalidRequest(#[from] crate::types::ValidationError),
    #[error("runtime is shutting down")]
    ShuttingDown,
}

#[derive(Debug, thiserror::Error)]
enum SupervisorError {
    #[error("turn cancelled")]
    Cancelled,
    #[error("no privacy-authorized {0:?} provider is available")]
    NoAuthorizedProvider(ProviderModality),
    #[error("all {modality:?} providers failed before output: {last_error}")]
    ProvidersExhausted {
        modality: ProviderModality,
        last_error: String,
    },
    #[error("provider stream failed after output began: {0}")]
    StreamFailed(ProviderError),
    #[error("provider response contract failed: {0}")]
    InvalidProviderResponse(#[from] StreamingResponseError),
    #[error("runtime dependency failed: {0}")]
    Dependency(#[from] RuntimeDependencyError),
}

impl SupervisorError {
    fn failure(&self) -> TurnFailure {
        match self {
            Self::Cancelled => TurnFailure {
                code: "cancelled".into(),
                message: self.to_string(),
                retryable: true,
            },
            Self::NoAuthorizedProvider(_) => TurnFailure {
                code: "privacy_policy_blocked".into(),
                message: self.to_string(),
                retryable: false,
            },
            Self::ProvidersExhausted { .. } => TurnFailure {
                code: "providers_exhausted".into(),
                message: self.to_string(),
                retryable: true,
            },
            Self::InvalidProviderResponse(_) => TurnFailure {
                code: "provider_response_invalid".into(),
                message: self.to_string(),
                retryable: true,
            },
            Self::StreamFailed(error) => TurnFailure {
                code: "provider_stream_failed".into(),
                message: self.to_string(),
                retryable: error.retryable,
            },
            Self::Dependency(error) => TurnFailure {
                code: "runtime_dependency_failed".into(),
                message: self.to_string(),
                retryable: error.retryable(),
            },
        }
    }
}

#[derive(Clone)]
struct ActiveTurn {
    identity: TurnIdentity,
    generation: GenerationToken,
}

/// Handle to a single foreground turn. Dropping it does not cancel the turn.
pub struct TurnHandle {
    pub identity: TurnIdentity,
    generation: GenerationToken,
    events: mpsc::Receiver<TurnEvent>,
    completion: oneshot::Receiver<TurnOutcome>,
}

impl TurnHandle {
    pub async fn next_event(&mut self) -> Option<TurnEvent> {
        self.events.recv().await
    }

    pub async fn outcome(self) -> Result<TurnOutcome, oneshot::error::RecvError> {
        self.completion.await
    }

    /// Cooperative cancellation for this turn. The generation guard also rejects
    /// output returned late by adapters that do not stop immediately.
    pub fn cancel(&self) {
        self.generation.cancel();
    }
}

/// Owns exactly one foreground conversation turn and provides deterministic barge-in.
pub struct TurnSupervisor {
    config: SupervisorConfig,
    dependencies: RuntimeDependencies,
    generations: CancellationGeneration,
    circuits: CircuitRegistry,
    start_gate: Mutex<()>,
    current: Mutex<Option<ActiveTurn>>,
    shutting_down: std::sync::atomic::AtomicBool,
}

impl TurnSupervisor {
    pub fn new(config: SupervisorConfig, dependencies: RuntimeDependencies) -> Arc<Self> {
        assert!(config.event_channel_capacity > 0);
        assert!(config.sentence_channel_capacity > 0);
        Arc::new(Self {
            circuits: CircuitRegistry::new(config.circuit_breaker.clone()),
            config,
            dependencies,
            generations: CancellationGeneration::default(),
            start_gate: Mutex::new(()),
            current: Mutex::new(None),
            shutting_down: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub async fn start_turn(
        self: &Arc<Self>,
        request: TurnRequest,
    ) -> Result<TurnHandle, StartTurnError> {
        request.validate()?;
        if self
            .shutting_down
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StartTurnError::ShuttingDown);
        }

        let _gate = self.start_gate.lock().await;
        self.barge_in_locked("superseded by a newer foreground turn")
            .await;

        let generation = self.generations.next();
        let identity = TurnIdentity {
            session_id: request.session_id.clone(),
            turn_id: request.turn_id.clone(),
            cancellation_generation: generation.generation(),
        };
        let (event_tx, event_rx) = mpsc::channel(self.config.event_channel_capacity);
        let (completion_tx, completion_rx) = oneshot::channel();
        *self.current.lock().await = Some(ActiveTurn {
            identity: identity.clone(),
            generation: generation.clone(),
        });

        let supervisor = Arc::clone(self);
        let task_identity = identity.clone();
        let task_generation = generation.clone();
        tokio::spawn(async move {
            let emitter = EventEmitter {
                identity: task_identity.clone(),
                generation: task_generation.clone(),
                tx: event_tx,
            };
            let outcome = supervisor
                .run_turn(
                    request,
                    task_identity.clone(),
                    task_generation,
                    emitter.clone(),
                )
                .await;
            emitter.emit_terminal(outcome.clone()).await;
            let _ = completion_tx.send(outcome);
            supervisor.clear_if_current(&task_identity).await;
        });

        Ok(TurnHandle {
            identity,
            generation,
            events: event_rx,
            completion: completion_rx,
        })
    }

    /// Cancels the foreground turn and asks the media sink to stop immediately.
    pub async fn barge_in(&self, reason: &str) -> Option<TurnIdentity> {
        let _gate = self.start_gate.lock().await;
        self.barge_in_locked(reason).await
    }

    pub async fn shutdown(&self) {
        self.shutting_down
            .store(true, std::sync::atomic::Ordering::Release);
        let _gate = self.start_gate.lock().await;
        self.barge_in_locked("runtime shutdown").await;
    }

    pub async fn active_turn(&self) -> Option<TurnIdentity> {
        self.current
            .lock()
            .await
            .as_ref()
            .map(|active| active.identity.clone())
    }

    async fn barge_in_locked(&self, reason: &str) -> Option<TurnIdentity> {
        let active = self.current.lock().await.take();
        if let Some(active) = active {
            info!(turn = %active.identity, reason, "cancelling foreground turn");
            active.generation.cancel();
            if let Err(error) = self.dependencies.audio.stop(&active.identity).await {
                warn!(turn = %active.identity, %error, "audio sink stop failed during barge-in");
            }
            Some(active.identity)
        } else {
            None
        }
    }

    async fn clear_if_current(&self, identity: &TurnIdentity) {
        let mut current = self.current.lock().await;
        if current.as_ref().is_some_and(|active| {
            active.identity.cancellation_generation == identity.cancellation_generation
        }) {
            *current = None;
        }
    }

    async fn run_turn(
        &self,
        request: TurnRequest,
        identity: TurnIdentity,
        generation: GenerationToken,
        emitter: EventEmitter,
    ) -> TurnOutcome {
        let timing = TimingCollector::new();
        let mut outcome = TurnOutcome {
            identity: identity.clone(),
            lifecycle: TurnLifecycle::Accepted,
            full_response: String::new(),
            structured_response: None,
            effects: NpcEffectsV1::neutral(),
            delivered: Vec::new(),
            degradations: Vec::new(),
            selected_llm_provider: None,
            selected_tts_providers: Vec::new(),
            error: None,
        };

        let result = self
            .run_turn_inner(
                &request,
                &identity,
                &generation,
                &emitter,
                &timing,
                &mut outcome,
            )
            .await;

        match result {
            Ok(()) if generation.is_cancelled() => {
                outcome.lifecycle = TurnLifecycle::Cancelled;
                outcome.error = Some(SupervisorError::Cancelled.failure());
            }
            Ok(()) => outcome.lifecycle = TurnLifecycle::Completed,
            Err(SupervisorError::Cancelled) => {
                outcome.lifecycle = TurnLifecycle::Cancelled;
                outcome.error = Some(SupervisorError::Cancelled.failure());
            }
            Err(error) => {
                warn!(turn = %identity, %error, "turn failed");
                outcome.lifecycle = TurnLifecycle::Failed;
                outcome.error = Some(error.failure());
            }
        }
        outcome
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_turn_inner(
        &self,
        request: &TurnRequest,
        identity: &TurnIdentity,
        generation: &GenerationToken,
        emitter: &EventEmitter,
        timing: &TimingCollector,
        outcome: &mut TurnOutcome,
    ) -> Result<(), SupervisorError> {
        emitter.lifecycle(TurnLifecycle::Accepted).await?;
        emitter.lifecycle(TurnLifecycle::Identifying).await?;

        let identity_span = timing.start("identity.resolve");
        let memory_span = timing.start("memory.retrieve");
        let cancellation = generation.cancellation_token();
        let (character_result, memory_result) = tokio::join!(
            self.dependencies
                .identity
                .resolve(request, cancellation.child_token()),
            self.dependencies
                .memory
                .retrieve(request, cancellation.child_token()),
        );

        let character = match character_result {
            Ok(character) => {
                emit_timing(
                    emitter,
                    identity_span.finish(SpanOutcome::Ok, BTreeMap::new()),
                )
                .await?;
                character
            }
            Err(RuntimeDependencyError::Cancelled) => return Err(SupervisorError::Cancelled),
            Err(error) => {
                emit_timing(
                    emitter,
                    identity_span
                        .finish(SpanOutcome::Degraded, error_attributes(&error.to_string())),
                )
                .await?;
                let degradation = Degradation::IdentityToExplicitSelection;
                outcome.degradations.push(degradation.clone());
                emitter
                    .degraded(degradation, format!("identity resolver failed: {error}"))
                    .await?;
                CharacterIdentity {
                    character_id: request.character_hint.clone(),
                    display_name: request
                        .character_hint
                        .clone()
                        .unwrap_or_else(|| "Selected NPC".into()),
                    confidence: 1.0,
                    evidence: vec!["explicit_or_generic_selection".into()],
                    explicit_selection: true,
                }
            }
        };
        emitter
            .emit(TurnEvent::CharacterResolved {
                identity: identity.clone(),
                character: character.clone(),
            })
            .await?;

        emitter.lifecycle(TurnLifecycle::Remembering).await?;
        let memory = match memory_result {
            Ok(memory) => {
                emit_timing(
                    emitter,
                    memory_span.finish(SpanOutcome::Ok, BTreeMap::new()),
                )
                .await?;
                memory
            }
            Err(RuntimeDependencyError::Cancelled) => return Err(SupervisorError::Cancelled),
            Err(error) => {
                emit_timing(
                    emitter,
                    memory_span.finish(SpanOutcome::Degraded, error_attributes(&error.to_string())),
                )
                .await?;
                let degradation = Degradation::MemoryWithoutVectorRetrieval;
                outcome.degradations.push(degradation.clone());
                emitter
                    .degraded(degradation, format!("memory retrieval failed: {error}"))
                    .await?;
                MemoryContext {
                    retrieval_degraded: true,
                    ..Default::default()
                }
            }
        };
        emitter
            .emit(TurnEvent::MemoryReady {
                identity: identity.clone(),
                context: memory.clone(),
            })
            .await?;

        let generation_request = GenerationRequest {
            identity: identity.clone(),
            transcript: request.transcript.clone(),
            character,
            memory,
            locale: request.locale.clone(),
            metadata: request.metadata.clone(),
        };
        let routing = RoutingPlan::from_request(request);
        self.emit_policy_rejections(identity, &routing, emitter)
            .await?;

        emitter.lifecycle(TurnLifecycle::Responding).await?;
        emitter
            .lane(TurnLane::SpokenResponse, LaneLifecycle::Running)
            .await?;
        emitter
            .lane(TurnLane::StructuredEffects, LaneLifecycle::Running)
            .await?;

        let (sentence_tx, sentence_rx) = mpsc::channel(self.config.sentence_channel_capacity);
        let speech_task = tokio::spawn(run_speech_lane(
            sentence_rx,
            identity.clone(),
            request.locale.clone(),
            routing.clone(),
            self.dependencies.providers.speech.clone(),
            Arc::clone(&self.dependencies.audio),
            generation.clone(),
            emitter.clone(),
            self.circuits.clone(),
        ));
        let effects_task = tokio::spawn(run_effects_lane(
            EffectsRequest {
                generation: generation_request.clone(),
                response_text_hint: None,
            },
            routing.clone(),
            self.dependencies.providers.effects.clone(),
            generation.clone(),
            emitter.clone(),
            self.circuits.clone(),
            self.config.allowed_actions.clone(),
        ));

        let llm_result = run_llm_lane(
            generation_request,
            routing,
            self.dependencies.providers.language_models.clone(),
            generation.clone(),
            emitter.clone(),
            self.circuits.clone(),
            self.config.sentence_segmentation.clone(),
            sentence_tx,
            timing,
        )
        .await;

        let speech_result = speech_task.await.map_err(|error| {
            SupervisorError::Dependency(RuntimeDependencyError::Internal(error.to_string()))
        })?;
        let effects_result = effects_task.await.map_err(|error| {
            SupervisorError::Dependency(RuntimeDependencyError::Internal(error.to_string()))
        })?;

        match effects_result {
            Ok(result) => {
                outcome.effects = result.effects;
                outcome.degradations.extend(result.degradations);
                emitter
                    .lane(TurnLane::StructuredEffects, result.lifecycle)
                    .await?;
            }
            Err(SupervisorError::Cancelled) => return Err(SupervisorError::Cancelled),
            Err(error) => {
                let degradation = Degradation::NeutralEffects;
                outcome.degradations.push(degradation.clone());
                emitter
                    .degraded(degradation, format!("effects lane failed: {error}"))
                    .await?;
                emitter
                    .lane(TurnLane::StructuredEffects, LaneLifecycle::Neutralized)
                    .await?;
            }
        }

        match speech_result {
            Ok(result) => {
                outcome.delivered = result.delivered;
                outcome.selected_tts_providers = result.providers;
                outcome.degradations.extend(result.degradations);
            }
            Err(SupervisorError::Cancelled) => return Err(SupervisorError::Cancelled),
            Err(error) => return Err(error),
        }

        let llm = llm_result?;
        outcome.full_response = llm.full_text;
        outcome.structured_response = llm.structured_response;
        outcome.selected_llm_provider = Some(llm.provider_id);
        outcome.degradations.extend(llm.degradations);
        emitter
            .lane(
                TurnLane::SpokenResponse,
                if llm.terminal_error.is_some() {
                    LaneLifecycle::Failed
                } else {
                    LaneLifecycle::Completed
                },
            )
            .await?;

        emitter.lifecycle(TurnLifecycle::Committing).await?;
        let commit_span = timing.start("memory.commit_delivered");
        match self
            .dependencies
            .memory
            .commit_delivered(
                identity,
                &request.transcript,
                &outcome.delivered,
                generation.cancellation_token(),
            )
            .await
        {
            Ok(()) => {
                emit_timing(
                    emitter,
                    commit_span.finish(SpanOutcome::Ok, BTreeMap::new()),
                )
                .await?;
            }
            Err(RuntimeDependencyError::Cancelled) => return Err(SupervisorError::Cancelled),
            Err(error) => {
                emit_timing(
                    emitter,
                    commit_span.finish(SpanOutcome::Degraded, error_attributes(&error.to_string())),
                )
                .await?;
                let degradation = Degradation::MemoryCommitDeferred;
                outcome.degradations.push(degradation.clone());
                emitter
                    .degraded(degradation, format!("memory commit deferred: {error}"))
                    .await?;
            }
        }
        if let Some(error) = llm.terminal_error {
            return Err(error);
        }
        emitter.lifecycle(TurnLifecycle::Completed).await?;
        Ok(())
    }

    async fn emit_policy_rejections(
        &self,
        identity: &TurnIdentity,
        routing: &RoutingPlan,
        emitter: &EventEmitter,
    ) -> Result<(), SupervisorError> {
        for descriptor in self
            .dependencies
            .providers
            .language_models
            .iter()
            .map(|provider| provider.descriptor())
            .chain(
                self.dependencies
                    .providers
                    .effects
                    .iter()
                    .map(|provider| provider.descriptor()),
            )
            .chain(
                self.dependencies
                    .providers
                    .speech
                    .iter()
                    .map(|provider| provider.descriptor()),
            )
        {
            let decision = routing.privacy.evaluate(descriptor);
            if decision != PrivacyDecision::Allowed {
                emitter
                    .emit(TurnEvent::ProviderRejected {
                        identity: identity.clone(),
                        provider_id: descriptor.id.clone(),
                        reason: format!("{decision:?}"),
                    })
                    .await?;
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct EventEmitter {
    identity: TurnIdentity,
    generation: GenerationToken,
    tx: mpsc::Sender<TurnEvent>,
}

impl EventEmitter {
    async fn emit(&self, event: TurnEvent) -> Result<(), SupervisorError> {
        if self.generation.is_cancelled() {
            return Err(SupervisorError::Cancelled);
        }
        debug_assert_eq!(event.identity(), &self.identity);
        match self.tx.try_send(event) {
            Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                // Observability must never create turn latency. Completion remains
                // lossless through the separate oneshot result channel.
                warn!(turn = %self.identity, "event channel full; dropping diagnostic event");
                Ok(())
            }
        }
    }

    async fn lifecycle(&self, stage: TurnLifecycle) -> Result<(), SupervisorError> {
        self.emit(TurnEvent::Lifecycle {
            identity: self.identity.clone(),
            stage,
        })
        .await
    }

    async fn lane(&self, lane: TurnLane, stage: LaneLifecycle) -> Result<(), SupervisorError> {
        self.emit(TurnEvent::LaneLifecycle {
            identity: self.identity.clone(),
            lane,
            stage,
        })
        .await
    }

    async fn degraded(
        &self,
        degradation: Degradation,
        reason: String,
    ) -> Result<(), SupervisorError> {
        self.emit(TurnEvent::Degraded {
            identity: self.identity.clone(),
            degradation,
            reason,
        })
        .await
    }

    async fn emit_terminal(&self, outcome: TurnOutcome) {
        // Terminal state must be observable even after cooperative cancellation.
        let _ = self.tx.try_send(TurnEvent::Terminal {
            outcome: Box::new(outcome),
        });
    }
}

struct LlmLaneResult {
    full_text: String,
    structured_response: Option<NpcResponseEnvelopeV1>,
    provider_id: String,
    degradations: Vec<Degradation>,
    terminal_error: Option<SupervisorError>,
}

#[allow(clippy::too_many_arguments)]
async fn run_llm_lane(
    request: GenerationRequest,
    routing: RoutingPlan,
    providers: Vec<Arc<dyn LanguageModelProvider>>,
    generation: GenerationToken,
    emitter: EventEmitter,
    circuits: CircuitRegistry,
    sentence_config: SentenceSegmenterConfig,
    sentence_tx: mpsc::Sender<SentenceSpan>,
    timing: &TimingCollector,
) -> Result<LlmLaneResult, SupervisorError> {
    // Missing metadata is the compatibility behavior for existing fixture/local
    // routes. Hosted route snapshots should set the key explicitly; importantly,
    // a selected structured route can never fall back to legacy parsing.
    let response_format = StreamingResponseFormatV1::from_route_metadata(&request.metadata)?
        .unwrap_or(StreamingResponseFormatV1::LegacyPlainText);
    let candidates = allowed_llm_candidates(&providers, &routing);
    if candidates.is_empty() {
        return Err(SupervisorError::NoAuthorizedProvider(
            ProviderModality::LanguageModel,
        ));
    }
    let primary = candidates[0].descriptor().clone();
    let mut last_error = "no provider attempted".to_owned();

    for provider in candidates {
        if !routing
            .fallback
            .allows_transition(&primary, provider.descriptor())
        {
            continue;
        }
        if generation.is_cancelled() {
            return Err(SupervisorError::Cancelled);
        }

        let circuit = circuits.for_provider(&circuit_key(provider.descriptor()));
        let permit = match circuit.try_acquire() {
            Ok(permit) => permit,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };
        let mut first_delta_span = Some(timing.start("llm.first_delta"));
        let stream = provider
            .stream_response(request.clone(), generation.cancellation_token())
            .await;
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                last_error = error.to_string();
                permit.failure();
                emit_timing(
                    &emitter,
                    first_delta_span
                        .take()
                        .expect("first-delta span is unfinished before streaming")
                        .finish(SpanOutcome::Error, error_attributes(&last_error)),
                )
                .await?;
                continue;
            }
        };

        let mut provider_output = String::new();
        let mut response_adapter = StreamingResponseAdapterV1::new(
            response_format,
            ResponseValidationPolicy::default(),
            sentence_config.clone(),
            generation.generation(),
        );
        let mut first_delta = true;

        loop {
            let next = tokio::select! {
                _ = generation.cancelled() => return Err(SupervisorError::Cancelled),
                next = stream.next() => next,
            };
            match next {
                Some(Ok(delta)) => {
                    if delta.text.is_empty() {
                        continue;
                    }
                    if first_delta {
                        let mut attributes = BTreeMap::new();
                        attributes.insert("provider_id".into(), provider.descriptor().id.clone());
                        emit_timing(
                            &emitter,
                            first_delta_span
                                .take()
                                .expect("first-delta span is finished once")
                                .finish(SpanOutcome::Ok, attributes),
                        )
                        .await?;
                        first_delta = false;
                    }
                    let update = match response_adapter.push_delta(
                        generation.generation(),
                        delta.sequence,
                        &delta.text,
                    ) {
                        Ok(update) => update,
                        Err(error) => {
                            permit.failure();
                            drop(sentence_tx);
                            return Ok(LlmLaneResult {
                                full_text: if response_format
                                    == StreamingResponseFormatV1::LegacyPlainText
                                {
                                    provider_output
                                } else {
                                    String::new()
                                },
                                structured_response: None,
                                provider_id: provider.descriptor().id.clone(),
                                degradations: Vec::new(),
                                terminal_error: Some(SupervisorError::InvalidProviderResponse(
                                    error,
                                )),
                            });
                        }
                    };
                    provider_output.push_str(&delta.text);
                    emitter
                        .emit(TurnEvent::TextDelta {
                            identity: request.identity.clone(),
                            delta: delta.clone(),
                        })
                        .await?;
                    for sentence in update.ready_sentences {
                        emitter
                            .emit(TurnEvent::SentenceReady {
                                identity: request.identity.clone(),
                                sentence_id: sentence.sentence_id,
                                text_start_bytes: sentence.text_start_bytes,
                                text_end_bytes: sentence.text_end_bytes,
                                text: sentence.text.clone(),
                            })
                            .await?;
                        sentence_tx
                            .send(sentence)
                            .await
                            .map_err(|_| SupervisorError::Cancelled)?;
                    }
                }
                Some(Err(error)) => {
                    permit.failure();
                    if provider_output.is_empty() {
                        last_error = error.to_string();
                        break;
                    }
                    drop(sentence_tx);
                    return Ok(LlmLaneResult {
                        full_text: if response_format == StreamingResponseFormatV1::LegacyPlainText
                        {
                            provider_output
                        } else {
                            String::new()
                        },
                        structured_response: None,
                        provider_id: provider.descriptor().id.clone(),
                        degradations: Vec::new(),
                        terminal_error: Some(SupervisorError::StreamFailed(error)),
                    });
                }
                None => {
                    if first_delta {
                        emit_timing(
                            &emitter,
                            first_delta_span
                                .take()
                                .expect("empty response retains first-delta span")
                                .finish(
                                    SpanOutcome::Error,
                                    error_attributes("empty model response"),
                                ),
                        )
                        .await?;
                        permit.failure();
                        last_error = "empty model response".into();
                        break;
                    }
                    let finalized = match response_adapter.finish(generation.generation()) {
                        Ok(finalized) => finalized,
                        Err(error) => {
                            permit.failure();
                            drop(sentence_tx);
                            return Ok(LlmLaneResult {
                                full_text: if response_format
                                    == StreamingResponseFormatV1::LegacyPlainText
                                {
                                    provider_output
                                } else {
                                    String::new()
                                },
                                structured_response: None,
                                provider_id: provider.descriptor().id.clone(),
                                degradations: Vec::new(),
                                terminal_error: Some(SupervisorError::InvalidProviderResponse(
                                    error,
                                )),
                            });
                        }
                    };
                    for sentence in finalized.ready_sentences {
                        emitter
                            .emit(TurnEvent::SentenceReady {
                                identity: request.identity.clone(),
                                sentence_id: sentence.sentence_id,
                                text_start_bytes: sentence.text_start_bytes,
                                text_end_bytes: sentence.text_end_bytes,
                                text: sentence.text.clone(),
                            })
                            .await?;
                        sentence_tx
                            .send(sentence)
                            .await
                            .map_err(|_| SupervisorError::Cancelled)?;
                    }
                    drop(sentence_tx);
                    permit.success();
                    let degradations = if provider.descriptor().id != primary.id {
                        let degradation = Degradation::ProviderFallback {
                            from: primary.id.clone(),
                            to: provider.descriptor().id.clone(),
                        };
                        emitter
                            .degraded(
                                degradation.clone(),
                                "primary LLM unavailable before output".into(),
                            )
                            .await?;
                        vec![degradation]
                    } else {
                        Vec::new()
                    };
                    return Ok(LlmLaneResult {
                        full_text: finalized.response.spoken_response.text.clone(),
                        structured_response: (response_format
                            == StreamingResponseFormatV1::StructuredV1)
                            .then_some(finalized.response),
                        provider_id: provider.descriptor().id.clone(),
                        degradations,
                        terminal_error: None,
                    });
                }
            }
        }
    }

    drop(sentence_tx);
    Err(SupervisorError::ProvidersExhausted {
        modality: ProviderModality::LanguageModel,
        last_error,
    })
}

fn allowed_llm_candidates(
    providers: &[Arc<dyn LanguageModelProvider>],
    routing: &RoutingPlan,
) -> Vec<Arc<dyn LanguageModelProvider>> {
    providers
        .iter()
        .filter(|provider| {
            routing.privacy.evaluate(provider.descriptor()) == PrivacyDecision::Allowed
        })
        .cloned()
        .collect()
}

struct EffectsLaneResult {
    effects: NpcEffectsV1,
    degradations: Vec<Degradation>,
    lifecycle: LaneLifecycle,
}

#[allow(clippy::too_many_arguments)]
async fn run_effects_lane(
    request: EffectsRequest,
    routing: RoutingPlan,
    providers: Vec<Arc<dyn EffectsProvider>>,
    generation: GenerationToken,
    emitter: EventEmitter,
    circuits: CircuitRegistry,
    allowed_actions: Vec<String>,
) -> Result<EffectsLaneResult, SupervisorError> {
    let candidates: Vec<_> = providers
        .into_iter()
        .filter(|provider| {
            routing.privacy.evaluate(provider.descriptor()) == PrivacyDecision::Allowed
        })
        .collect();
    let Some(primary) = candidates.first().map(|value| value.descriptor().clone()) else {
        let degradation = Degradation::NeutralEffects;
        emitter
            .degraded(
                degradation.clone(),
                "no authorized effects provider; neutral effects used".into(),
            )
            .await?;
        return Ok(EffectsLaneResult {
            effects: NpcEffectsV1::neutral(),
            degradations: vec![degradation],
            lifecycle: LaneLifecycle::Neutralized,
        });
    };

    for provider in candidates {
        if !routing
            .fallback
            .allows_transition(&primary, provider.descriptor())
        {
            continue;
        }
        let permit = match circuits
            .for_provider(&circuit_key(provider.descriptor()))
            .try_acquire()
        {
            Ok(permit) => permit,
            Err(_) => continue,
        };
        let result = tokio::select! {
            _ = generation.cancelled() => return Err(SupervisorError::Cancelled),
            result = provider.derive_effects(request.clone(), generation.cancellation_token()) => result,
        };
        match result {
            Ok(effects) => match effects.validate(&allowed_actions) {
                Ok(effects) => {
                    permit.success();
                    emitter
                        .emit(TurnEvent::EffectsReady {
                            identity: request.generation.identity.clone(),
                            effects: effects.clone(),
                        })
                        .await?;
                    let mut degradations = Vec::new();
                    if provider.descriptor().id != primary.id {
                        let degradation = Degradation::ProviderFallback {
                            from: primary.id.clone(),
                            to: provider.descriptor().id.clone(),
                        };
                        emitter
                            .degraded(
                                degradation.clone(),
                                "primary effects provider unavailable".into(),
                            )
                            .await?;
                        degradations.push(degradation);
                    }
                    return Ok(EffectsLaneResult {
                        effects,
                        degradations,
                        lifecycle: LaneLifecycle::Completed,
                    });
                }
                Err(error) => {
                    permit.failure();
                    warn!(provider = %provider.descriptor().id, %error, "invalid effects neutralized");
                }
            },
            Err(error) => {
                if error.kind == crate::provider::ProviderErrorKind::Cancelled {
                    return Err(SupervisorError::Cancelled);
                }
                permit.failure();
            }
        }
    }

    let degradation = Degradation::NeutralEffects;
    emitter
        .degraded(
            degradation.clone(),
            "effects providers failed; neutral effects used".into(),
        )
        .await?;
    Ok(EffectsLaneResult {
        effects: NpcEffectsV1::neutral(),
        degradations: vec![degradation],
        lifecycle: LaneLifecycle::Neutralized,
    })
}

struct SpeechLaneResult {
    delivered: Vec<DeliveredSentence>,
    providers: Vec<String>,
    degradations: Vec<Degradation>,
}

#[allow(clippy::too_many_arguments)]
async fn run_speech_lane(
    mut sentences: mpsc::Receiver<SentenceSpan>,
    identity: TurnIdentity,
    locale: String,
    routing: RoutingPlan,
    providers: Vec<Arc<dyn TtsProvider>>,
    audio: Arc<dyn crate::provider::AudioSink>,
    generation: GenerationToken,
    emitter: EventEmitter,
    circuits: CircuitRegistry,
) -> Result<SpeechLaneResult, SupervisorError> {
    let candidates: Vec<_> = providers
        .into_iter()
        .filter(|provider| {
            routing.privacy.evaluate(provider.descriptor()) == PrivacyDecision::Allowed
        })
        .collect();
    let primary = candidates.first().map(|value| value.descriptor().clone());
    let mut sessions: HashMap<String, Box<dyn TtsSession>> = HashMap::new();
    let mut delivered = Vec::new();
    let mut selected_providers = Vec::new();
    let mut degradations = Vec::new();
    let mut announced_voicing = false;

    while let Some(sentence_span) = tokio::select! {
        _ = generation.cancelled() => return Err(SupervisorError::Cancelled),
        value = sentences.recv() => value,
    } {
        let SentenceSpan {
            sentence_id,
            text_start_bytes,
            text_end_bytes,
            text,
        } = sentence_span;
        if !announced_voicing {
            emitter.lifecycle(TurnLifecycle::Voicing).await?;
            announced_voicing = true;
        }
        let mut delivered_audio = None;
        for provider in &candidates {
            let Some(primary) = &primary else { break };
            if !routing
                .fallback
                .allows_transition(primary, provider.descriptor())
            {
                continue;
            }

            let circuit = circuits.for_provider(&circuit_key(provider.descriptor()));
            let permit = match circuit.try_acquire() {
                Ok(permit) => permit,
                Err(_) => continue,
            };
            if !sessions.contains_key(&provider.descriptor().id) {
                match provider
                    .start_session(&identity, &locale, generation.cancellation_token())
                    .await
                {
                    Ok(session) => {
                        sessions.insert(provider.descriptor().id.clone(), session);
                    }
                    Err(_) => {
                        permit.failure();
                        continue;
                    }
                }
            }
            let session = sessions
                .get_mut(&provider.descriptor().id)
                .expect("session inserted above");
            let speech = session
                .synthesize(
                    SpeechRequest {
                        identity: identity.clone(),
                        sentence_id,
                        text: text.clone(),
                        locale: locale.clone(),
                        voice_hint: None,
                    },
                    generation.cancellation_token(),
                )
                .await;
            let speech = match speech {
                Ok(speech) => speech,
                Err(_) => {
                    permit.failure();
                    continue;
                }
            };

            emitter
                .emit(TurnEvent::SpeechStarted {
                    identity: identity.clone(),
                    sentence_id,
                    provider_id: provider.descriptor().id.clone(),
                })
                .await?;
            emitter.lifecycle(TurnLifecycle::Animating).await?;
            // Playback is the cooperative cancellation boundary. The sink must be
            // allowed to stop its device and report the frames that were actually
            // heard; racing this future against the same token would drop it before
            // it could return that partial receipt.
            let receipt = audio
                .play(
                    &identity,
                    sentence_id,
                    speech,
                    generation.cancellation_token(),
                )
                .await;
            match receipt {
                Ok(
                    receipt @ PlaybackReceipt {
                        completed: true, ..
                    },
                ) => {
                    permit.success();
                    if !selected_providers.contains(&provider.descriptor().id) {
                        selected_providers.push(provider.descriptor().id.clone());
                    }
                    if provider.descriptor().id != primary.id {
                        let degradation = Degradation::ProviderFallback {
                            from: primary.id.clone(),
                            to: provider.descriptor().id.clone(),
                        };
                        emitter
                            .degraded(
                                degradation.clone(),
                                "primary speech provider unavailable".into(),
                            )
                            .await?;
                        degradations.push(degradation);
                    }
                    delivered_audio = Some(receipt);
                    break;
                }
                Ok(PlaybackReceipt {
                    completed: false, ..
                }) => {
                    permit.failure();
                    return Err(SupervisorError::Cancelled);
                }
                Err(error) => {
                    // The sink may already have played part of the stream. Never try a
                    // second voice and risk duplicated dialogue; degrade to subtitles.
                    permit.failure();
                    warn!(turn = %identity, sentence_id, %error, "audio playback failed");
                    break;
                }
            }
        }

        let sentence = if let Some(receipt) = delivered_audio {
            DeliveredSentence {
                sentence_id,
                text_start_bytes,
                text_end_bytes,
                text,
                delivery: DeliveryMode::Audio,
                audible_frames: receipt.audible_frames,
                duration: receipt.duration,
            }
        } else {
            let degradation = Degradation::AudioToSubtitles;
            if !degradations.contains(&degradation) {
                emitter
                    .degraded(
                        degradation.clone(),
                        "speech unavailable; delivered through subtitles".into(),
                    )
                    .await?;
                degradations.push(degradation);
            }
            DeliveredSentence {
                sentence_id,
                text_start_bytes,
                text_end_bytes,
                text,
                delivery: DeliveryMode::Subtitle,
                audible_frames: 0,
                duration: std::time::Duration::ZERO,
            }
        };
        emitter
            .emit(TurnEvent::SpeechDelivered {
                identity: identity.clone(),
                sentence: sentence.clone(),
            })
            .await?;
        delivered.push(sentence);
    }

    for (_, mut session) in sessions {
        if let Err(error) = session.close().await {
            debug!(%error, "speech session close failed");
        }
    }
    Ok(SpeechLaneResult {
        delivered,
        providers: selected_providers,
        degradations,
    })
}

fn circuit_key(descriptor: &ProviderDescriptor) -> String {
    format!("{:?}:{}", descriptor.modality, descriptor.id)
}

async fn emit_timing(
    emitter: &EventEmitter,
    span: crate::timing::TimingSpan,
) -> Result<(), SupervisorError> {
    emitter
        .emit(TurnEvent::Timing {
            identity: emitter.identity.clone(),
            span,
        })
        .await
}

fn error_attributes(message: &str) -> BTreeMap<String, String> {
    BTreeMap::from([("error".into(), message.to_owned())])
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, time::Duration};

    use async_trait::async_trait;
    use futures_util::{stream, StreamExt};
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::{
        provider::{
            AudioSink, EffectsProvider, IdentityResolver, LlmStream, MemoryStore, ProviderPool,
            RuntimeDependencies, SpeechStream, TtsProvider, TtsSession,
        },
        types::{
            ActionProposal, AudioChunk, DataClass, ExecutionMode, LlmDelta, NetworkPolicy,
            ProviderLocation, SpeechStreamItem,
        },
    };

    fn descriptor(
        id: &str,
        modality: ProviderModality,
        location: ProviderLocation,
    ) -> ProviderDescriptor {
        ProviderDescriptor {
            id: id.into(),
            display_name: id.into(),
            modality,
            location,
            may_retain_data: false,
            transmitted_data: vec![DataClass::Transcript],
            capabilities: BTreeMap::new(),
        }
    }

    struct FakeIdentity;

    #[async_trait]
    impl IdentityResolver for FakeIdentity {
        async fn resolve(
            &self,
            request: &TurnRequest,
            _cancellation: CancellationToken,
        ) -> Result<CharacterIdentity, RuntimeDependencyError> {
            Ok(CharacterIdentity {
                character_id: Some("npc-1".into()),
                display_name: request
                    .character_hint
                    .clone()
                    .unwrap_or_else(|| "Nova".into()),
                confidence: 0.98,
                evidence: vec!["explicit_selection".into()],
                explicit_selection: true,
            })
        }
    }

    #[derive(Default)]
    struct FakeMemory {
        commits: Mutex<Vec<Vec<DeliveredSentence>>>,
    }

    #[async_trait]
    impl MemoryStore for FakeMemory {
        async fn retrieve(
            &self,
            _request: &TurnRequest,
            _cancellation: CancellationToken,
        ) -> Result<MemoryContext, RuntimeDependencyError> {
            Ok(MemoryContext {
                canon_facts: vec!["The harbor is under curfew.".into()],
                ..Default::default()
            })
        }

        async fn commit_delivered(
            &self,
            _identity: &TurnIdentity,
            _transcript: &str,
            delivered: &[DeliveredSentence],
            _cancellation: CancellationToken,
        ) -> Result<(), RuntimeDependencyError> {
            self.commits.lock().await.push(delivered.to_vec());
            Ok(())
        }
    }

    #[derive(Clone)]
    enum LlmBehavior {
        Text(String),
        OpenError,
        Delayed(String),
        ErrorAfter(String),
    }

    struct FakeLlm {
        descriptor: ProviderDescriptor,
        behavior: LlmBehavior,
    }

    #[async_trait]
    impl LanguageModelProvider for FakeLlm {
        fn descriptor(&self) -> &ProviderDescriptor {
            &self.descriptor
        }

        async fn stream_response(
            &self,
            request: GenerationRequest,
            _cancellation: CancellationToken,
        ) -> Result<LlmStream, ProviderError> {
            let behavior = if request.transcript == "slow" {
                LlmBehavior::Delayed("This response should be superseded.".into())
            } else {
                self.behavior.clone()
            };
            match behavior {
                LlmBehavior::Text(text) => Ok(Box::pin(stream::iter(vec![Ok(LlmDelta {
                    text,
                    sequence: 1,
                })]))),
                LlmBehavior::OpenError => Err(ProviderError::unavailable(
                    self.descriptor.id.clone(),
                    "fixture unavailable",
                )),
                LlmBehavior::Delayed(text) => Ok(Box::pin(stream::once(async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Ok(LlmDelta { text, sequence: 1 })
                }))),
                LlmBehavior::ErrorAfter(text) => Ok(Box::pin(stream::iter(vec![
                    Ok(LlmDelta { text, sequence: 1 }),
                    Err(ProviderError::unavailable(
                        self.descriptor.id.clone(),
                        "stream disconnected",
                    )),
                ]))),
            }
        }
    }

    struct FakeEffects {
        descriptor: ProviderDescriptor,
    }

    #[async_trait]
    impl EffectsProvider for FakeEffects {
        fn descriptor(&self) -> &ProviderDescriptor {
            &self.descriptor
        }

        async fn derive_effects(
            &self,
            _request: EffectsRequest,
            _cancellation: CancellationToken,
        ) -> Result<NpcEffectsV1, ProviderError> {
            Ok(NpcEffectsV1 {
                emotion: Some("warm".into()),
                valence: Some(0.6),
                arousal: Some(0.2),
                intensity: Some(0.4),
                action_proposals: vec![
                    ActionProposal {
                        action: "wave".into(),
                        arguments: BTreeMap::new(),
                    },
                    ActionProposal {
                        action: "execute_arbitrary_code".into(),
                        arguments: BTreeMap::new(),
                    },
                ],
                ..Default::default()
            })
        }
    }

    struct FakeTts {
        descriptor: ProviderDescriptor,
    }

    struct FakeTtsSession {
        descriptor: ProviderDescriptor,
    }

    #[async_trait]
    impl TtsProvider for FakeTts {
        fn descriptor(&self) -> &ProviderDescriptor {
            &self.descriptor
        }

        async fn start_session(
            &self,
            _identity: &TurnIdentity,
            _locale: &str,
            _cancellation: CancellationToken,
        ) -> Result<Box<dyn TtsSession>, ProviderError> {
            Ok(Box::new(FakeTtsSession {
                descriptor: self.descriptor.clone(),
            }))
        }
    }

    #[async_trait]
    impl TtsSession for FakeTtsSession {
        fn provider(&self) -> &ProviderDescriptor {
            &self.descriptor
        }

        async fn synthesize(
            &mut self,
            _request: SpeechRequest,
            _cancellation: CancellationToken,
        ) -> Result<SpeechStream, ProviderError> {
            Ok(Box::pin(stream::iter(vec![Ok(SpeechStreamItem::Audio(
                AudioChunk {
                    sequence: 1,
                    sample_rate_hz: 24_000,
                    channels: 1,
                    pcm_s16le: vec![0; 480],
                    end_of_stream: true,
                },
            ))])))
        }
    }

    #[derive(Default)]
    struct FakeAudio {
        stops: std::sync::atomic::AtomicU64,
    }

    #[async_trait]
    impl AudioSink for FakeAudio {
        async fn play(
            &self,
            _identity: &TurnIdentity,
            _sentence_id: u64,
            mut stream: SpeechStream,
            cancellation: CancellationToken,
        ) -> Result<PlaybackReceipt, RuntimeDependencyError> {
            let mut frames = 0;
            while let Some(item) = stream.next().await {
                if cancellation.is_cancelled() {
                    return Err(RuntimeDependencyError::Cancelled);
                }
                if let SpeechStreamItem::Audio(chunk) =
                    item.map_err(|error| RuntimeDependencyError::Unavailable(error.to_string()))?
                {
                    frames += (chunk.pcm_s16le.len() / 2) as u64;
                }
            }
            Ok(PlaybackReceipt {
                audible_frames: frames,
                duration: Duration::from_millis(10),
                completed: true,
            })
        }

        async fn stop(&self, _identity: &TurnIdentity) -> Result<(), RuntimeDependencyError> {
            self.stops
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        }
    }

    #[derive(Default)]
    struct CancellationAwareAudio {
        play_started: tokio::sync::Notify,
        play_calls: std::sync::atomic::AtomicU64,
        saw_cancellation: std::sync::atomic::AtomicBool,
        partial_receipts_returned: std::sync::atomic::AtomicU64,
    }

    #[async_trait]
    impl AudioSink for CancellationAwareAudio {
        async fn play(
            &self,
            _identity: &TurnIdentity,
            _sentence_id: u64,
            _stream: SpeechStream,
            cancellation: CancellationToken,
        ) -> Result<PlaybackReceipt, RuntimeDependencyError> {
            self.play_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.play_started.notify_one();
            cancellation.cancelled().await;
            self.saw_cancellation
                .store(true, std::sync::atomic::Ordering::Release);

            // A real device sink needs a short cooperative drain to stop playback and
            // measure what was heard. Keep this pending long enough to prove the
            // supervisor does not drop the sink future when cancellation fires.
            tokio::time::sleep(Duration::from_millis(20)).await;
            self.partial_receipts_returned
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(PlaybackReceipt {
                audible_frames: 240,
                duration: Duration::from_millis(10),
                completed: false,
            })
        }

        async fn stop(&self, _identity: &TurnIdentity) -> Result<(), RuntimeDependencyError> {
            Ok(())
        }
    }

    fn request(turn_id: &str, transcript: &str) -> TurnRequest {
        TurnRequest {
            session_id: "session-1".into(),
            turn_id: turn_id.into(),
            transcript: transcript.into(),
            character_hint: Some("Nova".into()),
            game_id: "eclipse-harbor".into(),
            locale: "en-US".into(),
            execution_mode: ExecutionMode::FullyLocal,
            network_policy: NetworkPolicy::Offline,
            authorized_cloud_providers: Vec::new(),
            allow_provider_fallback: true,
            allow_local_to_cloud_fallback: false,
            allow_retaining_providers: false,
            metadata: BTreeMap::new(),
        }
    }

    fn supervisor(
        llms: Vec<Arc<dyn LanguageModelProvider>>,
        with_effects: bool,
        with_tts: bool,
    ) -> (Arc<TurnSupervisor>, Arc<FakeMemory>, Arc<FakeAudio>) {
        let memory = Arc::new(FakeMemory::default());
        let audio = Arc::new(FakeAudio::default());
        let effects: Vec<Arc<dyn EffectsProvider>> = if with_effects {
            vec![Arc::new(FakeEffects {
                descriptor: descriptor(
                    "effects-local",
                    ProviderModality::Effects,
                    ProviderLocation::Local,
                ),
            })]
        } else {
            Vec::new()
        };
        let speech: Vec<Arc<dyn TtsProvider>> = if with_tts {
            vec![Arc::new(FakeTts {
                descriptor: descriptor(
                    "tts-local",
                    ProviderModality::Speech,
                    ProviderLocation::Local,
                ),
            })]
        } else {
            Vec::new()
        };
        let dependencies = RuntimeDependencies {
            providers: ProviderPool {
                language_models: llms,
                effects,
                speech,
                ..Default::default()
            },
            identity: Arc::new(FakeIdentity),
            memory: memory.clone(),
            audio: audio.clone(),
        };
        let config = SupervisorConfig {
            allowed_actions: vec!["wave".into()],
            ..Default::default()
        };
        (TurnSupervisor::new(config, dependencies), memory, audio)
    }

    #[tokio::test]
    async fn completes_spoken_and_effects_lanes_and_commits_only_delivery() {
        let llm: Arc<dyn LanguageModelProvider> = Arc::new(FakeLlm {
            descriptor: descriptor(
                "llm-local",
                ProviderModality::LanguageModel,
                ProviderLocation::Local,
            ),
            behavior: LlmBehavior::Text("The harbor remembers us. We should move now.".into()),
        });
        let (supervisor, memory, _) = supervisor(vec![llm], true, true);
        let outcome = supervisor
            .start_turn(request("turn-1", "Are we safe?"))
            .await
            .unwrap()
            .outcome()
            .await
            .unwrap();

        assert_eq!(outcome.lifecycle, TurnLifecycle::Completed);
        assert_eq!(outcome.selected_llm_provider.as_deref(), Some("llm-local"));
        assert_eq!(outcome.selected_tts_providers, vec!["tts-local"]);
        assert!(outcome
            .delivered
            .iter()
            .all(|sentence| sentence.delivery == DeliveryMode::Audio));
        assert_eq!(outcome.effects.emotion.as_deref(), Some("warm"));
        assert_eq!(outcome.effects.action_proposals.len(), 1);
        assert_eq!(outcome.effects.action_proposals[0].action, "wave");
        let commits = memory.commits.lock().await;
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0], outcome.delivered);
    }

    #[tokio::test]
    async fn structured_route_validates_before_tts_and_propagates_exact_spans() {
        let structured = r#"{
            "schema_version":"npc_response.v1",
            "spoken_response":{"text":"The 界 gate is open. We should leave now."},
            "emotion":{"kind":"concern","valence":-0.2,"arousal":0.6,"intensity":0.5},
            "voice_style":{"kind":"tense","speaking_rate":1.1,"pitch_semitones":0.0,"energy":1.1},
            "animation_cues":[],
            "memory_proposals":[],
            "interruption_behavior":"barge_in_allowed",
            "actions":[]
        }"#;
        let llm: Arc<dyn LanguageModelProvider> = Arc::new(FakeLlm {
            descriptor: descriptor(
                "llm-structured",
                ProviderModality::LanguageModel,
                ProviderLocation::Local,
            ),
            behavior: LlmBehavior::Text(structured.into()),
        });
        let (supervisor, memory, _) = supervisor(vec![llm], false, true);
        let mut turn_request = request("turn-structured", "Is the gate open?");
        turn_request.metadata.insert(
            StreamingResponseFormatV1::ROUTE_METADATA_KEY.into(),
            "structured_v1".into(),
        );
        let outcome = supervisor
            .start_turn(turn_request)
            .await
            .unwrap()
            .outcome()
            .await
            .unwrap();

        assert_eq!(outcome.lifecycle, TurnLifecycle::Completed);
        assert_eq!(
            outcome.full_response,
            "The 界 gate is open. We should leave now."
        );
        assert!(outcome.structured_response.is_some());
        assert_eq!(outcome.delivered.len(), 2);
        for sentence in &outcome.delivered {
            assert_eq!(
                outcome
                    .full_response
                    .get(sentence.text_start_bytes..sentence.text_end_bytes),
                Some(sentence.text.as_str())
            );
            assert!(!sentence.text.contains("schema_version"));
        }
        assert_eq!(memory.commits.lock().await.as_slice(), &[outcome.delivered]);
    }

    #[tokio::test]
    async fn malformed_structured_route_never_sends_json_to_tts_or_delivery() {
        let llm: Arc<dyn LanguageModelProvider> = Arc::new(FakeLlm {
            descriptor: descriptor(
                "llm-structured-bad",
                ProviderModality::LanguageModel,
                ProviderLocation::Local,
            ),
            behavior: LlmBehavior::Text(
                r#"{"schema_version":"npc_response.v1","spoken_response":{"text":"Never speak this."},"unsupported":true}"#
                    .into(),
            ),
        });
        let (supervisor, _, _) = supervisor(vec![llm], false, true);
        let mut turn_request = request("turn-structured-bad", "Status?");
        turn_request.metadata.insert(
            StreamingResponseFormatV1::ROUTE_METADATA_KEY.into(),
            "structured_v1".into(),
        );
        let outcome = supervisor
            .start_turn(turn_request)
            .await
            .unwrap()
            .outcome()
            .await
            .unwrap();

        assert_eq!(outcome.lifecycle, TurnLifecycle::Failed);
        assert!(outcome.full_response.is_empty());
        assert!(outcome.structured_response.is_none());
        assert!(outcome.delivered.is_empty());
        assert_eq!(
            outcome.error.as_ref().map(|error| error.code.as_str()),
            Some("provider_response_invalid")
        );
    }

    #[tokio::test]
    async fn falls_back_before_output_and_degrades_missing_effects_and_audio() {
        let failed: Arc<dyn LanguageModelProvider> = Arc::new(FakeLlm {
            descriptor: descriptor(
                "llm-primary",
                ProviderModality::LanguageModel,
                ProviderLocation::Local,
            ),
            behavior: LlmBehavior::OpenError,
        });
        let fallback: Arc<dyn LanguageModelProvider> = Arc::new(FakeLlm {
            descriptor: descriptor(
                "llm-fallback",
                ProviderModality::LanguageModel,
                ProviderLocation::Local,
            ),
            behavior: LlmBehavior::Text("Fallback response is safely delivered.".into()),
        });
        let (supervisor, _, _) = supervisor(vec![failed, fallback], false, false);
        let outcome = supervisor
            .start_turn(request("turn-2", "Status?"))
            .await
            .unwrap()
            .outcome()
            .await
            .unwrap();

        assert_eq!(outcome.lifecycle, TurnLifecycle::Completed);
        assert_eq!(
            outcome.selected_llm_provider.as_deref(),
            Some("llm-fallback")
        );
        assert!(outcome.degradations.contains(&Degradation::NeutralEffects));
        assert!(outcome
            .degradations
            .contains(&Degradation::AudioToSubtitles));
        assert!(outcome
            .degradations
            .contains(&Degradation::ProviderFallback {
                from: "llm-primary".into(),
                to: "llm-fallback".into(),
            }));
        assert!(outcome
            .delivered
            .iter()
            .all(|sentence| sentence.delivery == DeliveryMode::Subtitle));
    }

    #[tokio::test]
    async fn newer_turn_barges_in_and_old_generation_cannot_commit() {
        let llm: Arc<dyn LanguageModelProvider> = Arc::new(FakeLlm {
            descriptor: descriptor(
                "llm-local",
                ProviderModality::LanguageModel,
                ProviderLocation::Local,
            ),
            behavior: LlmBehavior::Text("The new turn wins.".into()),
        });
        let (supervisor, memory, audio) = supervisor(vec![llm], false, false);
        let first = supervisor
            .start_turn(request("turn-old", "slow"))
            .await
            .unwrap();
        let second = supervisor
            .start_turn(request("turn-new", "continue"))
            .await
            .unwrap();

        let first_outcome = tokio::time::timeout(Duration::from_secs(1), first.outcome())
            .await
            .expect("cancelled turn should settle promptly")
            .unwrap();
        let second_outcome = second.outcome().await.unwrap();
        assert_eq!(first_outcome.lifecycle, TurnLifecycle::Cancelled);
        assert!(first_outcome.delivered.is_empty());
        assert_eq!(second_outcome.lifecycle, TurnLifecycle::Completed);
        assert!(
            second_outcome.identity.cancellation_generation
                > first_outcome.identity.cancellation_generation
        );
        assert!(audio.stops.load(std::sync::atomic::Ordering::Relaxed) >= 1);
        let commits = memory.commits.lock().await;
        assert_eq!(commits.len(), 1, "cancelled turns must not be committed");
    }

    #[tokio::test]
    async fn cancelled_playback_returns_partial_receipt_without_committing_dialogue() {
        let llm: Arc<dyn LanguageModelProvider> = Arc::new(FakeLlm {
            descriptor: descriptor(
                "llm-local",
                ProviderModality::LanguageModel,
                ProviderLocation::Local,
            ),
            behavior: LlmBehavior::Text("This sentence starts playing before barge-in.".into()),
        });
        let memory = Arc::new(FakeMemory::default());
        let audio = Arc::new(CancellationAwareAudio::default());
        let dependencies = RuntimeDependencies {
            providers: ProviderPool {
                language_models: vec![llm],
                speech: vec![
                    Arc::new(FakeTts {
                        descriptor: descriptor(
                            "tts-primary",
                            ProviderModality::Speech,
                            ProviderLocation::Local,
                        ),
                    }),
                    Arc::new(FakeTts {
                        descriptor: descriptor(
                            "tts-fallback",
                            ProviderModality::Speech,
                            ProviderLocation::Local,
                        ),
                    }),
                ],
                ..Default::default()
            },
            identity: Arc::new(FakeIdentity),
            memory: memory.clone(),
            audio: audio.clone(),
        };
        let supervisor = TurnSupervisor::new(SupervisorConfig::default(), dependencies);

        let playback_started = audio.play_started.notified();
        let turn = supervisor
            .start_turn(request("turn-cancelled-audio", "Stop."))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), playback_started)
            .await
            .expect("audio playback should start");
        turn.cancel();

        let outcome = tokio::time::timeout(Duration::from_secs(1), turn.outcome())
            .await
            .expect("cancelled turn should settle after the sink returns its receipt")
            .unwrap();

        assert_eq!(outcome.lifecycle, TurnLifecycle::Cancelled);
        assert!(outcome.delivered.is_empty());
        assert_eq!(
            audio
                .partial_receipts_returned
                .load(std::sync::atomic::Ordering::Relaxed),
            1,
            "the sink must finish its partial-delivery receipt before cancellation settles"
        );
        assert!(
            audio
                .saw_cancellation
                .load(std::sync::atomic::Ordering::Acquire),
            "the sink must observe the cooperative cancellation token"
        );
        assert_eq!(
            audio.play_calls.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "cancellation must not retry playback and duplicate the voice"
        );
        assert!(
            memory.commits.lock().await.is_empty(),
            "cancelled partial dialogue must not be committed"
        );
    }

    #[tokio::test]
    async fn stream_failure_after_output_preserves_and_commits_delivered_sentence() {
        let llm: Arc<dyn LanguageModelProvider> = Arc::new(FakeLlm {
            descriptor: descriptor(
                "llm-flaky",
                ProviderModality::LanguageModel,
                ProviderLocation::Local,
            ),
            behavior: LlmBehavior::ErrorAfter("This sentence was fully delivered. ".into()),
        });
        let (supervisor, memory, _) = supervisor(vec![llm], false, true);
        let outcome = supervisor
            .start_turn(request("turn-partial", "Report."))
            .await
            .unwrap()
            .outcome()
            .await
            .unwrap();

        assert_eq!(outcome.lifecycle, TurnLifecycle::Failed);
        assert_eq!(outcome.delivered.len(), 1);
        assert_eq!(outcome.delivered[0].delivery, DeliveryMode::Audio);
        assert_eq!(
            outcome.error.as_ref().unwrap().code,
            "provider_stream_failed"
        );
        let commits = memory.commits.lock().await;
        assert_eq!(commits.as_slice(), &[outcome.delivered]);
    }
}
