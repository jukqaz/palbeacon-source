#![cfg(feature = "development-local-readonly-position")]

use pal_domain::Freshness;
use pal_overlay_win::local_alignment_diagnostic::{
    AlignmentSample, LocalAlignmentDiagnosticError, evaluate_local_alignment,
    parse_local_alignment_observation,
};
use sha2::{Digest, Sha256};

const KNOWN_MAP_SHA: &str = "aa81bdddbc525898b0a70b238cc936d0f8b0416c4ead922797b066d871938d8b";

#[test]
fn constant_samples_and_a_three_four_five_marker_delta_yield_five_pixels() {
    let observation = observation(103.0, 204.0);
    let samples = constant_samples(100.0, 200.0);

    let result = evaluate_local_alignment(&observation, &samples).expect("inside thresholds");

    assert_eq!(result.projected_spread_px(), 0.0);
    assert_eq!(result.residual_px(), 5.0);
    assert_eq!(result.sample_window_ms(), 900);
    assert!(!result.gate_b_approved());
    assert_eq!(result.claim(), "development_smoke_only_not_gate_b");
}

#[test]
fn a_moved_sample_over_one_pixel_is_rejected() {
    let observation = observation(100.0, 200.0);
    let mut samples = constant_samples(100.0, 200.0);
    samples[9] = sample(102.0, 200.0, 900);

    assert_eq!(
        evaluate_local_alignment(&observation, &samples),
        Err(LocalAlignmentDiagnosticError::SpreadThresholdExceeded)
    );
}

#[test]
fn sample_count_and_window_are_strict() {
    let observation = observation(100.0, 200.0);
    let samples = constant_samples(100.0, 200.0);

    assert_eq!(
        evaluate_local_alignment(&observation, &samples[..9]),
        Err(LocalAlignmentDiagnosticError::SampleCount)
    );

    let short_window: [AlignmentSample; 10] =
        std::array::from_fn(|index| sample(100.0, 200.0, index as u64 * 99));
    assert_eq!(
        evaluate_local_alignment(&observation, &short_window),
        Err(LocalAlignmentDiagnosticError::SampleWindow)
    );
}

#[test]
fn sample_offsets_must_be_strictly_increasing() {
    let observation = observation(100.0, 200.0);
    let mut out_of_order = constant_samples(100.0, 200.0);
    out_of_order[5] = sample(100.0, 200.0, 300);

    assert_eq!(
        evaluate_local_alignment(&observation, &out_of_order),
        Err(LocalAlignmentDiagnosticError::SampleOrder)
    );
}

#[test]
fn freshness_heading_finite_and_map_bounds_are_fail_closed() {
    let observation = observation(100.0, 200.0);

    let mut stale = constant_samples(100.0, 200.0);
    stale[4] = AlignmentSample::new(100.0, 200.0, 400, Freshness::Stale, true);
    assert_eq!(
        evaluate_local_alignment(&observation, &stale),
        Err(LocalAlignmentDiagnosticError::SampleNotLive)
    );

    let mut headingless = constant_samples(100.0, 200.0);
    headingless[4] = AlignmentSample::new(100.0, 200.0, 400, Freshness::Live, false);
    assert_eq!(
        evaluate_local_alignment(&observation, &headingless),
        Err(LocalAlignmentDiagnosticError::SampleMissingYaw)
    );

    for (x, y) in [
        (f64::NAN, 200.0),
        (100.0, f64::INFINITY),
        (-1.0, 200.0),
        (2_048.0, 200.0),
        (100.0, -1.0),
        (100.0, 2_048.0),
    ] {
        let mut invalid = constant_samples(100.0, 200.0);
        invalid[4] = sample(x, y, 400);
        assert!(evaluate_local_alignment(&observation, &invalid).is_err());
    }
}

#[test]
fn a_residual_over_eight_pixels_is_rejected() {
    let observation = observation(109.0, 200.0);

    assert_eq!(
        evaluate_local_alignment(&observation, &constant_samples(100.0, 200.0)),
        Err(LocalAlignmentDiagnosticError::ResidualThresholdExceeded)
    );
}

