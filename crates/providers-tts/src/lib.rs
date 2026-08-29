//! Hosted, streaming text-to-speech provider boundary.
//!
//! Provider-neutral adapters consume [`TtsTransport`], which keeps credentials
//! in the trusted runtime and makes every adapter deterministic under test. The
//! crate includes a bounded ElevenLabs WebSocket transport; other hosted wire
//! implementations remain host-supplied. Provider commands are explicit and
//! correspond to documented streaming protocols.

mod adapter;
mod clause;
mod elevenlabs_websocket;
mod error;
mod fallback;
mod nvidia_nim;
mod transport;
mod types;

pub use adapter::*;
pub use clause::*;
pub use elevenlabs_websocket::*;
pub use error::*;
pub use fallback::*;
pub use nvidia_nim::*;
pub use transport::*;
pub use types::*;
