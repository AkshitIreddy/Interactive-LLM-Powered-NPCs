use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use tokio_util::sync::CancellationToken;

/// Process-local monotonic cancellation generation.
///
/// Generation checks complement cooperative cancellation: adapters can return late
/// after cancellation and their output will still be rejected deterministically.
#[derive(Clone, Debug, Default)]
pub struct CancellationGeneration {
    current: Arc<AtomicU64>,
}

impl CancellationGeneration {
    pub fn current(&self) -> u64 {
        self.current.load(Ordering::Acquire)
    }

    pub fn next(&self) -> GenerationToken {
        let generation = self.current.fetch_add(1, Ordering::AcqRel) + 1;
        GenerationToken {
            generation,
            current: Arc::clone(&self.current),
            cancellation: CancellationToken::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GenerationToken {
    generation: u64,
    current: Arc<AtomicU64>,
    cancellation: CancellationToken,
}

impl GenerationToken {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn is_current(&self) -> bool {
        self.current.load(Ordering::Acquire) == self.generation
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled() || !self.is_current()
    }

    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub async fn cancelled(&self) {
        if !self.is_current() {
            return;
        }
        self.cancellation.cancelled().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advancing_generation_makes_old_tokens_stale() {
        let source = CancellationGeneration::default();
        let first = source.next();
        assert!(first.is_current());
        let second = source.next();
        assert!(!first.is_current());
        assert!(first.is_cancelled());
        assert!(second.is_current());
    }
}
