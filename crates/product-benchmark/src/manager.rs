use crate::{
    aggregate::Aggregator,
    evidence::{BaselineEvidenceV1, IterationEvidenceV1},
    model::{
        ActionCode, AvailabilityReason, BenchmarkClassificationV1, BenchmarkComponent,
        BenchmarkReportV1, BenchmarkRunRequestV1, BenchmarkRunState, BenchmarkStatusV1,
        EvidenceExecutionMode, HardwareEvidenceV1, MeasurementAvailability, MeasurementKind,
        PersistenceEvidenceV1, ProbeProvenanceV1, RunBoundsV1, SourceCoverageV1,
        BENCHMARK_REPORT_SCHEMA_V1,
    },
    BenchmarkReportStore,
};
use async_trait::async_trait;
use npc_system_telemetry::{collect, ResourceTelemetrySnapshotV1, TelemetryRequest};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct TelemetryPairV1 {
    pub application: ResourceTelemetrySnapshotV1,
    pub game: ResourceTelemetrySnapshotV1,
}

pub trait ResourceTelemetryCollector: Send + Sync + 'static {
    fn collect(&self, selected_game_pid: Option<u32>) -> TelemetryPairV1;
}

#[derive(Debug, Default)]
pub struct NativeTelemetryCollector;

impl ResourceTelemetryCollector for NativeTelemetryCollector {
    fn collect(&self, selected_game_pid: Option<u32>) -> TelemetryPairV1 {
        let application = collect(TelemetryRequest {
            selected_game_pid: Some(std::process::id()),
            ..TelemetryRequest::default()
        });
        let game = collect(TelemetryRequest {
            selected_game_pid,
            ..TelemetryRequest::default()
        });
        TelemetryPairV1 { application, game }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeError {
    RuntimeUnavailable,
    MediaBrokerUnavailable,
    TargetChanged,
    ProviderUnavailable,
    TimedOut,
    InternalFailure,
}

impl ProbeError {
    fn availability_reason(self) -> AvailabilityReason {
        match self {
            Self::RuntimeUnavailable | Self::ProviderUnavailable => {
                AvailabilityReason::NoLiveProviderRoute
            }
            Self::MediaBrokerUnavailable => AvailabilityReason::AudioReceiptUnavailable,
            Self::TargetChanged => AvailabilityReason::TargetNotAdvancing,
            Self::TimedOut => AvailabilityReason::TimedOut,
            Self::InternalFailure => AvailabilityReason::InternalFailure,
        }
    }
}

#[async_trait]
pub trait LiveBenchmarkProbe: Send + Sync + 'static {
    async fn preflight(
        &self,
        request: &BenchmarkRunRequestV1,
        cancellation: &CancellationToken,
    ) -> Result<crate::PreflightEvidenceV1, ProbeError>;

    async fn collect_baseline(
        &self,
        request: &BenchmarkRunRequestV1,
        cancellation: &CancellationToken,
    ) -> Result<BaselineEvidenceV1, ProbeError>;

    async fn measure_iteration(
        &self,
        request: &BenchmarkRunRequestV1,
        iteration: u16,
        cancellation: &CancellationToken,
    ) -> Result<IterationEvidenceV1, ProbeError>;
}

#[derive(Debug, Error)]
pub enum BenchmarkManagerError {
    #[error("benchmark request is invalid: {0}")]
    InvalidRequest(&'static str),
    #[error("a benchmark run is already active")]
    AlreadyRunning,
    #[error("no benchmark run is active")]
    NotRunning,
    #[error("the requested report is not available")]
    ReportUnavailable,
    #[error("benchmark state is temporarily unavailable")]
    StateUnavailable,
}

#[derive(Clone)]
pub struct BenchmarkManager {
    inner: Arc<ManagerInner>,
}

struct ManagerInner {
    state: Mutex<ManagerState>,
    probe: Arc<dyn LiveBenchmarkProbe>,
    telemetry: Arc<dyn ResourceTelemetryCollector>,
    store: BenchmarkReportStore,
}

struct ManagerState {
    status: BenchmarkStatusV1,
    cancellation: Option<CancellationToken>,
    report: Option<BenchmarkReportV1>,
    started: Option<Instant>,
}

impl BenchmarkManager {
    pub fn new(
        store: BenchmarkReportStore,
        probe: Arc<dyn LiveBenchmarkProbe>,
        telemetry: Arc<dyn ResourceTelemetryCollector>,
    ) -> Self {
        Self {
            inner: Arc::new(ManagerInner {
                state: Mutex::new(ManagerState {
                    status: BenchmarkStatusV1::idle(),
                    cancellation: None,
                    report: None,
                    started: None,
                }),
                probe,
                telemetry,
                store,
            }),
        }
    }

    pub fn start(
        &self,
        request: BenchmarkRunRequestV1,
    ) -> Result<BenchmarkStatusV1, BenchmarkManagerError> {
        request
            .validate()
            .map_err(BenchmarkManagerError::InvalidRequest)?;
        let now = Instant::now();
        let started_at_utc = utc_now();
        let report_id = report_id();
        let cancellation = CancellationToken::new();
        let status = BenchmarkStatusV1 {
            state: BenchmarkRunState::Running,
            report_id: Some(report_id.clone()),
            requested_iterations: request.requested_iterations,
            completed_iterations: 0,
            started_at_utc: Some(started_at_utc.clone()),
            elapsed_millis: 0,
            report_file_name: None,
            unavailable_components: Vec::new(),
            action_codes: Vec::new(),
        };
        {
            let mut state = self
                .inner
                .state
                .lock()
                .map_err(|_| BenchmarkManagerError::StateUnavailable)?;
            if matches!(
                state.status.state,
                BenchmarkRunState::Running | BenchmarkRunState::Cancelling
            ) {
                return Err(BenchmarkManagerError::AlreadyRunning);
            }
            state.status = status.clone();
            state.cancellation = Some(cancellation.clone());
            state.report = None;
            state.started = Some(now);
        }

        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            run_benchmark(inner, request, report_id, started_at_utc, now, cancellation).await;
        });
        Ok(status)
    }

