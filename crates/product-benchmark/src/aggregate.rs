use crate::{
    evidence::{BaselineEvidenceV1, ValidatedIteration},
    model::{BenchmarkMetric, FrameImpactV1, MetricSummaryV1},
};
use npc_system_telemetry::ResourceTelemetrySnapshotV1;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Aggregator {
    values: BTreeMap<BenchmarkMetric, Vec<f64>>,
    baseline_frame_time_ms: Vec<f64>,
    baseline_fps: Vec<f64>,
    pub invalid_receipt_count: u32,
}

impl Aggregator {
    pub fn add_baseline(&mut self, baseline: &BaselineEvidenceV1) {
        for sample in &baseline.frame_samples {
            if sample.validate().is_ok() {
                if let Some(value) = sample.game_frame_time_ms {
                    self.baseline_frame_time_ms.push(value);
                }
                if let Some(value) = sample.game_fps {
                    self.baseline_fps.push(value);
                }
            }
        }
    }

    pub fn add_iteration(
        &mut self,
        iteration: ValidatedIteration,
        app: &ResourceTelemetrySnapshotV1,
        game: &ResourceTelemetrySnapshotV1,
    ) {
        self.invalid_receipt_count = self
            .invalid_receipt_count
            .saturating_add(iteration.invalid_receipts);
        for (metric, value) in iteration.metrics {
            self.add(metric, value);
        }
        if let Some(value) = app.selected_game_working_set_bytes.value() {
            self.add(BenchmarkMetric::ProcessRamBytes, *value as f64);
        }
        if let Some(value) = app.current_process_local_vram_bytes.value() {
            self.add(BenchmarkMetric::ProcessVramBytes, *value as f64);
        }
        if let Some(value) = game.selected_game_working_set_bytes.value() {
            self.add(BenchmarkMetric::GameRamBytes, *value as f64);
        }
        if let Some(value) = game.selected_game_vram_bytes.value() {
            self.add(BenchmarkMetric::GameVramBytes, *value as f64);
        }
    }

    fn add(&mut self, metric: BenchmarkMetric, value: f64) {
        if value.is_finite() && value >= 0.0 {
            self.values.entry(metric).or_default().push(value);
        }
    }

    pub fn metric_summaries(&self) -> Vec<MetricSummaryV1> {
        BenchmarkMetric::all()
            .into_iter()
            .filter_map(|metric| {
                self.values
                    .get(&metric)
                    .and_then(|values| summarize(metric, values))
            })
            .collect()
    }

    pub fn missing_metrics(&self) -> Vec<BenchmarkMetric> {
        BenchmarkMetric::all()
            .into_iter()
            .filter(|metric| self.values.get(metric).is_none_or(Vec::is_empty))
            .collect()
    }

    pub fn frame_impact(&self) -> FrameImpactV1 {
        let baseline_frame_time = summarize(
            BenchmarkMetric::GameFrameTimeMs,
            &self.baseline_frame_time_ms,
        );
        let active_frame_time = self
            .values
            .get(&BenchmarkMetric::GameFrameTimeMs)
            .and_then(|values| summarize(BenchmarkMetric::GameFrameTimeMs, values));
        let baseline_fps = summarize(BenchmarkMetric::GameFps, &self.baseline_fps);
        let active_fps = self
            .values
            .get(&BenchmarkMetric::GameFps)
            .and_then(|values| summarize(BenchmarkMetric::GameFps, values));
        let p50_frame_time_delta_ms = pair_delta(
            baseline_frame_time.as_ref().map(|value| value.p50),
            active_frame_time.as_ref().map(|value| value.p50),
        );
        let p95_frame_time_delta_ms = pair_delta(
            baseline_frame_time.as_ref().map(|value| value.p95),
            active_frame_time.as_ref().map(|value| value.p95),
        );
        let p50_fps_delta = pair_delta(
            baseline_fps.as_ref().map(|value| value.p50),
            active_fps.as_ref().map(|value| value.p50),
        );
        let p50_fps_impact_percent = match (
            baseline_fps.as_ref().map(|value| value.p50),
            active_fps.as_ref().map(|value| value.p50),
        ) {
            (Some(baseline), Some(active)) if baseline > 0.0 => {
                Some(round6((active - baseline) * 100.0 / baseline))
            }
            _ => None,
        };
        FrameImpactV1 {
            baseline_frame_time_ms: baseline_frame_time,
            active_frame_time_ms: active_frame_time,
            baseline_fps,
            active_fps,
            p50_frame_time_delta_ms,
            p95_frame_time_delta_ms,
            p50_fps_delta,
            p50_fps_impact_percent,
        }
    }

    pub fn has_complete_frame_impact(&self) -> bool {
        self.baseline_frame_time_ms.len() >= 2
            && self.baseline_fps.len() >= 2
            && self
                .values
                .get(&BenchmarkMetric::GameFrameTimeMs)
                .is_some_and(|values| !values.is_empty())
            && self
                .values
                .get(&BenchmarkMetric::GameFps)
                .is_some_and(|values| !values.is_empty())
    }
}

fn pair_delta(baseline: Option<f64>, active: Option<f64>) -> Option<f64> {
    baseline
        .zip(active)
        .map(|(before, during)| round6(during - before))
}

pub(crate) fn summarize(metric: BenchmarkMetric, values: &[f64]) -> Option<MetricSummaryV1> {
    if values.is_empty()
        || values
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
    {
        return None;
    }
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    Some(MetricSummaryV1 {
        metric,
        unit: metric.unit().to_owned(),
        sample_count: ordered.len(),
        minimum: round6(ordered[0]),
        mean: round6(ordered.iter().sum::<f64>() / ordered.len() as f64),
        p50: round6(percentile_r7(&ordered, 0.50)),
        p95: round6(percentile_r7(&ordered, 0.95)),
        p99: round6(percentile_r7(&ordered, 0.99)),
        maximum: round6(ordered[ordered.len() - 1]),
    })
}

fn percentile_r7(ordered: &[f64], probability: f64) -> f64 {
    let position = (ordered.len() - 1) as f64 * probability;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        return ordered[lower];
    }
    let fraction = position - lower as f64;
    ordered[lower] + (ordered[upper] - ordered[lower]) * fraction
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r7_percentiles_match_the_reference_harness() {
        let summary =
            summarize(BenchmarkMetric::CaptureLatencyMs, &[1.0, 2.0, 3.0, 4.0]).expect("summary");
        assert_eq!(summary.p50, 2.5);
        assert_eq!(summary.p95, 3.85);
        assert_eq!(summary.p99, 3.97);
    }
}
