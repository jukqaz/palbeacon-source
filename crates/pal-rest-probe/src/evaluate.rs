use crate::{
    report::{
        CandidateEvaluation, CandidateLoadReport, GATE_A_CANDIDATE_INTERVALS_MS, GATE_A_PAIR_COUNT,
        GATE_A_REPORT_SCHEMA_VERSION, GATE_A_WINDOW_DURATION_MS, GateAEvidence, GateAReport,
        GateAThresholds, GateDecision, GateFailureReason, PairOrder,
    },
    stats::mean_confidence_interval_95,
};

pub fn evaluate(evidence: &GateAEvidence) -> GateAReport {
    let thresholds = GateAThresholds::default();
    let mut global_failures = Vec::new();
    if evidence.schema_version != GATE_A_REPORT_SCHEMA_VERSION {
        global_failures.push(GateFailureReason::SchemaVersionMismatch);
    }
    if !thresholds_are_valid(&thresholds) {
        global_failures.push(GateFailureReason::InvalidThresholds);
    }
    if !evidence.preflight.is_complete() {
        global_failures.push(GateFailureReason::PreflightIncomplete);
    }
    if !evidence.privacy.artifact_scan_clean {
        global_failures.push(GateFailureReason::ArtifactPrivacyScanFailed);
    }
    if !candidate_plan_is_valid(&evidence.candidates) {
        global_failures.push(GateFailureReason::CandidatePlanInvalid);
    }
    evaluate_movement(evidence, &thresholds, &mut global_failures);
    evaluate_rotation(evidence, &thresholds, &mut global_failures);

    let candidate_evaluations = evidence
        .candidates
        .iter()
        .map(|candidate| evaluate_candidate(candidate, &thresholds))
        .collect::<Vec<_>>();
    let selected_interval_ms = if global_failures.is_empty() {
        candidate_evaluations
            .iter()
            .filter(|evaluation| evaluation.passed)
            .map(|evaluation| evaluation.interval_ms)
            .min()
    } else {
        None
    };

    if selected_interval_ms.is_none() {
        for evaluation in &candidate_evaluations {
            for reason in &evaluation.failure_reasons {
                push_unique(&mut global_failures, reason.clone());
            }
        }
        push_unique(&mut global_failures, GateFailureReason::NoPassingCandidate);
    }

    let decision = if selected_interval_ms.is_some() {
        GateDecision::Go
    } else {
        GateDecision::NoGo
    };

    GateAReport {
        schema_version: evidence.schema_version,
        server_fingerprint: evidence.preflight.server_fingerprint,
        thresholds,
        preflight: evidence.preflight.clone(),
        candidates: evidence.candidates.clone(),
        candidate_evaluations,
        rotation: evidence.rotation.clone(),
        movement: evidence.movement.clone(),
        privacy: evidence.privacy,
        decision,
        selected_interval_ms,
        failure_reasons: global_failures,
    }
}

/// Evaluates imported aggregate evidence for diagnostics without granting a
/// production polling decision. Only the in-process live collector may call
/// [`evaluate`] to issue a Gate A GO result.
pub fn evaluate_unattested(evidence: &GateAEvidence) -> GateAReport {
    let mut report = evaluate(evidence);
    report.decision = GateDecision::NoGo;
    report.selected_interval_ms = None;
    push_unique(
        &mut report.failure_reasons,
        GateFailureReason::UnattestedEvidence,
    );
    report
}

fn thresholds_are_valid(thresholds: &GateAThresholds) -> bool {
    thresholds.minimum_pair_count >= 2
        && thresholds.minimum_request_samples_per_window > 0
        && thresholds.minimum_movement_markers > 0
        && thresholds.minimum_rotation_samples > 0
        && thresholds.minimum_cardinal_samples > 0
        && [
            thresholds.movement_p95_max_ms,
            thresholds.fps_mean_degradation_max_percent,
            thresholds.fps_confidence_upper_max_percent,
            thresholds.frame_time_pair_degradation_max_percent,
            thresholds.rotation_presence_minimum_ratio,
            thresholds.rotation_cardinal_median_max_degrees,
            thresholds.rotation_max_error_degrees,
        ]
        .into_iter()
        .all(|value| value.is_finite() && value >= 0.0)
        && (0.0..=1.0).contains(&thresholds.rotation_presence_minimum_ratio)
}

