//! Aggregate-only official REST Gate A probe core.
//!
//! Raw REST responses and credentials deliberately have no representation in this crate's
//! serializable report types. Network collection belongs to `pal-rest`; this crate consumes only
//! bounded, sanitized observations and aggregate measurement windows.

mod command;
mod evaluate;
mod report;
mod rest_adapter;
mod runner;
mod stats;

pub use command::{CommandError, ProbeCommand, parse_command};
pub use evaluate::{evaluate, evaluate_unattested};
pub use report::{
    CandidateEvaluation, CandidateLoadReport, CardinalDirection, CardinalRotationAggregate,
    EvidenceError, GATE_A_CANDIDATE_INTERVALS_MS, GATE_A_PAIR_COUNT, GATE_A_REPORT_SCHEMA_VERSION,
    GATE_A_WINDOW_DURATION_MS, GateAEvidence, GateAReport, GateAThresholds, GateDecision,
    GateFailureReason, LoadErrorCounts, MovementEvidence, MovementObservation, PairOrder,
    PairedLoadWindow, PreflightEvidence, PrivacyEvidence, ReportError, RotationEvidence,
    RotationObservation, SafetyAbortReason, ServerFingerprint, WindowMetrics,
    encode_redacted_report,
};
pub use rest_adapter::{RestWindowAggregate, RestWindowBuilder};
pub use runner::{
    LoadProbePlan, LoadProbeSource, LoadRun, LoadWindowMeasurement, LoadWindowRequest,
    LoadWindowRole, ProbeRunError, ProbeRunner, ServerFingerprintInput,
};
pub use stats::{
    DistributionSummary, MeanConfidenceInterval, StatsError, mean_confidence_interval_95,
};
