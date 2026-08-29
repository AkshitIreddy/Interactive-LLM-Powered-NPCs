//! Provider-neutral streaming STT contracts and hosted provider protocol adapters.
//!
//! This crate deliberately contains no WebSocket implementation. The application injects a
//! [`TransportFactory`], which keeps credentials in the trusted native process and makes every
//! provider protocol deterministically testable without network access.

mod contract;
mod error;
mod hosted;
mod nvidia;
mod providers;
mod transport;

pub use contract::*;
pub use error::*;
pub use hosted::{HostedRecognizer, HostedSession};
pub use nvidia::*;
pub use providers::{
    AssemblyAi, DeepgramFlux, ElevenLabsScribe, OpenAiRealtime, ProviderProtocol,
    ProviderSessionProtocol, WireEvent, WireTranscript,
};
pub use transport::*;