    pub fn cancel(&self) -> Result<BenchmarkStatusV1, BenchmarkManagerError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| BenchmarkManagerError::StateUnavailable)?;
        match state.status.state {
            BenchmarkRunState::Running => {
                let cancellation = state
                    .cancellation
                    .as_ref()
                    .ok_or(BenchmarkManagerError::NotRunning)?
                    .clone();
                cancellation.cancel();
                state.status.state = BenchmarkRunState::Cancelling;
                Ok(status_with_elapsed(&state))
            }
            BenchmarkRunState::Cancelling => Ok(status_with_elapsed(&state)),
            _ => Err(BenchmarkManagerError::NotRunning),
        }
    }

    pub fn status(&self) -> Result<BenchmarkStatusV1, BenchmarkManagerError> {
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| BenchmarkManagerError::StateUnavailable)?;
        Ok(status_with_elapsed(&state))
    }

    pub fn report(&self, report_id: &str) -> Result<BenchmarkReportV1, BenchmarkManagerError> {
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| BenchmarkManagerError::StateUnavailable)?;
        state
            .report
            .as_ref()
            .filter(|report| report.report_id == report_id)
            .cloned()
            .ok_or(BenchmarkManagerError::ReportUnavailable)
    }
}

fn status_with_elapsed(state: &ManagerState) -> BenchmarkStatusV1 {
    let mut status = state.status.clone();
    if !status.state.terminal() {
        status.elapsed_millis = state
            .started
            .map(|started| elapsed_millis(started.elapsed()))
            .unwrap_or(0);
    }
    status
}

async fn run_benchmark(
    inner: Arc<ManagerInner>,
    request: BenchmarkRunRequestV1,
    report_id: String,
    started_at_utc: String,
    started: Instant,
    cancellation: CancellationToken,
) {
    let deadline = Duration::from_millis(request.timeout_millis);
    let mut aggregator = Aggregator::default();
    let mut failed_iterations = 0_u16;
    let mut completed_iterations = 0_u16;
    let mut terminal_reason = None;
    let preflight = match timeout_step(
        started,
        deadline,
        &cancellation,
        inner.probe.preflight(&request, &cancellation),
    )
    .await
    {
        StepResult::Completed(Ok(preflight))
            if preflight.validate().is_ok() && preflight_matches_binding(&preflight, &request) =>
        {
            preflight
        }
        StepResult::Completed(Ok(_)) => fallback_preflight(AvailabilityReason::InvalidReceipt),
        StepResult::Completed(Err(error)) => fallback_preflight(error.availability_reason()),
        StepResult::Cancelled => fallback_preflight(AvailabilityReason::Cancelled),
        StepResult::TimedOut => fallback_preflight(AvailabilityReason::TimedOut),
    };
    let has_ready_component = preflight
        .components
        .iter()
        .any(|component| component.availability == MeasurementAvailability::Ready);

    if has_ready_component && !cancellation.is_cancelled() && started.elapsed() < deadline {
        match timeout_step(
            started,
            deadline,
            &cancellation,
            inner.probe.collect_baseline(&request, &cancellation),
        )
        .await
        {
            StepResult::Completed(Ok(baseline))
                if baseline.validate().is_ok()
                    && baseline.measured_window_ns()
                        >= request.baseline_window_millis.saturating_mul(1_000_000) =>
            {
                aggregator.add_baseline(&baseline);
            }
            StepResult::Completed(Ok(_)) => {
                terminal_reason = Some(AvailabilityReason::InvalidReceipt);
            }
            StepResult::Completed(Err(error)) => {
                terminal_reason = Some(error.availability_reason())
            }
            StepResult::Cancelled => terminal_reason = Some(AvailabilityReason::Cancelled),
            StepResult::TimedOut => terminal_reason = Some(AvailabilityReason::TimedOut),
        }
    }

    for iteration in 1..=request.requested_iterations {
        if !has_ready_component {
            break;
        }
        if cancellation.is_cancelled() || started.elapsed() >= deadline {
            terminal_reason = Some(if cancellation.is_cancelled() {
                AvailabilityReason::Cancelled
            } else {
                AvailabilityReason::TimedOut
            });
            break;
        }
        match timeout_step(
            started,
            deadline,
            &cancellation,
            inner
                .probe
                .measure_iteration(&request, iteration, &cancellation),
        )
        .await
        {
            StepResult::Completed(Ok(evidence)) => {
                let telemetry = inner.telemetry.collect(request.binding.selected_game_pid);
                aggregator.add_iteration(
                    evidence.validate_and_measure(&request.binding),
                    &telemetry.application,
                    &telemetry.game,
                );
                completed_iterations = completed_iterations.saturating_add(1);
                update_progress(&inner, completed_iterations, started);
            }
            StepResult::Completed(Err(error)) => {
                failed_iterations = failed_iterations.saturating_add(1);
                terminal_reason = Some(error.availability_reason());
            }
            StepResult::Cancelled => {
                terminal_reason = Some(AvailabilityReason::Cancelled);
                break;
            }
            StepResult::TimedOut => {
                terminal_reason = Some(AvailabilityReason::TimedOut);
                break;
            }
        }
    }

    let telemetry = inner.telemetry.collect(request.binding.selected_game_pid);
    let coverage_components = final_component_coverage(&preflight, &aggregator, terminal_reason);
    let unavailable = coverage_components
        .iter()
        .filter(|component| component.availability == MeasurementAvailability::Unavailable)
        .map(|component| component.component)
        .collect::<Vec<_>>();
    let state = terminal_state(
        &request,
        &preflight,
        &aggregator,
        completed_iterations,
        cancellation.is_cancelled(),
        started.elapsed() >= deadline,
    );
    let elapsed = elapsed_millis(started.elapsed());
    let mut report = build_report(
        &request,
        report_id,
        &started_at_utc,
        state,
        &preflight,
        aggregator,
        completed_iterations,
        failed_iterations,
        elapsed,
        telemetry.game,
        coverage_components,
        unavailable,
    );
    let expected_file_name = format!("this-pc-benchmark-{}.json", report.report_id);
    report.persistence = PersistenceEvidenceV1 {
        state: "persisted".into(),
        report_file_name: Some(expected_file_name.clone()),
        atomic_write: true,
        report_directory_recorded: false,
    };
    let persistence = inner.store.persist(&report);
    if persistence.is_err() {
        report.state = BenchmarkRunState::Failed;
        report.classification.acceptance_eligible = false;
        report.classification.reason =
            "The bounded run finished, but its redacted report could not be persisted.".into();
        report.persistence = PersistenceEvidenceV1 {
            state: "failed".into(),
            report_file_name: None,
            atomic_write: true,
            report_directory_recorded: false,
        };
    }
    finish_state(&inner, report, persistence.ok());
}