fn candidate_plan_is_valid(candidates: &[CandidateLoadReport]) -> bool {
    if candidates.is_empty() || candidates.len() > GATE_A_CANDIDATE_INTERVALS_MS.len() {
        return false;
    }
    if candidates
        .iter()
        .zip(GATE_A_CANDIDATE_INTERVALS_MS)
        .any(|(candidate, expected)| candidate.interval_ms != expected)
    {
        return false;
    }

    let final_candidate_aborted = candidates
        .last()
        .is_some_and(|candidate| candidate.safety_abort.is_some());
    if candidates.len() != GATE_A_CANDIDATE_INTERVALS_MS.len() && !final_candidate_aborted {
        return false;
    }

    candidates.iter().enumerate().all(|(index, candidate)| {
        let is_aborted_tail = index + 1 == candidates.len() && candidate.safety_abort.is_some();
        (is_aborted_tail || candidate.pairs.len() == GATE_A_PAIR_COUNT)
            && candidate.pairs.iter().all(|pair| {
                pair.baseline.duration_ms == GATE_A_WINDOW_DURATION_MS
                    && pair.candidate.duration_ms == GATE_A_WINDOW_DURATION_MS
            })
    })
}

fn evaluate_movement(
    evidence: &GateAEvidence,
    thresholds: &GateAThresholds,
    failures: &mut Vec<GateFailureReason>,
) {
    let movement = &evidence.movement;
    if movement.marker_count < thresholds.minimum_movement_markers
        || movement.changed_sample_count != movement.marker_count
        || movement.change_lag_ms.is_none()
    {
        failures.push(GateFailureReason::MovementEvidenceIncomplete);
        return;
    }
    let Some(change_lag) = movement.change_lag_ms else {
        return;
    };
    if !change_lag.is_valid_nonnegative()
        || change_lag.count != movement.changed_sample_count
        || change_lag.p95 > thresholds.movement_p95_max_ms
    {
        failures.push(GateFailureReason::MovementP95TooHigh);
    }
}

fn evaluate_rotation(
    evidence: &GateAEvidence,
    thresholds: &GateAThresholds,
    failures: &mut Vec<GateFailureReason>,
) {
    let rotation = &evidence.rotation;
    if !rotation.is_internally_consistent()
        || rotation.relevant_samples < thresholds.minimum_rotation_samples
        || rotation.relevant_samples == 0
        || rotation.error_degrees.is_none()
    {
        failures.push(GateFailureReason::RotationEvidenceInsufficient);
        return;
    }
    let presence = rotation.present_samples as f64 / rotation.relevant_samples as f64;
    if !presence.is_finite() || presence < thresholds.rotation_presence_minimum_ratio {
        failures.push(GateFailureReason::RotationPresenceTooLow);
    }

    if rotation.cardinal.iter().any(|cardinal| {
        cardinal.relevant_samples < thresholds.minimum_cardinal_samples
            || cardinal.present_samples < thresholds.minimum_cardinal_samples
            || cardinal.error_degrees.is_none()
    }) {
        failures.push(GateFailureReason::RotationCardinalsIncomplete);
    }
    if rotation.cardinal.iter().any(|cardinal| {
        cardinal.error_degrees.is_some_and(|summary| {
            !summary.is_valid_nonnegative()
                || summary.p50 > thresholds.rotation_cardinal_median_max_degrees
        })
    }) {
        failures.push(GateFailureReason::RotationMedianErrorTooHigh);
    }
    if rotation.error_degrees.is_some_and(|summary| {
        !summary.is_valid_nonnegative() || summary.max > thresholds.rotation_max_error_degrees
    }) {
        failures.push(GateFailureReason::RotationMaxErrorTooHigh);
    }
}