#[test]
fn evaluation_uses_coordinate_wise_median_instead_of_mean() {
    let observation = observation(100.125, 200.0);
    let x_values = [
        100.0, 100.0, 100.0, 100.0, 100.0, 100.25, 100.25, 100.25, 100.25, 101.0,
    ];
    let samples: [AlignmentSample; 10] =
        std::array::from_fn(|index| sample(x_values[index], 200.0, index as u64 * 100));

    let result = evaluate_local_alignment(&observation, &samples).unwrap();

    assert!(result.residual_px() < 1e-12);
    assert!((result.projected_spread_px() - 0.875).abs() < 1e-12);
}

#[test]
fn exact_spread_and_residual_thresholds_are_accepted() {
    let observation = observation(108.0, 200.0);
    let mut samples = constant_samples(100.0, 200.0);
    samples[9] = sample(101.0, 200.0, 900);

    let result = evaluate_local_alignment(&observation, &samples).unwrap();

    assert_eq!(result.projected_spread_px(), 1.0);
    assert_eq!(result.residual_px(), 8.0);
}

#[test]
fn observation_requires_exact_schema_claim_build_map_and_dimensions() {
    for (field, value) in [
        ("schema", serde_json::json!("wrong")),
        ("claim", serde_json::json!("gate_b")),
        ("game_build_id", serde_json::json!(24_181_526_u64)),
        ("map_sha256", serde_json::json!("00".repeat(32))),
        ("map_width_px", serde_json::json!(2_047_u64)),
        ("map_height_px", serde_json::json!(2_049_u64)),
    ] {
        let bytes = mutated_observation(field, value);
        assert!(
            parse_local_alignment_observation(&bytes, &sha256_hex(&bytes)).is_err(),
            "{field} must fail closed"
        );
    }
}

#[test]
fn observation_rejects_invalid_marker_nonce_and_unknown_fields() {
    for (field, value) in [
        ("observed_marker_x_px", serde_json::json!(-0.1)),
        ("observed_marker_x_px", serde_json::json!(2_048.0)),
        ("observed_marker_y_px", serde_json::json!(-0.1)),
        ("observed_marker_y_px", serde_json::json!(2_048.0)),
        ("nonce", serde_json::json!("")),
        ("nonce", serde_json::json!("0".repeat(31))),
        ("nonce", serde_json::json!("0".repeat(33))),
        ("nonce", serde_json::json!("A".repeat(32))),
    ] {
        let bytes = mutated_observation(field, value);
        assert!(
            parse_local_alignment_observation(&bytes, &sha256_hex(&bytes)).is_err(),
            "{field} must fail closed"
        );
    }

    let non_finite = String::from_utf8(valid_observation_bytes(100.0, 200.0))
        .unwrap()
        .replacen(
            "\"observed_marker_x_px\":100,",
            "\"observed_marker_x_px\":1e400,",
            1,
        )
        .into_bytes();
    assert!(parse_local_alignment_observation(&non_finite, &sha256_hex(&non_finite)).is_err());

    let mut unknown: serde_json::Value =
        serde_json::from_slice(&valid_observation_bytes(100.0, 200.0)).unwrap();
    unknown["unexpected"] = serde_json::json!(true);
    let bytes = serde_json::to_vec(&unknown).unwrap();
    assert!(parse_local_alignment_observation(&bytes, &sha256_hex(&bytes)).is_err());
}

#[test]
fn observation_sha_must_be_exact_lowercase_hex_and_match_the_bytes() {
    let bytes = valid_observation_bytes(100.0, 200.0);
    let valid_sha = sha256_hex(&bytes);

    for invalid in [
        "0".repeat(63),
        "g".repeat(64),
        valid_sha.to_uppercase(),
        "0".repeat(64),
    ] {
        assert!(parse_local_alignment_observation(&bytes, &invalid).is_err());
    }

    assert!(parse_local_alignment_observation(&bytes, &valid_sha).is_ok());
}

#[test]
fn canonical_csharp_float_spellings_accept_zero_integer_and_fractional_markers() {
    for (marker_x, marker_y) in [("0", "2047"), ("100", "200"), ("100.25", "199.75")] {
        let bytes = canonical_observation_bytes(marker_x, marker_y);
        assert!(
            parse_local_alignment_observation(&bytes, &sha256_hex(&bytes)).is_ok(),
            "{marker_x}, {marker_y}"
        );
    }

    for alternate in [("0.0", "2047"), ("100.0", "200"), ("100.250", "199.75")] {
        let bytes = canonical_observation_bytes(alternate.0, alternate.1);
        assert_eq!(
            parse_local_alignment_observation(&bytes, &sha256_hex(&bytes)),
            Err(LocalAlignmentDiagnosticError::InvalidObservationDocument)
        );
    }
}

