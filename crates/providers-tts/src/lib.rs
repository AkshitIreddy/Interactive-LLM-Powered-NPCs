//! Hosted, streaming text-to-speech provider boundary.
//!
//! The crate deliberately does not contain an HTTP/WebSocket implementation. A
//! host supplies [`TtsTransport`], which keeps credentials in the trusted runtime
//! and makes every adapter deterministic under test. Provider wire commands are
//! explicit and correspond to the providers' documented streaming protocols.

mod adapter;
mod clause;
mod error;
mod fallback;
mod nvidia_nim;
mod transport;
mod types;

pub use adapter::*;
pub use clause::*;
pub use error::*;
pub use fallback::*;
pub use nvidia_nim::*;
pub use transport::*;
pub use types::*;
