use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub recovery_success_threshold: u32,
    pub open_duration: Duration,
    pub half_open_max_in_flight: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            recovery_success_threshold: 2,
            open_duration: Duration::from_secs(30),
            half_open_max_in_flight: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CircuitStateSnapshot {
    Closed { consecutive_failures: u32 },
    Open { retry_after: Duration },
    HalfOpen { in_flight: u32, successes: u32 },
}

#[derive(Clone, Debug, thiserror::Error)]
#[error("provider circuit is open; retry after {retry_after:?}")]
pub struct CircuitOpen {
    pub retry_after: Duration,
}

#[derive(Debug)]
enum CircuitState {
    Closed { consecutive_failures: u32 },
    Open { opened_at: Instant },
    HalfOpen { in_flight: u32, successes: u32 },
}

/// Thread-safe circuit breaker. A permit must be completed to record provider health.
#[derive(Clone, Debug)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: Arc<Mutex<CircuitState>>,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        assert!(
            config.failure_threshold > 0,
            "failure threshold must be non-zero"
        );
        assert!(
            config.recovery_success_threshold > 0,
            "recovery success threshold must be non-zero"
        );
        assert!(
            config.half_open_max_in_flight > 0,
            "half-open concurrency must be non-zero"
        );
        Self {
            config,
            state: Arc::new(Mutex::new(CircuitState::Closed {
                consecutive_failures: 0,
            })),
        }
    }

    pub fn try_acquire(&self) -> Result<CircuitPermit, CircuitOpen> {
        self.try_acquire_at(Instant::now())
    }

    fn try_acquire_at(&self, now: Instant) -> Result<CircuitPermit, CircuitOpen> {
        let mut state = self.state.lock().expect("circuit mutex poisoned");
        if let CircuitState::Open { opened_at } = &*state {
            let elapsed = now.saturating_duration_since(*opened_at);
            if elapsed >= self.config.open_duration {
                *state = CircuitState::HalfOpen {
                    in_flight: 0,
                    successes: 0,
                };
            } else {
                return Err(CircuitOpen {
                    retry_after: self.config.open_duration - elapsed,
                });
            }
        }

        if let CircuitState::HalfOpen { in_flight, .. } = &mut *state {
            if *in_flight >= self.config.half_open_max_in_flight {
                return Err(CircuitOpen {
                    retry_after: self.config.open_duration,
                });
            }
            *in_flight += 1;
        }

        Ok(CircuitPermit {
            breaker: self.clone(),
            finished: false,
        })
    }

    pub fn snapshot(&self) -> CircuitStateSnapshot {
        let state = self.state.lock().expect("circuit mutex poisoned");
        match &*state {
            CircuitState::Closed {
                consecutive_failures,
            } => CircuitStateSnapshot::Closed {
                consecutive_failures: *consecutive_failures,
            },
            CircuitState::Open { opened_at } => CircuitStateSnapshot::Open {
                retry_after: self
                    .config
                    .open_duration
                    .saturating_sub(opened_at.elapsed()),
            },
            CircuitState::HalfOpen {
                in_flight,
                successes,
            } => CircuitStateSnapshot::HalfOpen {
                in_flight: *in_flight,
                successes: *successes,
            },
        }
    }

    fn record_success(&self) {
        let mut state = self.state.lock().expect("circuit mutex poisoned");
        match &mut *state {
            CircuitState::Closed {
                consecutive_failures,
            } => *consecutive_failures = 0,
            CircuitState::HalfOpen {
                in_flight,
                successes,
            } => {
                *in_flight = in_flight.saturating_sub(1);
                *successes += 1;
                if *successes >= self.config.recovery_success_threshold {
                    *state = CircuitState::Closed {
                        consecutive_failures: 0,
                    };
                }
            }
            CircuitState::Open { .. } => {}
        }
    }

    fn record_failure(&self) {
        let mut state = self.state.lock().expect("circuit mutex poisoned");
        match &mut *state {
            CircuitState::Closed {
                consecutive_failures,
            } => {
                *consecutive_failures += 1;
                if *consecutive_failures >= self.config.failure_threshold {
                    *state = CircuitState::Open {
                        opened_at: Instant::now(),
                    };
                }
            }
            CircuitState::HalfOpen { .. } => {
                *state = CircuitState::Open {
                    opened_at: Instant::now(),
                };
            }
            CircuitState::Open { .. } => {}
        }
    }
}

#[derive(Debug)]
pub struct CircuitPermit {
    breaker: CircuitBreaker,
    finished: bool,
}

impl CircuitPermit {
    pub fn success(mut self) {
        self.breaker.record_success();
        self.finished = true;
    }

    pub fn failure(mut self) {
        self.breaker.record_failure();
        self.finished = true;
    }
}

impl Drop for CircuitPermit {
    fn drop(&mut self) {
        if !self.finished {
            // A dropped provider call is an interrupted request, not evidence that the
            // provider is unhealthy. Release only a half-open concurrency slot.
            let mut state = self.breaker.state.lock().expect("circuit mutex poisoned");
            if let CircuitState::HalfOpen { in_flight, .. } = &mut *state {
                *in_flight = in_flight.saturating_sub(1);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct CircuitRegistry {
    config: CircuitBreakerConfig,
    entries: Arc<Mutex<HashMap<String, CircuitBreaker>>>,
}

impl CircuitRegistry {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn for_provider(&self, provider_id: &str) -> CircuitBreaker {
        let mut entries = self.entries.lock().expect("registry mutex poisoned");
        entries
            .entry(provider_id.to_owned())
            .or_insert_with(|| CircuitBreaker::new(self.config.clone()))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            failure_threshold: 2,
            recovery_success_threshold: 1,
            open_duration: Duration::from_millis(10),
            half_open_max_in_flight: 1,
        }
    }

    #[test]
    fn opens_after_threshold_and_recovers_after_probe() {
        let breaker = CircuitBreaker::new(config());
        breaker.try_acquire().unwrap().failure();
        assert!(breaker.try_acquire().is_ok());
        breaker.try_acquire().unwrap().failure();
        assert!(breaker.try_acquire().is_err());

        let future = Instant::now() + Duration::from_secs(1);
        breaker.try_acquire_at(future).unwrap().success();
        assert_eq!(
            breaker.snapshot(),
            CircuitStateSnapshot::Closed {
                consecutive_failures: 0
            }
        );
    }
}
