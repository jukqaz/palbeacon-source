use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::report::{
    CandidateLoadReport, GATE_A_CANDIDATE_INTERVALS_MS, GATE_A_PAIR_COUNT,
    GATE_A_WINDOW_DURATION_MS, LoadErrorCounts, PairOrder, PairedLoadWindow, PreflightEvidence,
    SafetyAbortReason, ServerFingerprint, WindowMetrics,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerFingerprintInput {
    pub rest_version: String,
    pub steam_manifest_id: Option<u64>,
    pub executable_sha256: [u8; 32],
    pub server_subject_id: [u8; 32],
    pub executable_hash_verified: bool,
    pub endpoint_private_lan: bool,
    pub auth_ok: bool,
    pub info_ok: bool,
    pub privacy_boundary_ok: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadProbePlan {
    pub candidate_intervals_ms: Vec<u64>,
    pub pair_count: usize,
    pub window_duration_ms: u64,
    pub cooldown_ms: u64,
}

impl LoadProbePlan {
    pub fn gate_a_default() -> Self {
        Self {
            candidate_intervals_ms: GATE_A_CANDIDATE_INTERVALS_MS.to_vec(),
            pair_count: GATE_A_PAIR_COUNT,
            window_duration_ms: GATE_A_WINDOW_DURATION_MS,
            cooldown_ms: 120_000,
        }
    }

    fn is_valid(&self) -> bool {
        self.pair_count > 0
            && self.window_duration_ms > 0
            && self.cooldown_ms > 0
            && !self.candidate_intervals_ms.is_empty()
            && self
                .candidate_intervals_ms
                .iter()
                .all(|interval| *interval > 0)
            && self
                .candidate_intervals_ms
                .windows(2)
                .all(|pair| pair[0] > pair[1])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadWindowRole {
    Baseline,
    Candidate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadWindowRequest {
    pub candidate_interval_ms: u64,
    pub pair_index: u32,
    pub order: PairOrder,
    pub role: LoadWindowRole,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadWindowMeasurement {
    pub metrics: WindowMetrics,
    pub safety_abort: Option<SafetyAbortReason>,
}

pub trait LoadProbeSource {
    fn measure_window(
        &mut self,
        request: LoadWindowRequest,
    ) -> Result<LoadWindowMeasurement, ProbeRunError>;

    fn cooldown(&mut self, duration_ms: u64) -> Result<(), ProbeRunError>;

    fn error_counts(&mut self, _candidate_interval_ms: u64) -> LoadErrorCounts {
        LoadErrorCounts::default()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadRun {
    pub candidates: Vec<CandidateLoadReport>,
    pub safety_aborted: bool,
}

pub struct ProbeRunner;

impl ProbeRunner {
    pub fn preflight(input: &ServerFingerprintInput) -> Result<PreflightEvidence, ProbeRunError> {
        if input.rest_version.is_empty()
            || input.rest_version.len() > 128
            || !input.rest_version.is_ascii()
            || input.executable_sha256 == [0; 32]
            || input.server_subject_id == [0; 32]
            || !input.executable_hash_verified
        {
            return Err(ProbeRunError::InvalidPreflightIdentity);
        }

        let mut hasher = Sha256::new();
        hasher.update(b"palcompanion/gate-a/server-fingerprint/v1");
        update_length_prefixed(&mut hasher, input.rest_version.as_bytes())?;
        match input.steam_manifest_id {
            Some(manifest) => {
                hasher.update([1]);
                hasher.update(manifest.to_le_bytes());
            }
            None => hasher.update([0]),
        }
        hasher.update(input.executable_sha256);
        hasher.update(input.server_subject_id);
        let server_fingerprint = ServerFingerprint::from_digest(hasher.finalize().into());

        Ok(PreflightEvidence {
            server_fingerprint,
            endpoint_private_lan: input.endpoint_private_lan,
            auth_ok: input.auth_ok,
            info_ok: input.info_ok,
            executable_hash_verified: input.executable_hash_verified,
            privacy_boundary_ok: input.privacy_boundary_ok,
        })
    }

    pub fn run_load<S>(plan: &LoadProbePlan, source: &mut S) -> Result<LoadRun, ProbeRunError>
    where
        S: LoadProbeSource,
    {
        if !plan.is_valid() {
            return Err(ProbeRunError::InvalidPlan);
        }

        let mut candidates = Vec::with_capacity(plan.candidate_intervals_ms.len());
        for &candidate_interval_ms in &plan.candidate_intervals_ms {
            let mut candidate = CandidateLoadReport {
                interval_ms: candidate_interval_ms,
                pairs: Vec::with_capacity(plan.pair_count),
                errors: LoadErrorCounts::default(),
                safety_abort: None,
            };
            for pair_index in 0..plan.pair_count {
                let pair_index =
                    u32::try_from(pair_index).map_err(|_| ProbeRunError::InvalidPlan)?;
                let order = if pair_index % 2 == 0 {
                    PairOrder::BaselineThenCandidate
                } else {
                    PairOrder::CandidateThenBaseline
                };
                let (first_role, second_role) = match order {
                    PairOrder::BaselineThenCandidate => {
                        (LoadWindowRole::Baseline, LoadWindowRole::Candidate)
                    }
                    PairOrder::CandidateThenBaseline => {
                        (LoadWindowRole::Candidate, LoadWindowRole::Baseline)
                    }
                };
                let first = source.measure_window(LoadWindowRequest {
                    candidate_interval_ms,
                    pair_index,
                    order,
                    role: first_role,
                    duration_ms: plan.window_duration_ms,
                })?;
                if let Some(reason) = first.safety_abort {
                    candidate.safety_abort = Some(reason);
                    candidate.errors = source.error_counts(candidate_interval_ms);
                    candidates.push(candidate);
                    return Ok(LoadRun {
                        candidates,
                        safety_aborted: true,
                    });
                }
                let second = source.measure_window(LoadWindowRequest {
                    candidate_interval_ms,
                    pair_index,
                    order,
                    role: second_role,
                    duration_ms: plan.window_duration_ms,
                })?;
                if let Some(reason) = second.safety_abort {
                    candidate.safety_abort = Some(reason);
                    candidate.errors = source.error_counts(candidate_interval_ms);
                    candidates.push(candidate);
                    return Ok(LoadRun {
                        candidates,
                        safety_aborted: true,
                    });
                }
                let (baseline, measured) = match order {
                    PairOrder::BaselineThenCandidate => (first.metrics, second.metrics),
                    PairOrder::CandidateThenBaseline => (second.metrics, first.metrics),
                };
                candidate.pairs.push(PairedLoadWindow {
                    pair_index,
                    order,
                    baseline,
                    candidate: measured,
                });
                source.cooldown(plan.cooldown_ms)?;
            }
            candidate.errors = source.error_counts(candidate_interval_ms);
            candidates.push(candidate);
        }
        Ok(LoadRun {
            candidates,
            safety_aborted: false,
        })
    }
}

fn update_length_prefixed(hasher: &mut Sha256, bytes: &[u8]) -> Result<(), ProbeRunError> {
    let length = u32::try_from(bytes.len()).map_err(|_| ProbeRunError::InvalidPreflightIdentity)?;
    hasher.update(length.to_le_bytes());
    hasher.update(bytes);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProbeRunError {
    #[error("probe plan is invalid")]
    InvalidPlan,
    #[error("dedicated-server identity is invalid")]
    InvalidPreflightIdentity,
    #[error("probe source failed without exposing response data")]
    SourceFailure,
}
