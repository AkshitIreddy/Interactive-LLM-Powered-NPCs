//! Model pack manifests, trusted catalogs, and an explicit install lifecycle.
//!
//! This crate intentionally performs no network I/O. A caller supplies bytes or
//! verified artifact evidence from its downloader, and a storage implementation
//! owns staging and atomic activation. That keeps network policy, credentials,
//! and filesystem privileges outside the model-manager's trusted core.

mod archive;
mod attestation;
mod catalog;
mod crypto;
mod downloader;
mod experimental_visual_pack;
mod extraction;
mod filesystem;
mod journal;
mod lifecycle;
mod local_catalog;
mod manifest;
mod manifest_v2;
mod optional_lifecycle;
mod release_catalog;
mod resource_governor;
mod secure_path;
mod selected_loadout;
mod selection;
mod system_telemetry;

pub use archive::*;
pub use attestation::*;
pub use catalog::*;
pub use crypto::*;
pub use downloader::*;
pub use experimental_visual_pack::*;
pub use extraction::*;
pub use filesystem::*;
pub use journal::*;
pub use lifecycle::*;
pub use local_catalog::*;
pub use manifest::*;
pub use manifest_v2::*;
pub use optional_lifecycle::*;
pub use release_catalog::*;
pub use resource_governor::*;
pub use selected_loadout::*;
pub use selection::*;