#[test]
fn success_json_is_one_fixed_identity_free_line() {
    let observation_bytes = valid_observation_bytes(103.0, 204.0);
    let observation_sha = sha256_hex(&observation_bytes);
    let observation =
        parse_local_alignment_observation(&observation_bytes, &observation_sha).unwrap();
    let result = evaluate_local_alignment(&observation, &constant_samples(100.0, 200.0)).unwrap();

    let line = result.to_json_line();
    assert!(line.ends_with('\n'));
    assert_eq!(line.lines().count(), 1);

    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    let object = value.as_object().unwrap();
    // serde_json's workspace-wide `preserve_order` feature changes Value's iteration
    // order under `--all-features`; key membership must not depend on feature unification.
    let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "build_id",
            "claim",
            "gate_b_approved",
            "map_sha256",
            "projected_spread_px",
            "residual_px",
            "sample_count",
            "sample_window_ms",
            "schema",
            "within_development_smoke_threshold",
        ]
    );
    assert_eq!(value["schema"], "pal_companion.local_alignment_smoke.v1");
    assert_eq!(value["claim"], "development_smoke_only_not_gate_b");
    assert_eq!(value["gate_b_approved"], false);
    assert_eq!(value["build_id"], 24_181_527_u64);
    assert_eq!(value["map_sha256"], KNOWN_MAP_SHA);
    assert!(value.get("observation_sha256").is_none());
    assert_eq!(value["sample_count"], 10);
    assert_eq!(value["sample_window_ms"], 900);
    assert_eq!(value["projected_spread_px"], 0.0);
    assert_eq!(value["residual_px"], 5.0);
    assert_eq!(value["within_development_smoke_threshold"], true);

    for sensitive in [
        "operator-generated-opaque-value",
        "capture_nonce",
        observation_sha.as_str(),
        "world_x",
        "world_y",
        "yaw",
        "path",
        "pid",
        "hwnd",
        "timestamp",
        "accepted_monotonic",
    ] {
        assert!(!line.to_ascii_lowercase().contains(sensitive));
    }
}

fn observation(
    marker_x: f64,
    marker_y: f64,
) -> pal_overlay_win::local_alignment_diagnostic::LocalAlignmentObservation {
    let bytes = valid_observation_bytes(marker_x, marker_y);
    parse_local_alignment_observation(&bytes, &sha256_hex(&bytes)).unwrap()
}

fn constant_samples(x: f64, y: f64) -> [AlignmentSample; 10] {
    std::array::from_fn(|index| sample(x, y, index as u64 * 100))
}

fn sample(x: f64, y: f64, accepted_at_ms: u64) -> AlignmentSample {
    AlignmentSample::new(x, y, accepted_at_ms, Freshness::Live, true)
}

fn valid_observation_bytes(marker_x: f64, marker_y: f64) -> Vec<u8> {
    canonical_observation_bytes(&marker_x.to_string(), &marker_y.to_string())
}

fn canonical_observation_bytes(marker_x: &str, marker_y: &str) -> Vec<u8> {
    format!(
        concat!(
            "{{",
            "\"schema\":\"pal_companion.local_alignment_observation.v1\",",
            "\"claim\":\"independent_native_marker_observation_not_gate_b\",",
            "\"game_build_id\":24181527,",
            "\"map_sha256\":\"{KNOWN_MAP_SHA}\",",
            "\"map_width_px\":2048,",
            "\"map_height_px\":2048,",
            "\"observed_marker_x_px\":{marker_x},",
            "\"observed_marker_y_px\":{marker_y},",
            "\"nonce\":\"000102030405060708090a0b0c0d0e0f\"",
            "}}"
        ),
        KNOWN_MAP_SHA = KNOWN_MAP_SHA,
        marker_x = marker_x,
        marker_y = marker_y,
    )
    .into_bytes()
}

fn mutated_observation(field: &str, value: serde_json::Value) -> Vec<u8> {
    let mut observation: serde_json::Value =
        serde_json::from_slice(&valid_observation_bytes(100.0, 200.0)).unwrap();
    observation[field] = value;
    serde_json::to_vec(&observation).unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
