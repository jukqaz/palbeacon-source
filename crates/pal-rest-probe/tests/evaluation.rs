use pal_rest_probe::{
    CandidateLoadReport, CardinalDirection, DistributionSummary, GateAEvidence, GateAThresholds,
    GateDecision, GateFailureReason, LoadErrorCounts, MovementEvidence, MovementObservation,
    PairOrder, PairedLoadWindow, PreflightEvidence, PrivacyEvidence, ProbeRunError, ProbeRunner,
    RotationEvidence, RotationObservation, ServerFingerprint, ServerFingerprintInput,
    WindowMetrics, evaluate, evaluate_unattested,
};

fn summary(value: f64, count: u64) -> DistributionSummary {
    DistributionSummary::from_constant(value, count).expect("finite distribution")
}

fn window(fps: f64, frame_time_p95_ms: f64, selected: u64) -> WindowMetrics {
    WindowMetrics {
        duration_ms: 300_000,
        request_attempts: 30,
        request_successes: 30,
        selected_player_samples: selected,
        response_latency_ms: summary(20.0, 30),
        decode_latency_ms: summary(2.0, 30),
        decoded_body_bytes: summary(64_000.0, 30),
        actor_count: summary(100.0, 30),
        server_fps: summary(fps, 30),
        frame_time_ms: DistributionSummary {
            count: 30,
            min: frame_time_p95_ms,
            max: frame_time_p95_ms,
            mean: frame_time_p95_ms,
            p50: frame_time_p95_ms,
            p95: frame_time_p95_ms,
        },
        probe_cpu_percent: summary(0.4, 30),
        probe_private_bytes_peak: 24 * 1024 * 1024,
    }
}

fn candidate(interval_ms: u64, impacts_percent: &[f64]) -> CandidateLoadReport {
    let pairs = impacts_percent
        .iter()
        .enumerate()
        .map(|(index, impact)| {
            let baseline_fps = 60.0;
            let candidate_fps = baseline_fps * (1.0 - impact / 100.0);
            PairedLoadWindow {
                pair_index: index as u32,
                order: if index % 2 == 0 {
                    PairOrder::BaselineThenCandidate
                } else {
                    PairOrder::CandidateThenBaseline
                },
                baseline: window(baseline_fps, 16.0, 30),
                candidate: window(candidate_fps, 16.1, 30),
            }
        })
        .collect();

    CandidateLoadReport {
        interval_ms,
        pairs,
        errors: LoadErrorCounts::default(),
        safety_abort: None,
    }
}

fn rotation() -> RotationEvidence {
    let observations = (0..100).map(|index| {
        let expected = match index % 4 {
            0 => CardinalDirection::North,
            1 => CardinalDirection::East,
            2 => CardinalDirection::South,
            _ => CardinalDirection::West,
        };
        RotationObservation {
            expected,
            observed_degrees: Some(expected.degrees() + 2.0),
        }
    });
    RotationEvidence::from_observations(observations).expect("valid rotation evidence")
}

fn movement() -> MovementEvidence {
    MovementEvidence::from_observations((0..5).map(|_| MovementObservation {
        changed_after_ms: Some(900.0),
    }))
    .expect("valid movement evidence")
}

fn valid_evidence() -> GateAEvidence {
    GateAEvidence {
        schema_version: 1,
        preflight: PreflightEvidence::complete(ServerFingerprint::from_digest([7; 32])),
        candidates: vec![
            candidate(2_000, &[0.5; 5]),
            candidate(1_000, &[0.5; 5]),
            candidate(500, &[0.5; 5]),
        ],
        rotation: rotation(),
        movement: movement(),
        privacy: PrivacyEvidence {
            artifact_scan_clean: true,
        },
    }
}

#[test]
fn evaluator_embeds_the_fixed_gate_a_thresholds() {
    let report = evaluate(&valid_evidence());

    assert_eq!(report.thresholds, GateAThresholds::default());
}

