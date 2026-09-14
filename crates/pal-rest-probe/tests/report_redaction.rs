use pal_rest_probe::{
    CandidateLoadReport, CardinalDirection, DistributionSummary, GateAEvidence, LoadErrorCounts,
    MovementEvidence, MovementObservation, PairOrder, PairedLoadWindow, PreflightEvidence,
    PrivacyEvidence, ReportError, RotationEvidence, RotationObservation, ServerFingerprint,
    WindowMetrics, encode_redacted_report, evaluate,
};

fn summary(value: f64, count: u64) -> DistributionSummary {
    DistributionSummary::from_constant(value, count).expect("finite distribution")
}

fn window() -> WindowMetrics {
    WindowMetrics {
        duration_ms: 300_000,
        request_attempts: 30,
        request_successes: 30,
        selected_player_samples: 30,
        response_latency_ms: summary(20.0, 30),
        decode_latency_ms: summary(2.0, 30),
        decoded_body_bytes: summary(64_000.0, 30),
        actor_count: summary(100.0, 30),
        server_fps: summary(60.0, 30),
        frame_time_ms: summary(16.0, 30),
        probe_cpu_percent: summary(0.4, 30),
        probe_private_bytes_peak: 20 * 1024 * 1024,
    }
}

fn report() -> pal_rest_probe::GateAReport {
    let pairs = (0..5)
        .map(|index| PairedLoadWindow {
            pair_index: index,
            order: if index % 2 == 0 {
                PairOrder::BaselineThenCandidate
            } else {
                PairOrder::CandidateThenBaseline
            },
            baseline: window(),
            candidate: window(),
        })
        .collect();
    let rotation = RotationEvidence::from_observations((0..100).map(|index| {
        let expected = match index % 4 {
            0 => CardinalDirection::North,
            1 => CardinalDirection::East,
            2 => CardinalDirection::South,
            _ => CardinalDirection::West,
        };
        RotationObservation {
            expected,
            observed_degrees: Some(expected.degrees()),
        }
    }))
    .expect("rotation");
    let movement = MovementEvidence::from_observations((0..5).map(|_| MovementObservation {
        changed_after_ms: Some(500.0),
    }))
    .expect("movement");
    let evidence = GateAEvidence {
        schema_version: 1,
        preflight: PreflightEvidence::complete(ServerFingerprint::from_digest([9; 32])),
        candidates: vec![CandidateLoadReport {
            interval_ms: 1_000,
            pairs,
            errors: LoadErrorCounts::default(),
            safety_abort: None,
        }],
        rotation,
        movement,
        privacy: PrivacyEvidence {
            artifact_scan_clean: true,
        },
    };
    evaluate(&evidence)
}

#[test]
fn serialized_report_contains_aggregates_and_no_raw_or_secret_fields() {
    let encoded = encode_redacted_report(
        &report(),
        &[
            b"203.0.113.42".as_slice(),
            b"account-secret".as_slice(),
            b"other-player-id".as_slice(),
            b"guild-private".as_slice(),
            b"Basic ".as_slice(),
        ],
    )
    .expect("redacted report");
    let text = String::from_utf8(encoded).expect("json");

    for forbidden_field in [
        "\"response_body\"",
        "\"raw_response\"",
        "\"password\"",
        "\"credential\"",
        "\"nickname\"",
        "\"guild\"",
        "\"user_id\"",
        "\"instance_id\"",
    ] {
        assert!(!text.contains(forbidden_field), "{forbidden_field}");
    }
    assert!(text.contains("\"p95\""));
    assert!(text.contains("\"pairs\""));
}

#[test]
fn artifact_sentinel_match_fails_closed() {
    let error = encode_redacted_report(&report(), &[b"schema_version"])
        .expect_err("scanner must reject matching bytes");

    assert_eq!(error, ReportError::SensitiveSentinel);
}

#[test]
fn empty_sentinels_are_rejected_instead_of_vacuously_matching_everywhere() {
    let error =
        encode_redacted_report(&report(), &[b""]).expect_err("empty sentinel is invalid input");

    assert_eq!(error, ReportError::EmptySentinel);
}
