//! Trusted runtime-host boundary.
//!
//! This process owns policy, profile/catalog loading, the authoritative SQLite
//! database, control IPC, and worker lifecycle boundaries. It deliberately does
//! not accept credentials or conversation text in diagnostics/log labels.

pub mod bootstrap;
pub mod cli;
pub mod control;
pub mod doctor;
pub mod framing;
pub mod profile_replays;
pub mod profiles;
pub mod simulation;
pub mod supervisor;

pub use bootstrap::{CatalogTrustState, HostConfig, HostState};
pub use control::{ControlRequest, ControlResponse, ServeOptions};
pub use doctor::{DoctorReport, DoctorStatus};
pub use profiles::{LoadedProfile, ProfileCorpus, RuntimeIntegrationPolicy};
pub use simulation::{SimulationRequest, SimulationResult};

pub const APPLICATION_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REQUIRED_PROFILE_COUNT: usize = 20;
pub const MAX_CONTROL_MESSAGE_BYTES: usize = 1024 * 1024;