#[test]
fn preflight_derives_a_stable_opaque_server_fingerprint() {
    let input = ServerFingerprintInput {
        rest_version: "v1.0.1".into(),
        steam_manifest_id: Some(24_181_527),
        executable_sha256: [3; 32],
        server_subject_id: [5; 32],
        executable_hash_verified: true,
        endpoint_private_lan: true,
        auth_ok: true,
        info_ok: true,
        privacy_boundary_ok: true,
    };

    let first = ProbeRunner::preflight(&input).expect("preflight");
    let second = ProbeRunner::preflight(&input).expect("preflight");
    let changed = ProbeRunner::preflight(&ServerFingerprintInput {
        executable_sha256: [4; 32],
        ..input.clone()
    })
    .expect("preflight");
    let changed_server = ProbeRunner::preflight(&ServerFingerprintInput {
        server_subject_id: [6; 32],
        ..input
    })
    .expect("preflight");

    assert_eq!(first.server_fingerprint, second.server_fingerprint);
    assert_ne!(first.server_fingerprint, changed.server_fingerprint);
    assert_ne!(first.server_fingerprint, changed_server.server_fingerprint);
}

#[test]
fn preflight_rejects_a_missing_executable_hash() {
    let input = ServerFingerprintInput {
        rest_version: "v1.0.1".into(),
        steam_manifest_id: None,
        executable_sha256: [0; 32],
        server_subject_id: [5; 32],
        executable_hash_verified: true,
        endpoint_private_lan: true,
        auth_ok: true,
        info_ok: true,
        privacy_boundary_ok: true,
    };

    assert_eq!(
        ProbeRunner::preflight(&input),
        Err(ProbeRunError::InvalidPreflightIdentity)
    );
}

#[test]
fn preflight_rejects_unverified_executable_or_missing_server_identity() {
    let input = ServerFingerprintInput {
        rest_version: "v1.0.1".into(),
        steam_manifest_id: None,
        executable_sha256: [3; 32],
        server_subject_id: [5; 32],
        executable_hash_verified: false,
        endpoint_private_lan: true,
        auth_ok: true,
        info_ok: true,
        privacy_boundary_ok: true,
    };

    assert_eq!(
        ProbeRunner::preflight(&input),
        Err(ProbeRunError::InvalidPreflightIdentity)
    );
    assert_eq!(
        ProbeRunner::preflight(&ServerFingerprintInput {
            executable_hash_verified: true,
            server_subject_id: [0; 32],
            ..input
        }),
        Err(ProbeRunError::InvalidPreflightIdentity)
    );
}

#[test]
fn selects_the_shortest_candidate_that_passes_every_gate() {
    let report = evaluate(&valid_evidence());

    assert_eq!(report.decision, GateDecision::Go);
    assert_eq!(report.selected_interval_ms, Some(500));
    assert!(report.failure_reasons.is_empty());
}

#[test]
fn imported_unattested_evidence_can_never_issue_a_go_decision() {
    let report = evaluate_unattested(&valid_evidence());

    assert_eq!(report.decision, GateDecision::NoGo);
    assert_eq!(report.selected_interval_ms, None);
    assert!(
        report
            .failure_reasons
            .contains(&GateFailureReason::UnattestedEvidence)
    );
}

#[test]
fn rejects_noncanonical_candidate_intervals_pair_counts_and_window_duration() {
    let mut evidence = valid_evidence();
    evidence.candidates[0].interval_ms = 1;
    evidence.candidates[1].pairs.pop();
    evidence.candidates[1].pairs[0].baseline.duration_ms = 1;
    evidence.candidates[1].pairs[0].candidate.duration_ms = 1;

    let report = evaluate(&evidence);

    assert_eq!(report.decision, GateDecision::NoGo);
    assert!(
        report
            .failure_reasons
            .contains(&GateFailureReason::CandidatePlanInvalid)
    );
}

#[test]
fn rejects_any_candidate_with_request_or_privacy_errors() {
    let mut evidence = valid_evidence();
    evidence.candidates[0].errors.timeouts = 1;
    evidence.candidates[1].errors.privacy = 1;
    evidence.candidates[2].errors.auth = 1;

    let report = evaluate(&evidence);

    assert_eq!(report.decision, GateDecision::NoGo);
    assert_eq!(report.selected_interval_ms, None);
    assert!(report.failure_reasons.iter().any(|reason| matches!(
        reason,
        GateFailureReason::CandidateErrors { interval_ms: 500 }
    )));
}

