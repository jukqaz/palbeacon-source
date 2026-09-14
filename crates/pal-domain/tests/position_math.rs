use pal_domain::{
    Freshness, PositionSample, PositionValidationError, SampleClock, classify_freshness,
    shortest_angle_lerp,
};

fn assert_degrees(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.001,
        "expected {expected} degrees, got {actual}"
    );
}

fn valid_sample(heading_degrees: Option<f32>) -> Result<PositionSample, PositionValidationError> {
    PositionSample::new(
        "world-a",
        b"subject-a",
        b"boot-a",
        4,
        17,
        10.0,
        -20.0,
        3.5,
        heading_degrees,
        SampleClock::received_with_age(25, 1_000),
    )
}

#[test]
fn interpolates_across_zero_by_the_short_path_in_both_directions() {
    assert_degrees(shortest_angle_lerp(359.0, 1.0, 0.5), 0.0);
    assert_degrees(shortest_angle_lerp(1.0, 359.0, 0.5), 0.0);
}

#[test]
fn clamps_angle_interpolation_progress_to_the_segment() {
    assert_degrees(shortest_angle_lerp(359.0, 1.0, -1.0), 359.0);
    assert_degrees(shortest_angle_lerp(359.0, 1.0, 2.0), 1.0);
}

#[test]
fn classifies_every_freshness_boundary() {
    let cases = [
        (1_499, Freshness::Live),
        (1_500, Freshness::Live),
        (1_501, Freshness::Delayed),
        (5_000, Freshness::Delayed),
        (5_001, Freshness::Stale),
    ];

    for (age_ms, expected) in cases {
        assert_eq!(classify_freshness(age_ms, true), expected);
    }
}

#[test]
fn disconnected_state_takes_precedence_over_sample_age() {
    assert_eq!(classify_freshness(0, false), Freshness::Offline);
    assert_eq!(classify_freshness(u64::MAX, false), Freshness::Offline);
}

#[test]
fn sample_age_uses_only_saturating_monotonic_progress() {
    let clock = SampleClock::received_with_age(1_501, 10_000);

    assert_eq!(clock.age_upper_bound_ms(10_000), 1_501);
    assert_eq!(clock.age_upper_bound_ms(13_500), 5_001);
    assert_eq!(clock.age_upper_bound_ms(9_000), 1_501);

    let near_limit = SampleClock::received_with_age(u64::MAX - 5, 10_000);
    assert_eq!(near_limit.age_upper_bound_ms(10_010), u64::MAX);
}

#[test]
fn position_construction_preserves_identity_source_and_coordinates() {
    let sample = valid_sample(None).unwrap();

    assert_eq!(sample.world_alias(), "world-a");
    assert_eq!(sample.subject_id(), b"subject-a");
    assert_eq!(sample.agent_boot_id(), b"boot-a");
    assert_eq!(sample.source_connection_generation(), 4);
    assert_eq!(sample.sequence(), 17);
    assert_eq!(sample.x(), 10.0);
    assert_eq!(sample.y(), -20.0);
    assert_eq!(sample.z(), 3.5);
    assert_eq!(sample.heading_degrees(), None);
    assert_eq!(sample.clock(), SampleClock::received_with_age(25, 1_000));
}

#[test]
fn position_construction_normalizes_finite_headings() {
    assert_degrees(
        valid_sample(Some(721.0))
            .unwrap()
            .heading_degrees()
            .unwrap(),
        1.0,
    );
    assert_degrees(
        valid_sample(Some(-1.0)).unwrap().heading_degrees().unwrap(),
        359.0,
    );
}

#[test]
fn position_construction_rejects_empty_identity_and_boot_values() {
    let make = |world_alias, subject_id: &[u8], boot_id: &[u8]| {
        PositionSample::new(
            world_alias,
            subject_id,
            boot_id,
            1,
            1,
            0.0,
            0.0,
            0.0,
            None,
            SampleClock::received_with_age(0, 0),
        )
    };

    assert_eq!(
        make("", b"subject", b"boot").unwrap_err().field(),
        "world_alias"
    );
    assert_eq!(
        make("world", b"", b"boot").unwrap_err().field(),
        "subject_id"
    );
    assert_eq!(
        make("world", b"subject", b"").unwrap_err().field(),
        "agent_boot_id"
    );
}

#[test]
fn position_construction_rejects_each_non_finite_coordinate() {
    for (x, y, z, expected_field) in [
        (f64::NAN, 0.0, 0.0, "x"),
        (0.0, f64::INFINITY, 0.0, "y"),
        (0.0, 0.0, f64::NEG_INFINITY, "z"),
    ] {
        let error = PositionSample::new(
            "world",
            b"subject",
            b"boot",
            1,
            1,
            x,
            y,
            z,
            None,
            SampleClock::received_with_age(0, 0),
        )
        .unwrap_err();

        assert_eq!(error.field(), expected_field);
    }
}

#[test]
fn position_construction_rejects_non_finite_heading() {
    for heading in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            valid_sample(Some(heading)).unwrap_err().field(),
            "heading_degrees"
        );
    }
}
