use std::{
    fmt,
    sync::{Arc, Mutex},
};

use pal_rest::RestError;
use serde::Serialize;

use crate::AgentMetrics;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthErrorKind {
    SelectionMissing,
    SelectionInactive,
    SelectionAmbiguous,
    Authentication,
    Timeout,
    Decode,
    Policy,
    Transport,
    Publication,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HealthSnapshot {
    pub degraded: bool,
    pub successful_polls: u64,
    pub failed_polls: u64,
    pub emitted_samples: u64,
    pub publish_failures: u64,
    pub consecutive_failures: u64,
    pub last_error: Option<HealthErrorKind>,
}

#[derive(Clone, Default)]
pub struct HealthMonitor {
    state: Arc<Mutex<HealthState>>,
    metrics: AgentMetrics,
}

#[derive(Default)]
struct HealthState {
    consecutive_failures: u64,
    last_error: Option<HealthErrorKind>,
}

impl fmt::Debug for HealthMonitor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HealthMonitor")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

impl HealthMonitor {
    pub fn record_poll_success(&self) {
        self.metrics.successful_poll();
        let mut state = self.state.lock().expect("health state poisoned");
        state.consecutive_failures = 0;
        state.last_error = None;
    }

    pub fn record_poll_failure(&self, error: &RestError) {
        self.metrics.failed_poll();
        let mut state = self.state.lock().expect("health state poisoned");
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        state.last_error = Some(classify(error));
    }

    pub(crate) fn record_emitted(&self) {
        self.metrics.emitted_sample();
    }

    pub(crate) fn record_publish_failure(&self) {
        self.metrics.publish_failure();
        let mut state = self.state.lock().expect("health state poisoned");
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        state.last_error = Some(HealthErrorKind::Publication);
    }

    pub fn snapshot(&self) -> HealthSnapshot {
        let metrics = self.metrics.snapshot();
        let state = self.state.lock().expect("health state poisoned");
        HealthSnapshot {
            degraded: state.consecutive_failures > 0,
            successful_polls: metrics.successful_polls,
            failed_polls: metrics.failed_polls,
            emitted_samples: metrics.emitted_samples,
            publish_failures: metrics.publish_failures,
            consecutive_failures: state.consecutive_failures,
            last_error: state.last_error,
        }
    }

    pub fn metrics(&self) -> AgentMetrics {
        self.metrics.clone()
    }
}

fn classify(error: &RestError) -> HealthErrorKind {
    match error {
        RestError::SelectedPlayerMissing => HealthErrorKind::SelectionMissing,
        RestError::SelectedPlayerInactive => HealthErrorKind::SelectionInactive,
        RestError::SelectedPlayerAmbiguous => HealthErrorKind::SelectionAmbiguous,
        RestError::Unauthorized => HealthErrorKind::Authentication,
        RestError::Timeout => HealthErrorKind::Timeout,
        RestError::Decode { .. } => HealthErrorKind::Decode,
        RestError::InvalidSelector
        | RestError::InvalidWorldAlias
        | RestError::InvalidEndpoint
        | RestError::EndpointNotAllowed
        | RestError::InvalidClientConfig
        | RestError::ConnectionUntrusted
        | RestError::RedirectRejected
        | RestError::CompressionNotAllowed
        | RestError::ContentTypeRejected
        | RestError::ResponseTooLarge => HealthErrorKind::Policy,
        RestError::Transport | RestError::UnexpectedStatus(_) => HealthErrorKind::Transport,
    }
}
