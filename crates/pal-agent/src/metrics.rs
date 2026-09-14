use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AgentMetricsSnapshot {
    pub successful_polls: u64,
    pub failed_polls: u64,
    pub emitted_samples: u64,
    pub publish_failures: u64,
}

#[derive(Clone, Default)]
pub struct AgentMetrics {
    inner: Arc<MetricsInner>,
}

#[derive(Default)]
struct MetricsInner {
    successful_polls: AtomicU64,
    failed_polls: AtomicU64,
    emitted_samples: AtomicU64,
    publish_failures: AtomicU64,
}

impl AgentMetrics {
    pub(crate) fn successful_poll(&self) {
        saturating_increment(&self.inner.successful_polls);
    }

    pub(crate) fn failed_poll(&self) {
        saturating_increment(&self.inner.failed_polls);
    }

    pub(crate) fn emitted_sample(&self) {
        saturating_increment(&self.inner.emitted_samples);
    }

    pub(crate) fn publish_failure(&self) {
        saturating_increment(&self.inner.publish_failures);
    }

    pub fn snapshot(&self) -> AgentMetricsSnapshot {
        AgentMetricsSnapshot {
            successful_polls: self.inner.successful_polls.load(Ordering::Relaxed),
            failed_polls: self.inner.failed_polls.load(Ordering::Relaxed),
            emitted_samples: self.inner.emitted_samples.load(Ordering::Relaxed),
            publish_failures: self.inner.publish_failures.load(Ordering::Relaxed),
        }
    }
}

fn saturating_increment(value: &AtomicU64) {
    let _ = value.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(1))
    });
}
