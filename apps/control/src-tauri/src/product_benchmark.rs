use crate::media_broker::MediaBrokerSupervisor;
use crate::sidecar_supervisor::RuntimeSupervisor;
use async_trait::async_trait;
use npc_product_benchmark::{
    ActionCode, AvailabilityReason, BaselineEvidenceV1, BenchmarkComponent, BenchmarkRunRequestV1,
    ComponentReadinessV1, EvidenceExecutionMode, IterationEvidenceV1, LiveBenchmarkProbe,
    MeasurementKind, PreflightEvidenceV1, ProbeError, ProcessLoadSampleV1,
    ProviderObservationSourceV1,
};
use npc_system_telemetry::{collect, Observation, ResourceTelemetrySnapshotV1, TelemetryRequest};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Product-backed benchmark probe. It inspects the real supervised runtime and
/// broker state, but never substitutes fixtures when one of the receipt
/// producers is not connected to the benchmark coordinator yet.
pub struct ControlBenchmarkProbe {
    runtime: RuntimeSupervisor,
    media_broker: MediaBrokerSupervisor,
}

impl ControlBenchmarkProbe {
    pub fn new(runtime: RuntimeSupervisor, media_broker: MediaBrokerSupervisor) -> Self {
        Self {
            runtime,
            media_broker,
        }
    }

    fn resource_snapshots(selected_game_pid: Option<u32>) -> ResourceSnapshots {
        ResourceSnapshots {
            application: collect(TelemetryRequest {
                selected_game_pid: Some(std::process::id()),
                ..TelemetryRequest::default()
            }),
            game: collect(TelemetryRequest {
                selected_game_pid,
                ..TelemetryRequest::default()
            }),
        }
    }
}

struct ResourceSnapshots {
    application: ResourceTelemetrySnapshotV1,
    game: ResourceTelemetrySnapshotV1,
}

impl ResourceSnapshots {
    fn ram_ready(&self) -> bool {
        observation_available(&self.application.selected_game_working_set_bytes)
            && observation_available(&self.game.selected_game_working_set_bytes)
    }

    fn vram_ready(&self) -> bool {
        observation_available(&self.application.selected_game_vram_bytes)
            && observation_available(&self.game.selected_game_vram_bytes)
    }
}

fn observation_available<T>(observation: &Observation<T>) -> bool {
    observation.value().is_some()
}

fn monotonic_ns(origin: Instant) -> u64 {
    origin.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64 + 1
}

fn unavailable_process_load(sampled_monotonic_ns: u64) -> ProcessLoadSampleV1 {
    ProcessLoadSampleV1 {
        sampled_monotonic_ns,
        process_cpu_percent: None,
        device_gpu_percent: None,
        game_frame_time_ms: None,
        game_fps: None,
        cpu_unavailable_reason: Some(AvailabilityReason::OperatingSystemMetricUnavailable),
        gpu_unavailable_reason: Some(AvailabilityReason::DriverMetricUnavailable),
        game_frame_unavailable_reason: Some(AvailabilityReason::CounterDiscontinuity),
    }
}

