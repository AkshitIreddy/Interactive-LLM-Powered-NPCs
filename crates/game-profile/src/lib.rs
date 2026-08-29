//! Strict, versioned, data-only game profile contracts.
//!
//! Profiles are untrusted data. Loading therefore happens in layers: JSON limits,
//! JSON Schema validation, typed deserialization, and semantic/security checks.
//! A successfully loaded profile cannot request code execution, network access,
//! DLL injection, or an anti-cheat bypass.

mod migration;
mod model;
mod validation;

pub use migration::{migrate_to_v2, MigrationError, MigrationOutcome};
pub use model::*;
pub use validation::{
    load_profile, validate_json_schema, IssueCode, ProfileLoadError, ValidationIssue,
    ValidationReport, MAX_PROFILE_BYTES,
};

/// Canonical machine-readable JSON Schema bundled into the crate.
pub const GAME_PROFILE_V2_SCHEMA: &str =
    include_str!("../../../schemas/game-profile-v2.schema.json");
