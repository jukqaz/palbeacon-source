use pal_domain::{Freshness, PositionValidationError};
use pal_state::{
    ClockInvalidReason, Event, PositionSource, PositionSourceEvent, ServerAgentControlOutcome,
    ServerAgentFrame, ServerAgentIngestOutcome, ServerAgentSourceError, StateReducer,
};

const SUBJECT: [u8; 32] = [0xA1; 32];

#[test]
fn production_channel_requires_a_bounded_ascii_world_and_32_byte_subject() {
    assert!(matches!(
        pal_state::server_agent_channel("", SUBJECT),
        Err(ServerAgentSourceError::InvalidExpectedIdentity)
    ));
    assert!(matches!(
        pal_state::server_agent_channel("세계", SUBJECT),
        Err(ServerAgentSourceError::InvalidExpectedIdentity)
    ));
    assert!(matches!(
        pal_state::server_agent_channel("world-a", [0xA1; 31]),
        Err(ServerAgentSourceError::InvalidExpectedIdentity)
    ));
}

#[test]
fn connected_without_a_sample_remains_connected_and_stale() {
    let (sender, mut source) = server_agent_channel();

    assert_eq!(sender.connect(1), ServerAgentControlOutcome::Accepted);

    let connected = source
        .poll(10_000)
        .expect("poll should be infallible")
        .expect("connection event");
    assert_eq!(connected, PositionSourceEvent::Connected { generation: 1 });
    assert_eq!(
        source.poll(10_001).expect("poll should be infallible"),
        None
    );

    let mut reducer = StateReducer::default();
    reducer
        .apply(Event::PositionSource(connected))
        .expect("position source events are infallible");
    let state = reducer.core_state(10_001);
    assert!(state.connected());
    assert_eq!(state.freshness(), Freshness::Stale);
    assert!(state.position_sample().is_none());
}

#[test]
fn unavailable_world_supersedes_a_sample_and_keeps_the_generation_recoverable() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(2), ServerAgentControlOutcome::Accepted);
    assert_eq!(
        source.poll(1).unwrap(),
        Some(PositionSourceEvent::Connected { generation: 2 })
    );
    assert_eq!(
        sender.publish_position(frame(2, b"boot-a", 1), 2).unwrap(),
        ServerAgentIngestOutcome::Accepted
    );
    assert_eq!(sender.unavailable(2), ServerAgentControlOutcome::Accepted);
    assert_eq!(
        source.poll(3).unwrap(),
        Some(PositionSourceEvent::Unavailable { generation: 2 })
    );
    assert_eq!(
        sender.publish_position(frame(2, b"boot-a", 2), 4).unwrap(),
        ServerAgentIngestOutcome::Accepted
    );
    assert!(matches!(
        source.poll(4).unwrap(),
        Some(PositionSourceEvent::Sample(sample)) if sample.sequence() == 2
    ));
}

#[test]
fn increasing_sequence_gaps_are_counted_and_only_the_latest_sample_is_retained() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(7), ServerAgentControlOutcome::Accepted);
    assert!(matches!(
        source.poll(20_000).expect("poll"),
        Some(PositionSourceEvent::Connected { generation: 7 })
    ));

    assert_eq!(
        sender
            .publish_position(frame(7, b"boot-a", 1), 20_010)
            .expect("valid sample"),
        ServerAgentIngestOutcome::Accepted
    );
    assert_eq!(
        sender
            .publish_position(frame(7, b"boot-a", 3), 20_020)
            .expect("valid sample"),
        ServerAgentIngestOutcome::Accepted
    );

    let sample = match source.poll(20_020).expect("poll") {
        Some(PositionSourceEvent::Sample(sample)) => sample,
        other => panic!("expected latest sample, got {other:?}"),
    };
    assert_eq!(sample.sequence(), 3);
    assert_eq!(source.poll(20_021).expect("poll"), None);

    let diagnostics = sender.diagnostics();
    assert_eq!(diagnostics.accepted_samples, 2);
    assert_eq!(diagnostics.gap_events, 1);
    assert_eq!(diagnostics.missing_sequences, 1);
    assert_eq!(diagnostics.superseded_samples, 1);
}

