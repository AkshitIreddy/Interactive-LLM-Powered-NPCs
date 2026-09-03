//! Runtime orchestration for Interactive LLM Powered NPCs 2.0.
//!
//! This crate deliberately owns policy and temporal correctness, but not provider
//! implementations or Windows media I/O. Provider adapters implement the traits in
//! [`provider`]; the desktop/runtime host drives [`TurnSupervisor`].

pub mod cancellation;
pub mod circuit_breaker;
pub mod policy;
pub mod provider;
pub mod resource_broker;
pub mod sentence;
pub mod structured_response;
pub mod supervisor;
pub mod timing;
pub mod types;

pub use cancellation::{CancellationGeneration, GenerationToken};
pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitRegistry};
pub use policy::{FallbackPolicy, PrivacyDecision, PrivacyPolicy, RoutingPlan};
pub use provider::*;
pub use resource_broker::*;
pub use sentence::{SentenceSegmenter, SentenceSegmenterConfig, SentenceSpan};
pub use structured_response::*;
pub use supervisor::{SupervisorConfig, TurnHandle, TurnSupervisor};
pub use timing::{SpanOutcome, TimingCollector, TimingSpan};
pub use types::*;
