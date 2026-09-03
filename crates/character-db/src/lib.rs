//! Safe orchestration over the canonical `npc-game-profile` and `npc-memory`
//! stores.
//!
//! This crate deliberately does not define another authoritative game-profile or
//! memory database. It supplies deterministic selection, encounter identity,
//! authority-separated prompt assembly, a quarantine-first v1 importer, and
//! versioned transport records that map into the existing canonical crates.

mod database;
mod embedding;
mod encounter;
mod import;
mod prompt;
mod selection;
mod turn;

pub use database::{CharacterDatabase, CharacterDbError};
pub use embedding::*;
pub use encounter::*;
pub use import::*;
pub use prompt::*;
pub use selection::*;
pub use turn::*;

pub const CHARACTER_DB_SCHEMA_VERSION: &str = "character-db/1.0.0";
