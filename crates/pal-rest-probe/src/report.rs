use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::stats::{DistributionSummary, MeanConfidenceInterval, StatsError};

pub const GATE_A_REPORT_SCHEMA_VERSION: u32 = 1;
pub const GATE_A_PAIR_COUNT: usize = 5;
pub const GATE_A_WINDOW_DURATION_MS: u64 = 300_000;
pub const GATE_A_CANDIDATE_INTERVALS_MS: [u64; 3] = [2_000, 1_000, 500];

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerFingerprint {
    digest_sha256: [u8; 32],
}

impl ServerFingerprint {
    pub const fn from_digest(digest_sha256: [u8; 32]) -> Self {
        Self { digest_sha256 }
    }

    pub const fn as_digest(&self) -> &[u8; 32] {
        &self.digest_sha256
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightEvidence {
    pub server_fingerprint: ServerFingerprint,
    pub endpoint_private_lan: bool,
    pub auth_ok: bool,
    pub info_ok: bool,
    pub executable_hash_verified: bool,
    pub privacy_boundary_ok: bool,
}

impl PreflightEvidence {
    pub const fn complete(server_fingerprint: ServerFingerprint) -> Self {
        Self {
            server_fingerprint,
            endpoint_private_lan: true,
            auth_ok: true,
            info_ok: true,
            executable_hash_verified: true,
            privacy_boundary_ok: true,
        }
    }

    pub const fn is_complete(&self) -> bool {
        self.endpoint_private_lan
            && self.auth_ok
            && self.info_ok
            && self.executable_hash_verified
            && self.privacy_boundary_ok
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadErrorCounts {
    pub timeouts: u64,
    pub parse: u64,
    pub auth: u64,
    pub privacy: u64,
    pub transport: u64,
    pub selected_player_missing: u64,
    pub selected_player_ambiguous: u64,
    pub selected_player_inactive: u64,
}

impl LoadErrorCounts {
    pub const fn is_zero(self) -> bool {
        self.timeouts == 0
            && self.parse == 0
            && self.auth == 0
            && self.privacy == 0
            && self.transport == 0
            && self.selected_player_missing == 0
            && self.selected_player_ambiguous == 0
            && self.selected_player_inactive == 0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowMetrics {
    pub duration_ms: u64,
    pub request_attempts: u64,
    pub request_successes: u64,
    pub selected_player_samples: u64,
    pub response_latency_ms: DistributionSummary,
    pub decode_latency_ms: DistributionSummary,
    pub decoded_body_bytes: DistributionSummary,
    pub actor_count: DistributionSummary,
    pub server_fps: DistributionSummary,
    pub frame_time_ms: DistributionSummary,
    pub probe_cpu_percent: DistributionSummary,
    pub probe_private_bytes_peak: u64,
}

impl WindowMetrics {
    pub fn is_valid(&self, minimum_request_samples: u64) -> bool {
        let request_derived_counts = [
            self.response_latency_ms.count,
            self.decode_latency_ms.count,
            self.decoded_body_bytes.count,
            self.actor_count.count,
            self.server_fps.count,
            self.frame_time_ms.count,
            self.probe_cpu_percent.count,
        ];
        self.duration_ms > 0
            && self.request_attempts >= minimum_request_samples
            && self.request_successes == self.request_attempts
            && self.selected_player_samples == self.request_successes
            && request_derived_counts
                .into_iter()
                .all(|count| count == self.request_successes)
            && [
                self.response_latency_ms,
                self.decode_latency_ms,
                self.decoded_body_bytes,
                self.actor_count,
                self.server_fps,
                self.frame_time_ms,
                self.probe_cpu_percent,
            ]
            .into_iter()
            .all(|summary| {
                summary.count >= minimum_request_samples && summary.is_valid_nonnegative()
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairOrder {
    BaselineThenCandidate,
    CandidateThenBaseline,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedLoadWindow {
    pub pair_index: u32,
    pub order: PairOrder,
    pub baseline: WindowMetrics,
    pub candidate: WindowMetrics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyAbortReason {
    ServerFpsFloor,
    FrameTimeCeiling,
    OperatorAbort,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateLoadReport {
    pub interval_ms: u64,
    pub pairs: Vec<PairedLoadWindow>,
    pub errors: LoadErrorCounts,
    pub safety_abort: Option<SafetyAbortReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardinalDirection {
    North,
    East,
    South,
    West,
}

impl CardinalDirection {
    pub const ALL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    pub const fn degrees(self) -> f64 {
        match self {
            Self::North => 0.0,
            Self::East => 90.0,
            Self::South => 180.0,
            Self::West => 270.0,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::North => 0,
            Self::East => 1,
            Self::South => 2,
            Self::West => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotationObservation {
    pub expected: CardinalDirection,
    pub observed_degrees: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardinalRotationAggregate {
    pub direction: CardinalDirection,
    pub relevant_samples: u64,
    pub present_samples: u64,
    pub error_degrees: Option<DistributionSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotationEvidence {
    pub relevant_samples: u64,
    pub present_samples: u64,
    pub cardinal: [CardinalRotationAggregate; 4],
    pub error_degrees: Option<DistributionSummary>,
}

impl RotationEvidence {
    pub fn from_observations<I>(observations: I) -> Result<Self, EvidenceError>
    where
        I: IntoIterator<Item = RotationObservation>,
    {
        let mut relevant = [0_u64; 4];
        let mut present = [0_u64; 4];
        let mut per_cardinal: [Vec<f64>; 4] = std::array::from_fn(|_| Vec::new());
        let mut all_errors = Vec::new();

        for observation in observations {
            let index = observation.expected.index();
            relevant[index] = relevant[index]
                .checked_add(1)
                .ok_or(EvidenceError::CountOverflow)?;
            if let Some(observed) = observation.observed_degrees {
                if !observed.is_finite() {
                    return Err(EvidenceError::NonFinite);
                }
                let error = angular_error_degrees(observation.expected.degrees(), observed);
                present[index] = present[index]
                    .checked_add(1)
                    .ok_or(EvidenceError::CountOverflow)?;
                per_cardinal[index].push(error);
                all_errors.push(error);
            }
        }

        let relevant_samples = relevant
            .into_iter()
            .try_fold(0_u64, |sum, value| sum.checked_add(value))
            .ok_or(EvidenceError::CountOverflow)?;
        let present_samples = present
            .into_iter()
            .try_fold(0_u64, |sum, value| sum.checked_add(value))
            .ok_or(EvidenceError::CountOverflow)?;
        let cardinal = std::array::from_fn(|index| CardinalRotationAggregate {
            direction: CardinalDirection::ALL[index],
            relevant_samples: relevant[index],
            present_samples: present[index],
            error_degrees: optional_summary(std::mem::take(&mut per_cardinal[index])),
        });

        Ok(Self {
            relevant_samples,
            present_samples,
            cardinal,
            error_degrees: optional_summary(all_errors),
        })
    }

    pub fn is_internally_consistent(&self) -> bool {
        if self.present_samples > self.relevant_samples {
            return false;
        }

        let mut relevant_sum = 0_u64;
        let mut present_sum = 0_u64;
        let mut weighted_error_sum = 0.0;
        let mut minimum_error = f64::INFINITY;
        let mut maximum_error = f64::NEG_INFINITY;

        for (index, cardinal) in self.cardinal.iter().enumerate() {
            if cardinal.direction != CardinalDirection::ALL[index]
                || cardinal.present_samples > cardinal.relevant_samples
            {
                return false;
            }
            relevant_sum = match relevant_sum.checked_add(cardinal.relevant_samples) {
                Some(value) => value,
                None => return false,
            };
            present_sum = match present_sum.checked_add(cardinal.present_samples) {
                Some(value) => value,
                None => return false,
            };

            match (cardinal.present_samples, cardinal.error_degrees) {
                (0, None) => {}
                (0, Some(_)) | (_, None) => return false,
                (present, Some(summary)) => {
                    if summary.count != present || !summary.is_valid_nonnegative() {
                        return false;
                    }
                    weighted_error_sum += summary.mean * present as f64;
                    if !weighted_error_sum.is_finite() {
                        return false;
                    }
                    minimum_error = minimum_error.min(summary.min);
                    maximum_error = maximum_error.max(summary.max);
                }
            }
        }

        if relevant_sum != self.relevant_samples || present_sum != self.present_samples {
            return false;
        }

        match (self.present_samples, self.error_degrees) {
            (0, None) => true,
            (0, Some(_)) | (_, None) => false,
            (present, Some(summary)) => {
                if summary.count != present || !summary.is_valid_nonnegative() {
                    return false;
                }
                let weighted_mean = weighted_error_sum / present as f64;
                nearly_equal(summary.min, minimum_error)
                    && nearly_equal(summary.max, maximum_error)
                    && nearly_equal(summary.mean, weighted_mean)
            }
        }
    }
}

fn nearly_equal(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= f64::EPSILON * scale * 16.0
}

fn angular_error_degrees(expected: f64, observed: f64) -> f64 {
    let delta = (observed - expected + 180.0).rem_euclid(360.0) - 180.0;
    delta.abs()
}

fn optional_summary(samples: Vec<f64>) -> Option<DistributionSummary> {
    if samples.is_empty() {
        None
    } else {
        DistributionSummary::from_samples(samples).ok()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovementObservation {
    pub changed_after_ms: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementEvidence {
    pub marker_count: u64,
    pub changed_sample_count: u64,
    pub change_lag_ms: Option<DistributionSummary>,
}

impl MovementEvidence {
    pub fn from_observations<I>(observations: I) -> Result<Self, EvidenceError>
    where
        I: IntoIterator<Item = MovementObservation>,
    {
        let mut marker_count = 0_u64;
        let mut change_lags = Vec::new();
        for observation in observations {
            marker_count = marker_count
                .checked_add(1)
                .ok_or(EvidenceError::CountOverflow)?;
            if let Some(change_lag) = observation.changed_after_ms {
                if !change_lag.is_finite() {
                    return Err(EvidenceError::NonFinite);
                }
                if change_lag < 0.0 {
                    return Err(EvidenceError::NegativeDuration);
                }
                change_lags.push(change_lag);
            }
        }
        let changed_sample_count =
            u64::try_from(change_lags.len()).map_err(|_| EvidenceError::CountOverflow)?;
        Ok(Self {
            marker_count,
            changed_sample_count,
            change_lag_ms: optional_summary(change_lags),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivacyEvidence {
    pub artifact_scan_clean: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateAEvidence {
    pub schema_version: u32,
    pub preflight: PreflightEvidence,
    pub candidates: Vec<CandidateLoadReport>,
    pub rotation: RotationEvidence,
    pub movement: MovementEvidence,
    pub privacy: PrivacyEvidence,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateAThresholds {
    pub minimum_pair_count: usize,
    pub minimum_request_samples_per_window: u64,
    pub minimum_movement_markers: u64,
    pub movement_p95_max_ms: f64,
    pub fps_mean_degradation_max_percent: f64,
    pub fps_confidence_upper_max_percent: f64,
    pub frame_time_pair_degradation_max_percent: f64,
    pub maximum_frame_time_regressed_pairs: usize,
    pub minimum_rotation_samples: u64,
    pub minimum_cardinal_samples: u64,
    pub rotation_presence_minimum_ratio: f64,
    pub rotation_cardinal_median_max_degrees: f64,
    pub rotation_max_error_degrees: f64,
}

impl Default for GateAThresholds {
    fn default() -> Self {
        Self {
            minimum_pair_count: 5,
            minimum_request_samples_per_window: 30,
            minimum_movement_markers: 5,
            movement_p95_max_ms: 1_500.0,
            fps_mean_degradation_max_percent: 1.0,
            fps_confidence_upper_max_percent: 2.0,
            frame_time_pair_degradation_max_percent: 2.0,
            maximum_frame_time_regressed_pairs: 1,
            minimum_rotation_samples: 100,
            minimum_cardinal_samples: 3,
            rotation_presence_minimum_ratio: 0.99,
            rotation_cardinal_median_max_degrees: 10.0,
            rotation_max_error_degrees: 20.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateDecision {
    Go,
    NoGo,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "code")]
pub enum GateFailureReason {
    UnattestedEvidence,
    CandidatePlanInvalid,
    SchemaVersionMismatch,
    InvalidThresholds,
    PreflightIncomplete,
    ArtifactPrivacyScanFailed,
    MovementEvidenceIncomplete,
    MovementP95TooHigh,
    RotationEvidenceInsufficient,
    RotationPresenceTooLow,
    RotationCardinalsIncomplete,
    RotationMedianErrorTooHigh,
    RotationMaxErrorTooHigh,
    CandidatePairCountTooLow { interval_ms: u64 },
    CandidatePairSequenceInvalid { interval_ms: u64 },
    CandidateWindowInvalid { interval_ms: u64 },
    CandidateErrors { interval_ms: u64 },
    CandidateSafetyAbort { interval_ms: u64 },
    SelectedPlayerCoverageIncomplete { interval_ms: u64 },
    FpsMeanDegradationTooHigh { interval_ms: u64 },
    FpsConfidenceUpperBoundTooHigh { interval_ms: u64 },
    FrameTimePairRegressions { interval_ms: u64 },
    NoPassingCandidate,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateEvaluation {
    pub interval_ms: u64,
    pub passed: bool,
    pub fps_degradation_percent_95: Option<MeanConfidenceInterval>,
    pub frame_time_regressed_pairs: usize,
    pub failure_reasons: Vec<GateFailureReason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateAReport {
    pub schema_version: u32,
    pub server_fingerprint: ServerFingerprint,
    pub thresholds: GateAThresholds,
    pub preflight: PreflightEvidence,
    pub candidates: Vec<CandidateLoadReport>,
    pub candidate_evaluations: Vec<CandidateEvaluation>,
    pub rotation: RotationEvidence,
    pub movement: MovementEvidence,
    pub privacy: PrivacyEvidence,
    pub decision: GateDecision,
    pub selected_interval_ms: Option<u64>,
    pub failure_reasons: Vec<GateFailureReason>,
}

pub fn encode_redacted_report(
    report: &GateAReport,
    forbidden_sentinels: &[&[u8]],
) -> Result<Vec<u8>, ReportError> {
    if forbidden_sentinels
        .iter()
        .any(|sentinel| sentinel.is_empty())
    {
        return Err(ReportError::EmptySentinel);
    }
    let encoded = serde_json::to_vec(report).map_err(|_| ReportError::Serialization)?;
    if forbidden_sentinels.iter().any(|sentinel| {
        encoded
            .windows(sentinel.len())
            .any(|window| window == *sentinel)
    }) {
        return Err(ReportError::SensitiveSentinel);
    }
    Ok(encoded)
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ReportError {
    #[error("report serialization failed")]
    Serialization,
    #[error("forbidden sentinel must not be empty")]
    EmptySentinel,
    #[error("sensitive sentinel found in report")]
    SensitiveSentinel,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum EvidenceError {
    #[error("evidence contains a non-finite number")]
    NonFinite,
    #[error("duration cannot be negative")]
    NegativeDuration,
    #[error("evidence count overflow")]
    CountOverflow,
    #[error("aggregate could not be constructed")]
    Aggregate,
}

impl From<StatsError> for EvidenceError {
    fn from(_: StatsError) -> Self {
        Self::Aggregate
    }
}
