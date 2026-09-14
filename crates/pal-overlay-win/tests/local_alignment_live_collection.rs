#![cfg(feature = "development-local-alignment-diagnostic")]

use pal_domain::{PositionSample, SampleClock};
use pal_overlay_win::{
    local_alignment_diagnostic::{
        LocalAlignmentDiagnosticError, LocalAlignmentObservation, parse_local_alignment_observation,
    },
    local_alignment_runtime::{
        LocalAlignmentCollectionError, LocalAlignmentCollectionUpdate, LocalAlignmentCollector,
    },
};
use pal_state::{ClockInvalidReason, PositionSourceEvent};
use sha2::{Digest, Sha256};

const MAP_SHA256: &str = "aa81bdddbc525898b0a70b238cc936d0f8b0416c4ead922797b066d871938d8b";
const CENTER_WORLD_X: f64 = -375_000.0;
const CENTER_WORLD_Y: f64 = 0.0;

#[test]
fn ten_constant_live_heading_samples_complete_at_exactly_nine_hundred_ms() {
    let mut collector = connected_collector();

    for index in 0..9 {
        assert_eq!(
            collector
                .consume(Some(sample(index + 1, index * 100, 0, true)), index * 100)
                .unwrap(),
            LocalAlignmentCollectionUpdate::Pending
        );
    }
    assert_eq!(collector.accepted_sample_count(), 9);

    let result = match collector
        .consume(Some(sample(10, 900, 0, true)), 900)
        .unwrap()
    {
        LocalAlignmentCollectionUpdate::Complete(result) => result,
        LocalAlignmentCollectionUpdate::Pending => panic!("the tenth sample must complete"),
    };

    assert_eq!(collector.accepted_sample_count(), 10);
    assert_eq!(result.sample_window_ms(), 900);
    assert_eq!(result.projected_spread_px(), 0.0);
    assert_eq!(result.residual_px(), 0.0);
    assert!(!result.gate_b_approved());
}

#[test]
fn empty_polls_and_repeated_same_generation_connects_do_not_count() {
    let mut collector = LocalAlignmentCollector::new(observation());

    assert_eq!(
        collector.consume(None, 0).unwrap(),
        LocalAlignmentCollectionUpdate::Pending
    );
    assert_eq!(
        collector
            .consume(Some(PositionSourceEvent::Connected { generation: 1 }), 1)
            .unwrap(),
        LocalAlignmentCollectionUpdate::Pending
    );
    assert_eq!(
        collector
            .consume(Some(PositionSourceEvent::Connected { generation: 1 }), 2)
            .unwrap(),
        LocalAlignmentCollectionUpdate::Pending
    );
    assert_eq!(collector.accepted_sample_count(), 0);
}

#[test]
fn a_sample_before_connection_or_from_a_new_generation_fails_closed() {
    let mut unconnected = LocalAlignmentCollector::new(observation());
    assert_eq!(
        unconnected
            .consume(Some(sample(1, 0, 0, true)), 0)
            .unwrap_err(),
        LocalAlignmentCollectionError::SampleBeforeConnection
    );

    let mut changed = connected_collector();
    assert_eq!(
        changed
            .consume(Some(PositionSourceEvent::Connected { generation: 2 }), 100)
            .unwrap_err(),
        LocalAlignmentCollectionError::ConnectionChanged
    );
}

#[test]
fn stale_and_headingless_samples_fail_immediately() {
    let mut stale = connected_collector();
    assert_eq!(
        stale
            .consume(Some(sample(1, 0, 1_501, true)), 0)
            .unwrap_err(),
        LocalAlignmentCollectionError::SampleNotLive
    );

    let mut headingless = connected_collector();
    assert_eq!(
        headingless
            .consume(Some(sample(1, 0, 0, false)), 0)
            .unwrap_err(),
        LocalAlignmentCollectionError::SampleMissingYaw
    );
}

