//! Conservative, game-agnostic actor continuity and identity decisions.
//!
//! The crate consumes detections produced by another component. It does not
//! capture a game, run a face detector, infer demographics, or mutate another
//! process. Recognition is deliberately conservative: callers receive explicit
//! pending, ambiguous, and no-match states instead of a forced identity.

mod embedding;
mod engine;
mod geometry;
mod identity;
mod qualified;
mod tracker;

pub use embedding::{
    EmbeddingError, EmbeddingMetadataV1, EmbeddingModelV1, NormalizedEmbeddingV1,
    EMBEDDING_SCHEMA_VERSION,
};
pub use engine::{ActorIdentityEngineV1, ActorIdentityV1, EngineError, FrameIdentityUpdateV1};
pub use geometry::{BoundingBoxV1, GeometryError};
pub use identity::{
    IdentityConfigV1, IdentityDecisionV1, IdentityError, IdentityGalleryV1, IdentityReferenceV1,
    IdentityResolverV1, SubjectIdentityV1,
};
pub use qualified::*;
pub use tracker::{
    ActorDetectionV1, ActorTrackSnapshotV1, ActorTrackerV1, FrameActorsV1, TrackEpoch,
    TrackEventV1, TrackId, TrackPhaseV1, TrackedDetectionV1, TrackerConfigV1, TrackerError,
    TrackerUpdateV1,
};