#[async_trait]
impl LiveBenchmarkProbe for ControlBenchmarkProbe {
    async fn preflight(
        &self,
        request: &BenchmarkRunRequestV1,
        _cancellation: &CancellationToken,
    ) -> Result<PreflightEvidenceV1, ProbeError> {
        let runtime_connected = self.runtime.health().connected;
        let broker_connected = self.media_broker.health().connected;
        let has_target = request.binding.selected_game_pid.is_some();
        let has_live_llm = request.binding.provider_routes.iter().any(|route| {
            route.role == npc_product_benchmark::ProviderRole::Llm
                && route.execution_mode == EvidenceExecutionMode::Live
        });
        let has_live_tts = request.binding.provider_routes.iter().any(|route| {
            route.role == npc_product_benchmark::ProviderRole::Tts
                && route.execution_mode == EvidenceExecutionMode::Live
        });
        let resources = Self::resource_snapshots(request.binding.selected_game_pid);
        let mut components = Vec::with_capacity(BenchmarkComponent::all().len());
        components.push(if has_target {
            ComponentReadinessV1::ready(BenchmarkComponent::SelectedGameTarget)
        } else {
            ComponentReadinessV1::unavailable(
                BenchmarkComponent::SelectedGameTarget,
                AvailabilityReason::NoSelectedGameTarget,
                ActionCode::SelectRunningGame,
            )
        });
        components.push(ComponentReadinessV1::unavailable(
            BenchmarkComponent::AdvancingCapture,
            if has_target {
                AvailabilityReason::CaptureReceiptUnavailable
            } else {
                AvailabilityReason::NoSelectedGameTarget
            },
            if has_target {
                ActionCode::WaitForAdvancingFrame
            } else {
                ActionCode::SelectRunningGame
            },
        ));
        components.push(ComponentReadinessV1::unavailable(
            BenchmarkComponent::IdentityTiming,
            AvailabilityReason::RuntimeTimingReceiptUnavailable,
            ActionCode::ReviewDiagnostics,
        ));
        components.push(ComponentReadinessV1::unavailable(
            BenchmarkComponent::LiveLlmProvider,
            if runtime_connected && has_live_llm {
                AvailabilityReason::RuntimeTimingReceiptUnavailable
            } else {
                AvailabilityReason::NoLiveProviderRoute
            },
            if has_live_llm {
                ActionCode::RetryBenchmark
            } else {
                ActionCode::ChooseQualifiedLiveLlm
            },
        ));
        components.push(ComponentReadinessV1::unavailable(
            BenchmarkComponent::LiveTtsProvider,
            if runtime_connected && has_live_tts {
                AvailabilityReason::RuntimeTimingReceiptUnavailable
            } else {
                AvailabilityReason::NoLiveProviderRoute
            },
            if has_live_tts {
                ActionCode::RetryBenchmark
            } else {
                ActionCode::ChooseQualifiedLiveTts
            },
        ));
        components.push(ComponentReadinessV1::unavailable(
            BenchmarkComponent::BrokerAudioSubmission,
            AvailabilityReason::AudioReceiptUnavailable,
            if broker_connected {
                ActionCode::EnableAudioOutput
            } else {
                ActionCode::StartMediaBroker
            },
        ));
        components.push(ComponentReadinessV1::unavailable(
            BenchmarkComponent::NativeCompositor,
            AvailabilityReason::CompositorReceiptUnavailable,
            ActionCode::EnableVisualPath,
        ));
        components.push(ComponentReadinessV1::unavailable(
            BenchmarkComponent::ProcessCpuSampler,
            AvailabilityReason::OperatingSystemMetricUnavailable,
            ActionCode::ReviewDiagnostics,
        ));
        components.push(ComponentReadinessV1::unavailable(
            BenchmarkComponent::DeviceGpuSampler,
            AvailabilityReason::DriverMetricUnavailable,
            ActionCode::UpdateGraphicsDriver,
        ));
        components.push(if resources.ram_ready() {
            ComponentReadinessV1::ready(BenchmarkComponent::SystemRamTelemetry)
        } else {
            ComponentReadinessV1::unavailable(
                BenchmarkComponent::SystemRamTelemetry,
                AvailabilityReason::OperatingSystemMetricUnavailable,
                ActionCode::ReviewDiagnostics,
            )
        });
        components.push(if resources.vram_ready() {
            ComponentReadinessV1::ready(BenchmarkComponent::ProcessVramTelemetry)
        } else {
            ComponentReadinessV1::unavailable(
                BenchmarkComponent::ProcessVramTelemetry,
                AvailabilityReason::DriverMetricUnavailable,
                ActionCode::UpdateGraphicsDriver,
            )
        });
        components.push(ComponentReadinessV1::unavailable(
            BenchmarkComponent::GameFrameSampler,
            if has_target {
                AvailabilityReason::CounterDiscontinuity
            } else {
                AvailabilityReason::NoSelectedGameTarget
            },
            if has_target {
                ActionCode::WaitForAdvancingFrame
            } else {
                ActionCode::SelectRunningGame
            },
        ));

        Ok(PreflightEvidenceV1 {
            execution_mode: if cfg!(windows) {
                EvidenceExecutionMode::Live
            } else {
                EvidenceExecutionMode::Mocked
            },
            measurement_kind: MeasurementKind::Measured,
            provider_observation_source: ProviderObservationSourceV1::ProductRuntimeReceipt,
            runtime_revision: "runtime-host-v2".into(),
            broker_revision: "native-media-broker-v1".into(),
            compositor_revision: "native-subtitle-presenter-v1".into(),
            process_load_sampler_revision: "system-telemetry-v1".into(),
            game_frame_sampler_revision: "unavailable-v1".into(),
            components,
        })
    }

    async fn collect_baseline(
        &self,
        request: &BenchmarkRunRequestV1,
        cancellation: &CancellationToken,
    ) -> Result<BaselineEvidenceV1, ProbeError> {
        if request.binding.selected_game_pid.is_none() {
            return Err(ProbeError::TargetChanged);
        }
        let origin = Instant::now();
        let started = monotonic_ns(origin);
        let first = unavailable_process_load(started);
        tokio::select! {
            () = cancellation.cancelled() => return Err(ProbeError::TimedOut),
            () = tokio::time::sleep(Duration::from_millis(request.baseline_window_millis)) => {}
        }
        let ended = monotonic_ns(origin);
        Ok(BaselineEvidenceV1 {
            window_started_monotonic_ns: started,
            window_ended_monotonic_ns: ended,
            frame_samples: vec![first, unavailable_process_load(ended)],
        })
    }

