//! Typed diagnostics designed around data minimization.
//!
//! The builder intentionally has no API for attaching arbitrary files, raw
//! logs, prompts, transcripts, screenshots, audio, or minidumps. Sensitive
//! artifacts require a separate, explicit user flow in the desktop shell.

mod egress;
mod event;
mod redaction;
mod report;

pub use egress::{DataClass, EgressDecision, EgressManifest, NetworkMode, NetworkPolicy};
pub use event::{DiagnosticEvent, DiagnosticStatus, Severity};
pub use redaction::{RedactionFinding, RedactionKind, Redactor};
pub use report::{
    DiagnosticFact, DiagnosticReport, DiagnosticReportBuilder, ExportPreview, ReportError,
};
