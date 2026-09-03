//! Stable contracts shared by the desktop shell, runtime, media broker, and workers.
//!
//! The crate deliberately separates protobuf wire messages from JSON-authored catalog
//! data. Wire messages are bounded, ordered, versioned, and cancellation-aware.
//! Game-profile wire summaries and model manifests are human-reviewable serde
//! DTOs with explicit validation rather than opaque executable configuration.
//! The authoritative authored profile model lives in `npc-game-profile`; this
//! crate exports the intentionally distinct [`GameProfileWireV2`] boundary.

pub mod effects;
pub mod envelope;
pub mod error;
pub mod events;
pub mod ids;
pub mod model;
pub mod profile;
pub mod provider;
pub mod safety;
pub mod version;

pub use effects::*;
pub use envelope::*;
pub use error::*;
pub use events::*;
pub use ids::*;
pub use model::*;
pub use profile::*;
pub use provider::*;
pub use safety::*;
pub use version::*;

/// Maximum accepted protobuf frame, including the length prefix and envelope.
pub const DEFAULT_MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
/// Maximum encoded body inside an envelope.
pub const DEFAULT_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
/// Hard upper bound for a single streaming audio chunk.
pub const MAX_AUDIO_CHUNK_BYTES: usize = 1024 * 1024;
/// Hard upper bound for a single text field received from a provider.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;
