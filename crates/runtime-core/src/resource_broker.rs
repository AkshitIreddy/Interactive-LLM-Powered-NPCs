//! Resource admission, pressure management, and GPU residency ownership.
//!
//! The broker is deliberately platform-neutral. The Windows runtime reports DXGI,
//! NVML, and process budget values through [`BudgetTelemetry`]; this module makes all
//! scheduling decisions deterministically and performs no hardware or network I/O.

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex, Weak},
};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::types::{ExecutionMode, NetworkPolicy, ProviderDescriptor};

const MIB: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPriority {
    Opportunistic,
    Background,
    Interactive,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceJobKind {
    ForegroundLlm {
        utterance_id: String,
    },
    SpeechSynthesis {
        utterance_id: String,
        sentence_id: u64,
    },
    Embedding,
    ContinuousVision {
        frame_sequence: u64,
        captured_at_ms: u64,
    },
    ScreenSpaceLipSync {
        frame_sequence: u64,
        captured_at_ms: u64,
    },
    NativeRigAnimation {
        utterance_id: String,
    },
    ModelLoad,
    Maintenance,
}

impl ResourceJobKind {
    fn is_foreground_llm(&self) -> bool {
        matches!(self, Self::ForegroundLlm { .. })
    }

    fn utterance_id(&self) -> Option<&str> {
        match self {
            Self::ForegroundLlm { utterance_id }
            | Self::SpeechSynthesis { utterance_id, .. }
            | Self::NativeRigAnimation { utterance_id } => Some(utterance_id),
            _ => None,
        }
    }

    fn captured_at_ms(&self) -> Option<u64> {
        match self {
            Self::ContinuousVision { captured_at_ms, .. }
            | Self::ScreenSpaceLipSync { captured_at_ms, .. } => Some(*captured_at_ms),
            _ => None,
        }
    }

    fn is_visual_burst(&self) -> bool {
        matches!(
            self,
            Self::ContinuousVision { .. } | Self::ScreenSpaceLipSync { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceEstimate {
    pub ram_bytes: u64,
    pub vram_resident_bytes: u64,
    /// Temporary workspace allocated above resident model memory.
    pub vram_transient_bytes: u64,
    pub transient_duration_ms: u64,
    pub gpu_time_ms: u32,
}

impl ResourceEstimate {
    fn total_vram(self) -> u64 {
        self.vram_resident_bytes
            .saturating_add(self.vram_transient_bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuResidency {
    None,
    Shared,
    /// No other GPU-resident job may overlap this lease.
    Exclusive,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputeTarget {
    pub target_id: String,
    pub model_id: Option<String>,
    pub provider: ProviderDescriptor,
    pub estimate: ResourceEstimate,
    pub gpu_residency: GpuResidency,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum DropPolicy {
    Never,
    DropIfLate,
    DropIfStale {
        maximum_age_ms: u64,
    },
    DropUnderPressure,
    /// Keep only the newest queued job with this coalescing key.
    Supersede(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerPrivacyContext {
    pub execution_mode: ExecutionMode,
    pub network_policy: NetworkPolicy,
    #[serde(default)]
    pub authorized_cloud_providers: HashSet<String>,
    pub allow_retaining_providers: bool,
    pub allow_local_to_cloud_fallback: bool,
}

impl BrokerPrivacyContext {
    pub fn fully_local() -> Self {
        Self {
            execution_mode: ExecutionMode::FullyLocal,
            network_policy: NetworkPolicy::Offline,
            authorized_cloud_providers: HashSet::new(),
            allow_retaining_providers: false,
            allow_local_to_cloud_fallback: false,
        }
    }

    pub fn allows(&self, provider: &ProviderDescriptor) -> bool {
        if provider.may_retain_data && !self.allow_retaining_providers {
            return false;
        }
        if !provider.location.is_networked() {
            return true;
        }
        self.network_policy == NetworkPolicy::Online
            && self.execution_mode != ExecutionMode::FullyLocal
            && self.authorized_cloud_providers.contains(&provider.id)
    }

    pub fn allows_fallback(
        &self,
        primary: &ProviderDescriptor,
        fallback: &ProviderDescriptor,
    ) -> bool {
        self.allows(fallback)
            && (!primary.location.is_local()
                || fallback.location.is_local()
                || self.allow_local_to_cloud_fallback)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceJob {
    pub job_id: String,
    pub kind: ResourceJobKind,
    pub priority: JobPriority,
    /// Host-monotonic deadline. `None` means the job remains useful until cancelled.
    pub deadline_ms: Option<u64>,
    pub primary: ComputeTarget,
    #[serde(default)]
    pub fallbacks: Vec<ComputeTarget>,
    pub drop_policy: DropPolicy,
    pub privacy: BrokerPrivacyContext,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BudgetSnapshot {
    pub monotonic_ms: u64,
    pub system_ram_budget_bytes: u64,
    pub system_ram_used_bytes: u64,
    pub gpu_vram_budget_bytes: u64,
    pub gpu_vram_used_bytes: u64,
    pub gpu_utilization: f32,
}

impl BudgetSnapshot {
    pub fn validate(self) -> Result<Self, BrokerError> {
        if self.system_ram_budget_bytes == 0 || self.gpu_vram_budget_bytes == 0 {
            return Err(BrokerError::InvalidTelemetry(
                "RAM and VRAM budgets must be non-zero".into(),
            ));
        }
        if !self.gpu_utilization.is_finite() || !(0.0..=1.0).contains(&self.gpu_utilization) {
            return Err(BrokerError::InvalidTelemetry(
                "GPU utilization must be finite and within 0..=1".into(),
            ));
        }
        Ok(self)
    }
}

pub trait BudgetTelemetry: Send + Sync {
    fn snapshot(&self) -> Result<BudgetSnapshot, BrokerError>;
}

/// Thread-safe host-fed telemetry implementation. The media/runtime host updates it
/// after reading DXGI/NVML; the broker only consumes the last validated sample.
#[derive(Clone, Debug)]
pub struct ReportedBudgetTelemetry {
    snapshot: Arc<Mutex<BudgetSnapshot>>,
}

impl ReportedBudgetTelemetry {
    pub fn new(snapshot: BudgetSnapshot) -> Result<Self, BrokerError> {
        Ok(Self {
            snapshot: Arc::new(Mutex::new(snapshot.validate()?)),
        })
    }

    pub fn update(&self, snapshot: BudgetSnapshot) -> Result<(), BrokerError> {
        let snapshot = snapshot.validate()?;
        let mut current = self.snapshot.lock().expect("telemetry mutex poisoned");
        if snapshot.monotonic_ms < current.monotonic_ms {
            return Err(BrokerError::InvalidTelemetry(
                "telemetry time moved backwards".into(),
            ));
        }
        *current = snapshot;
        Ok(())
    }
}

impl BudgetTelemetry for ReportedBudgetTelemetry {
    fn snapshot(&self) -> Result<BudgetSnapshot, BrokerError> {
        Ok(*self.snapshot.lock().expect("telemetry mutex poisoned"))
    }
}

#[derive(Clone, Debug)]
pub struct ResourceBrokerConfig {
    pub vram_ceiling_bytes: u64,
    pub transient_vram_allowance_bytes: u64,
    pub transient_vram_grace_ms: u64,
    pub amber_enter_ratio: f64,
    pub green_reenter_ratio: f64,
    pub red_enter_ratio: f64,
    pub amber_reenter_ratio: f64,
    pub recovery_green_samples: u32,
    pub default_vision_stale_ms: u64,
    pub oom_failures_before_quarantine: u32,
    pub quarantine_ms: u64,
    pub diagnostic_capacity: usize,
}

impl Default for ResourceBrokerConfig {
    fn default() -> Self {
        Self {
            vram_ceiling_bytes: 10 * 1024 * MIB,
            transient_vram_allowance_bytes: 256 * MIB,
            transient_vram_grace_ms: 2_000,
            amber_enter_ratio: 0.75,
            green_reenter_ratio: 0.65,
            red_enter_ratio: 0.90,
            amber_reenter_ratio: 0.82,
            recovery_green_samples: 3,
            default_vision_stale_ms: 100,
            oom_failures_before_quarantine: 2,
            quarantine_ms: 60_000,
            diagnostic_capacity: 2_048,
        }
    }
}

impl ResourceBrokerConfig {
    fn validate(&self) {
        assert!(self.vram_ceiling_bytes > 0);
        assert!(self.transient_vram_grace_ms > 0);
        assert!(self.green_reenter_ratio < self.amber_enter_ratio);
        assert!(self.amber_enter_ratio < self.amber_reenter_ratio);
        assert!(self.amber_reenter_ratio < self.red_enter_ratio);
        assert!(self.red_enter_ratio <= 1.0);
        assert!(self.recovery_green_samples > 0);
        assert!(self.oom_failures_before_quarantine > 0);
        assert!(self.diagnostic_capacity > 0);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PressureLevel {
    #[default]
    Green,
    Amber,
    Red,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerDegradation {
    ReduceContinuousVision,
    DisableScreenSpaceLipSync,
    NeutralizeOptionalEffects,
    DisableVectorRetrieval,
    AudioAndSubtitlesOnly,
    TypedInputOrSubtitles,
    RetryableLlmFailure,
}

pub const DEGRADATION_ORDER: [BrokerDegradation; 7] = [
    BrokerDegradation::ReduceContinuousVision,
    BrokerDegradation::DisableScreenSpaceLipSync,
    BrokerDegradation::NeutralizeOptionalEffects,
    BrokerDegradation::DisableVectorRetrieval,
    BrokerDegradation::AudioAndSubtitlesOnly,
    BrokerDegradation::TypedInputOrSubtitles,
    BrokerDegradation::RetryableLlmFailure,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionOutcome {
    AdmittedPrimary,
    AdmittedFallback,
    Queued,
    DroppedStale,
    DroppedLate,
    DroppedPressure,
    RejectedPrivacy,
    RejectedQuarantined,
    RejectedNoCapacity,
    RejectedDuplicate,
    RejectedUtteranceBinding,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResourceProjection {
    pub ram_bytes: u64,
    pub vram_bytes: u64,
    pub vram_ceiling_bytes: u64,
    pub transient_overshoot_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdmissionDiagnostic {
    pub monotonic_ms: u64,
    pub job_id: String,
    pub kind: ResourceJobKind,
    pub priority: JobPriority,
    pub outcome: AdmissionOutcome,
    pub selected_target_id: Option<String>,
    pub pressure: PressureLevel,
    pub active_degradations: Vec<BrokerDegradation>,
    pub projection: ResourceProjection,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionStatus {
    Completed,
    Failed,
    Cancelled,
    OutOfMemory,
    DeadlineMissed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActualUsage {
    pub peak_ram_bytes: u64,
    pub peak_vram_bytes: u64,
    pub gpu_time_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActualDiagnostic {
    pub monotonic_ms: u64,
    pub job_id: String,
    pub target_id: String,
    pub estimated: ResourceEstimate,
    pub actual: ActualUsage,
    pub status: CompletionStatus,
    pub deadline_missed: bool,
    pub cancelled_by_broker: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrokerDiagnostic {
    PressureChanged {
        monotonic_ms: u64,
        from: PressureLevel,
        to: PressureLevel,
        utilization_ratio: f64,
    },
    DegradationChanged {
        monotonic_ms: u64,
        active: Vec<BrokerDegradation>,
        reason: String,
    },
    Admission(AdmissionDiagnostic),
    Actual(ActualDiagnostic),
    TargetQuarantined {
        monotonic_ms: u64,
        target_id: String,
        until_ms: u64,
        oom_failures: u32,
    },
    CancellationRequested {
        monotonic_ms: u64,
        job_id: String,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryDirective {
    RetrySameTargetAfterEviction { target_id: String },
    RetryFallback { target_id: String },
    AbortUtteranceNoModelSwap,
    QuarantinedNoSafeFallback,
    JobNotActive,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum BrokerError {
    #[error("invalid telemetry: {0}")]
    InvalidTelemetry(String),
    #[error("job is invalid: {0}")]
    InvalidJob(String),
}

#[derive(Debug)]
pub struct Submission {
    pub diagnostic: AdmissionDiagnostic,
    pub lease: Option<ResourceLease>,
}

#[derive(Clone, Debug, Default)]
struct Reservations {
    ram_bytes: u64,
    vram_resident_bytes: u64,
    vram_transient_bytes: u64,
    gpu_leases: u32,
    exclusive_gpu_job: Option<String>,
    foreground_llm_job: Option<String>,
}

#[derive(Clone)]
struct ActiveRecord {
    job: ResourceJob,
    target: ComputeTarget,
    cancellation: CancellationToken,
    output_started: bool,
    cancelled_by_broker: bool,
    transient_deadline_ms: Option<u64>,
}

#[derive(Clone, Debug, Default)]
struct TargetHealth {
    oom_failures: u32,
    quarantine_until_ms: u64,
}

#[derive(Clone)]
struct QueuedRecord {
    sequence: u64,
    job: ResourceJob,
}

struct BrokerState {
    pressure: PressureLevel,
    green_samples: u32,
    degradation_count: usize,
    sequence: u64,
    reservations: Reservations,
    active: HashMap<String, ActiveRecord>,
    queued: Vec<QueuedRecord>,
    target_health: HashMap<String, TargetHealth>,
    utterance_bindings: HashMap<String, String>,
    diagnostics: VecDeque<BrokerDiagnostic>,
}

impl Default for BrokerState {
    fn default() -> Self {
        Self {
            pressure: PressureLevel::Green,
            green_samples: 0,
            degradation_count: 0,
            sequence: 0,
            reservations: Reservations::default(),
            active: HashMap::new(),
            queued: Vec::new(),
            target_health: HashMap::new(),
            utterance_bindings: HashMap::new(),
            diagnostics: VecDeque::new(),
        }
    }
}

struct ResourceBrokerInner {
    config: ResourceBrokerConfig,
    telemetry: Arc<dyn BudgetTelemetry>,
    state: Mutex<BrokerState>,
}

#[derive(Clone)]
pub struct ResourceBroker {
    inner: Arc<ResourceBrokerInner>,
}

impl ResourceBroker {
    pub fn new(config: ResourceBrokerConfig, telemetry: Arc<dyn BudgetTelemetry>) -> Self {
        config.validate();
        Self {
            inner: Arc::new(ResourceBrokerInner {
                config,
                telemetry,
                state: Mutex::new(BrokerState::default()),
            }),
        }
    }

    pub fn submit(&self, job: ResourceJob) -> Result<Submission, BrokerError> {
        validate_job(&job)?;
        let snapshot = self.inner.telemetry.snapshot()?.validate()?;
        let mut state = self.inner.state.lock().expect("broker mutex poisoned");
        self.expire_transient_overages_locked(&mut state, snapshot.monotonic_ms);
        self.update_pressure_locked(&mut state, snapshot);

        if state.active.contains_key(&job.job_id)
            || state
                .queued
                .iter()
                .any(|queued| queued.job.job_id == job.job_id)
        {
            return Ok(self.rejected_submission(
                &mut state,
                snapshot,
                &job,
                AdmissionOutcome::RejectedDuplicate,
                "job ID is already active or queued",
            ));
        }

        if let Some(submission) = self.drop_before_admission(&mut state, snapshot, &job) {
            return Ok(submission);
        }

        if let DropPolicy::Supersede(key) = &job.drop_policy {
            let mut removed = Vec::new();
            state.queued.retain(|queued| {
                let superseded =
                    matches!(&queued.job.drop_policy, DropPolicy::Supersede(other) if other == key);
                if superseded {
                    removed.push(queued.job.clone());
                }
                !superseded
            });
            for old in removed {
                let diagnostic = admission_diagnostic(
                    &self.inner.config,
                    snapshot,
                    &state,
                    &old,
                    AdmissionOutcome::DroppedStale,
                    None,
                    "superseded by a newer coalesced job",
                );
                self.push_admission(&mut state, diagnostic);
            }
        }

        if let Some((target, outcome)) = self.select_target_locked(&state, snapshot, &job, true) {
            return Ok(self.admit_locked(&mut state, snapshot, job, target, outcome));
        }

        if let Some((outcome, reason)) = self.terminal_rejection(&state, snapshot, &job) {
            return Ok(self.rejected_submission(&mut state, snapshot, &job, outcome, reason));
        }

        match job.drop_policy {
            DropPolicy::DropUnderPressure => Ok(self.rejected_submission(
                &mut state,
                snapshot,
                &job,
                AdmissionOutcome::DroppedPressure,
                "job cannot fit the current pressure and residency constraints",
            )),
            DropPolicy::DropIfLate if is_late(&job, snapshot.monotonic_ms) => Ok(self
                .rejected_submission(
                    &mut state,
                    snapshot,
                    &job,
                    AdmissionOutcome::DroppedLate,
                    "job deadline has elapsed",
                )),
            _ => {
                self.escalate_degradation_locked(
                    &mut state,
                    snapshot.monotonic_ms,
                    "admission could not fit current budget",
                );
                state.sequence += 1;
                let sequence = state.sequence;
                state.queued.push(QueuedRecord {
                    sequence,
                    job: job.clone(),
                });
                let diagnostic = admission_diagnostic(
                    &self.inner.config,
                    snapshot,
                    &state,
                    &job,
                    AdmissionOutcome::Queued,
                    None,
                    "waiting for resource, foreground, or residency lease",
                );
                self.push_admission(&mut state, diagnostic.clone());
                Ok(Submission {
                    diagnostic,
                    lease: None,
                })
            }
        }
    }

    /// Re-evaluates queued work after telemetry or lease changes. Returned leases are
    /// ordered by priority, deadline, and stable submission order.
    pub fn poll_ready(&self) -> Result<Vec<ResourceLease>, BrokerError> {
        let snapshot = self.inner.telemetry.snapshot()?.validate()?;
        let mut state = self.inner.state.lock().expect("broker mutex poisoned");
        self.expire_transient_overages_locked(&mut state, snapshot.monotonic_ms);
        self.update_pressure_locked(&mut state, snapshot);
        state.queued.sort_by(queue_order);
        let queued = std::mem::take(&mut state.queued);
        let mut leases = Vec::new();

        for queued in queued {
            if is_late(&queued.job, snapshot.monotonic_ms)
                && matches!(queued.job.drop_policy, DropPolicy::DropIfLate)
            {
                let diagnostic = admission_diagnostic(
                    &self.inner.config,
                    snapshot,
                    &state,
                    &queued.job,
                    AdmissionOutcome::DroppedLate,
                    None,
                    "queued job deadline elapsed",
                );
                self.push_admission(&mut state, diagnostic);
                continue;
            }
            if self.is_stale(&queued.job, snapshot.monotonic_ms) {
                let diagnostic = admission_diagnostic(
                    &self.inner.config,
                    snapshot,
                    &state,
                    &queued.job,
                    AdmissionOutcome::DroppedStale,
                    None,
                    "queued vision frame became stale",
                );
                self.push_admission(&mut state, diagnostic);
                continue;
            }
            if let Some((target, outcome)) =
                self.select_target_locked(&state, snapshot, &queued.job, true)
            {
                let submission =
                    self.admit_locked(&mut state, snapshot, queued.job, target, outcome);
                if let Some(lease) = submission.lease {
                    leases.push(lease);
                }
            } else {
                state.queued.push(queued);
            }
        }
        Ok(leases)
    }

    pub fn refresh_pressure(&self) -> Result<PressureLevel, BrokerError> {
        let snapshot = self.inner.telemetry.snapshot()?.validate()?;
        let mut state = self.inner.state.lock().expect("broker mutex poisoned");
        self.expire_transient_overages_locked(&mut state, snapshot.monotonic_ms);
        self.update_pressure_locked(&mut state, snapshot);
        Ok(state.pressure)
    }

    pub fn pressure(&self) -> PressureLevel {
        self.inner
            .state
            .lock()
            .expect("broker mutex poisoned")
            .pressure
    }

    pub fn active_degradations(&self) -> Vec<BrokerDegradation> {
        let state = self.inner.state.lock().expect("broker mutex poisoned");
        DEGRADATION_ORDER[..state.degradation_count].to_vec()
    }

    pub fn cancel(&self, job_id: &str, reason: impl Into<String>) -> bool {
        let snapshot = match self.inner.telemetry.snapshot() {
            Ok(snapshot) => snapshot,
            Err(_) => return false,
        };
        let mut state = self.inner.state.lock().expect("broker mutex poisoned");
        if let Some(index) = state
            .queued
            .iter()
            .position(|queued| queued.job.job_id == job_id)
        {
            let queued = state.queued.remove(index);
            let diagnostic = admission_diagnostic(
                &self.inner.config,
                snapshot,
                &state,
                &queued.job,
                AdmissionOutcome::Cancelled,
                None,
                "queued job cancelled",
            );
            self.push_admission(&mut state, diagnostic);
            return true;
        }
        let cancellation = if let Some(active) = state.active.get_mut(job_id) {
            active.cancelled_by_broker = true;
            Some(active.cancellation.clone())
        } else {
            None
        };
        if let Some(cancellation) = cancellation {
            cancellation.cancel();
            self.push_diagnostic(
                &mut state,
                BrokerDiagnostic::CancellationRequested {
                    monotonic_ms: snapshot.monotonic_ms,
                    job_id: job_id.to_owned(),
                    reason: reason.into(),
                },
            );
            true
        } else {
            false
        }
    }

    /// Marks that user-visible output began. From this point onward an utterance's
    /// model binding is immutable even if the target subsequently OOMs.
    pub fn mark_output_started(&self, job_id: &str) -> bool {
        let mut state = self.inner.state.lock().expect("broker mutex poisoned");
        let Some(active) = state.active.get_mut(job_id) else {
            return false;
        };
        active.output_started = true;
        if let Some(utterance_id) = active.job.kind.utterance_id().map(str::to_owned) {
            let target_id = active.target.target_id.clone();
            state.utterance_bindings.insert(utterance_id, target_id);
        }
        true
    }

    pub fn finish_utterance(&self, utterance_id: &str) {
        self.inner
            .state
            .lock()
            .expect("broker mutex poisoned")
            .utterance_bindings
            .remove(utterance_id);
    }

    pub fn report_oom(&self, job_id: &str) -> Result<RecoveryDirective, BrokerError> {
        let snapshot = self.inner.telemetry.snapshot()?.validate()?;
        let mut state = self.inner.state.lock().expect("broker mutex poisoned");
        let Some(active) = state.active.get(job_id).cloned() else {
            return Ok(RecoveryDirective::JobNotActive);
        };
        let health = state
            .target_health
            .entry(active.target.target_id.clone())
            .or_default();
        health.oom_failures += 1;
        if health.oom_failures < self.inner.config.oom_failures_before_quarantine {
            return Ok(RecoveryDirective::RetrySameTargetAfterEviction {
                target_id: active.target.target_id,
            });
        }

        health.quarantine_until_ms = snapshot
            .monotonic_ms
            .saturating_add(self.inner.config.quarantine_ms);
        let until_ms = health.quarantine_until_ms;
        let oom_failures = health.oom_failures;
        self.push_diagnostic(
            &mut state,
            BrokerDiagnostic::TargetQuarantined {
                monotonic_ms: snapshot.monotonic_ms,
                target_id: active.target.target_id.clone(),
                until_ms,
                oom_failures,
            },
        );

        if active.output_started || active.job.kind.is_foreground_llm() {
            return Ok(RecoveryDirective::AbortUtteranceNoModelSwap);
        }
        let fallback = active.job.fallbacks.iter().find(|fallback| {
            active
                .job
                .privacy
                .allows_fallback(&active.job.primary.provider, &fallback.provider)
                && !self.is_target_quarantined(&state, &fallback.target_id, snapshot.monotonic_ms)
                && self.target_fits(&state, snapshot, fallback)
        });
        Ok(match fallback {
            Some(fallback) => RecoveryDirective::RetryFallback {
                target_id: fallback.target_id.clone(),
            },
            None => RecoveryDirective::QuarantinedNoSafeFallback,
        })
    }

    pub fn diagnostics(&self) -> Vec<BrokerDiagnostic> {
        self.inner
            .state
            .lock()
            .expect("broker mutex poisoned")
            .diagnostics
            .iter()
            .cloned()
            .collect()
    }

    pub fn drain_diagnostics(&self) -> Vec<BrokerDiagnostic> {
        self.inner
            .state
            .lock()
            .expect("broker mutex poisoned")
            .diagnostics
            .drain(..)
            .collect()
    }

    fn drop_before_admission(
        &self,
        state: &mut BrokerState,
        snapshot: BudgetSnapshot,
        job: &ResourceJob,
    ) -> Option<Submission> {
        if is_late(job, snapshot.monotonic_ms) && matches!(job.drop_policy, DropPolicy::DropIfLate)
        {
            return Some(self.rejected_submission(
                state,
                snapshot,
                job,
                AdmissionOutcome::DroppedLate,
                "job arrived after its deadline",
            ));
        }
        if self.is_stale(job, snapshot.monotonic_ms) {
            return Some(self.rejected_submission(
                state,
                snapshot,
                job,
                AdmissionOutcome::DroppedStale,
                "vision frame is stale",
            ));
        }
        if job.kind.is_visual_burst()
            && state.pressure != PressureLevel::Green
            && matches!(job.drop_policy, DropPolicy::DropUnderPressure)
        {
            return Some(self.rejected_submission(
                state,
                snapshot,
                job,
                AdmissionOutcome::DroppedPressure,
                "visual burst dropped under memory pressure",
            ));
        }
        None
    }

    fn is_stale(&self, job: &ResourceJob, now_ms: u64) -> bool {
        let Some(captured_at_ms) = job.kind.captured_at_ms() else {
            return false;
        };
        let maximum_age_ms = match job.drop_policy {
            DropPolicy::DropIfStale { maximum_age_ms } => maximum_age_ms,
            _ if job.kind.is_visual_burst() => self.inner.config.default_vision_stale_ms,
            _ => return false,
        };
        now_ms.saturating_sub(captured_at_ms) > maximum_age_ms
    }

    fn select_target_locked(
        &self,
        state: &BrokerState,
        snapshot: BudgetSnapshot,
        job: &ResourceJob,
        allow_fallback: bool,
    ) -> Option<(ComputeTarget, AdmissionOutcome)> {
        let bound_target = job
            .kind
            .utterance_id()
            .and_then(|utterance| state.utterance_bindings.get(utterance));
        let mut candidates = std::iter::once((&job.primary, AdmissionOutcome::AdmittedPrimary))
            .chain(
                job.fallbacks
                    .iter()
                    .map(|target| (target, AdmissionOutcome::AdmittedFallback)),
            );
        candidates.find_map(|(target, outcome)| {
            if outcome == AdmissionOutcome::AdmittedFallback && !allow_fallback {
                return None;
            }
            if bound_target.is_some_and(|bound| bound != &target.target_id) {
                return None;
            }
            let privacy_allowed = if outcome == AdmissionOutcome::AdmittedPrimary {
                job.privacy.allows(&target.provider)
            } else {
                job.privacy
                    .allows_fallback(&job.primary.provider, &target.provider)
            };
            if !privacy_allowed
                || self.is_target_quarantined(state, &target.target_id, snapshot.monotonic_ms)
                || !self.target_fits(state, snapshot, target)
                || !self.residency_available(state, job, target)
            {
                return None;
            }
            Some((target.clone(), outcome))
        })
    }

    fn target_fits(
        &self,
        state: &BrokerState,
        snapshot: BudgetSnapshot,
        target: &ComputeTarget,
    ) -> bool {
        let estimate = target.estimate;
        let projected_ram = snapshot
            .system_ram_used_bytes
            .saturating_add(state.reservations.ram_bytes)
            .saturating_add(estimate.ram_bytes);
        if projected_ram > snapshot.system_ram_budget_bytes {
            return false;
        }
        let ceiling = self
            .inner
            .config
            .vram_ceiling_bytes
            .min(snapshot.gpu_vram_budget_bytes);
        let projected_resident = snapshot
            .gpu_vram_used_bytes
            .saturating_add(state.reservations.vram_resident_bytes)
            .saturating_add(state.reservations.vram_transient_bytes)
            .saturating_add(estimate.vram_resident_bytes);
        let projected_total = projected_resident.saturating_add(estimate.vram_transient_bytes);
        if projected_resident > ceiling {
            return false;
        }
        if projected_total <= ceiling {
            return true;
        }
        let overshoot = projected_total - ceiling;
        overshoot <= self.inner.config.transient_vram_allowance_bytes
            && estimate.transient_duration_ms > 0
            && estimate.transient_duration_ms <= self.inner.config.transient_vram_grace_ms
    }

    fn terminal_rejection(
        &self,
        state: &BrokerState,
        snapshot: BudgetSnapshot,
        job: &ResourceJob,
    ) -> Option<(AdmissionOutcome, &'static str)> {
        let targets: Vec<_> = std::iter::once(&job.primary)
            .chain(job.fallbacks.iter())
            .collect();
        let privacy_allowed: Vec<_> = targets
            .iter()
            .copied()
            .filter(|target| {
                if target.target_id == job.primary.target_id {
                    job.privacy.allows(&target.provider)
                } else {
                    job.privacy
                        .allows_fallback(&job.primary.provider, &target.provider)
                }
            })
            .collect();
        if privacy_allowed.is_empty() {
            return Some((
                AdmissionOutcome::RejectedPrivacy,
                "no target is authorized by the job privacy policy",
            ));
        }
        if let Some(bound) = job
            .kind
            .utterance_id()
            .and_then(|utterance| state.utterance_bindings.get(utterance))
        {
            if privacy_allowed
                .iter()
                .all(|target| &target.target_id != bound)
            {
                return Some((
                    AdmissionOutcome::RejectedUtteranceBinding,
                    "no authorized target matches the utterance model binding",
                ));
            }
        }
        if privacy_allowed.iter().all(|target| {
            self.is_target_quarantined(state, &target.target_id, snapshot.monotonic_ms)
        }) {
            return Some((
                AdmissionOutcome::RejectedQuarantined,
                "all privacy-authorized targets are quarantined",
            ));
        }
        None
    }

    fn residency_available(
        &self,
        state: &BrokerState,
        job: &ResourceJob,
        target: &ComputeTarget,
    ) -> bool {
        if job.kind.is_foreground_llm() && state.reservations.foreground_llm_job.is_some() {
            return false;
        }
        match target.gpu_residency {
            GpuResidency::None => true,
            GpuResidency::Shared => state.reservations.exclusive_gpu_job.is_none(),
            GpuResidency::Exclusive => state.reservations.gpu_leases == 0,
        }
    }

    fn is_target_quarantined(&self, state: &BrokerState, target_id: &str, now_ms: u64) -> bool {
        state
            .target_health
            .get(target_id)
            .is_some_and(|health| health.quarantine_until_ms > now_ms)
    }

    fn admit_locked(
        &self,
        state: &mut BrokerState,
        snapshot: BudgetSnapshot,
        job: ResourceJob,
        target: ComputeTarget,
        outcome: AdmissionOutcome,
    ) -> Submission {
        let cancellation = CancellationToken::new();
        let diagnostic = admission_diagnostic(
            &self.inner.config,
            snapshot,
            state,
            &job,
            outcome,
            Some(target.target_id.clone()),
            if outcome == AdmissionOutcome::AdmittedFallback {
                "primary target unavailable; authorized fallback admitted"
            } else {
                "resource lease admitted"
            },
        );
        reserve(&mut state.reservations, &job, &target);
        state.active.insert(
            job.job_id.clone(),
            ActiveRecord {
                job: job.clone(),
                target: target.clone(),
                cancellation: cancellation.clone(),
                output_started: false,
                cancelled_by_broker: false,
                transient_deadline_ms: (diagnostic.projection.transient_overshoot_bytes > 0)
                    .then_some(
                        snapshot
                            .monotonic_ms
                            .saturating_add(self.inner.config.transient_vram_grace_ms),
                    ),
            },
        );
        self.push_admission(state, diagnostic.clone());
        Submission {
            diagnostic,
            lease: Some(ResourceLease {
                broker: Arc::downgrade(&self.inner),
                job_id: job.job_id,
                target_id: target.target_id,
                cancellation,
                completed: false,
            }),
        }
    }

    fn rejected_submission(
        &self,
        state: &mut BrokerState,
        snapshot: BudgetSnapshot,
        job: &ResourceJob,
        outcome: AdmissionOutcome,
        reason: &str,
    ) -> Submission {
        let diagnostic = admission_diagnostic(
            &self.inner.config,
            snapshot,
            state,
            job,
            outcome,
            None,
            reason,
        );
        self.push_admission(state, diagnostic.clone());
        Submission {
            diagnostic,
            lease: None,
        }
    }

    fn update_pressure_locked(&self, state: &mut BrokerState, snapshot: BudgetSnapshot) {
        let ceiling = self
            .inner
            .config
            .vram_ceiling_bytes
            .min(snapshot.gpu_vram_budget_bytes);
        let vram = snapshot
            .gpu_vram_used_bytes
            .saturating_add(state.reservations.vram_resident_bytes)
            .saturating_add(state.reservations.vram_transient_bytes) as f64
            / ceiling as f64;
        let ram = snapshot
            .system_ram_used_bytes
            .saturating_add(state.reservations.ram_bytes) as f64
            / snapshot.system_ram_budget_bytes as f64;
        let utilization = vram.max(ram).max(snapshot.gpu_utilization as f64);
        let previous = state.pressure;
        state.pressure = match state.pressure {
            PressureLevel::Green if utilization >= self.inner.config.red_enter_ratio => {
                PressureLevel::Red
            }
            PressureLevel::Green if utilization >= self.inner.config.amber_enter_ratio => {
                PressureLevel::Amber
            }
            PressureLevel::Amber if utilization >= self.inner.config.red_enter_ratio => {
                PressureLevel::Red
            }
            PressureLevel::Amber if utilization <= self.inner.config.green_reenter_ratio => {
                PressureLevel::Green
            }
            PressureLevel::Red if utilization <= self.inner.config.amber_reenter_ratio => {
                PressureLevel::Amber
            }
            current => current,
        };

        if state.pressure != previous {
            self.push_diagnostic(
                state,
                BrokerDiagnostic::PressureChanged {
                    monotonic_ms: snapshot.monotonic_ms,
                    from: previous,
                    to: state.pressure,
                    utilization_ratio: utilization,
                },
            );
        }

        let pressure_floor = match state.pressure {
            PressureLevel::Green => 0,
            PressureLevel::Amber => 1,
            PressureLevel::Red => 2,
        };
        if state.degradation_count < pressure_floor {
            state.degradation_count = pressure_floor;
            self.emit_degradation_locked(state, snapshot.monotonic_ms, "pressure floor increased");
        }

        if state.pressure == PressureLevel::Green {
            state.green_samples += 1;
            if state.green_samples >= self.inner.config.recovery_green_samples
                && state.degradation_count > 0
            {
                state.degradation_count -= 1;
                state.green_samples = 0;
                self.emit_degradation_locked(
                    state,
                    snapshot.monotonic_ms,
                    "sustained green pressure recovery",
                );
            }
        } else {
            state.green_samples = 0;
        }
    }

    fn expire_transient_overages_locked(&self, state: &mut BrokerState, now_ms: u64) {
        let expired: Vec<_> = state
            .active
            .iter_mut()
            .filter_map(|(job_id, active)| {
                let expired = active
                    .transient_deadline_ms
                    .is_some_and(|deadline| now_ms > deadline);
                if !expired || active.cancelled_by_broker {
                    return None;
                }
                active.cancelled_by_broker = true;
                active.cancellation.cancel();
                Some(job_id.clone())
            })
            .collect();
        for job_id in expired {
            self.push_diagnostic(
                state,
                BrokerDiagnostic::CancellationRequested {
                    monotonic_ms: now_ms,
                    job_id,
                    reason: "transient VRAM ceiling grace expired".into(),
                },
            );
        }
    }

    fn escalate_degradation_locked(&self, state: &mut BrokerState, now_ms: u64, reason: &str) {
        if state.degradation_count < DEGRADATION_ORDER.len() {
            state.degradation_count += 1;
            self.emit_degradation_locked(state, now_ms, reason);
        }
    }

    fn emit_degradation_locked(&self, state: &mut BrokerState, now_ms: u64, reason: &str) {
        self.push_diagnostic(
            state,
            BrokerDiagnostic::DegradationChanged {
                monotonic_ms: now_ms,
                active: DEGRADATION_ORDER[..state.degradation_count].to_vec(),
                reason: reason.into(),
            },
        );
    }

    fn push_admission(&self, state: &mut BrokerState, diagnostic: AdmissionDiagnostic) {
        self.push_diagnostic(state, BrokerDiagnostic::Admission(diagnostic));
    }

    fn push_diagnostic(&self, state: &mut BrokerState, diagnostic: BrokerDiagnostic) {
        if state.diagnostics.len() == self.inner.config.diagnostic_capacity {
            state.diagnostics.pop_front();
        }
        state.diagnostics.push_back(diagnostic);
    }
}

fn validate_job(job: &ResourceJob) -> Result<(), BrokerError> {
    if job.job_id.trim().is_empty() {
        return Err(BrokerError::InvalidJob("job_id is empty".into()));
    }
    if job.primary.target_id.trim().is_empty() {
        return Err(BrokerError::InvalidJob("primary target_id is empty".into()));
    }
    if job.primary.provider.modality
        != job
            .fallbacks
            .first()
            .map_or(job.primary.provider.modality, |target| {
                target.provider.modality
            })
        || job
            .fallbacks
            .iter()
            .any(|target| target.provider.modality != job.primary.provider.modality)
    {
        return Err(BrokerError::InvalidJob(
            "fallback modality differs from primary".into(),
        ));
    }
    let mut ids = HashSet::new();
    if !ids.insert(job.primary.target_id.as_str())
        || job
            .fallbacks
            .iter()
            .any(|target| !ids.insert(target.target_id.as_str()))
    {
        return Err(BrokerError::InvalidJob(
            "target IDs must be unique within a job".into(),
        ));
    }
    Ok(())
}

fn is_late(job: &ResourceJob, now_ms: u64) -> bool {
    job.deadline_ms.is_some_and(|deadline| now_ms > deadline)
}

fn queue_order(left: &QueuedRecord, right: &QueuedRecord) -> Ordering {
    right
        .job
        .priority
        .cmp(&left.job.priority)
        .then_with(|| {
            left.job
                .deadline_ms
                .unwrap_or(u64::MAX)
                .cmp(&right.job.deadline_ms.unwrap_or(u64::MAX))
        })
        .then_with(|| left.sequence.cmp(&right.sequence))
}

fn reserve(reservations: &mut Reservations, job: &ResourceJob, target: &ComputeTarget) {
    reservations.ram_bytes = reservations
        .ram_bytes
        .saturating_add(target.estimate.ram_bytes);
    reservations.vram_resident_bytes = reservations
        .vram_resident_bytes
        .saturating_add(target.estimate.vram_resident_bytes);
    reservations.vram_transient_bytes = reservations
        .vram_transient_bytes
        .saturating_add(target.estimate.vram_transient_bytes);
    if target.gpu_residency != GpuResidency::None {
        reservations.gpu_leases += 1;
    }
    if target.gpu_residency == GpuResidency::Exclusive {
        reservations.exclusive_gpu_job = Some(job.job_id.clone());
    }
    if job.kind.is_foreground_llm() {
        reservations.foreground_llm_job = Some(job.job_id.clone());
    }
}

fn release(reservations: &mut Reservations, job: &ResourceJob, target: &ComputeTarget) {
    reservations.ram_bytes = reservations
        .ram_bytes
        .saturating_sub(target.estimate.ram_bytes);
    reservations.vram_resident_bytes = reservations
        .vram_resident_bytes
        .saturating_sub(target.estimate.vram_resident_bytes);
    reservations.vram_transient_bytes = reservations
        .vram_transient_bytes
        .saturating_sub(target.estimate.vram_transient_bytes);
    if target.gpu_residency != GpuResidency::None {
        reservations.gpu_leases = reservations.gpu_leases.saturating_sub(1);
    }
    if reservations.exclusive_gpu_job.as_deref() == Some(&job.job_id) {
        reservations.exclusive_gpu_job = None;
    }
    if reservations.foreground_llm_job.as_deref() == Some(&job.job_id) {
        reservations.foreground_llm_job = None;
    }
}

fn projection(
    config: &ResourceBrokerConfig,
    snapshot: BudgetSnapshot,
    state: &BrokerState,
    estimate: ResourceEstimate,
) -> ResourceProjection {
    let ceiling = config
        .vram_ceiling_bytes
        .min(snapshot.gpu_vram_budget_bytes);
    let ram = snapshot
        .system_ram_used_bytes
        .saturating_add(state.reservations.ram_bytes)
        .saturating_add(estimate.ram_bytes);
    let vram = snapshot
        .gpu_vram_used_bytes
        .saturating_add(state.reservations.vram_resident_bytes)
        .saturating_add(state.reservations.vram_transient_bytes)
        .saturating_add(estimate.total_vram());
    ResourceProjection {
        ram_bytes: ram,
        vram_bytes: vram,
        vram_ceiling_bytes: ceiling,
        transient_overshoot_bytes: vram.saturating_sub(ceiling),
    }
}

#[allow(clippy::too_many_arguments)]
fn admission_diagnostic(
    config: &ResourceBrokerConfig,
    snapshot: BudgetSnapshot,
    state: &BrokerState,
    job: &ResourceJob,
    outcome: AdmissionOutcome,
    selected_target_id: Option<String>,
    reason: &str,
) -> AdmissionDiagnostic {
    let estimate = selected_target_id
        .as_deref()
        .and_then(|id| {
            std::iter::once(&job.primary)
                .chain(job.fallbacks.iter())
                .find(|target| target.target_id == id)
        })
        .unwrap_or(&job.primary)
        .estimate;
    AdmissionDiagnostic {
        monotonic_ms: snapshot.monotonic_ms,
        job_id: job.job_id.clone(),
        kind: job.kind.clone(),
        priority: job.priority,
        outcome,
        selected_target_id,
        pressure: state.pressure,
        active_degradations: DEGRADATION_ORDER[..state.degradation_count].to_vec(),
        projection: projection(config, snapshot, state, estimate),
        reason: reason.into(),
    }
}

/// RAII reservation. Dropping without [`ResourceLease::complete`] records a
/// cancellation and releases reservations; it never leaks an exclusive GPU lease.
#[derive(Debug)]
pub struct ResourceLease {
    broker: Weak<ResourceBrokerInner>,
    job_id: String,
    target_id: String,
    cancellation: CancellationToken,
    completed: bool,
}

impl ResourceLease {
    pub fn job_id(&self) -> &str {
        &self.job_id
    }

    pub fn target_id(&self) -> &str {
        &self.target_id
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    pub fn complete(mut self, actual: ActualUsage, status: CompletionStatus) {
        if let Some(inner) = self.broker.upgrade() {
            finish_lease(&inner, &self.job_id, actual, status);
        }
        self.completed = true;
    }
}

impl Drop for ResourceLease {
    fn drop(&mut self) {
        if !self.completed {
            if let Some(inner) = self.broker.upgrade() {
                finish_lease(
                    &inner,
                    &self.job_id,
                    ActualUsage::default(),
                    CompletionStatus::Cancelled,
                );
            }
        }
    }
}

fn finish_lease(
    inner: &Arc<ResourceBrokerInner>,
    job_id: &str,
    actual: ActualUsage,
    status: CompletionStatus,
) {
    let snapshot = inner.telemetry.snapshot().ok();
    let mut state = inner.state.lock().expect("broker mutex poisoned");
    let Some(active) = state.active.remove(job_id) else {
        return;
    };
    release(&mut state.reservations, &active.job, &active.target);
    if status == CompletionStatus::Completed {
        state.target_health.remove(&active.target.target_id);
    }
    let now_ms = snapshot.map_or(0, |value| value.monotonic_ms);
    let deadline_missed = active
        .job
        .deadline_ms
        .is_some_and(|deadline| now_ms > deadline);
    let diagnostic = BrokerDiagnostic::Actual(ActualDiagnostic {
        monotonic_ms: now_ms,
        job_id: job_id.into(),
        target_id: active.target.target_id,
        estimated: active.target.estimate,
        actual,
        status,
        deadline_missed,
        cancelled_by_broker: active.cancelled_by_broker,
    });
    if state.diagnostics.len() == inner.config.diagnostic_capacity {
        state.diagnostics.pop_front();
    }
    state.diagnostics.push_back(diagnostic);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DataClass, ProviderLocation, ProviderModality};
    use std::collections::BTreeMap;

    fn snapshot(now: u64, vram_used: u64, vram_budget: u64) -> BudgetSnapshot {
        BudgetSnapshot {
            monotonic_ms: now,
            system_ram_budget_bytes: 16_000 * MIB,
            system_ram_used_bytes: 1_000 * MIB,
            gpu_vram_budget_bytes: vram_budget,
            gpu_vram_used_bytes: vram_used,
            gpu_utilization: 0.1,
        }
    }

    fn local_provider(id: &str) -> ProviderDescriptor {
        ProviderDescriptor {
            id: id.into(),
            display_name: id.into(),
            modality: ProviderModality::LanguageModel,
            location: ProviderLocation::Local,
            may_retain_data: false,
            transmitted_data: vec![DataClass::Transcript],
            capabilities: BTreeMap::new(),
        }
    }

    fn cloud_provider(id: &str) -> ProviderDescriptor {
        ProviderDescriptor {
            id: id.into(),
            display_name: id.into(),
            modality: ProviderModality::LanguageModel,
            location: ProviderLocation::Cloud { service: id.into() },
            may_retain_data: false,
            transmitted_data: vec![DataClass::Transcript],
            capabilities: BTreeMap::new(),
        }
    }

    fn target(
        id: &str,
        provider: ProviderDescriptor,
        resident_mib: u64,
        transient_mib: u64,
        transient_ms: u64,
        residency: GpuResidency,
    ) -> ComputeTarget {
        ComputeTarget {
            target_id: id.into(),
            model_id: Some(id.into()),
            provider,
            estimate: ResourceEstimate {
                ram_bytes: 128 * MIB,
                vram_resident_bytes: resident_mib * MIB,
                vram_transient_bytes: transient_mib * MIB,
                transient_duration_ms: transient_ms,
                gpu_time_ms: 10,
            },
            gpu_residency: residency,
        }
    }

    fn job(job_id: &str, kind: ResourceJobKind, primary: ComputeTarget) -> ResourceJob {
        ResourceJob {
            job_id: job_id.into(),
            kind,
            priority: JobPriority::Interactive,
            deadline_ms: Some(10_000),
            primary,
            fallbacks: Vec::new(),
            drop_policy: DropPolicy::Never,
            privacy: BrokerPrivacyContext::fully_local(),
        }
    }

    fn config() -> ResourceBrokerConfig {
        ResourceBrokerConfig {
            vram_ceiling_bytes: 1_000 * MIB,
            transient_vram_allowance_bytes: 256 * MIB,
            transient_vram_grace_ms: 2_000,
            quarantine_ms: 1_000,
            ..Default::default()
        }
    }

    fn expected_pressure(current: PressureLevel, used: u64, budget: u64) -> PressureLevel {
        let ratio = used as f64 / budget as f64;
        match current {
            PressureLevel::Green if ratio >= 0.90 => PressureLevel::Red,
            PressureLevel::Green if ratio >= 0.75 => PressureLevel::Amber,
            PressureLevel::Amber if ratio >= 0.90 => PressureLevel::Red,
            PressureLevel::Amber if ratio <= 0.65 => PressureLevel::Green,
            PressureLevel::Red if ratio <= 0.82 => PressureLevel::Amber,
            value => value,
        }
    }

    #[test]
    fn pressure_state_machine_exhaustively_resists_threshold_oscillation() {
        // Explore every length-four trace over representative values on both sides
        // of every hysteresis threshold. This is deterministic model-based testing,
        // not a timing-sensitive statistical test.
        let values = [640, 700, 760, 830, 910];
        for first in values {
            for second in values {
                for third in values {
                    for fourth in values {
                        let telemetry = Arc::new(
                            ReportedBudgetTelemetry::new(snapshot(0, 100, 1_000)).unwrap(),
                        );
                        let broker = ResourceBroker::new(config(), telemetry.clone());
                        let mut expected = PressureLevel::Green;
                        for (step, used) in [first, second, third, fourth].into_iter().enumerate() {
                            telemetry
                                .update(snapshot((step + 1) as u64, used, 1_000))
                                .unwrap();
                            expected = expected_pressure(expected, used, 1_000);
                            assert_eq!(broker.refresh_pressure().unwrap(), expected);
                            let active = broker.active_degradations();
                            assert_eq!(
                                active,
                                DEGRADATION_ORDER[..active.len()],
                                "degradation state must always be an ordered prefix"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn stale_vision_is_dropped_before_it_can_reserve_gpu_memory() {
        let telemetry = Arc::new(
            ReportedBudgetTelemetry::new(snapshot(1_000, 100 * MIB, 2_000 * MIB)).unwrap(),
        );
        let broker = ResourceBroker::new(config(), telemetry.clone());
        let mut request = job(
            "vision-old",
            ResourceJobKind::ContinuousVision {
                frame_sequence: 7,
                captured_at_ms: 800,
            },
            target(
                "vision",
                local_provider("vision"),
                100,
                0,
                0,
                GpuResidency::Shared,
            ),
        );
        request.drop_policy = DropPolicy::DropIfStale {
            maximum_age_ms: 100,
        };
        let submission = broker.submit(request).unwrap();
        assert_eq!(
            submission.diagnostic.outcome,
            AdmissionOutcome::DroppedStale
        );
        assert!(submission.lease.is_none());
    }

    #[test]
    fn foreground_llm_is_single_lease_and_cancellation_releases_queue() {
        let telemetry =
            Arc::new(ReportedBudgetTelemetry::new(snapshot(0, 100 * MIB, 2_000 * MIB)).unwrap());
        let broker = ResourceBroker::new(config(), telemetry);
        let first = broker
            .submit(job(
                "llm-1",
                ResourceJobKind::ForegroundLlm {
                    utterance_id: "u1".into(),
                },
                target(
                    "model-a",
                    local_provider("local"),
                    100,
                    0,
                    0,
                    GpuResidency::Shared,
                ),
            ))
            .unwrap();
        let first_lease = first.lease.unwrap();
        let token = first_lease.cancellation_token();
        let second = broker
            .submit(job(
                "llm-2",
                ResourceJobKind::ForegroundLlm {
                    utterance_id: "u2".into(),
                },
                target(
                    "model-a",
                    local_provider("local"),
                    100,
                    0,
                    0,
                    GpuResidency::Shared,
                ),
            ))
            .unwrap();
        assert_eq!(second.diagnostic.outcome, AdmissionOutcome::Queued);
        assert!(broker.cancel("llm-1", "barge-in"));
        assert!(token.is_cancelled());
        drop(first_lease);
        let ready = broker.poll_ready().unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].job_id(), "llm-2");
        ready.into_iter().next().unwrap().complete(
            ActualUsage {
                peak_ram_bytes: 100 * MIB,
                peak_vram_bytes: 90 * MIB,
                gpu_time_ms: 8,
            },
            CompletionStatus::Completed,
        );
        assert!(broker.diagnostics().iter().any(|diagnostic| {
            matches!(
                diagnostic,
                BrokerDiagnostic::Actual(actual)
                    if actual.job_id == "llm-1" && actual.cancelled_by_broker
            )
        }));
    }

    #[test]
    fn exclusive_gpu_residency_blocks_shared_work_until_release() {
        let telemetry =
            Arc::new(ReportedBudgetTelemetry::new(snapshot(0, 100 * MIB, 2_000 * MIB)).unwrap());
        let broker = ResourceBroker::new(config(), telemetry);
        let exclusive = broker
            .submit(job(
                "burst",
                ResourceJobKind::ModelLoad,
                target(
                    "burst-model",
                    local_provider("local"),
                    100,
                    0,
                    0,
                    GpuResidency::Exclusive,
                ),
            ))
            .unwrap()
            .lease
            .unwrap();
        let shared = broker
            .submit(job(
                "embedding",
                ResourceJobKind::Embedding,
                target(
                    "embed",
                    local_provider("local"),
                    20,
                    0,
                    0,
                    GpuResidency::Shared,
                ),
            ))
            .unwrap();
        assert_eq!(shared.diagnostic.outcome, AdmissionOutcome::Queued);
        drop(exclusive);
        assert_eq!(broker.poll_ready().unwrap().len(), 1);
    }

    #[test]
    fn queued_jobs_are_stably_ordered_by_priority_then_deadline() {
        let telemetry =
            Arc::new(ReportedBudgetTelemetry::new(snapshot(0, 100 * MIB, 2_000 * MIB)).unwrap());
        let broker = ResourceBroker::new(config(), telemetry);
        let blocker = broker
            .submit(job(
                "exclusive-blocker",
                ResourceJobKind::ModelLoad,
                target(
                    "exclusive",
                    local_provider("local"),
                    100,
                    0,
                    0,
                    GpuResidency::Exclusive,
                ),
            ))
            .unwrap()
            .lease
            .unwrap();

        for (id, priority, deadline) in [
            ("background", JobPriority::Background, 100),
            ("interactive-late", JobPriority::Interactive, 900),
            ("critical", JobPriority::Critical, 900),
            ("interactive-early", JobPriority::Interactive, 100),
        ] {
            let mut request = job(
                id,
                ResourceJobKind::Embedding,
                target(id, local_provider("local"), 20, 0, 0, GpuResidency::Shared),
            );
            request.priority = priority;
            request.deadline_ms = Some(deadline);
            assert_eq!(
                broker.submit(request).unwrap().diagnostic.outcome,
                AdmissionOutcome::Queued
            );
        }
        drop(blocker);
        let ready = broker.poll_ready().unwrap();
        let order: Vec<_> = ready.iter().map(ResourceLease::job_id).collect();
        assert_eq!(
            order,
            [
                "critical",
                "interactive-early",
                "interactive-late",
                "background"
            ]
        );
    }

    #[test]
    fn transient_ceiling_allows_only_256_mib_for_at_most_two_seconds() {
        let telemetry =
            Arc::new(ReportedBudgetTelemetry::new(snapshot(0, 700 * MIB, 2_000 * MIB)).unwrap());
        let broker = ResourceBroker::new(config(), telemetry.clone());
        let allowed = broker
            .submit(job(
                "allowed-transient",
                ResourceJobKind::ModelLoad,
                target(
                    "allowed",
                    local_provider("local"),
                    200,
                    356,
                    2_000,
                    GpuResidency::Shared,
                ),
            ))
            .unwrap();
        assert_eq!(
            allowed.diagnostic.projection.transient_overshoot_bytes,
            256 * MIB
        );
        allowed
            .lease
            .unwrap()
            .complete(ActualUsage::default(), CompletionStatus::Completed);

        let expiring = broker
            .submit(job(
                "expiring-transient",
                ResourceJobKind::ModelLoad,
                target(
                    "expiring",
                    local_provider("local"),
                    200,
                    356,
                    2_000,
                    GpuResidency::Shared,
                ),
            ))
            .unwrap()
            .lease
            .unwrap();
        let token = expiring.cancellation_token();
        telemetry
            .update(snapshot(2_001, 700 * MIB, 2_000 * MIB))
            .unwrap();
        let _ = broker.refresh_pressure().unwrap();
        assert!(
            token.is_cancelled(),
            "actual overshoot may not outlive grace"
        );
        drop(expiring);

        for (id, transient, duration) in [("too-large", 357, 2_000), ("too-long", 356, 2_001)] {
            let mut request = job(
                id,
                ResourceJobKind::ModelLoad,
                target(
                    id,
                    local_provider("local"),
                    200,
                    transient,
                    duration,
                    GpuResidency::Shared,
                ),
            );
            request.drop_policy = DropPolicy::DropUnderPressure;
            let denied = broker.submit(request).unwrap();
            assert_eq!(denied.diagnostic.outcome, AdmissionOutcome::DroppedPressure);
        }
    }

    #[test]
    fn oom_retry_then_quarantine_uses_only_privacy_safe_fallbacks() {
        struct Case {
            mode: ExecutionMode,
            network: NetworkPolicy,
            authorized: bool,
            allow_local_to_cloud: bool,
            expects_fallback: bool,
        }
        let cases = [
            Case {
                mode: ExecutionMode::FullyLocal,
                network: NetworkPolicy::Offline,
                authorized: false,
                allow_local_to_cloud: false,
                expects_fallback: false,
            },
            Case {
                mode: ExecutionMode::Hybrid,
                network: NetworkPolicy::Online,
                authorized: true,
                allow_local_to_cloud: false,
                expects_fallback: false,
            },
            Case {
                mode: ExecutionMode::Hybrid,
                network: NetworkPolicy::Online,
                authorized: false,
                allow_local_to_cloud: true,
                expects_fallback: false,
            },
            Case {
                mode: ExecutionMode::Hybrid,
                network: NetworkPolicy::Online,
                authorized: true,
                allow_local_to_cloud: true,
                expects_fallback: true,
            },
        ];

        for (index, case) in cases.into_iter().enumerate() {
            let telemetry = Arc::new(
                ReportedBudgetTelemetry::new(snapshot(0, 100 * MIB, 2_000 * MIB)).unwrap(),
            );
            let broker = ResourceBroker::new(config(), telemetry);
            let mut request = job(
                &format!("oom-{index}"),
                ResourceJobKind::ModelLoad,
                target(
                    "local-primary",
                    local_provider("local"),
                    100,
                    0,
                    0,
                    GpuResidency::Shared,
                ),
            );
            request.fallbacks.push(target(
                "cloud-fallback",
                cloud_provider("cloud"),
                0,
                0,
                0,
                GpuResidency::None,
            ));
            request.privacy = BrokerPrivacyContext {
                execution_mode: case.mode,
                network_policy: case.network,
                authorized_cloud_providers: if case.authorized {
                    HashSet::from(["cloud".into()])
                } else {
                    HashSet::new()
                },
                allow_retaining_providers: false,
                allow_local_to_cloud_fallback: case.allow_local_to_cloud,
            };
            let lease = broker.submit(request).unwrap().lease.unwrap();
            assert_eq!(
                broker.report_oom(lease.job_id()).unwrap(),
                RecoveryDirective::RetrySameTargetAfterEviction {
                    target_id: "local-primary".into()
                }
            );
            let second = broker.report_oom(lease.job_id()).unwrap();
            assert_eq!(
                second,
                if case.expects_fallback {
                    RecoveryDirective::RetryFallback {
                        target_id: "cloud-fallback".into(),
                    }
                } else {
                    RecoveryDirective::QuarantinedNoSafeFallback
                }
            );
        }
    }

    #[test]
    fn foreground_oom_never_swaps_model_mid_utterance() {
        let telemetry =
            Arc::new(ReportedBudgetTelemetry::new(snapshot(0, 100 * MIB, 2_000 * MIB)).unwrap());
        let broker = ResourceBroker::new(config(), telemetry);
        let mut request = job(
            "llm-oom",
            ResourceJobKind::ForegroundLlm {
                utterance_id: "utterance".into(),
            },
            target(
                "model-a",
                local_provider("local"),
                100,
                0,
                0,
                GpuResidency::Shared,
            ),
        );
        request.fallbacks.push(target(
            "model-b",
            local_provider("local"),
            50,
            0,
            0,
            GpuResidency::Shared,
        ));
        let lease = broker.submit(request).unwrap().lease.unwrap();
        assert!(broker.mark_output_started(lease.job_id()));
        let _ = broker.report_oom(lease.job_id()).unwrap();
        assert_eq!(
            broker.report_oom(lease.job_id()).unwrap(),
            RecoveryDirective::AbortUtteranceNoModelSwap
        );
    }

    #[test]
    fn repeated_admission_pressure_advances_only_in_fixed_order() {
        let telemetry =
            Arc::new(ReportedBudgetTelemetry::new(snapshot(0, 950 * MIB, 2_000 * MIB)).unwrap());
        let broker = ResourceBroker::new(config(), telemetry);
        for index in 0..(DEGRADATION_ORDER.len() + 2) {
            let request = job(
                &format!("blocked-{index}"),
                ResourceJobKind::Maintenance,
                target(
                    &format!("huge-{index}"),
                    local_provider("local"),
                    200,
                    0,
                    0,
                    GpuResidency::Shared,
                ),
            );
            let _ = broker.submit(request).unwrap();
            let active = broker.active_degradations();
            assert_eq!(active, DEGRADATION_ORDER[..active.len()]);
        }
        assert_eq!(broker.active_degradations(), DEGRADATION_ORDER);
    }
}
