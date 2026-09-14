//! Trusted live telemetry transport boundary.

mod client;
mod clock;
mod cursor;
mod identity;
mod server;
mod wire;

pub use client::{
    IngestError, MonotonicEpoch, NetworkConsumer, NetworkConsumerConfig, NetworkConsumerError,
    NetworkFailureClass, TelemetryIngest,
};
pub use clock::{
    ClockError, ClockEstimate, ClockEstimator, ClockProbeObservation, ESTIMATE_EXPIRY,
    PROBE_CADENCE, reconnect_backoff,
};
pub use cursor::{
    LatestConnectOutcome, LatestDiagnostics, LatestPublishOutcome, LatestTelemetry,
    LatestTelemetryError, SequenceCursor, SequenceDecision, SequenceDiagnostics,
};
pub use identity::{AllowlistEntry, ClientAllowlist, IdentityError, certificate_sha256};
pub use server::{PositionTelemetryEndpoint, ServiceBuildError};
pub use wire::{
    MAX_DECODED_MESSAGE_SIZE, ValidationError, decode_envelope, validate_clock_probe_request,
    validate_clock_probe_response, validate_descriptor, validate_descriptor_request,
    validate_envelope, validate_subscribe_request,
};