#[test]
fn rejects_missing_or_slow_controlled_movement_evidence() {
    let mut evidence = valid_evidence();
    evidence.movement = MovementEvidence::from_observations((0..5).map(|_| MovementObservation {
        changed_after_ms: Some(1_501.0),
    }))
    .expect("valid aggregate");

    let report = evaluate(&evidence);

    assert_eq!(report.decision, GateDecision::NoGo);
    assert!(
        report
            .failure_reasons
            .contains(&GateFailureReason::MovementP95TooHigh)
    );
}

#[test]
fn rejects_rotation_without_cardinal_accuracy_and_coverage() {
    let mut evidence = valid_evidence();
    evidence.rotation =
        RotationEvidence::from_observations((0..100).map(|index| RotationObservation {
            expected: CardinalDirection::North,
            observed_degrees: if index == 0 { None } else { Some(25.0) },
        }))
        .expect("valid aggregate");

    let report = evaluate(&evidence);

    assert_eq!(report.decision, GateDecision::NoGo);
    assert!(
        report
            .failure_reasons
            .contains(&GateFailureReason::RotationCardinalsIncomplete)
    );
    assert!(
        report
            .failure_reasons
            .contains(&GateFailureReason::RotationMaxErrorTooHigh)
    );
}

#[test]
fn rejects_a_confidence_bound_above_two_percent_even_if_mean_is_below_one() {
    let mut evidence = valid_evidence();
    evidence.candidates = vec![candidate(1_000, &[0.0, 0.0, 0.0, 0.0, 3.0])];

    let report = evaluate(&evidence);

    assert_eq!(report.decision, GateDecision::NoGo);
    assert!(report.failure_reasons.iter().any(|reason| matches!(
        reason,
        GateFailureReason::FpsConfidenceUpperBoundTooHigh { interval_ms: 1_000 }
    )));
}

#[test]
fn rejects_two_pairs_with_more_than_two_percent_frame_time_degradation() {
    let mut evidence = valid_evidence();
    evidence.candidates = vec![candidate(1_000, &[0.5; 5])];
    evidence.candidates[0].pairs[0].candidate.frame_time_ms = summary(16.4, 30);
    evidence.candidates[0].pairs[1].candidate.frame_time_ms = summary(16.4, 30);

    let report = evaluate(&evidence);

    assert_eq!(report.decision, GateDecision::NoGo);
    assert!(report.failure_reasons.iter().any(|reason| matches!(
        reason,
        GateFailureReason::FrameTimePairRegressions { interval_ms: 1_000 }
    )));
}

#[test]
fn rejects_less_than_full_selected_player_coverage() {
    let mut evidence = valid_evidence();
    for candidate in &mut evidence.candidates {
        candidate.pairs[0].candidate.selected_player_samples = 29;
    }

    let report = evaluate(&evidence);

    assert_eq!(report.decision, GateDecision::NoGo);
    assert!(report.failure_reasons.iter().any(|reason| matches!(
        reason,
        GateFailureReason::SelectedPlayerCoverageIncomplete { .. }
    )));
}

#[test]
fn rejects_request_aggregates_with_inconsistent_sample_counts() {
    let mut evidence = valid_evidence();
    for candidate in &mut evidence.candidates {
        candidate.pairs[0].candidate.response_latency_ms.count = 31;
        candidate.pairs[0].candidate.actor_count.count = 29;
    }

    let report = evaluate(&evidence);

    assert_eq!(report.decision, GateDecision::NoGo);
    assert!(
        report
            .failure_reasons
            .iter()
            .any(|reason| matches!(reason, GateFailureReason::CandidateWindowInvalid { .. }))
    );
}

#[test]
fn rejects_internally_inconsistent_rotation_aggregates() {
    let mut evidence = valid_evidence();
    evidence.rotation.present_samples = evidence.rotation.relevant_samples + 1;
    evidence.rotation.cardinal[0].direction = CardinalDirection::East;
    evidence.rotation.cardinal[1]
        .error_degrees
        .as_mut()
        .expect("cardinal summary")
        .count += 1;

    let report = evaluate(&evidence);

    assert_eq!(report.decision, GateDecision::NoGo);
    assert!(
        report
            .failure_reasons
            .contains(&GateFailureReason::RotationEvidenceInsufficient)
    );
}
