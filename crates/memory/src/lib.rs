//! Durable, local-first memory for the NPC runtime.
//!
//! The database is authoritative for textual memory. Embeddings are explicitly
//! derived, generation-scoped data: deleting or rebuilding them never deletes
//! the underlying memory. All writes are serialized through one dedicated
//! thread while reads use independent query-only WAL connections.

mod domain;
mod retrieval;
mod schema;
mod store;

pub use domain::*;
pub use retrieval::{rank_candidates, CandidateRanks};
pub use store::{MemoryStore, StoreOptions};