#[test]
fn duplicate_and_out_of_order_sequences_are_dropped_separately() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(3), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(30_000).expect("poll");

    assert_eq!(
        sender
            .publish_position(frame(3, b"boot-a", 2), 30_010)
            .expect("valid sample"),
        ServerAgentIngestOutcome::Accepted
    );
    assert_eq!(
        sender
            .publish_position(frame(3, b"boot-a", 2), 30_020)
            .expect("valid duplicate"),
        ServerAgentIngestOutcome::DuplicateDropped
    );
    assert_eq!(
        sender
            .publish_position(frame(3, b"boot-a", 1), 30_030)
            .expect("valid out-of-order sample"),
        ServerAgentIngestOutcome::OutOfOrderDropped
    );

    let sample = match source.poll(30_030).expect("poll") {
        Some(PositionSourceEvent::Sample(sample)) => sample,
        other => panic!("expected accepted sample, got {other:?}"),
    };
    assert_eq!(sample.sequence(), 2);

    let diagnostics = sender.diagnostics();
    assert_eq!(diagnostics.duplicate_drops, 1);
    assert_eq!(diagnostics.out_of_order_drops, 1);
}

#[test]
fn a_frame_for_another_world_or_subject_is_dropped_before_it_reaches_state() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(4), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(35_000).expect("poll");

    let wrong_world =
        ServerAgentFrame::new("world-b", SUBJECT, b"boot-a", 4, 1, 1.0, 2.0, 3.0, None, 0);
    let wrong_subject = ServerAgentFrame::new(
        "world-a", [0xB2; 32], b"boot-a", 4, 1, 1.0, 2.0, 3.0, None, 0,
    );

    assert_eq!(
        sender
            .publish_position(wrong_world, 35_010)
            .expect("structurally valid frame"),
        ServerAgentIngestOutcome::IdentityMismatchDropped
    );
    assert_eq!(
        sender
            .publish_position(wrong_subject, 35_020)
            .expect("structurally valid frame"),
        ServerAgentIngestOutcome::IdentityMismatchDropped
    );
    assert_eq!(source.poll(35_020).expect("poll"), None);
    assert_eq!(sender.diagnostics().identity_mismatch_drops, 2);
}

#[test]
fn frame_debug_never_discloses_identity_or_coordinates() {
    let frame = ServerAgentFrame::new(
        "private-world",
        [0xCC; 32],
        b"private-boot",
        1,
        1,
        987_654_321.0,
        2.0,
        3.0,
        None,
        0,
    );

    let debug = format!("{frame:?}");
    assert_eq!(debug, "[REDACTED SERVER-AGENT FRAME]");
    assert!(!debug.contains("private"));
    assert!(!debug.contains("987654321"));
}

#[test]
fn disconnect_preempts_a_pending_sample_and_makes_the_reducer_offline() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(9), ServerAgentControlOutcome::Accepted);
    let connected = source.poll(40_000).expect("poll").expect("connected event");
    assert_eq!(
        sender
            .publish_position(frame(9, b"boot-a", 1), 40_010)
            .expect("valid sample"),
        ServerAgentIngestOutcome::Accepted
    );

    assert_eq!(sender.disconnect(9), ServerAgentControlOutcome::Accepted);
    let disconnected = source
        .poll(40_011)
        .expect("poll")
        .expect("disconnect event");
    assert_eq!(
        disconnected,
        PositionSourceEvent::Disconnected { generation: 9 }
    );
    assert_eq!(source.poll(40_012).expect("poll"), None);

    let mut reducer = StateReducer::default();
    reducer
        .apply(Event::PositionSource(connected))
        .expect("connected event");
    reducer
        .apply(Event::PositionSource(disconnected))
        .expect("disconnected event");
    let state = reducer.core_state(40_012);
    assert!(!state.connected());
    assert_eq!(state.freshness(), Freshness::Offline);
}

