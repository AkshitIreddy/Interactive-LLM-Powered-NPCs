use crate::model::{
    validate_public_id, validate_safe_id, AvailabilityReason, BenchmarkMetric, ProductBindingV1,
    ProviderObservationSourceV1, ProviderRole,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_BASELINE_FRAME_SAMPLES: usize = 4_096;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeTurnTimingReceiptV1 {
    pub receipt_id: String,
    pub turn_id: String,
    pub provider_observation_source: ProviderObservationSourceV1,
    pub llm_provider_id: String,
    pub llm_model_id: String,
    pub llm_route_revision: String,
    pub llm_egress: String,
    pub structured_response_validated: bool,
    pub tts_provider_id: String,
    pub tts_model_id: String,
    pub tts_voice_id: Option<String>,
    pub tts_route_revision: String,
    pub tts_egress: String,
    pub cancellation_probe_terminal: bool,
    pub input_finalized_monotonic_ns: u64,
    pub identity_started_monotonic_ns: u64,
    pub identity_completed_monotonic_ns: u64,
    pub llm_submitted_monotonic_ns: u64,
    pub llm_first_token_monotonic_ns: u64,
    pub llm_completed_monotonic_ns: u64,
    pub output_tokens: u32,
    pub tts_submitted_monotonic_ns: u64,
    pub tts_first_decoded_audio_monotonic_ns: u64,
    pub tts_final_decoded_audio_monotonic_ns: u64,
    pub live_provider_receipts: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureReceiptV1 {
    pub receipt_id: String,
    pub turn_id: String,
    pub requested_monotonic_ns: u64,
    pub owned_frame_monotonic_ns: u64,
    pub target_process_matched: bool,
    pub advancing_frame: bool,
    pub protected_content: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioSubmissionReceiptV1 {
    pub receipt_id: String,
    pub turn_id: String,
    pub decoded_audio_ready_monotonic_ns: u64,
    pub endpoint_submitted_monotonic_ns: u64,
    pub source_frames: u64,
    pub device_frames: u64,
    pub source_submission_complete: bool,
    pub endpoint_drain_complete: bool,
    pub cancelled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompositorReceiptV1 {
    pub receipt_id: String,
    pub turn_id: String,
    pub source_frame_monotonic_ns: u64,
    pub composed_frame_monotonic_ns: u64,
    pub window_duration_ns: u64,
    pub frames_presented: u32,
    pub stale_outputs_dropped: u32,
    pub target_generation_matched: bool,
    pub frame_presented: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessLoadSampleV1 {
    pub sampled_monotonic_ns: u64,
    pub process_cpu_percent: Option<f64>,
    pub device_gpu_percent: Option<f64>,
    pub game_frame_time_ms: Option<f64>,
    pub game_fps: Option<f64>,
    pub cpu_unavailable_reason: Option<AvailabilityReason>,
    pub gpu_unavailable_reason: Option<AvailabilityReason>,
    pub game_frame_unavailable_reason: Option<AvailabilityReason>,
}

impl ProcessLoadSampleV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.sampled_monotonic_ns == 0 {
            return Err("process load timestamp is missing");
        }
        validate_percent_pair(self.process_cpu_percent, self.cpu_unavailable_reason)?;
        validate_percent_pair(self.device_gpu_percent, self.gpu_unavailable_reason)?;
        match (
            self.game_frame_time_ms,
            self.game_fps,
            self.game_frame_unavailable_reason,
        ) {
            (Some(frame), Some(fps), None)
                if frame.is_finite() && frame > 0.0 && fps.is_finite() && fps > 0.0 => {}
            (None, None, Some(_)) => {}
            _ => return Err("game frame sample availability is inconsistent"),
        }
        Ok(())
    }
}

fn validate_percent_pair(
    value: Option<f64>,
    reason: Option<AvailabilityReason>,
) -> Result<(), &'static str> {
    match (value, reason) {
        (Some(value), None) if value.is_finite() && (0.0..=100.0).contains(&value) => Ok(()),
        (None, Some(_)) => Ok(()),
        _ => Err("percentage sample availability is inconsistent"),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BaselineEvidenceV1 {
    pub window_started_monotonic_ns: u64,
    pub window_ended_monotonic_ns: u64,
    pub frame_samples: Vec<ProcessLoadSampleV1>,
}

impl BaselineEvidenceV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.window_started_monotonic_ns == 0
            || self.window_ended_monotonic_ns <= self.window_started_monotonic_ns
            || self.frame_samples.len() < 2
            || self.frame_samples.len() > MAX_BASELINE_FRAME_SAMPLES
        {
            return Err("baseline sample count exceeds its hard bound");
        }
        self.frame_samples.iter().try_for_each(|sample| {
            sample.validate()?;
            if sample.sampled_monotonic_ns < self.window_started_monotonic_ns
                || sample.sampled_monotonic_ns > self.window_ended_monotonic_ns
            {
                return Err("baseline sample falls outside its measured window");
            }
            Ok(())
        })
    }

    pub(crate) fn measured_window_ns(&self) -> u64 {
        self.window_ended_monotonic_ns
            .saturating_sub(self.window_started_monotonic_ns)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IterationEvidenceV1 {
    pub iteration: u16,
    pub runtime: Option<RuntimeTurnTimingReceiptV1>,
    pub capture: Option<CaptureReceiptV1>,
    pub audio: Option<AudioSubmissionReceiptV1>,
    pub compositor: Option<CompositorReceiptV1>,
    pub process_load: ProcessLoadSampleV1,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ValidatedIteration {
    pub metrics: BTreeMap<BenchmarkMetric, f64>,
    pub invalid_receipts: u32,
}

impl IterationEvidenceV1 {
    pub(crate) fn validate_and_measure(&self, binding: &ProductBindingV1) -> ValidatedIteration {
        let mut result = ValidatedIteration::default();
        if self.iteration == 0 || self.process_load.validate().is_err() {
            result.invalid_receipts = result.invalid_receipts.saturating_add(1);
        } else {
            if let Some(value) = self.process_load.process_cpu_percent {
                result
                    .metrics
                    .insert(BenchmarkMetric::ProcessCpuPercent, value);
            }
            if let Some(value) = self.process_load.device_gpu_percent {
                result
                    .metrics
                    .insert(BenchmarkMetric::DeviceGpuPercent, value);
            }
            if let Some(value) = self.process_load.game_frame_time_ms {
                result
                    .metrics
                    .insert(BenchmarkMetric::GameFrameTimeMs, value);
            }
            if let Some(value) = self.process_load.game_fps {
                result.metrics.insert(BenchmarkMetric::GameFps, value);
            }
        }

        let turn_id = self
            .runtime
            .as_ref()
            .map(|receipt| receipt.turn_id.as_str());
        match &self.runtime {
            Some(receipt) if valid_runtime(receipt, binding) => {
                result.metrics.insert(
                    BenchmarkMetric::IdentityLatencyMs,
                    duration_ms(
                        receipt.identity_started_monotonic_ns,
                        receipt.identity_completed_monotonic_ns,
                    ),
                );
                result.metrics.insert(
                    BenchmarkMetric::LlmTimeToFirstTokenMs,
                    duration_ms(
                        receipt.llm_submitted_monotonic_ns,
                        receipt.llm_first_token_monotonic_ns,
                    ),
                );
                result.metrics.insert(
                    BenchmarkMetric::LlmOutputTokens,
                    f64::from(receipt.output_tokens),
                );
                let generation_ns = receipt
                    .llm_completed_monotonic_ns
                    .saturating_sub(receipt.llm_first_token_monotonic_ns);
                let throughput = if generation_ns == 0 {
                    f64::from(receipt.output_tokens)
                } else {
                    f64::from(receipt.output_tokens) * 1_000_000_000.0 / generation_ns as f64
                };
                result
                    .metrics
                    .insert(BenchmarkMetric::LlmTokensPerSecond, throughput);
                result.metrics.insert(
                    BenchmarkMetric::TtsTimeToFirstAudioMs,
                    duration_ms(
                        receipt.tts_submitted_monotonic_ns,
                        receipt.tts_first_decoded_audio_monotonic_ns,
                    ),
                );
                result.metrics.insert(
                    BenchmarkMetric::TtsAudioCompletionMs,
                    duration_ms(
                        receipt.tts_submitted_monotonic_ns,
                        receipt.tts_final_decoded_audio_monotonic_ns,
                    ),
                );
            }
            Some(_) => result.invalid_receipts = result.invalid_receipts.saturating_add(1),
            None => {}
        }

        match &self.capture {
            Some(receipt)
                if valid_capture(receipt, turn_id)
                    && self.runtime.as_ref().is_none_or(|runtime| {
                        receipt.owned_frame_monotonic_ns <= runtime.identity_started_monotonic_ns
                    }) =>
            {
                result.metrics.insert(
                    BenchmarkMetric::CaptureLatencyMs,
                    duration_ms(
                        receipt.requested_monotonic_ns,
                        receipt.owned_frame_monotonic_ns,
                    ),
                );
            }
            Some(_) => result.invalid_receipts = result.invalid_receipts.saturating_add(1),
            None => {}
        }

        match &self.audio {
            Some(receipt)
                if valid_audio(receipt, turn_id)
                    && self.runtime.as_ref().is_none_or(|runtime| {
                        receipt.decoded_audio_ready_monotonic_ns
                            >= runtime.tts_first_decoded_audio_monotonic_ns
                            && receipt.endpoint_submitted_monotonic_ns
                                >= runtime.input_finalized_monotonic_ns
                    }) =>
            {
                result.metrics.insert(
                    BenchmarkMetric::AudioSubmissionMs,
                    duration_ms(
                        receipt.decoded_audio_ready_monotonic_ns,
                        receipt.endpoint_submitted_monotonic_ns,
                    ),
                );
                if let Some(runtime) = &self.runtime {
                    result.metrics.insert(
                        BenchmarkMetric::EndToEndTurnMs,
                        duration_ms(
                            runtime.input_finalized_monotonic_ns,
                            receipt.endpoint_submitted_monotonic_ns,
                        ),
                    );
                }
            }
            Some(_) => result.invalid_receipts = result.invalid_receipts.saturating_add(1),
            None => {}
        }

        match &self.compositor {
            Some(receipt) if valid_compositor(receipt, turn_id) => {
                result.metrics.insert(
                    BenchmarkMetric::CompositorLatencyMs,
                    duration_ms(
                        receipt.source_frame_monotonic_ns,
                        receipt.composed_frame_monotonic_ns,
                    ),
                );
                result.metrics.insert(
                    BenchmarkMetric::CompositorFps,
                    f64::from(receipt.frames_presented) * 1_000_000_000.0
                        / receipt.window_duration_ns as f64,
                );
                result.metrics.insert(
                    BenchmarkMetric::CompositorStaleDrops,
                    f64::from(receipt.stale_outputs_dropped),
                );
            }
            Some(_) => result.invalid_receipts = result.invalid_receipts.saturating_add(1),
            None => {}
        }
        result
    }
}

fn valid_runtime(receipt: &RuntimeTurnTimingReceiptV1, binding: &ProductBindingV1) -> bool {
    let llm = binding
        .provider_routes
        .iter()
        .find(|route| route.role == ProviderRole::Llm);
    let tts = binding
        .provider_routes
        .iter()
        .find(|route| route.role == ProviderRole::Tts);
    validate_safe_id(&receipt.receipt_id, 96).is_ok()
        && validate_safe_id(&receipt.turn_id, 96).is_ok()
        && receipt.provider_observation_source == ProviderObservationSourceV1::ProductRuntimeReceipt
        && validate_public_id(&receipt.llm_provider_id, 96).is_ok()
        && validate_public_id(&receipt.llm_model_id, 96).is_ok()
        && validate_public_id(&receipt.llm_route_revision, 96).is_ok()
        && validate_public_id(&receipt.llm_egress, 192).is_ok()
        && receipt.llm_egress.starts_with("provider_cloud:")
        && validate_public_id(&receipt.tts_provider_id, 96).is_ok()
        && validate_public_id(&receipt.tts_model_id, 96).is_ok()
        && receipt
            .tts_voice_id
            .as_deref()
            .is_none_or(|value| validate_public_id(value, 128).is_ok())
        && validate_public_id(&receipt.tts_route_revision, 96).is_ok()
        && validate_public_id(&receipt.tts_egress, 192).is_ok()
        && receipt.tts_egress.starts_with("provider_cloud:")
        && receipt.structured_response_validated
        && receipt.cancellation_probe_terminal
        && llm.is_some_and(|route| {
            route.execution_mode == crate::EvidenceExecutionMode::Live
                && route.provider_id == receipt.llm_provider_id
                && route.model_id == receipt.llm_model_id
                && route.voice_id.is_none()
                && route.route_revision == receipt.llm_route_revision
                && route.egress == receipt.llm_egress
        })
        && tts.is_some_and(|route| {
            route.execution_mode == crate::EvidenceExecutionMode::Live
                && route.provider_id == receipt.tts_provider_id
                && route.model_id == receipt.tts_model_id
                && route.voice_id == receipt.tts_voice_id
                && route.route_revision == receipt.tts_route_revision
                && route.egress == receipt.tts_egress
        })
        && receipt.live_provider_receipts
        && receipt.input_finalized_monotonic_ns > 0
        && receipt.identity_started_monotonic_ns >= receipt.input_finalized_monotonic_ns
        && receipt.identity_completed_monotonic_ns >= receipt.identity_started_monotonic_ns
        && receipt.llm_submitted_monotonic_ns >= receipt.identity_completed_monotonic_ns
        && receipt.llm_first_token_monotonic_ns >= receipt.llm_submitted_monotonic_ns
        && receipt.llm_completed_monotonic_ns >= receipt.llm_first_token_monotonic_ns
        && receipt.output_tokens > 0
        && receipt.tts_submitted_monotonic_ns >= receipt.llm_first_token_monotonic_ns
        && receipt.tts_first_decoded_audio_monotonic_ns >= receipt.tts_submitted_monotonic_ns
        && receipt.tts_final_decoded_audio_monotonic_ns
            >= receipt.tts_first_decoded_audio_monotonic_ns
}

fn valid_capture(receipt: &CaptureReceiptV1, turn_id: Option<&str>) -> bool {
    validate_safe_id(&receipt.receipt_id, 96).is_ok()
        && validate_safe_id(&receipt.turn_id, 96).is_ok()
        && turn_id.is_none_or(|expected| receipt.turn_id == expected)
        && receipt.requested_monotonic_ns > 0
        && receipt.owned_frame_monotonic_ns >= receipt.requested_monotonic_ns
        && receipt.target_process_matched
        && receipt.advancing_frame
        && !receipt.protected_content
}

fn valid_audio(receipt: &AudioSubmissionReceiptV1, turn_id: Option<&str>) -> bool {
    validate_safe_id(&receipt.receipt_id, 96).is_ok()
        && validate_safe_id(&receipt.turn_id, 96).is_ok()
        && turn_id.is_none_or(|expected| receipt.turn_id == expected)
        && receipt.decoded_audio_ready_monotonic_ns > 0
        && receipt.endpoint_submitted_monotonic_ns >= receipt.decoded_audio_ready_monotonic_ns
        && receipt.source_frames > 0
        && receipt.device_frames > 0
        && receipt.source_submission_complete
        && receipt.endpoint_drain_complete
        && !receipt.cancelled
}

fn valid_compositor(receipt: &CompositorReceiptV1, turn_id: Option<&str>) -> bool {
    validate_safe_id(&receipt.receipt_id, 96).is_ok()
        && validate_safe_id(&receipt.turn_id, 96).is_ok()
        && turn_id.is_none_or(|expected| receipt.turn_id == expected)
        && receipt.source_frame_monotonic_ns > 0
        && receipt.composed_frame_monotonic_ns >= receipt.source_frame_monotonic_ns
        && receipt.window_duration_ns > 0
        && receipt.frames_presented > 0
        && receipt.target_generation_matched
        && receipt.frame_presented
}

fn duration_ms(start_ns: u64, end_ns: u64) -> f64 {
    end_ns.saturating_sub(start_ns) as f64 / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EvidenceExecutionMode, ProviderRouteBindingV1};

    fn binding() -> ProductBindingV1 {
        ProductBindingV1 {
            selected_game_pid: Some(42),
            game_profile_id: "fixture-game".into(),
            executable_sha256: "a".repeat(64),
            target_instance_recorded: false,
            loadout_revision: "loadout-1".into(),
            provider_routes: vec![
                ProviderRouteBindingV1 {
                    role: ProviderRole::Llm,
                    provider_id: "cohere".into(),
                    model_id: "command-a-plus-05-2026".into(),
                    voice_id: None,
                    route_revision: "catalog-7".into(),
                    egress: "provider_cloud:transcript.game_context".into(),
                    execution_mode: EvidenceExecutionMode::Live,
                },
                ProviderRouteBindingV1 {
                    role: ProviderRole::Tts,
                    provider_id: "elevenlabs".into(),
                    model_id: "eleven_flash_v2_5".into(),
                    voice_id: Some("EXAVITQu4vr4xnSDxMaL".into()),
                    route_revision: "catalog-7".into(),
                    egress: "provider_cloud:response_text".into(),
                    execution_mode: EvidenceExecutionMode::Live,
                },
            ],
        }
    }

    fn receipt() -> RuntimeTurnTimingReceiptV1 {
        RuntimeTurnTimingReceiptV1 {
            receipt_id: "runtime-1".into(),
            turn_id: "turn-1".into(),
            provider_observation_source: ProviderObservationSourceV1::ProductRuntimeReceipt,
            llm_provider_id: "cohere".into(),
            llm_model_id: "command-a-plus-05-2026".into(),
            llm_route_revision: "catalog-7".into(),
            llm_egress: "provider_cloud:transcript.game_context".into(),
            structured_response_validated: true,
            tts_provider_id: "elevenlabs".into(),
            tts_model_id: "eleven_flash_v2_5".into(),
            tts_voice_id: Some("EXAVITQu4vr4xnSDxMaL".into()),
            tts_route_revision: "catalog-7".into(),
            tts_egress: "provider_cloud:response_text".into(),
            cancellation_probe_terminal: true,
            input_finalized_monotonic_ns: 1,
            identity_started_monotonic_ns: 2,
            identity_completed_monotonic_ns: 3,
            llm_submitted_monotonic_ns: 4,
            llm_first_token_monotonic_ns: 5,
            llm_completed_monotonic_ns: 6,
            output_tokens: 1,
            tts_submitted_monotonic_ns: 7,
            tts_first_decoded_audio_monotonic_ns: 8,
            tts_final_decoded_audio_monotonic_ns: 9,
            live_provider_receipts: true,
        }
    }

    #[test]
    fn provider_timings_require_exact_product_routes_structure_and_terminal_cancel_probe() {
        let binding = binding();
        let valid = receipt();
        assert!(valid_runtime(&valid, &binding));

        for source in [
            ProviderObservationSourceV1::QualificationArtifact,
            ProviderObservationSourceV1::SyntheticFixture,
            ProviderObservationSourceV1::HistoricalObservation,
        ] {
            let mut non_product = valid.clone();
            non_product.provider_observation_source = source;
            assert!(!valid_runtime(&non_product, &binding));
        }

        let mut wrong_voice = valid.clone();
        wrong_voice.tts_voice_id = Some("different-stock-voice".into());
        assert!(!valid_runtime(&wrong_voice, &binding));

        let mut unstructured = valid.clone();
        unstructured.structured_response_validated = false;
        assert!(!valid_runtime(&unstructured, &binding));

        let mut no_cancel_proof = valid;
        no_cancel_proof.cancellation_probe_terminal = false;
        assert!(!valid_runtime(&no_cancel_proof, &binding));
    }
}
