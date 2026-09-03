//! Bounded orchestration for the in-app **This PC** benchmark.
//!
//! This crate deliberately does not know how to call an LLM, TTS provider, game,
//! capture API, audio endpoint, or compositor. Product adapters supply typed
//! receipts through [`LiveBenchmarkProbe`]. The coordinator validates those
//! receipts, samples the existing system-telemetry boundary, aggregates only
//! observed values, and persists a redacted report. Missing evidence stays
//! missing; fixture or planning values can never be promoted to live evidence.

mod aggregate;
mod evidence;
mod manager;
mod model;
mod store;

pub use evidence::{
    AudioSubmissionReceiptV1, BaselineEvidenceV1, CaptureReceiptV1, CompositorReceiptV1,
    IterationEvidenceV1, ProcessLoadSampleV1, RuntimeTurnTimingReceiptV1,
};
pub use manager::{
    BenchmarkManager, BenchmarkManagerError, LiveBenchmarkProbe, NativeTelemetryCollector,
    ProbeError, ResourceTelemetryCollector, TelemetryPairV1,
};
pub use model::{
    ActionCode, AvailabilityReason, BenchmarkClassificationV1, BenchmarkComponent, BenchmarkMetric,
    BenchmarkReportV1, BenchmarkRunRequestV1, BenchmarkRunState, BenchmarkStatusV1,
    ComponentReadinessV1, EvidenceExecutionMode, FrameImpactV1, HardwareEvidenceV1,
    MeasurementAvailability, MeasurementKind, MetricSummaryV1, PersistenceEvidenceV1,
    PreflightEvidenceV1, ProbeProvenanceV1, ProductBindingV1, ProviderObservationSourceV1,
    ProviderRole, ProviderRouteBindingV1, RunBoundsV1, SourceCoverageV1,
    BENCHMARK_REPORT_SCHEMA_V1, BENCHMARK_REQUEST_SCHEMA_V1,
};
pub use store::{BenchmarkReportStore, ReportStoreError};