#[test]
fn a_new_generation_resumes_active_boot_and_rejects_old_generation_or_mid_session_boot_change() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(10), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(50_000).expect("poll");
    assert_eq!(
        sender
            .publish_position(frame(10, b"boot-a", 1), 50_010)
            .expect("valid sample"),
        ServerAgentIngestOutcome::Accepted
    );
    let _ = source.poll(50_010).expect("poll");
    assert_eq!(sender.disconnect(10), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(50_020).expect("poll");

    assert_eq!(sender.connect(11), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(50_030).expect("poll");
    assert_eq!(
        sender
            .publish_position(frame(10, b"boot-a", 2), 50_040)
            .expect("structurally valid old-generation sample"),
        ServerAgentIngestOutcome::OldGenerationDropped
    );
    assert_eq!(
        sender
            .publish_position(frame(11, b"boot-a", 2), 50_050)
            .expect("same boot resumes after reconnect"),
        ServerAgentIngestOutcome::Accepted
    );
    assert_eq!(
        sender
            .publish_position(frame(11, b"boot-c", 3), 50_070)
            .expect("structurally valid mismatched boot"),
        ServerAgentIngestOutcome::BootMismatchDropped
    );
    assert_eq!(
        sender.connect(10),
        ServerAgentControlOutcome::OldGenerationDropped
    );

    let sample = match source.poll(50_070).expect("poll") {
        Some(PositionSourceEvent::Sample(sample)) => sample,
        other => panic!("expected new-generation sample, got {other:?}"),
    };
    assert_eq!(sample.source_connection_generation(), 11);
    assert_eq!(sample.agent_boot_id(), b"boot-a");
    assert_eq!(sample.sequence(), 2);

    let diagnostics = sender.diagnostics();
    assert_eq!(diagnostics.old_generation_drops, 2);
    assert_eq!(diagnostics.retired_boot_drops, 0);
    assert_eq!(diagnostics.boot_mismatch_drops, 1);
}

#[test]
fn transient_reconnect_resumes_same_boot_then_replacement_retires_it() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(1), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(1).unwrap();
    assert_eq!(
        sender.publish_position(frame(1, b"boot-a", 7), 7).unwrap(),
        ServerAgentIngestOutcome::Accepted
    );
    let _ = source.poll(7).unwrap();
    assert_eq!(sender.disconnect(1), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(8).unwrap();

    assert_eq!(sender.connect(2), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(9).unwrap();
    assert_eq!(
        sender.publish_position(frame(2, b"boot-a", 8), 10).unwrap(),
        ServerAgentIngestOutcome::Accepted
    );
    assert_eq!(
        sender.publish_position(frame(1, b"boot-a", 9), 11).unwrap(),
        ServerAgentIngestOutcome::OldGenerationDropped
    );
    let resumed = match source.poll(11).unwrap() {
        Some(PositionSourceEvent::Sample(sample)) => sample,
        other => panic!("expected resumed sample, got {other:?}"),
    };
    assert_eq!(resumed.sequence(), 8);
    assert_eq!(resumed.agent_boot_id(), b"boot-a");
    assert_eq!(resumed.source_connection_generation(), 2);

    assert_eq!(sender.disconnect(2), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(12).unwrap();
    assert_eq!(sender.connect(3), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(13).unwrap();
    assert_eq!(
        sender.publish_position(frame(3, b"boot-b", 1), 14).unwrap(),
        ServerAgentIngestOutcome::Accepted
    );
    assert_eq!(
        sender.publish_position(frame(3, b"boot-a", 9), 15).unwrap(),
        ServerAgentIngestOutcome::RetiredBootDropped
    );
    let replacement = match source.poll(15).unwrap() {
        Some(PositionSourceEvent::Sample(sample)) => sample,
        other => panic!("expected replacement sample, got {other:?}"),
    };
    assert_eq!(replacement.agent_boot_id(), b"boot-b");
    assert_eq!(replacement.sequence(), 1);
}

#[test]
fn clock_invalid_preempts_pending_sample_and_is_ordered_before_disconnect() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(4), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(1).unwrap();
    assert_eq!(
        sender.publish_position(frame(4, b"boot-a", 1), 2).unwrap(),
        ServerAgentIngestOutcome::Accepted
    );
    assert_eq!(
        sender.clock_invalid(4, ClockInvalidReason::ProbeInvalid),
        ServerAgentControlOutcome::Accepted
    );
    assert_eq!(sender.disconnect(4), ServerAgentControlOutcome::Accepted);

    assert_eq!(
        source.poll(3).unwrap(),
        Some(PositionSourceEvent::ClockInvalid {
            generation: 4,
            reason: ClockInvalidReason::ProbeInvalid,
        })
    );
    assert_eq!(
        source.poll(4).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 4 })
    );
    assert_eq!(source.poll(5).unwrap(), None);
}

#[test]
fn reconnect_cannot_erase_unpolled_clock_failure_lifecycle() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(1), ServerAgentControlOutcome::Accepted);
    assert_eq!(
        source.poll(0).unwrap(),
        Some(PositionSourceEvent::Connected { generation: 1 })
    );
    assert_eq!(
        sender.publish_position(frame(1, b"boot-a", 1), 1).unwrap(),
        ServerAgentIngestOutcome::Accepted
    );
    assert!(matches!(
        source.poll(1).unwrap(),
        Some(PositionSourceEvent::Sample(_))
    ));
    assert_eq!(
        sender.clock_invalid(1, ClockInvalidReason::ProbeInvalid),
        ServerAgentControlOutcome::Accepted
    );
    assert_eq!(sender.disconnect(1), ServerAgentControlOutcome::Accepted);
    assert_eq!(sender.connect(2), ServerAgentControlOutcome::Accepted);
    assert_eq!(
        sender.publish_position(frame(1, b"boot-a", 2), 2).unwrap(),
        ServerAgentIngestOutcome::OldGenerationDropped
    );
    assert_eq!(
        sender.publish_position(frame(2, b"boot-a", 2), 3).unwrap(),
        ServerAgentIngestOutcome::Accepted
    );

    assert_eq!(
        source.poll(3).unwrap(),
        Some(PositionSourceEvent::ClockInvalid {
            generation: 1,
            reason: ClockInvalidReason::ProbeInvalid,
        })
    );
    assert_eq!(
        source.poll(3).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 1 })
    );
    assert_eq!(
        source.poll(3).unwrap(),
        Some(PositionSourceEvent::Connected { generation: 2 })
    );
    let sample = match source.poll(3).unwrap() {
        Some(PositionSourceEvent::Sample(sample)) => sample,
        other => panic!("expected current-generation sample, got {other:?}"),
    };
    assert_eq!(sample.source_connection_generation(), 2);
    assert_eq!(sample.sequence(), 2);
    assert_eq!(source.poll(3).unwrap(), None);
}

