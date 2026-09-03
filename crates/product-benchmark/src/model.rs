use npc_system_telemetry::{Observation, ResourceTelemetrySnapshotV1};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub const BENCHMARK_REQUEST_SCHEMA_V1: &str = "interactive-npcs-this-pc-benchmark-request/v1";
pub const BENCHMARK_REPORT_SCHEMA_V1: &str = "interactive-npcs-this-pc-benchmark-report/v1";
pub const MIN_ITERATIONS: u16 = 3;
pub const MAX_ITERATIONS: u16 = 120;
pub const MIN_TIMEOUT_MILLIS: u64 = 5_000;
pub const MAX_TIMEOUT_MILLIS: u64 = 300_000;
pub const MIN_BASELINE_MILLIS: u64 = 500;
pub const MAX_BASELINE_MILLIS: u64 = 30_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BenchmarkRunState {
    Idle,
    Running,
    Cancelling,
    Completed,
    Partial,
    Unavailable,
    Cancelled,
    Failed,
}

impl BenchmarkRunState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Partial | Self::Unavailable | Self::Cancelled | Self::Failed
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceExecutionMode {
    Live,
    Mocked,
    Simulated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementKind {
    Measured,
    PlanningEstimate,
}

/// Identifies the producer of provider timing observations. Only receipts
/// emitted by the running product may contribute to an acceptance-eligible
/// This-PC benchmark; qualification artifacts and fixtures remain inspectable
/// but non-promotable.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderObservationSourceV1 {
    ProductRuntimeReceipt,
    QualificationArtifact,
    SyntheticFixture,
    HistoricalObservation,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkComponent {
    SelectedGameTarget,
    AdvancingCapture,
    IdentityTiming,
    LiveLlmProvider,
    LiveTtsProvider,
    BrokerAudioSubmission,
    NativeCompositor,
    ProcessCpuSampler,
    DeviceGpuSampler,
    SystemRamTelemetry,
    ProcessVramTelemetry,
    GameFrameSampler,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvailabilityReason {
    NoSelectedGameTarget,
    TargetNotAdvancing,
    ProtectedOrPolicyBlockedTarget,
    NoLiveProviderRoute,
    ProviderNotQualified,
    RuntimeTimingReceiptUnavailable,
    CaptureReceiptUnavailable,
    AudioReceiptUnavailable,
    CompositorReceiptUnavailable,
    VisualPathDisabled,
    OperatingSystemMetricUnavailable,
    DriverMetricUnavailable,
    PermissionDenied,
    UnsupportedPlatform,
    CounterDiscontinuity,
    InvalidReceipt,
    TimedOut,
    Cancelled,
    InternalFailure,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionCode {
    SelectRunningGame,
    WaitForAdvancingFrame,
    ChooseQualifiedLiveLlm,
    ChooseQualifiedLiveTts,
    EnableAudioOutput,
    StartMediaBroker,
    EnableVisualPath,
    UpdateGraphicsDriver,
    GrantProcessMetricPermission,
    CloseCompetingWorkloads,
    RetryBenchmark,
    ReviewDiagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementAvailability {
    Ready,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentReadinessV1 {
    pub component: BenchmarkComponent,
    pub availability: MeasurementAvailability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<AvailabilityReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<ActionCode>,
}

impl ComponentReadinessV1 {
    pub fn ready(component: BenchmarkComponent) -> Self {
        Self {
            component,
            availability: MeasurementAvailability::Ready,
            reason: None,
            action: None,
        }
    }

    pub fn unavailable(
        component: BenchmarkComponent,
        reason: AvailabilityReason,
        action: ActionCode,
    ) -> Self {
        Self {
            component,
            availability: MeasurementAvailability::Unavailable,
            reason: Some(reason),
            action: Some(action),
        }
    }

    pub(crate) fn valid(&self) -> bool {
        match self.availability {
            MeasurementAvailability::Ready => self.reason.is_none() && self.action.is_none(),
            MeasurementAvailability::Unavailable => self.reason.is_some() && self.action.is_some(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRole {
    Stt,
    Llm,
    Tts,
    Embedding,
    VisualSignal,
    MouthAnimation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderRouteBindingV1 {
    pub role: ProviderRole,
    pub provider_id: String,
    pub model_id: String,
    pub voice_id: Option<String>,
    pub route_revision: String,
    pub egress: String,
    pub execution_mode: EvidenceExecutionMode,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProductBindingV1 {
    /// Used to collect live process telemetry; deliberately omitted from reports.
    #[serde(skip_serializing)]
    pub selected_game_pid: Option<u32>,
    pub game_profile_id: String,
    pub executable_sha256: String,
    pub target_instance_recorded: bool,
    pub loadout_revision: String,
    pub provider_routes: Vec<ProviderRouteBindingV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BenchmarkRunRequestV1 {
    pub schema_version: String,
    pub requested_iterations: u16,
    pub timeout_millis: u64,
    pub baseline_window_millis: u64,
    pub binding: ProductBindingV1,
}

impl BenchmarkRunRequestV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != BENCHMARK_REQUEST_SCHEMA_V1 {
            return Err("unsupported request schema");
        }
        if !(MIN_ITERATIONS..=MAX_ITERATIONS).contains(&self.requested_iterations) {
            return Err("iteration count is outside the bounded range");
        }
        if !(MIN_TIMEOUT_MILLIS..=MAX_TIMEOUT_MILLIS).contains(&self.timeout_millis) {
            return Err("timeout is outside the bounded range");
        }
        if !(MIN_BASELINE_MILLIS..=MAX_BASELINE_MILLIS).contains(&self.baseline_window_millis)
            || self.baseline_window_millis >= self.timeout_millis
        {
            return Err("baseline window is outside the bounded range");
        }
        validate_safe_id(&self.binding.game_profile_id, 96)?;
        validate_sha256(&self.binding.executable_sha256)?;
        validate_safe_id(&self.binding.loadout_revision, 96)?;
        if self.binding.target_instance_recorded {
            return Err("ephemeral target identifiers must not be persisted");
        }
        if self.binding.provider_routes.len() > 12 {
            return Err("too many provider routes");
        }
        let mut roles = BTreeSet::new();
        for route in &self.binding.provider_routes {
            if !roles.insert(route.role) {
                return Err("provider roles must be unique");
            }
            validate_public_id(&route.provider_id, 96)?;
            validate_public_id(&route.model_id, 96)?;
            if let Some(voice_id) = route.voice_id.as_deref() {
                validate_public_id(voice_id, 128)?;
            }
            if (route.role == ProviderRole::Tts) != route.voice_id.is_some() {
                return Err("only TTS routes must bind one exact selected voice");
            }
            validate_public_id(&route.route_revision, 96)?;
            validate_public_id(&route.egress, 192)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreflightEvidenceV1 {
    pub execution_mode: EvidenceExecutionMode,
    pub measurement_kind: MeasurementKind,
    pub provider_observation_source: ProviderObservationSourceV1,
    pub runtime_revision: String,
    pub broker_revision: String,
    pub compositor_revision: String,
    pub process_load_sampler_revision: String,
    pub game_frame_sampler_revision: String,
    pub components: Vec<ComponentReadinessV1>,
}

impl PreflightEvidenceV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_safe_id(&self.runtime_revision, 96)?;
        validate_safe_id(&self.broker_revision, 96)?;
        validate_safe_id(&self.compositor_revision, 96)?;
        validate_safe_id(&self.process_load_sampler_revision, 96)?;
        validate_safe_id(&self.game_frame_sampler_revision, 96)?;
        if self.components.len() != 12 {
            return Err("preflight must describe every benchmark component exactly once");
        }
        let mut components = BTreeSet::new();
        for component in &self.components {
            if !component.valid() || !components.insert(component.component) {
                return Err("preflight component coverage is invalid");
            }
        }
        if components != BenchmarkComponent::all().into_iter().collect() {
            return Err("preflight component coverage is incomplete");
        }
        Ok(())
    }
}

impl BenchmarkComponent {
    pub const fn all() -> [Self; 12] {
        [
            Self::SelectedGameTarget,
            Self::AdvancingCapture,
            Self::IdentityTiming,
            Self::LiveLlmProvider,
            Self::LiveTtsProvider,
            Self::BrokerAudioSubmission,
            Self::NativeCompositor,
            Self::ProcessCpuSampler,
            Self::DeviceGpuSampler,
            Self::SystemRamTelemetry,
            Self::ProcessVramTelemetry,
            Self::GameFrameSampler,
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkMetric {
    CaptureLatencyMs,
    IdentityLatencyMs,
    LlmTimeToFirstTokenMs,
    LlmOutputTokens,
    LlmTokensPerSecond,
    TtsTimeToFirstAudioMs,
    TtsAudioCompletionMs,
    AudioSubmissionMs,
    CompositorLatencyMs,
    CompositorFps,
    CompositorStaleDrops,
    EndToEndTurnMs,
    ProcessCpuPercent,
    DeviceGpuPercent,
    ProcessRamBytes,
    ProcessVramBytes,
    GameRamBytes,
    GameVramBytes,
    GameFrameTimeMs,
    GameFps,
}

impl BenchmarkMetric {
    pub const fn all() -> [Self; 20] {
        [
            Self::CaptureLatencyMs,
            Self::IdentityLatencyMs,
            Self::LlmTimeToFirstTokenMs,
            Self::LlmOutputTokens,
            Self::LlmTokensPerSecond,
            Self::TtsTimeToFirstAudioMs,
            Self::TtsAudioCompletionMs,
            Self::AudioSubmissionMs,
            Self::CompositorLatencyMs,
            Self::CompositorFps,
            Self::CompositorStaleDrops,
            Self::EndToEndTurnMs,
            Self::ProcessCpuPercent,
            Self::DeviceGpuPercent,
            Self::ProcessRamBytes,
            Self::ProcessVramBytes,
            Self::GameRamBytes,
            Self::GameVramBytes,
            Self::GameFrameTimeMs,
            Self::GameFps,
        ]
    }

    pub const fn unit(self) -> &'static str {
        match self {
            Self::CaptureLatencyMs
            | Self::IdentityLatencyMs
            | Self::LlmTimeToFirstTokenMs
            | Self::TtsTimeToFirstAudioMs
            | Self::TtsAudioCompletionMs
            | Self::AudioSubmissionMs
            | Self::CompositorLatencyMs
            | Self::EndToEndTurnMs
            | Self::GameFrameTimeMs => "ms",
            Self::LlmOutputTokens => "tokens",
            Self::LlmTokensPerSecond => "tokens_per_second",
            Self::CompositorFps | Self::GameFps => "frames_per_second",
            Self::CompositorStaleDrops => "count",
            Self::ProcessCpuPercent | Self::DeviceGpuPercent => "percent",
            Self::ProcessRamBytes
            | Self::ProcessVramBytes
            | Self::GameRamBytes
            | Self::GameVramBytes => "bytes",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricSummaryV1 {
    pub metric: BenchmarkMetric,
    pub unit: String,
    pub sample_count: usize,
    pub minimum: f64,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub maximum: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrameImpactV1 {
    pub baseline_frame_time_ms: Option<MetricSummaryV1>,
    pub active_frame_time_ms: Option<MetricSummaryV1>,
    pub baseline_fps: Option<MetricSummaryV1>,
    pub active_fps: Option<MetricSummaryV1>,
    pub p50_frame_time_delta_ms: Option<f64>,
    pub p95_frame_time_delta_ms: Option<f64>,
    pub p50_fps_delta: Option<f64>,
    pub p50_fps_impact_percent: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HardwareEvidenceV1 {
    pub operating_system: String,
    pub architecture: String,
    pub logical_processor_count: usize,
    pub adapter_description: Option<String>,
    pub adapter_fingerprint_sha256: Option<String>,
    pub physical_ram_bytes: Option<u64>,
    pub dedicated_vram_bytes: Option<u64>,
    pub hostname_recorded: bool,
    pub environment_variables_recorded: bool,
}

impl HardwareEvidenceV1 {
    pub(crate) fn from_snapshot(snapshot: &ResourceTelemetrySnapshotV1) -> Self {
        let adapter_description = snapshot
            .adapter
            .value()
            .map(|adapter| bounded_label(&adapter.description, 128));
        Self {
            operating_system: std::env::consts::OS.to_owned(),
            architecture: std::env::consts::ARCH.to_owned(),
            logical_processor_count: std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1),
            adapter_description,
            adapter_fingerprint_sha256: observation_value(&snapshot.device_fingerprint_sha256),
            physical_ram_bytes: observation_value(&snapshot.physical_ram_bytes),
            dedicated_vram_bytes: observation_value(&snapshot.dedicated_vram_bytes),
            hostname_recorded: false,
            environment_variables_recorded: false,
        }
    }
}

fn observation_value<T: Clone>(observation: &Observation<T>) -> Option<T> {
    observation.value().cloned()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeProvenanceV1 {
    pub runtime_revision: String,
    pub broker_revision: String,
    pub compositor_revision: String,
    pub process_load_sampler_revision: String,
    pub game_frame_sampler_revision: String,
    pub provider_observation_source: ProviderObservationSourceV1,
    pub timing_clock: String,
    pub system_telemetry_schema: String,
    pub provider_payloads_recorded: bool,
    pub prompts_recorded: bool,
    pub transcripts_recorded: bool,
    pub audio_recorded: bool,
    pub screenshots_recorded: bool,
    pub credentials_recorded: bool,
    pub file_paths_recorded: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BenchmarkClassificationV1 {
    pub execution_mode: EvidenceExecutionMode,
    pub measurement_kind: MeasurementKind,
    pub acceptance_eligible: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunBoundsV1 {
    pub requested_iterations: u16,
    pub completed_iterations: u16,
    pub timeout_millis: u64,
    pub baseline_window_millis: u64,
    pub elapsed_millis: u64,
    pub completed_within_bounds: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceCoverageV1 {
    pub components: Vec<ComponentReadinessV1>,
    pub unavailable_components: Vec<BenchmarkComponent>,
    pub invalid_receipt_count: u32,
    pub failed_iteration_count: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistenceEvidenceV1 {
    pub state: String,
    pub report_file_name: Option<String>,
    pub atomic_write: bool,
    pub report_directory_recorded: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BenchmarkReportV1 {
    pub schema_version: String,
    pub report_id: String,
    pub generated_at_utc: String,
    pub state: BenchmarkRunState,
    pub classification: BenchmarkClassificationV1,
    pub bounds: RunBoundsV1,
    pub binding: ProductBindingV1,
    pub hardware: HardwareEvidenceV1,
    pub provenance: ProbeProvenanceV1,
    pub coverage: SourceCoverageV1,
    pub metrics: Vec<MetricSummaryV1>,
    pub frame_impact: FrameImpactV1,
    pub persistence: PersistenceEvidenceV1,
}

impl BenchmarkReportV1 {
    /// Validate both report shape and claim semantics before persistence or
    /// consumption. This is intentionally stricter than serde deserialization.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != BENCHMARK_REPORT_SCHEMA_V1 {
            return Err("unsupported report schema");
        }
        validate_safe_id(&self.report_id, 96)?;
        OffsetDateTime::parse(&self.generated_at_utc, &Rfc3339)
            .map_err(|_| "report timestamp is invalid")?;
        if !self.state.terminal() {
            return Err("persisted report state must be terminal");
        }
        if self.classification.reason.is_empty()
            || self.classification.reason.len() > 240
            || self.classification.reason.chars().any(char::is_control)
        {
            return Err("classification reason is invalid");
        }
        if self.classification.acceptance_eligible
            && (self.state != BenchmarkRunState::Completed
                || self.classification.execution_mode != EvidenceExecutionMode::Live
                || self.classification.measurement_kind != MeasurementKind::Measured
                || self.provenance.provider_observation_source
                    != ProviderObservationSourceV1::ProductRuntimeReceipt
                || !cfg!(windows))
        {
            return Err("acceptance eligibility conflicts with report evidence");
        }
        BenchmarkRunRequestV1 {
            schema_version: BENCHMARK_REQUEST_SCHEMA_V1.into(),
            requested_iterations: self.bounds.requested_iterations,
            timeout_millis: self.bounds.timeout_millis,
            baseline_window_millis: self.bounds.baseline_window_millis,
            binding: self.binding.clone(),
        }
        .validate()?;
        if self.bounds.completed_iterations > self.bounds.requested_iterations
            || self.bounds.completed_within_bounds
                != (self.bounds.elapsed_millis <= self.bounds.timeout_millis)
        {
            return Err("report bounds are inconsistent");
        }
        if self.hardware.logical_processor_count == 0
            || self.hardware.hostname_recorded
            || self.hardware.environment_variables_recorded
            || self.hardware.operating_system.is_empty()
            || self.hardware.operating_system.len() > 64
            || self.hardware.architecture.is_empty()
            || self.hardware.architecture.len() > 64
            || self
                .hardware
                .adapter_description
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > 128)
            || self
                .hardware
                .adapter_fingerprint_sha256
                .as_deref()
                .is_some_and(|value| validate_sha256(value).is_err())
        {
            return Err("hardware evidence is invalid");
        }
        validate_safe_id(&self.provenance.runtime_revision, 96)?;
        validate_safe_id(&self.provenance.broker_revision, 96)?;
        validate_safe_id(&self.provenance.compositor_revision, 96)?;
        validate_safe_id(&self.provenance.process_load_sampler_revision, 96)?;
        validate_safe_id(&self.provenance.game_frame_sampler_revision, 96)?;
        if self.provenance.timing_clock != "monotonic-nanoseconds"
            || self.provenance.system_telemetry_schema
                != "npc.system-telemetry/resource-snapshot-v1"
            || self.provenance.provider_payloads_recorded
            || self.provenance.prompts_recorded
            || self.provenance.transcripts_recorded
            || self.provenance.audio_recorded
            || self.provenance.screenshots_recorded
            || self.provenance.credentials_recorded
            || self.provenance.file_paths_recorded
        {
            return Err("report provenance violates the redacted contract");
        }
        validate_coverage(&self.coverage)?;
        validate_metrics(&self.metrics, self.bounds.completed_iterations)?;
        if self.state == BenchmarkRunState::Completed
            && (self.metrics.len() != BenchmarkMetric::all().len()
                || self.frame_impact.baseline_frame_time_ms.is_none()
                || self.frame_impact.active_frame_time_ms.is_none()
                || self.frame_impact.baseline_fps.is_none()
                || self.frame_impact.active_fps.is_none())
        {
            return Err("complete report is missing required evidence");
        }
        if self.persistence.report_directory_recorded || !self.persistence.atomic_write {
            return Err("report persistence metadata is invalid");
        }
        match (
            self.persistence.state.as_str(),
            self.persistence.report_file_name.as_deref(),
        ) {
            ("persisted", Some(file_name))
                if file_name == format!("this-pc-benchmark-{}.json", self.report_id) => {}
            ("failed", None) => {}
            _ => return Err("report persistence state is inconsistent"),
        }
        Ok(())
    }
}

fn validate_coverage(coverage: &SourceCoverageV1) -> Result<(), &'static str> {
    if coverage.components.len() != BenchmarkComponent::all().len() {
        return Err("report component coverage is incomplete");
    }
    let mut seen = BTreeSet::new();
    let mut unavailable = BTreeSet::new();
    for component in &coverage.components {
        if !component.valid() || !seen.insert(component.component) {
            return Err("report component coverage is invalid");
        }
        if component.availability == MeasurementAvailability::Unavailable {
            unavailable.insert(component.component);
        }
    }
    if seen != BenchmarkComponent::all().into_iter().collect()
        || unavailable != coverage.unavailable_components.iter().copied().collect()
    {
        return Err("report unavailable component index is inconsistent");
    }
    Ok(())
}

fn validate_metrics(
    metrics: &[MetricSummaryV1],
    completed_iterations: u16,
) -> Result<(), &'static str> {
    if metrics.len() > BenchmarkMetric::all().len() {
        return Err("report contains too many metrics");
    }
    let mut seen = BTreeSet::new();
    for metric in metrics {
        if !seen.insert(metric.metric)
            || metric.unit != metric.metric.unit()
            || metric.sample_count == 0
            || metric.sample_count > usize::from(completed_iterations)
            || [
                metric.minimum,
                metric.mean,
                metric.p50,
                metric.p95,
                metric.p99,
                metric.maximum,
            ]
            .into_iter()
            .any(|value| !value.is_finite() || value < 0.0)
            || metric.minimum > metric.mean
            || metric.mean > metric.maximum
            || metric.minimum > metric.p50
            || metric.p50 > metric.p95
            || metric.p95 > metric.p99
            || metric.p99 > metric.maximum
        {
            return Err("report metric summary is invalid");
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BenchmarkStatusV1 {
    pub state: BenchmarkRunState,
    pub report_id: Option<String>,
    pub requested_iterations: u16,
    pub completed_iterations: u16,
    pub started_at_utc: Option<String>,
    pub elapsed_millis: u64,
    pub report_file_name: Option<String>,
    pub unavailable_components: Vec<BenchmarkComponent>,
    pub action_codes: Vec<ActionCode>,
}

impl BenchmarkStatusV1 {
    pub(crate) fn idle() -> Self {
        Self {
            state: BenchmarkRunState::Idle,
            report_id: None,
            requested_iterations: 0,
            completed_iterations: 0,
            started_at_utc: None,
            elapsed_millis: 0,
            report_file_name: None,
            unavailable_components: Vec::new(),
            action_codes: Vec::new(),
        }
    }
}

pub(crate) fn validate_safe_id(value: &str, maximum: usize) -> Result<(), &'static str> {
    if value.is_empty() || value.len() > maximum {
        return Err("identifier length is invalid");
    }
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return Err("identifier is empty");
    };
    if !first.is_ascii_alphanumeric() {
        return Err("identifier prefix is invalid");
    }
    if bytes
        .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-')))
    {
        return Err("identifier contains unsupported characters");
    }
    Ok(())
}

pub(crate) fn validate_public_id(value: &str, maximum: usize) -> Result<(), &'static str> {
    validate_safe_id(value, maximum)?;
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("sk-")
        || lower.starts_with("nvapi-")
        || lower.starts_with("ghp_")
        || lower.starts_with("github_pat_")
        || lower.starts_with("eyj")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("bearer")
        || lower.contains("password")
        || lower.contains("credential")
    {
        return Err("provider binding resembles a credential rather than a public identifier");
    }
    Ok(())
}

pub(crate) fn validate_sha256(value: &str) -> Result<(), &'static str> {
    if value.len() != 64
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    {
        return Err("SHA-256 must be lowercase hexadecimal");
    }
    Ok(())
}

fn bounded_label(value: &str, maximum: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(maximum)
        .collect::<String>()
}
