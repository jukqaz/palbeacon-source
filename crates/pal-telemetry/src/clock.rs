use std::{fmt, time::Duration};

use thiserror::Error;

pub const PROBE_CADENCE: Duration = Duration::from_secs(30);
pub const ESTIMATE_EXPIRY: Duration = Duration::from_secs(90);
const WALL_CLOCK_STEP_LIMIT_MS: i64 = 1_000;

#[derive(Clone, PartialEq, Eq)]
pub struct ClockProbeObservation {
    pub expected_nonce: Vec<u8>,
    pub echoed_nonce: Vec<u8>,
    pub client_send_unix_ms: i64,
    pub agent_receive_unix_ms: i64,
    pub agent_send_unix_ms: i64,
    pub client_receive_unix_ms: i64,
}

impl fmt::Debug for ClockProbeObservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED CLOCK PROBE OBSERVATION]")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockEstimate {
    pub full_rtt_upper_bound_ms: u64,
    pub offset_lower_bound_ms: i64,
    pub offset_upper_bound_ms: i64,
    pub observed_at_monotonic_ms: u64,
}

impl ClockEstimate {
    pub const fn age_upper_bound_ms(self, age_at_emit_ms: u32) -> u64 {
        (age_at_emit_ms as u64).saturating_add(self.full_rtt_upper_bound_ms)
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ClockError {
    #[error("clock probe nonce mismatch")]
    NonceMismatch,
    #[error("clock probe timestamp ordering is impossible")]
    ImpossibleOrdering,
    #[error("clock probe round trip is an outlier")]
    RttOutlier,
    #[error("clock probe detected a wall-clock step")]
    WallClockStep,
}

#[derive(Clone, Debug)]
pub struct ClockEstimator {
    maximum_rtt_ms: u64,
    current: Option<ClockEstimate>,
}

impl ClockEstimator {
    pub const fn new(maximum_rtt_ms: u64) -> Self {
        Self {
            maximum_rtt_ms,
            current: None,
        }
    }

    pub fn observe(
        &mut self,
        observation: ClockProbeObservation,
        observed_at_monotonic_ms: u64,
    ) -> Result<ClockEstimate, ClockError> {
        if observation.expected_nonce.len() != 16
            || observation.echoed_nonce.len() != 16
            || observation.echoed_nonce != observation.expected_nonce
        {
            self.current = None;
            return Err(ClockError::NonceMismatch);
        }
        if observation.client_send_unix_ms <= 0
            || observation.agent_receive_unix_ms <= 0
            || observation.agent_send_unix_ms <= 0
            || observation.client_receive_unix_ms <= 0
            || observation.client_receive_unix_ms < observation.client_send_unix_ms
            || observation.agent_send_unix_ms < observation.agent_receive_unix_ms
        {
            self.current = None;
            return Err(ClockError::ImpossibleOrdering);
        }
        let client_elapsed = observation
            .client_receive_unix_ms
            .checked_sub(observation.client_send_unix_ms)
            .and_then(|value| u64::try_from(value).ok())
            .ok_or(ClockError::ImpossibleOrdering)?;
        let agent_elapsed = observation
            .agent_send_unix_ms
            .checked_sub(observation.agent_receive_unix_ms)
            .and_then(|value| u64::try_from(value).ok())
            .ok_or(ClockError::ImpossibleOrdering)?;
        if client_elapsed == 0 || agent_elapsed > client_elapsed {
            self.current = None;
            return Err(ClockError::ImpossibleOrdering);
        }
        if client_elapsed > self.maximum_rtt_ms {
            self.current = None;
            return Err(ClockError::RttOutlier);
        }
        let lower = observation
            .agent_send_unix_ms
            .saturating_sub(observation.client_receive_unix_ms);
        let upper = observation
            .agent_receive_unix_ms
            .saturating_sub(observation.client_send_unix_ms);
        if lower > upper {
            self.current = None;
            return Err(ClockError::ImpossibleOrdering);
        }
        if self.current.is_some_and(|previous| {
            lower
                > previous
                    .offset_upper_bound_ms
                    .saturating_add(WALL_CLOCK_STEP_LIMIT_MS)
                || upper
                    < previous
                        .offset_lower_bound_ms
                        .saturating_sub(WALL_CLOCK_STEP_LIMIT_MS)
        }) {
            self.current = None;
            return Err(ClockError::WallClockStep);
        }
        let estimate = ClockEstimate {
            full_rtt_upper_bound_ms: client_elapsed,
            offset_lower_bound_ms: lower,
            offset_upper_bound_ms: upper,
            observed_at_monotonic_ms,
        };
        self.current = Some(estimate);
        Ok(estimate)
    }

    pub fn current(&self, now_monotonic_ms: u64) -> Option<ClockEstimate> {
        self.current.filter(|estimate| {
            now_monotonic_ms.saturating_sub(estimate.observed_at_monotonic_ms)
                <= ESTIMATE_EXPIRY.as_millis() as u64
        })
    }

    pub fn invalidate(&mut self) {
        self.current = None;
    }
}

pub fn reconnect_backoff(attempt: usize) -> Duration {
    const BACKOFF_MS: [u64; 6] = [250, 500, 1_000, 2_000, 4_000, 5_000];
    Duration::from_millis(BACKOFF_MS[attempt.min(BACKOFF_MS.len() - 1)])
}