#[test]
fn ordinary_unpolled_disconnect_is_ordered_before_reconnect_and_new_sample() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(8), ServerAgentControlOutcome::Accepted);
    assert_eq!(
        source.poll(8).unwrap(),
        Some(PositionSourceEvent::Connected { generation: 8 })
    );
    assert_eq!(
        sender.publish_position(frame(8, b"boot-a", 1), 8).unwrap(),
        ServerAgentIngestOutcome::Accepted
    );
    assert!(matches!(
        source.poll(8).unwrap(),
        Some(PositionSourceEvent::Sample(_))
    ));
    assert_eq!(sender.disconnect(8), ServerAgentControlOutcome::Accepted);
    assert_eq!(sender.connect(9), ServerAgentControlOutcome::Accepted);
    assert_eq!(
        sender.publish_position(frame(9, b"boot-a", 2), 9).unwrap(),
        ServerAgentIngestOutcome::Accepted
    );

    assert_eq!(
        source.poll(9).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 8 })
    );
    assert_eq!(
        source.poll(9).unwrap(),
        Some(PositionSourceEvent::Connected { generation: 9 })
    );
    let sample = match source.poll(9).unwrap() {
        Some(PositionSourceEvent::Sample(sample)) => sample,
        other => panic!("expected current-generation sample, got {other:?}"),
    };
    assert_eq!(sample.source_connection_generation(), 9);
    assert_eq!(sample.sequence(), 2);
}

#[test]
fn full_control_queue_rejects_new_connection_without_erasing_lifecycle() {
    let (sender, mut source) = server_agent_channel();
    for generation in 1..=7 {
        assert_eq!(
            sender.connect(generation),
            ServerAgentControlOutcome::Accepted
        );
        assert_eq!(
            sender.disconnect(generation),
            ServerAgentControlOutcome::Accepted
        );
    }

    assert_eq!(
        sender.connect(8),
        ServerAgentControlOutcome::QueueFullDropped
    );
    assert_eq!(
        sender.publish_position(frame(8, b"boot-a", 1), 8).unwrap(),
        ServerAgentIngestOutcome::Dropped
    );

    for generation in 1..=7 {
        assert_eq!(
            source.poll(8).unwrap(),
            Some(PositionSourceEvent::Connected { generation })
        );
        assert_eq!(
            source.poll(8).unwrap(),
            Some(PositionSourceEvent::Disconnected { generation })
        );
    }
    assert_eq!(source.poll(8).unwrap(), None);
    assert_eq!(sender.diagnostics().control_queue_full_drops, 1);

    assert_eq!(sender.connect(8), ServerAgentControlOutcome::Accepted);
    assert_eq!(
        source.poll(9).unwrap(),
        Some(PositionSourceEvent::Connected { generation: 8 })
    );
}