fn evaluate_candidate(
    candidate: &CandidateLoadReport,
    thresholds: &GateAThresholds,
) -> CandidateEvaluation {
    let mut failures = Vec::new();
    if candidate.interval_ms == 0 || candidate.pairs.len() < thresholds.minimum_pair_count {
        failures.push(GateFailureReason::CandidatePairCountTooLow {
            interval_ms: candidate.interval_ms,
        });
    }
    if !pair_sequence_is_valid(candidate) {
        failures.push(GateFailureReason::CandidatePairSequenceInvalid {
            interval_ms: candidate.interval_ms,
        });
    }
    if !candidate.errors.is_zero() {
        failures.push(GateFailureReason::CandidateErrors {
            interval_ms: candidate.interval_ms,
        });
    }
    if candidate.safety_abort.is_some() {
        failures.push(GateFailureReason::CandidateSafetyAbort {
            interval_ms: candidate.interval_ms,
        });
    }

    let mut fps_degradation = Vec::with_capacity(candidate.pairs.len());
    let mut frame_time_regressed_pairs = 0_usize;
    let mut windows_valid = true;
    let mut selected_coverage_complete = true;
    for pair in &candidate.pairs {
        let baseline = &pair.baseline;
        let measured = &pair.candidate;
        if baseline.request_successes != baseline.request_attempts
            || measured.request_successes != measured.request_attempts
            || baseline.selected_player_samples != baseline.request_successes
            || measured.selected_player_samples != measured.request_successes
        {
            selected_coverage_complete = false;
        }
        if baseline.duration_ms != measured.duration_ms
            || !baseline.is_valid(thresholds.minimum_request_samples_per_window)
            || !measured.is_valid(thresholds.minimum_request_samples_per_window)
            || baseline.server_fps.mean <= 0.0
            || baseline.frame_time_ms.p95 <= 0.0
        {
            windows_valid = false;
            continue;
        }

        fps_degradation.push(
            (baseline.server_fps.mean - measured.server_fps.mean) / baseline.server_fps.mean
                * 100.0,
        );
        let frame_time_degradation = (measured.frame_time_ms.p95 - baseline.frame_time_ms.p95)
            / baseline.frame_time_ms.p95
            * 100.0;
        if frame_time_degradation > thresholds.frame_time_pair_degradation_max_percent {
            frame_time_regressed_pairs += 1;
        }
    }

    if !windows_valid || fps_degradation.len() != candidate.pairs.len() {
        failures.push(GateFailureReason::CandidateWindowInvalid {
            interval_ms: candidate.interval_ms,
        });
    }
    if !selected_coverage_complete {
        failures.push(GateFailureReason::SelectedPlayerCoverageIncomplete {
            interval_ms: candidate.interval_ms,
        });
    }

    let confidence = mean_confidence_interval_95(&fps_degradation).ok();
    if let Some(interval) = confidence {
        if interval.mean > thresholds.fps_mean_degradation_max_percent {
            failures.push(GateFailureReason::FpsMeanDegradationTooHigh {
                interval_ms: candidate.interval_ms,
            });
        }
        if interval.upper > thresholds.fps_confidence_upper_max_percent {
            failures.push(GateFailureReason::FpsConfidenceUpperBoundTooHigh {
                interval_ms: candidate.interval_ms,
            });
        }
    } else if !failures.iter().any(|reason| {
        matches!(
            reason,
            GateFailureReason::CandidateWindowInvalid { .. }
                | GateFailureReason::CandidatePairCountTooLow { .. }
        )
    }) {
        failures.push(GateFailureReason::CandidateWindowInvalid {
            interval_ms: candidate.interval_ms,
        });
    }
    if frame_time_regressed_pairs > thresholds.maximum_frame_time_regressed_pairs {
        failures.push(GateFailureReason::FrameTimePairRegressions {
            interval_ms: candidate.interval_ms,
        });
    }

    CandidateEvaluation {
        interval_ms: candidate.interval_ms,
        passed: failures.is_empty(),
        fps_degradation_percent_95: confidence,
        frame_time_regressed_pairs,
        failure_reasons: failures,
    }
}

fn pair_sequence_is_valid(candidate: &CandidateLoadReport) -> bool {
    candidate.pairs.iter().enumerate().all(|(index, pair)| {
        pair.pair_index == index as u32
            && pair.order
                == if index % 2 == 0 {
                    PairOrder::BaselineThenCandidate
                } else {
                    PairOrder::CandidateThenBaseline
                }
    })
}

fn push_unique(reasons: &mut Vec<GateFailureReason>, reason: GateFailureReason) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}