enum StepResult<T> {
    Completed(T),
    Cancelled,
    TimedOut,
}

async fn timeout_step<F, T>(
    started: Instant,
    deadline: Duration,
    cancellation: &CancellationToken,
    future: F,
) -> StepResult<T>
where
    F: std::future::Future<Output = T>,
{
    let Some(remaining) = deadline.checked_sub(started.elapsed()) else {
        return StepResult::TimedOut;
    };
    tokio::select! {
        biased;
        () = cancellation.cancelled() => StepResult::Cancelled,
        result = tokio::time::timeout(remaining, future) => match result {
            Ok(result) => StepResult::Completed(result),
            Err(_) => StepResult::TimedOut,
        }
    }
}

fn fallback_preflight(reason: AvailabilityReason) -> crate::PreflightEvidenceV1 {
    let action = if reason == AvailabilityReason::Cancelled {
        ActionCode::RetryBenchmark
    } else {
        ActionCode::ReviewDiagnostics
    };
    crate::PreflightEvidenceV1 {
        execution_mode: EvidenceExecutionMode::Live,
        measurement_kind: MeasurementKind::Measured,
        provider_observation_source: crate::ProviderObservationSourceV1::Unavailable,
        runtime_revision: "unavailable".into(),
        broker_revision: "unavailable".into(),
        compositor_revision: "unavailable".into(),
        process_load_sampler_revision: "unavailable".into(),
        game_frame_sampler_revision: "unavailable".into(),
        components: BenchmarkComponent::all()
            .into_iter()
            .map(|component| crate::ComponentReadinessV1::unavailable(component, reason, action))
            .collect(),
    }
}

fn update_progress(inner: &ManagerInner, completed: u16, started: Instant) {
    if let Ok(mut state) = inner.state.lock() {
        state.status.completed_iterations = completed;
        state.status.elapsed_millis = elapsed_millis(started.elapsed());
    }
}

fn preflight_matches_binding(
    preflight: &crate::PreflightEvidenceV1,
    request: &BenchmarkRunRequestV1,
) -> bool {
    let role_is_live =
        |role| {
            request.binding.provider_routes.iter().any(|route| {
                route.role == role && route.execution_mode == EvidenceExecutionMode::Live
            })
        };
    let component_ready = |component| {
        preflight.components.iter().any(|item| {
            item.component == component && item.availability == MeasurementAvailability::Ready
        })
    };
    (!component_ready(BenchmarkComponent::LiveLlmProvider)
        || role_is_live(crate::ProviderRole::Llm))
        && (!component_ready(BenchmarkComponent::LiveTtsProvider)
            || role_is_live(crate::ProviderRole::Tts))
}

fn final_component_coverage(
    preflight: &crate::PreflightEvidenceV1,
    aggregator: &Aggregator,
    terminal_reason: Option<AvailabilityReason>,
) -> Vec<crate::ComponentReadinessV1> {
    let mut components = preflight.components.clone();
    for metric in aggregator.missing_metrics() {
        if let Some(component) = metric_component(metric) {
            mark_component_unavailable(&mut components, component);
        }
    }
    if let Some(reason) = terminal_reason {
        let action = if reason == AvailabilityReason::Cancelled {
            ActionCode::RetryBenchmark
        } else {
            ActionCode::ReviewDiagnostics
        };
        for component in &mut components {
            if component.availability == MeasurementAvailability::Ready {
                *component =
                    crate::ComponentReadinessV1::unavailable(component.component, reason, action);
            }
        }
    }
    components
}

