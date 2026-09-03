//! Provider-neutral streaming STT contracts and hosted provider protocol adapters.
//!
//! Provider protocols remain independently testable through [`TransportFactory`]. Production
//! callers can use the bounded, destination-pinned [`HostedWebSocketTransportFactory`]; credential
//! material is borrowed only while constructing the authenticated upgrade request.

mod contract;
mod error;
mod hosted;
mod nvidia;
mod providers;
mod transport;
mod websocket;

pub use contract::*;
pub use error::*;
pub use hosted::{HostedRecognizer, HostedSession};
pub use nvidia::*;
pub use providers::{
    AssemblyAi, DeepgramFlux, ElevenLabsScribe, OpenAiRealtime, ProviderProtocol,
    ProviderSessionProtocol, WireEvent, WireTranscript,
};
pub use transport::*;
pub use websocket::*;
