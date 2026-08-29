use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanOutcome {
    Ok,
    Cancelled,
    Degraded,
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimingSpan {
    pub name: String,
    pub started_after_turn_start: Duration,
    pub duration: Duration,
    pub outcome: SpanOutcome,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct TimingCollector {
    turn_start: Instant,
    spans: Arc<Mutex<Vec<TimingSpan>>>,
}

impl Default for TimingCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl TimingCollector {
    pub fn new() -> Self {
        Self {
            turn_start: Instant::now(),
            spans: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn start(&self, name: impl Into<String>) -> TimingGuard {
        TimingGuard {
            collector: self.clone(),
            name: name.into(),
            started: Instant::now(),
            finished: false,
        }
    }

    pub fn snapshot(&self) -> Vec<TimingSpan> {
        self.spans.lock().expect("timing mutex poisoned").clone()
    }

    fn push(&self, span: TimingSpan) {
        self.spans.lock().expect("timing mutex poisoned").push(span);
    }
}

pub struct TimingGuard {
    collector: TimingCollector,
    name: String,
    started: Instant,
    finished: bool,
}

impl TimingGuard {
    pub fn finish(
        mut self,
        outcome: SpanOutcome,
        attributes: BTreeMap<String, String>,
    ) -> TimingSpan {
        let span = TimingSpan {
            name: self.name.clone(),
            started_after_turn_start: self.started.duration_since(self.collector.turn_start),
            duration: self.started.elapsed(),
            outcome,
            attributes,
        };
        self.collector.push(span.clone());
        self.finished = true;
        span
    }
}

impl Drop for TimingGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.collector.push(TimingSpan {
                name: self.name.clone(),
                started_after_turn_start: self.started.duration_since(self.collector.turn_start),
                duration: self.started.elapsed(),
                outcome: SpanOutcome::Cancelled,
                attributes: BTreeMap::new(),
            });
        }
    }
}