#[test]
fn rejected_connect_cannot_mutate_the_current_accepted_session() {
    let (sender, mut source) = server_agent_channel();
    for generation in 1..=6 {
        assert_eq!(
            sender.connect(generation),
            ServerAgentControlOutcome::Accepted
        );
        assert_eq!(
            sender.disconnect(generation),
            ServerAgentControlOutcome::Accepted
        );
    }
    assert_eq!(sender.connect(7), ServerAgentControlOutcome::Accepted);
    assert_eq!(
        sender.publish_position(frame(7, b"boot-a", 1), 7).unwrap(),
        ServerAgentIngestOutcome::Accepted
    );

    assert_eq!(
        sender.connect(8),
        ServerAgentControlOutcome::QueueFullDropped
    );
    assert_eq!(
        sender.publish_position(frame(8, b"boot-a", 1), 8).unwrap(),
        ServerAgentIngestOutcome::Dropped
    );

    let mut reducer = StateReducer::default();
    for generation in 1..=6 {
        for expected in [
            PositionSourceEvent::Connected { generation },
            PositionSourceEvent::Disconnected { generation },
        ] {
            let event = source.poll(8).unwrap().expect("queued lifecycle");
            assert_eq!(event, expected);
            reducer.apply(Event::PositionSource(event)).unwrap();
        }
    }
    let connected = source.poll(8).unwrap().expect("generation 7 connected");
    assert_eq!(connected, PositionSourceEvent::Connected { generation: 7 });
    reducer.apply(Event::PositionSource(connected)).unwrap();
    let sample = source
        .poll(8)
        .unwrap()
        .expect("generation 7 sample retained");
    assert!(matches!(sample, PositionSourceEvent::Sample(_)));
    reducer.apply(Event::PositionSource(sample)).unwrap();
    assert!(reducer.has_trusted_position());
    assert_eq!(
        reducer
            .last_position()
            .expect("generation 7 sample")
            .source_connection_generation(),
        7
    );

    assert_eq!(sender.disconnect(7), ServerAgentControlOutcome::Accepted);
    let disconnected = source.poll(9).unwrap().expect("generation 7 disconnect");
    assert_eq!(
        disconnected,
        PositionSourceEvent::Disconnected { generation: 7 }
    );
    reducer.apply(Event::PositionSource(disconnected)).unwrap();
    assert!(!reducer.has_trusted_position());
    assert_eq!(reducer.core_state(9).freshness(), Freshness::Offline);
    assert_eq!(source.poll(9).unwrap(), None);
    assert_eq!(sender.diagnostics().control_queue_full_drops, 1);

    assert_eq!(sender.connect(8), ServerAgentControlOutcome::Accepted);
    assert_eq!(
        source.poll(10).unwrap(),
        Some(PositionSourceEvent::Connected { generation: 8 })
    );
}

#[test]
fn retired_boot_rejection_has_no_eviction_false_negative() {
    let (sender, mut source) = server_agent_channel();

    for generation in 1..=65 {
        assert_eq!(
            sender.connect(generation),
            ServerAgentControlOutcome::Accepted
        );
        let _ = source.poll(55_000 + generation).expect("connected poll");
        let boot_id = format!("boot-{generation}");
        assert_eq!(
            sender
                .publish_position(
                    frame(generation, boot_id.as_bytes(), 1),
                    56_000 + generation,
                )
                .expect("valid sample"),
            ServerAgentIngestOutcome::Accepted
        );
        let _ = source.poll(56_000 + generation).expect("sample poll");
        assert_eq!(
            sender.disconnect(generation),
            ServerAgentControlOutcome::Accepted
        );
        let _ = source.poll(57_000 + generation).expect("disconnect poll");
    }

    assert_eq!(sender.connect(66), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(58_000).expect("connected poll");
    assert_eq!(
        sender
            .publish_position(frame(66, b"boot-1", 1), 58_010)
            .expect("structurally valid retired boot"),
        ServerAgentIngestOutcome::RetiredBootDropped
    );
}

#[test]
fn zero_generation_or_sequence_is_rejected_as_an_invalid_transition() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(4), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(60_000).expect("poll");

    assert_eq!(
        sender
            .publish_position(frame(4, b"boot-a", 0), 60_010)
            .expect("structurally valid sample"),
        ServerAgentIngestOutcome::InvalidTransitionDropped
    );
    assert_eq!(
        sender
            .publish_position(frame(0, b"boot-a", 1), 60_020)
            .expect("structurally valid sample"),
        ServerAgentIngestOutcome::InvalidTransitionDropped
    );
    assert_eq!(source.poll(60_020).expect("poll"), None);
    assert_eq!(sender.diagnostics().invalid_transition_drops, 2);
}

