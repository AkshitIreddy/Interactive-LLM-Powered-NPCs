//! Store-aware game discovery for Interactive LLM Powered NPCs.
//!
//! Discovery returns evidence rather than a bare boolean. Profiles can then
//! decide whether a candidate is safe and compatible without trusting folder
//! names, localized window titles, or one unstable launcher database.

mod epic;
mod evidence;
mod filesystem;
mod gog;
mod manual;
mod path_security;
mod scanner;
mod steam;
mod vdf;

#[cfg(windows)]
mod windows;

pub use epic::{parse_epic_item, parse_launcher_installed, EpicInstall};
pub use evidence::{
    Confidence, DetectionSource, EditionEvidence, InstallationCandidate, InstallationEvidence,
    StoreKind,
};
pub use filesystem::{EpicFilesystemScanner, GogFilesystemScanner, SteamFilesystemScanner};
pub use gog::{parse_gog_info, GogInstall};
pub use manual::{validate_manual_executable, ManualSelectionError};
pub use path_security::StoreRelativePathError;
pub use scanner::{DiscoveryError, DiscoveryService, StoreScanner};
pub use steam::{parse_app_manifest, parse_library_folders, SteamApp, SteamLibrary};

#[cfg(windows)]
pub use windows::WindowsStoreLocator;