#[test]
fn disconnect_and_clock_invalid_fail_immediately() {
    let mut disconnected = connected_collector();
    assert_eq!(
        disconnected
            .consume(Some(PositionSourceEvent::Disconnected { generation: 1 }), 0)
            .unwrap_err(),
        LocalAlignmentCollectionError::SourceTerminated
    );

    let mut invalid_clock = connected_collector();
    assert_eq!(
        invalid_clock
            .consume(
                Some(PositionSourceEvent::ClockInvalid {
                    generation: 1,
                    reason: ClockInvalidReason::ProbeInvalid,
                }),
                0
            )
            .unwrap_err(),
        LocalAlignmentCollectionError::SourceTerminated
    );
}

#[test]
fn samples_outside_the_authoritative_main_map_fail_closed() {
    let mut collector = connected_collector();
    let outside = PositionSample::new(
        "world",
        b"subject",
        b"boot",
        1,
        1,
        400_000.0,
        CENTER_WORLD_Y,
        0.0,
        Some(0.0),
        SampleClock::received_with_age(0, 0),
    )
    .unwrap();

    assert_eq!(
        collector
            .consume(Some(PositionSourceEvent::Sample(outside)), 0)
            .unwrap_err(),
        LocalAlignmentCollectionError::SampleOutsideMainMap
    );
}

#[test]
fn duplicate_or_regressing_accepted_offsets_are_rejected_by_slice_a() {
    let mut duplicate = connected_collector();
    for index in 0..9 {
        assert_eq!(
            duplicate
                .consume(Some(sample(index + 1, index * 100, 0, true)), index * 100)
                .unwrap(),
            LocalAlignmentCollectionUpdate::Pending
        );
    }
    assert_eq!(
        duplicate
            .consume(Some(sample(10, 800, 0, true)), 900)
            .unwrap_err(),
        LocalAlignmentCollectionError::Diagnostic(LocalAlignmentDiagnosticError::SampleOrder)
    );
}

#[test]
fn a_completed_collector_refuses_an_eleventh_sample() {
    let mut collector = connected_collector();
    for index in 0..10 {
        collector
            .consume(Some(sample(index + 1, index * 100, 0, true)), index * 100)
            .unwrap();
    }

    assert_eq!(
        collector
            .consume(Some(sample(11, 1_000, 0, true)), 1_000)
            .unwrap_err(),
        LocalAlignmentCollectionError::AlreadyComplete
    );
    assert_eq!(collector.accepted_sample_count(), 10);
}

fn connected_collector() -> LocalAlignmentCollector {
    let mut collector = LocalAlignmentCollector::new(observation());
    assert_eq!(
        collector
            .consume(Some(PositionSourceEvent::Connected { generation: 1 }), 0)
            .unwrap(),
        LocalAlignmentCollectionUpdate::Pending
    );
    collector
}

fn sample(
    sequence: u64,
    received_at_monotonic_ms: u64,
    age_at_receive_upper_bound_ms: u64,
    has_yaw: bool,
) -> PositionSourceEvent {
    PositionSourceEvent::Sample(
        PositionSample::new(
            "world",
            b"subject",
            b"boot",
            1,
            sequence,
            CENTER_WORLD_X,
            CENTER_WORLD_Y,
            0.0,
            has_yaw.then_some(0.0),
            SampleClock::received_with_age(age_at_receive_upper_bound_ms, received_at_monotonic_ms),
        )
        .unwrap(),
    )
}

fn observation() -> LocalAlignmentObservation {
    let bytes = format!(
        concat!(
            "{{",
            "\"schema\":\"pal_companion.local_alignment_observation.v1\",",
            "\"claim\":\"independent_native_marker_observation_not_gate_b\",",
            "\"game_build_id\":24181527,",
            "\"map_sha256\":\"{MAP_SHA256}\",",
            "\"map_width_px\":2048,",
            "\"map_height_px\":2048,",
            "\"observed_marker_x_px\":1024,",
            "\"observed_marker_y_px\":1024,",
            "\"nonce\":\"000102030405060708090a0b0c0d0e0f\"",
            "}}"
        ),
        MAP_SHA256 = MAP_SHA256,
    )
    .into_bytes();
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    parse_local_alignment_observation(&bytes, &sha256).unwrap()
}