fn mark_component_unavailable(
    components: &mut [crate::ComponentReadinessV1],
    target: BenchmarkComponent,
) {
    let Some(component) = components
        .iter_mut()
        .find(|component| component.component == target)
    else {
        return;
    };
    if component.availability == MeasurementAvailability::Unavailable {
        return;
    }
    let (reason, action) = match target {
        BenchmarkComponent::SelectedGameTarget => (
            AvailabilityReason::NoSelectedGameTarget,
            ActionCode::SelectRunningGame,
        ),
        BenchmarkComponent::AdvancingCapture => (
            AvailabilityReason::CaptureReceiptUnavailable,
            ActionCode::WaitForAdvancingFrame,
        ),
        BenchmarkComponent::IdentityTiming => (
            AvailabilityReason::RuntimeTimingReceiptUnavailable,
            ActionCode::ReviewDiagnostics,
        ),
        BenchmarkComponent::LiveLlmProvider => (
            AvailabilityReason::NoLiveProviderRoute,
            ActionCode::ChooseQualifiedLiveLlm,
        ),
        BenchmarkComponent::LiveTtsProvider => (
            AvailabilityReason::NoLiveProviderRoute,
            ActionCode::ChooseQualifiedLiveTts,
        ),
        BenchmarkComponent::BrokerAudioSubmission => (
            AvailabilityReason::AudioReceiptUnavailable,
            ActionCode::StartMediaBroker,
        ),
        BenchmarkComponent::NativeCompositor => (
            AvailabilityReason::CompositorReceiptUnavailable,
            ActionCode::EnableVisualPath,
        ),
        BenchmarkComponent::ProcessCpuSampler => (
            AvailabilityReason::OperatingSystemMetricUnavailable,
            ActionCode::ReviewDiagnostics,
        ),
        BenchmarkComponent::DeviceGpuSampler => (
            AvailabilityReason::DriverMetricUnavailable,
            ActionCode::UpdateGraphicsDriver,
        ),
        BenchmarkComponent::SystemRamTelemetry => (
            AvailabilityReason::OperatingSystemMetricUnavailable,
            ActionCode::ReviewDiagnostics,
        ),
        BenchmarkComponent::ProcessVramTelemetry => (
            AvailabilityReason::DriverMetricUnavailable,
            ActionCode::UpdateGraphicsDriver,
        ),
        BenchmarkComponent::GameFrameSampler => (
            AvailabilityReason::OperatingSystemMetricUnavailable,
            ActionCode::WaitForAdvancingFrame,
        ),
    };
    *component = crate::ComponentReadinessV1::unavailable(target, reason, action);
}

fn metric_component(metric: crate::BenchmarkMetric) -> Option<BenchmarkComponent> {
    use crate::BenchmarkMetric as Metric;
    Some(match metric {
        Metric::CaptureLatencyMs => BenchmarkComponent::AdvancingCapture,
        Metric::IdentityLatencyMs => BenchmarkComponent::IdentityTiming,
        Metric::LlmTimeToFirstTokenMs | Metric::LlmOutputTokens | Metric::LlmTokensPerSecond => {
            BenchmarkComponent::LiveLlmProvider
        }
        Metric::TtsTimeToFirstAudioMs | Metric::TtsAudioCompletionMs => {
            BenchmarkComponent::LiveTtsProvider
        }
        Metric::AudioSubmissionMs | Metric::EndToEndTurnMs => {
            BenchmarkComponent::BrokerAudioSubmission
        }
        Metric::CompositorLatencyMs | Metric::CompositorFps | Metric::CompositorStaleDrops => {
            BenchmarkComponent::NativeCompositor
        }
        Metric::ProcessCpuPercent => BenchmarkComponent::ProcessCpuSampler,
        Metric::DeviceGpuPercent => BenchmarkComponent::DeviceGpuSampler,
        Metric::ProcessRamBytes | Metric::GameRamBytes => BenchmarkComponent::SystemRamTelemetry,
        Metric::ProcessVramBytes | Metric::GameVramBytes => {
            BenchmarkComponent::ProcessVramTelemetry
        }
        Metric::GameFrameTimeMs | Metric::GameFps => BenchmarkComponent::GameFrameSampler,
    })
}

fn terminal_state(
    request: &BenchmarkRunRequestV1,
    preflight: &crate::PreflightEvidenceV1,
    aggregator: &Aggregator,
    completed_iterations: u16,
    cancelled: bool,
    timed_out: bool,
) -> BenchmarkRunState {
    if cancelled {
        return BenchmarkRunState::Cancelled;
    }
    if timed_out {
        return if completed_iterations == 0 {
            BenchmarkRunState::Unavailable
        } else {
            BenchmarkRunState::Partial
        };
    }
    let ready_count = preflight
        .components
        .iter()
        .filter(|component| component.availability == MeasurementAvailability::Ready)
        .count();
    if completed_iterations == 0 || ready_count == 0 {
        return BenchmarkRunState::Unavailable;
    }
    if completed_iterations == request.requested_iterations
        && aggregator.missing_metrics().is_empty()
        && aggregator.has_complete_frame_impact()
        && aggregator.invalid_receipt_count == 0
        && ready_count == BenchmarkComponent::all().len()
    {
        BenchmarkRunState::Completed
    } else {
        BenchmarkRunState::Partial
    }
}

