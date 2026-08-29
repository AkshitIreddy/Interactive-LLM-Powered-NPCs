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
mod extraction;
mod filesystem;
mod journal;
mod lifecycle;
mod manifest;
mod secure_path;
mod selection;

pub use archive::*;
pub use attestation::*;
pub use catalog::*;
pub use crypto::*;
pub use downloader::*;
pub use extraction::*;
pub use filesystem::*;
pub use journal::*;
pub use lifecycle::*;
pub use manifest::*;
pub use selection::*;