    async fn measure_iteration(
        &self,
        request: &BenchmarkRunRequestV1,
        iteration: u16,
        cancellation: &CancellationToken,
    ) -> Result<IterationEvidenceV1, ProbeError> {
        if cancellation.is_cancelled() {
            return Err(ProbeError::TimedOut);
        }
        if request.binding.selected_game_pid.is_none() {
            return Err(ProbeError::TargetChanged);
        }
        let sampled = Self::resource_snapshots(request.binding.selected_game_pid);
        if !sampled.ram_ready() && !sampled.vram_ready() {
            return Err(ProbeError::RuntimeUnavailable);
        }
        Ok(IterationEvidenceV1 {
            iteration,
            runtime: None,
            capture: None,
            audio: None,
            compositor: None,
            process_load: unavailable_process_load(
                sampled
                    .game
                    .captured_monotonic_millis
                    .saturating_mul(1_000_000)
                    .max(1),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_broker::MediaBrokerLaunchConfig;
    use crate::sidecar_supervisor::RuntimeLaunchConfig;
    use npc_product_benchmark::{
        BenchmarkManager, BenchmarkMetric, BenchmarkReportStore, BenchmarkRunState,
        NativeTelemetryCollector, ProductBindingV1, BENCHMARK_REQUEST_SCHEMA_V1,
    };
    use std::sync::Arc;

    #[test]
    fn product_request_schema_never_serializes_target_pid() {
        let request = BenchmarkRunRequestV1 {
            schema_version: BENCHMARK_REQUEST_SCHEMA_V1.into(),
            requested_iterations: 3,
            timeout_millis: 10_000,
            baseline_window_millis: 1_000,
            binding: ProductBindingV1 {
                selected_game_pid: Some(42),
                game_profile_id: "skyrim-special-edition".into(),
                executable_sha256: "a".repeat(64),
                target_instance_recorded: false,
                loadout_revision: "loadout-v1".into(),
                provider_routes: Vec::new(),
            },
        };
        let wire = serde_json::to_value(request).expect("serialize benchmark request");
        assert!(wire["binding"].get("selectedGamePid").is_none());
    }

    #[test]
    fn unavailable_process_sample_is_valid_and_never_fabricates_counters() {
        let sample = unavailable_process_load(9);
        sample.validate().expect("explicit unavailable sample");
        assert_eq!(sample.process_cpu_percent, None);
        assert_eq!(sample.device_gpu_percent, None);
        assert_eq!(sample.game_frame_time_ms, None);
        assert_eq!(sample.game_fps, None);
    }

    #[tokio::test]
    async fn live_windows_resource_probe_finishes_a_bounded_partial_product_report() {
        let directory = tempfile::tempdir().expect("benchmark tempdir");
        let executable = std::env::current_exe().expect("current test executable");
        let runtime = RuntimeSupervisor::try_new(RuntimeLaunchConfig {
            executable,
            resource_root: directory.path().to_path_buf(),
            app_data: directory.path().join("runtime-data"),
            development_fixture_allowed: false,
        })
        .expect("runtime supervisor");
        let broker = MediaBrokerSupervisor::new(
            MediaBrokerLaunchConfig::from_application(false, directory.path())
                .expect("broker config"),
            runtime.clone(),
        );
        let manager = BenchmarkManager::new(
            BenchmarkReportStore::new(directory.path().join("reports")).expect("report store"),
            Arc::new(ControlBenchmarkProbe::new(runtime, broker)),
            Arc::new(NativeTelemetryCollector),
        );
        let status = manager
            .start(BenchmarkRunRequestV1 {
                schema_version: BENCHMARK_REQUEST_SCHEMA_V1.into(),
                requested_iterations: 3,
                timeout_millis: 5_000,
                baseline_window_millis: 500,
                binding: ProductBindingV1 {
                    selected_game_pid: Some(std::process::id()),
                    game_profile_id: "native-resource-probe".into(),
                    executable_sha256: "a".repeat(64),
                    target_instance_recorded: false,
                    loadout_revision: "native-resource-probe-v1".into(),
                    provider_routes: Vec::new(),
                },
            })
            .expect("start benchmark");
        let report_id = status.report_id.expect("report id");
        let terminal = loop {
            let status = manager.status().expect("benchmark status");
            if status.state.terminal() {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_eq!(terminal.state, BenchmarkRunState::Partial);
        assert_eq!(terminal.completed_iterations, 3);
        let report = manager.report(&report_id).expect("persisted report");
        assert_eq!(report.state, BenchmarkRunState::Partial);
        assert!(report
            .metrics
            .iter()
            .any(|metric| metric.metric == BenchmarkMetric::ProcessRamBytes));
        assert!(!report.classification.acceptance_eligible);
    }
}
