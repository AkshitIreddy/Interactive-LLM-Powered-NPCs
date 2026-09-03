//! Typed diagnostics designed around data minimization.
//!
//! The builder intentionally has no API for attaching arbitrary files, raw
//! logs, prompts, transcripts, screenshots, audio, or minidumps. Sensitive
//! artifacts require a separate, explicit user flow in the desktop shell.

mod canary;
mod check;
mod egress;
mod event;
mod export;
mod matrix;
mod privacy_proof;
mod recovery;
mod redaction;
mod report;
mod store;

pub use canary::{CanaryScanResult, CanarySurface, SecretCanarySuite, SecretPatternScan};
pub use check::{
    CheckCategory, CheckResult, CheckValidationError, DiagnosticMetric, MetricUnit,
    ObservationProvenance, SuggestedAction, SuggestedActionKind, MAX_CHECK_METRICS,
    MAX_CHECK_RESULTS, MAX_SUGGESTED_ACTIONS,
};
pub use egress::{
    DataClass, EgressDecision, EgressManifest, ExportInitiation, LocalDiagnosticDataClass,
    NetworkMode, NetworkPolicy, PrivacyEgressDeclaration, TelemetryPolicy,
};
pub use event::{
    ClockSource, DiagnosticEvent, DiagnosticStatus, DiagnosticVerbosity, Severity, TimingEvidence,
};
pub use export::{
    validate_export, DiagnosticsExport, DiagnosticsExportBuilder, DiagnosticsExportError,
    DiagnosticsExportPreview, DIAGNOSTICS_EXPORT_SCHEMA_JSON, MAX_EXPORT_BYTES, MAX_EXPORT_EVENTS,
};
pub use matrix::{
    CredentialPresenceState, DiagnosticMatrixBuilder, DiagnosticMatrixError, DiagnosticMatrixKind,
    DiagnosticMatrixResult, ProviderCredentialPresence, PRODUCT_DIAGNOSTIC_MATRIX,
};
pub use privacy_proof::{
    ArtifactIdentity, DenyAllEgressProof, DenyAllScenario, NetworkEnforcement, PrivacyProofBundle,
    PrivacyProofError, ProofEvidenceSource, ProofOutcome, RemoteTelemetryAbsenceProof,
    PRIVACY_PROOF_SCHEMA_JSON,
};
pub use recovery::{
    CrashMarker, CrashMarkerStore, PreviousExitStatus, PreviousSessionMetadata, RecoveryError,
    RecoveryMetadata, RecoveryPhase,
};
pub use redaction::{RedactionFinding, RedactionKind, Redactor};
pub use report::{
    DiagnosticFact, DiagnosticReport, DiagnosticReportBuilder, ExportPreview, ReportError,
};
pub use store::{
    EventReadResult, EventStoreError, EventWriteReceipt, LocalEventLog, LocalEventLogConfig,
    StoredDiagnosticEvent,
};