#[allow(clippy::too_many_arguments)]
fn build_report(
    request: &BenchmarkRunRequestV1,
    report_id: String,
    _started_at_utc: &str,
    state: BenchmarkRunState,
    preflight: &crate::PreflightEvidenceV1,
    aggregator: Aggregator,
    completed_iterations: u16,
    failed_iterations: u16,
    elapsed_millis: u64,
    hardware_snapshot: ResourceTelemetrySnapshotV1,
    coverage_components: Vec<crate::ComponentReadinessV1>,
    unavailable_components: Vec<BenchmarkComponent>,
) -> BenchmarkReportV1 {
    let acceptance_eligible = state == BenchmarkRunState::Completed
        && preflight.execution_mode == EvidenceExecutionMode::Live
        && preflight.measurement_kind == MeasurementKind::Measured
        && preflight.provider_observation_source
            == crate::ProviderObservationSourceV1::ProductRuntimeReceipt
        && cfg!(windows);
    let reason = if acceptance_eligible {
        "Complete live measured Windows run; acceptance still requires scenario and artifact review."
    } else if preflight.measurement_kind == MeasurementKind::PlanningEstimate {
        "Planning estimates are never benchmark evidence."
    } else if preflight.execution_mode != EvidenceExecutionMode::Live {
        "Mocked and simulated measurements are test evidence, not live product evidence."
    } else if preflight.provider_observation_source
        != crate::ProviderObservationSourceV1::ProductRuntimeReceipt
    {
        "Qualification, synthetic, and historical provider observations are never This-PC product evidence."
    } else if state == BenchmarkRunState::Cancelled {
        "The run was cancelled; partial observations are not acceptance evidence."
    } else if state == BenchmarkRunState::Partial {
        "The run is partial because one or more live evidence sources were unavailable or invalid."
    } else if state == BenchmarkRunState::Unavailable {
        "No complete live iteration was available; follow the reported action codes and retry."
    } else if !cfg!(windows) {
        "This-PC acceptance evidence must be captured by the packaged app on Windows."
    } else {
        "This run is not acceptance-eligible."
    };
    let metrics = aggregator.metric_summaries();
    let frame_impact = aggregator.frame_impact();
    BenchmarkReportV1 {
        schema_version: BENCHMARK_REPORT_SCHEMA_V1.into(),
        report_id,
        generated_at_utc: utc_now(),
        state,
        classification: BenchmarkClassificationV1 {
            execution_mode: preflight.execution_mode,
            measurement_kind: preflight.measurement_kind,
            acceptance_eligible,
            reason: reason.into(),
        },
        bounds: RunBoundsV1 {
            requested_iterations: request.requested_iterations,
            completed_iterations,
            timeout_millis: request.timeout_millis,
            baseline_window_millis: request.baseline_window_millis,
            elapsed_millis,
            completed_within_bounds: elapsed_millis <= request.timeout_millis,
        },
        binding: request.binding.clone(),
        hardware: HardwareEvidenceV1::from_snapshot(&hardware_snapshot),
        provenance: ProbeProvenanceV1 {
            runtime_revision: preflight.runtime_revision.clone(),
            broker_revision: preflight.broker_revision.clone(),
            compositor_revision: preflight.compositor_revision.clone(),
            process_load_sampler_revision: preflight.process_load_sampler_revision.clone(),
            game_frame_sampler_revision: preflight.game_frame_sampler_revision.clone(),
            provider_observation_source: preflight.provider_observation_source,
            timing_clock: "monotonic-nanoseconds".into(),
            system_telemetry_schema: hardware_snapshot.schema,
            provider_payloads_recorded: false,
            prompts_recorded: false,
            transcripts_recorded: false,
            audio_recorded: false,
            screenshots_recorded: false,
            credentials_recorded: false,
            file_paths_recorded: false,
        },
        coverage: SourceCoverageV1 {
            components: coverage_components,
            unavailable_components,
            invalid_receipt_count: aggregator.invalid_receipt_count,
            failed_iteration_count: failed_iterations,
        },
        metrics,
        frame_impact,
        persistence: PersistenceEvidenceV1 {
            state: "pending".into(),
            report_file_name: None,
            atomic_write: true,
            report_directory_recorded: false,
        },
    }
}

fn finish_state(
    inner: &ManagerInner,
    report: BenchmarkReportV1,
    persisted_file_name: Option<String>,
) {
    if let Ok(mut state) = inner.state.lock() {
        state.status.state = report.state;
        state.status.completed_iterations = report.bounds.completed_iterations;
        state.status.elapsed_millis = report.bounds.elapsed_millis;
        state.status.report_file_name = persisted_file_name;
        state.status.unavailable_components = report.coverage.unavailable_components.clone();
        state.status.action_codes = action_codes(&report.coverage.components);
        state.cancellation = None;
        state.started = None;
        state.report = Some(report);
    }
}

