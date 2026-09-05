//! Hosted, streaming text-to-speech provider boundary.
//!
//! Provider-neutral adapters consume [`TtsTransport`], which keeps credentials
//! in the trusted runtime and makes every adapter deterministic under test. The
//! crate includes bounded WebSocket transports for Cartesia, Deepgram,
//! ElevenLabs, and Inworld. Provider commands are explicit and correspond to
//! documented streaming protocols.

mod adapter;
mod cartesia_websocket;
mod clause;
mod deepgram_websocket;
mod elevenlabs_websocket;
mod error;
mod fallback;
mod hosted_websocket;
mod inworld_websocket;
mod nvidia_nim;
mod nvidia_riva;
mod transport;
mod types;

pub use adapter::*;
pub use cartesia_websocket::*;
pub use clause::*;
pub use deepgram_websocket::*;
pub use elevenlabs_websocket::*;
pub use error::*;
pub use fallback::*;
pub use hosted_websocket::{CartesiaConnectionStats, HostedWebSocketLimits};
pub use inworld_websocket::*;
pub use nvidia_nim::*;
pub use nvidia_riva::*;
pub use transport::*;
pub use types::*;