#[test]
fn position_and_optional_heading_use_exact_domain_finite_validation() {
    let invalid_cases = [
        (
            f64::NAN,
            2.0,
            3.0,
            None,
            PositionValidationError::NonFiniteX,
        ),
        (
            1.0,
            f64::INFINITY,
            3.0,
            None,
            PositionValidationError::NonFiniteY,
        ),
        (
            1.0,
            2.0,
            f64::NEG_INFINITY,
            None,
            PositionValidationError::NonFiniteZ,
        ),
        (
            1.0,
            2.0,
            3.0,
            Some(f32::NAN),
            PositionValidationError::NonFiniteHeading,
        ),
    ];

    for (x, y, z, heading, expected) in invalid_cases {
        let (sender, mut source) = server_agent_channel();
        assert_eq!(sender.connect(1), ServerAgentControlOutcome::Accepted);
        let _ = source.poll(70_000).expect("poll");
        let frame = ServerAgentFrame::new("world-a", SUBJECT, b"boot-a", 1, 1, x, y, z, heading, 0);

        assert_eq!(
            sender.publish_position(frame, 70_010),
            Err(ServerAgentSourceError::Position(expected))
        );
        assert_eq!(source.poll(70_010).expect("poll"), None);
    }
}

#[test]
fn sample_age_uses_local_receive_monotonic_and_explicit_upper_bound_only() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(2), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(80_000).expect("poll");
    let frame = ServerAgentFrame::new(
        "world-a", SUBJECT, b"boot-a", 2, 1, 10.0, 20.0, 30.0, None, 275,
    );

    assert_eq!(
        sender
            .publish_position(frame, 80_125)
            .expect("valid sample"),
        ServerAgentIngestOutcome::Accepted
    );
    let sample = match source.poll(80_125).expect("poll") {
        Some(PositionSourceEvent::Sample(sample)) => sample,
        other => panic!("expected sample, got {other:?}"),
    };

    assert_eq!(sample.heading_degrees(), None);
    assert_eq!(sample.clock().received_at_monotonic_ms, 80_125);
    assert_eq!(sample.clock().age_at_receive_upper_bound_ms, 275);
    assert_eq!(sample.clock().age_upper_bound_ms(80_625), 775);
}

#[test]
fn local_monotonic_regression_is_dropped_and_emits_clock_invalid() {
    let (sender, mut source) = server_agent_channel();
    assert_eq!(sender.connect(6), ServerAgentControlOutcome::Accepted);
    let _ = source.poll(90_000).expect("poll");
    assert_eq!(
        sender
            .publish_position(frame(6, b"boot-a", 1), 90_100)
            .expect("valid sample"),
        ServerAgentIngestOutcome::Accepted
    );
    let _ = source.poll(90_100).expect("poll");

    assert_eq!(
        sender
            .publish_position(frame(6, b"boot-a", 2), 90_099)
            .expect("structurally valid sample"),
        ServerAgentIngestOutcome::ClockRegressionDropped
    );
    assert_eq!(
        source.poll(90_100).expect("poll"),
        Some(PositionSourceEvent::ClockInvalid {
            generation: 6,
            reason: ClockInvalidReason::MonotonicRegression,
        })
    );
    assert_eq!(source.poll(90_101).expect("poll"), None);
    assert_eq!(sender.diagnostics().clock_regression_drops, 1);

    assert_eq!(
        sender
            .publish_position(frame(6, b"boot-a", 2), 90_101)
            .expect("clock recovered"),
        ServerAgentIngestOutcome::Accepted
    );
}

fn frame(generation: u64, boot_id: &[u8], sequence: u64) -> ServerAgentFrame {
    ServerAgentFrame::new(
        "world-a",
        SUBJECT,
        boot_id,
        generation,
        sequence,
        1_234.5,
        -678.25,
        42.0,
        Some(90.0),
        250,
    )
}

fn server_agent_channel() -> (
    pal_state::ServerAgentSender,
    pal_state::ServerAgentPositionSource,
) {
    pal_state::server_agent_channel("world-a", SUBJECT).expect("valid bound identity")
}