fn action_codes(components: &[crate::ComponentReadinessV1]) -> Vec<ActionCode> {
    components
        .iter()
        .filter_map(|component| component.action)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn elapsed_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn utc_now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

fn report_id() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{}-{}-{}",
        epoch.as_millis(),
        std::process::id(),
        epoch.subsec_nanos()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AudioSubmissionReceiptV1, BaselineEvidenceV1, CaptureReceiptV1, ComponentReadinessV1,
        CompositorReceiptV1, IterationEvidenceV1, ProcessLoadSampleV1, ProductBindingV1,
        ProviderRole, ProviderRouteBindingV1, RuntimeTurnTimingReceiptV1,
        BENCHMARK_REQUEST_SCHEMA_V1,
    };
    use npc_system_telemetry::{
        AdapterLuid, GraphicsAdapterIdentity, Observation, ObservationProvenance, TelemetrySource,
        RESOURCE_TELEMETRY_SCHEMA_V1,
    };
    use std::{
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
    };

    #[derive(Clone, Copy)]
    enum FixtureMode {
        Complete,
        MissingVisual,
        Unavailable,
        Slow,
    }

    struct FixtureProbe {
        mode: FixtureMode,
        measurements: AtomicUsize,
    }

    impl FixtureProbe {
        fn new(mode: FixtureMode) -> Self {
            Self {
                mode,
                measurements: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl LiveBenchmarkProbe for FixtureProbe {
        async fn preflight(
            &self,
            _request: &BenchmarkRunRequestV1,
            cancellation: &CancellationToken,
        ) -> Result<crate::PreflightEvidenceV1, ProbeError> {
            if matches!(self.mode, FixtureMode::Slow) {
                cancellation.cancelled().await;
                return Err(ProbeError::InternalFailure);
            }
            let mut components = BenchmarkComponent::all()
                .into_iter()
                .map(ComponentReadinessV1::ready)
                .collect::<Vec<_>>();
            if matches!(self.mode, FixtureMode::MissingVisual) {
                set_unavailable(
                    &mut components,
                    BenchmarkComponent::NativeCompositor,
                    AvailabilityReason::VisualPathDisabled,
                    ActionCode::EnableVisualPath,
                );
            }
            if matches!(self.mode, FixtureMode::Unavailable) {
                components = BenchmarkComponent::all()
                    .into_iter()
                    .map(|component| {
                        ComponentReadinessV1::unavailable(
                            component,
                            AvailabilityReason::NoSelectedGameTarget,
                            ActionCode::SelectRunningGame,
                        )
                    })
                    .collect();
            }
            Ok(crate::PreflightEvidenceV1 {
                execution_mode: if cfg!(windows) {
                    EvidenceExecutionMode::Live
                } else {
                    // Tests on non-Windows must remain honest about their path.
                    EvidenceExecutionMode::Mocked
                },
                measurement_kind: MeasurementKind::Measured,
                provider_observation_source: crate::ProviderObservationSourceV1::SyntheticFixture,
                runtime_revision: "runtime-test-1".into(),
                broker_revision: "broker-test-1".into(),
                compositor_revision: "compositor-test-1".into(),
                process_load_sampler_revision: "process-load-test-1".into(),
                game_frame_sampler_revision: "game-frame-test-1".into(),
                components,
            })
        }

        async fn collect_baseline(
            &self,
            _request: &BenchmarkRunRequestV1,
            _cancellation: &CancellationToken,
        ) -> Result<BaselineEvidenceV1, ProbeError> {
            assert!(!matches!(self.mode, FixtureMode::Unavailable));
            Ok(BaselineEvidenceV1 {
                window_started_monotonic_ns: 1_000_000_000,
                window_ended_monotonic_ns: 1_500_000_000,
                frame_samples: (0..4)
                    .map(|index| process_load(1_050_000_000 + index * 100_000_000, 16.0, 62.5))
                    .collect(),
            })
        }

        async fn measure_iteration(
            &self,
            _request: &BenchmarkRunRequestV1,
            iteration: u16,
            _cancellation: &CancellationToken,
        ) -> Result<IterationEvidenceV1, ProbeError> {
            assert!(!matches!(self.mode, FixtureMode::Unavailable));
            self.measurements.fetch_add(1, Ordering::SeqCst);
            Ok(iteration_evidence(
                iteration,
                !matches!(self.mode, FixtureMode::MissingVisual),
            ))
        }
    }

    fn set_unavailable(
        components: &mut [ComponentReadinessV1],
        target: BenchmarkComponent,
        reason: AvailabilityReason,
        action: ActionCode,
    ) {
        let component = components
            .iter_mut()
            .find(|component| component.component == target)
            .expect("component");
        *component = ComponentReadinessV1::unavailable(target, reason, action);
    }

    struct FixtureTelemetry;

    impl ResourceTelemetryCollector for FixtureTelemetry {
        fn collect(&self, selected_game_pid: Option<u32>) -> TelemetryPairV1 {
            TelemetryPairV1 {
                application: snapshot(Some(std::process::id()), 410_000_000, 220_000_000),
                game: snapshot(selected_game_pid, 2_400_000_000, 1_100_000_000),
            }
        }
    }

    fn provenance(source: TelemetrySource) -> ObservationProvenance {
        ObservationProvenance {
            captured_unix_millis: 1_777_777_777_000,
            captured_monotonic_millis: 123,
            source,
        }
    }

    fn available<T>(value: T, source: TelemetrySource) -> Observation<T> {
        Observation::Available {
            value,
            provenance: provenance(source),
        }
    }

    fn snapshot(
        pid: Option<u32>,
        working_set: u64,
        process_vram: u64,
    ) -> ResourceTelemetrySnapshotV1 {
        let adapter = GraphicsAdapterIdentity {
            description: "Review GPU".into(),
            luid: AdapterLuid {
                low_part: 7,
                high_part: 8,
            },
            vendor_id: 0x10de,
            device_id: 0x1234,
            subsystem_id: 9,
            revision: 1,
        };
        ResourceTelemetrySnapshotV1 {
            schema: RESOURCE_TELEMETRY_SCHEMA_V1.into(),
            captured_unix_millis: 1_777_777_777_000,
            captured_monotonic_millis: 123,
            selected_game_pid: pid,
            physical_ram_bytes: available(
                32 * 1024 * 1024 * 1024,
                TelemetrySource::Win32GlobalMemoryStatusEx,
            ),
            available_ram_bytes: available(
                16 * 1024 * 1024 * 1024,
                TelemetrySource::Win32GlobalMemoryStatusEx,
            ),
            adapter: available(adapter, TelemetrySource::DxgiAdapterDescription),
            device_fingerprint_sha256: available(
                "a".repeat(64),
                TelemetrySource::DxgiAdapterDescription,
            ),
            dedicated_vram_bytes: available(
                12 * 1024 * 1024 * 1024,
                TelemetrySource::DxgiAdapterDescription,
            ),
            os_local_vram_budget_bytes: available(
                10 * 1024 * 1024 * 1024,
                TelemetrySource::DxgiProcessVideoMemoryInfo,
            ),
            current_process_local_vram_bytes: available(
                process_vram,
                TelemetrySource::DxgiProcessVideoMemoryInfo,
            ),
            total_device_pressure_vram_bytes: available(
                4 * 1024 * 1024 * 1024,
                TelemetrySource::NvmlDeviceMemoryInfo,
            ),
            selected_game_working_set_bytes: available(
                working_set,
                TelemetrySource::Win32ProcessMemoryInfo,
            ),
            selected_game_vram_bytes: available(
                process_vram,
                TelemetrySource::NvmlRunningProcesses,
            ),
        }
    }

    fn process_load(timestamp: u64, frame_ms: f64, fps: f64) -> ProcessLoadSampleV1 {
        ProcessLoadSampleV1 {
            sampled_monotonic_ns: timestamp,
            process_cpu_percent: Some(12.5),
            device_gpu_percent: Some(31.25),
            game_frame_time_ms: Some(frame_ms),
            game_fps: Some(fps),
            cpu_unavailable_reason: None,
            gpu_unavailable_reason: None,
            game_frame_unavailable_reason: None,
        }
    }

    fn iteration_evidence(iteration: u16, compositor: bool) -> IterationEvidenceV1 {
        let base = u64::from(iteration) * 1_000_000_000;
        let turn_id = format!("turn-{iteration}");
        IterationEvidenceV1 {
            iteration,
            runtime: Some(RuntimeTurnTimingReceiptV1 {
                receipt_id: format!("runtime-{iteration}"),
                turn_id: turn_id.clone(),
                provider_observation_source:
                    crate::ProviderObservationSourceV1::ProductRuntimeReceipt,
                llm_provider_id: "cohere".into(),
                llm_model_id: "command-a-plus-05-2026".into(),
                llm_route_revision: "route-1".into(),
                llm_egress: "provider_cloud:transcript.game_context".into(),
                structured_response_validated: true,
                tts_provider_id: "elevenlabs".into(),
                tts_model_id: "eleven_flash_v2_5".into(),
                tts_voice_id: Some("EXAVITQu4vr4xnSDxMaL".into()),
                tts_route_revision: "route-1".into(),
                tts_egress: "provider_cloud:response_text".into(),
                cancellation_probe_terminal: true,
                input_finalized_monotonic_ns: base + 1_000_000,
                identity_started_monotonic_ns: base + 10_000_000,
                identity_completed_monotonic_ns: base + 20_000_000,
                llm_submitted_monotonic_ns: base + 25_000_000,
                llm_first_token_monotonic_ns: base + 125_000_000,
                llm_completed_monotonic_ns: base + 525_000_000,
                output_tokens: 20,
                tts_submitted_monotonic_ns: base + 130_000_000,
                tts_first_decoded_audio_monotonic_ns: base + 230_000_000,
                tts_final_decoded_audio_monotonic_ns: base + 630_000_000,
                live_provider_receipts: true,
            }),
            capture: Some(CaptureReceiptV1 {
                receipt_id: format!("capture-{iteration}"),
                turn_id: turn_id.clone(),
                requested_monotonic_ns: base + 2_000_000,
                owned_frame_monotonic_ns: base + 8_000_000,
                target_process_matched: true,
                advancing_frame: true,
                protected_content: false,
            }),
            audio: Some(AudioSubmissionReceiptV1 {
                receipt_id: format!("audio-{iteration}"),
                turn_id: turn_id.clone(),
                decoded_audio_ready_monotonic_ns: base + 230_000_000,
                endpoint_submitted_monotonic_ns: base + 240_000_000,
                source_frames: 24_000,
                device_frames: 24_000,
                source_submission_complete: true,
                endpoint_drain_complete: true,
                cancelled: false,
            }),
            compositor: compositor.then(|| CompositorReceiptV1 {
                receipt_id: format!("compositor-{iteration}"),
                turn_id,
                source_frame_monotonic_ns: base + 8_000_000,
                composed_frame_monotonic_ns: base + 18_000_000,
                window_duration_ns: 1_000_000_000,
                frames_presented: 60,
                stale_outputs_dropped: 1,
                target_generation_matched: true,
                frame_presented: true,
            }),
            process_load: process_load(base + 900_000_000, 17.0, 58.8),
        }
    }

    fn request() -> BenchmarkRunRequestV1 {
        BenchmarkRunRequestV1 {
            schema_version: BENCHMARK_REQUEST_SCHEMA_V1.into(),
            requested_iterations: 4,
            timeout_millis: 5_000,
            baseline_window_millis: 500,
            binding: ProductBindingV1 {
                selected_game_pid: Some(42),
                game_profile_id: "synthetic-review-game".into(),
                executable_sha256: "b".repeat(64),
                target_instance_recorded: false,
                loadout_revision: "loadout-revision-7".into(),
                provider_routes: vec![
                    ProviderRouteBindingV1 {
                        role: ProviderRole::Llm,
                        provider_id: "cohere".into(),
                        model_id: "command-a-plus-05-2026".into(),
                        voice_id: None,
                        route_revision: "route-1".into(),
                        egress: "provider_cloud:transcript.game_context".into(),
                        execution_mode: EvidenceExecutionMode::Live,
                    },
                    ProviderRouteBindingV1 {
                        role: ProviderRole::Tts,
                        provider_id: "elevenlabs".into(),
                        model_id: "eleven_flash_v2_5".into(),
                        voice_id: Some("EXAVITQu4vr4xnSDxMaL".into()),
                        route_revision: "route-1".into(),
                        egress: "provider_cloud:response_text".into(),
                        execution_mode: EvidenceExecutionMode::Live,
                    },
                ],
            },
        }
    }

    fn manager(directory: &Path, probe: Arc<dyn LiveBenchmarkProbe>) -> BenchmarkManager {
        BenchmarkManager::new(
            BenchmarkReportStore::new(directory).expect("store"),
            probe,
            Arc::new(FixtureTelemetry),
        )
    }

    async fn wait_terminal(manager: &BenchmarkManager) -> BenchmarkStatusV1 {
        for _ in 0..200 {
            let status = manager.status().expect("status");
            if status.state.terminal() {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("benchmark did not reach a terminal state");
    }

    #[tokio::test]
    async fn complete_run_persists_twenty_receipt_and_resource_metrics() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let manager = manager(
            temporary.path(),
            Arc::new(FixtureProbe::new(FixtureMode::Complete)),
        );
        let started = manager.start(request()).expect("start");
        assert_eq!(started.state, BenchmarkRunState::Running);
        let terminal = wait_terminal(&manager).await;
        assert_eq!(terminal.state, BenchmarkRunState::Completed);
        assert_eq!(terminal.completed_iterations, 4);
        let report = manager
            .report(terminal.report_id.as_deref().expect("report id"))
            .expect("report");
        assert_eq!(report.metrics.len(), 20);
        assert_eq!(report.bounds.completed_iterations, 4);
        assert_eq!(report.coverage.invalid_receipt_count, 0);
        assert_eq!(report.frame_impact.p50_fps_delta, Some(-3.7));
        assert!(report.persistence.atomic_write);
        assert!(!report.provenance.prompts_recorded);
        assert!(!report.provenance.credentials_recorded);
        let persisted = std::fs::read_to_string(
            temporary
                .path()
                .join(terminal.report_file_name.expect("file name")),
        )
        .expect("persisted report");
        assert!(!persisted.contains("selected_game_pid"));
    }

    #[tokio::test]
    async fn missing_visual_receipts_remain_partial_and_actionable() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let manager = manager(
            temporary.path(),
            Arc::new(FixtureProbe::new(FixtureMode::MissingVisual)),
        );
        let started = manager.start(request()).expect("start");
        let terminal = wait_terminal(&manager).await;
        assert_eq!(terminal.state, BenchmarkRunState::Partial);
        assert!(terminal
            .unavailable_components
            .contains(&BenchmarkComponent::NativeCompositor));
        assert!(terminal
            .action_codes
            .contains(&ActionCode::EnableVisualPath));
        let report = manager
            .report(started.report_id.as_deref().expect("report id"))
            .expect("report");
        assert!(!report.classification.acceptance_eligible);
        assert!(!report
            .metrics
            .iter()
            .any(|metric| metric.metric == crate::BenchmarkMetric::CompositorLatencyMs));
    }

    #[tokio::test]
    async fn all_unavailable_preflight_never_invokes_measurement() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let probe = Arc::new(FixtureProbe::new(FixtureMode::Unavailable));
        let manager = manager(temporary.path(), probe.clone());
        manager.start(request()).expect("start");
        let terminal = wait_terminal(&manager).await;
        assert_eq!(terminal.state, BenchmarkRunState::Unavailable);
        assert_eq!(terminal.completed_iterations, 0);
        assert_eq!(probe.measurements.load(Ordering::SeqCst), 0);
        assert!(terminal
            .action_codes
            .contains(&ActionCode::SelectRunningGame));
    }

    #[tokio::test]
    async fn mocked_route_cannot_be_relabelled_by_a_live_preflight() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let probe = Arc::new(FixtureProbe::new(FixtureMode::Complete));
        let manager = manager(temporary.path(), probe.clone());
        let mut request = request();
        request.binding.provider_routes[0].execution_mode = EvidenceExecutionMode::Mocked;
        manager.start(request).expect("start");
        let terminal = wait_terminal(&manager).await;
        assert_eq!(terminal.state, BenchmarkRunState::Unavailable);
        assert_eq!(probe.measurements.load(Ordering::SeqCst), 0);
        let report = manager
            .report(terminal.report_id.as_deref().expect("report id"))
            .expect("report");
        assert!(!report.classification.acceptance_eligible);
        assert_eq!(report.coverage.invalid_receipt_count, 0);
    }

    #[tokio::test]
    async fn cancellation_is_bounded_even_when_probe_waits() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let manager = manager(
            temporary.path(),
            Arc::new(FixtureProbe::new(FixtureMode::Slow)),
        );
        manager.start(request()).expect("start");
        tokio::task::yield_now().await;
        let cancelling = manager.cancel().expect("cancel");
        assert_eq!(cancelling.state, BenchmarkRunState::Cancelling);
        let terminal = wait_terminal(&manager).await;
        assert_eq!(terminal.state, BenchmarkRunState::Cancelled);
        assert!(terminal.report_file_name.is_some());
    }

    #[tokio::test]
    async fn request_bounds_and_concurrent_start_fail_closed() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let manager = manager(
            temporary.path(),
            Arc::new(FixtureProbe::new(FixtureMode::Slow)),
        );
        let mut invalid = request();
        invalid.requested_iterations = 121;
        assert!(matches!(
            manager.start(invalid),
            Err(BenchmarkManagerError::InvalidRequest(_))
        ));
        manager.start(request()).expect("first start");
        assert!(matches!(
            manager.start(request()),
            Err(BenchmarkManagerError::AlreadyRunning)
        ));
        manager.cancel().expect("cancel");
        let _ = wait_terminal(&manager).await;
    }
}
