//! Position source contract.

use pal_domain::PositionSample;
use thiserror::Error;

use crate::ReplayError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockInvalidReason {
    MonotonicRegression,
    ProbeInvalid,
    EvidenceExpired,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PositionSourceEvent {
    Connected {
        generation: u64,
    },
    Sample(PositionSample),
    Unavailable {
        generation: u64,
    },
    Disconnected {
        generation: u64,
    },
    ClockInvalid {
        generation: u64,
        reason: ClockInvalidReason,
    },
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PositionSourceError {
    #[error("replay source failed: {0}")]
    Replay(#[source] ReplayError),
}

pub trait PositionSource {
    fn poll(
        &mut self,
        now_monotonic_ms: u64,
    ) -> Result<Option<PositionSourceEvent>, PositionSourceError>;
}
