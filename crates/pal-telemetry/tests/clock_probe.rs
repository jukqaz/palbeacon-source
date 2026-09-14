use pal_protocol::v2::{
    ClockProbeRequest, ClockProbeResponse, CoordinateSpace, HeadingSource, SubscribeResponse,
};
use pal_state::{ClockInvalidReason, PositionSource, PositionSourceEvent, server_agent_channel};
use pal_telemetry::TelemetryIngest;
use pal_telemetry::{
    ClockError, ClockEstimator, ClockProbeObservation, ESTIMATE_EXPIRY, PROBE_CADENCE,
    reconnect_backoff,
};

const PROFILE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SUBJECT: [u8; 32] = [0x42; 32];

fn observation() -> ClockProbeObservation {
    ClockProbeObservation {
        expected_nonce: vec![7; 16],
        echoed_nonce: vec![7; 16],
        client_send_unix_ms: 10_000,
        agent_receive_unix_ms: 10_040,
        agent_send_unix_ms: 10_045,
        client_receive_unix_ms: 10_100,
    }
}

#[test]
fn constants_and_no_jitter_backoff_are_conservative_and_deterministic() {
    assert_eq!(PROBE_CADENCE.as_secs(), 30);
    assert_eq!(ESTIMATE_EXPIRY.as_secs(), 90);
    let actual: Vec<_> = (0..7)
        .map(|attempt| reconnect_backoff(attempt).as_millis())
        .collect();
    assert_eq!(actual, [250, 500, 1_000, 2_000, 4_000, 5_000, 5_000]);
}

#[test]
fn clock_probe_rejects_nonce_ordering_outlier_step_and_expiry() {
    let mut estimator = ClockEstimator::new(500);
    let estimate = estimator.observe(observation(), 1_000).unwrap();
    assert_eq!(estimate.full_rtt_upper_bound_ms, 100);
    assert!(estimator.current(90_999).is_some());
    assert!(estimator.current(91_001).is_none());

    let mut mismatch = observation();
    mismatch.echoed_nonce[0] ^= 1;
    assert_eq!(
        ClockEstimator::new(500).observe(mismatch, 0),
        Err(ClockError::NonceMismatch)
    );

    let mut impossible = observation();
    impossible.agent_send_unix_ms = impossible.agent_receive_unix_ms - 1;
    assert_eq!(
        ClockEstimator::new(500).observe(impossible, 0),
        Err(ClockError::ImpossibleOrdering)
    );

    let mut outlier = observation();
    outlier.client_receive_unix_ms = 10_700;
    assert_eq!(
        ClockEstimator::new(500).observe(outlier, 0),
        Err(ClockError::RttOutlier)
    );

    let mut zero_rtt = observation();
    zero_rtt.agent_receive_unix_ms = zero_rtt.client_send_unix_ms;
    zero_rtt.agent_send_unix_ms = zero_rtt.client_send_unix_ms;
    zero_rtt.client_receive_unix_ms = zero_rtt.client_send_unix_ms;
    assert_eq!(
        ClockEstimator::new(500).observe(zero_rtt, 0),
        Err(ClockError::ImpossibleOrdering)
    );

    let mut stepped_estimator = ClockEstimator::new(500);
    stepped_estimator.observe(observation(), 1).unwrap();
    let mut stepped = observation();
    stepped.agent_receive_unix_ms += 10_000;
    stepped.agent_send_unix_ms += 10_000;
    assert_eq!(
        stepped_estimator.observe(stepped, 2),
        Err(ClockError::WallClockStep)
    );
}

#[test]
fn runtime_age_uses_emit_age_plus_full_probe_rtt_with_saturation() {
    let estimate = ClockEstimator::new(500).observe(observation(), 0).unwrap();
    assert_eq!(estimate.age_upper_bound_ms(25), 125);
    assert_eq!(estimate.age_upper_bound_ms(u32::MAX), u32::MAX as u64 + 100);

    let saturated = pal_telemetry::ClockEstimate {
        full_rtt_upper_bound_ms: u64::MAX,
        offset_lower_bound_ms: 0,
        offset_upper_bound_ms: 0,
        observed_at_monotonic_ms: 0,
    };
    assert_eq!(saturated.age_upper_bound_ms(u32::MAX), u64::MAX);
}

fn envelope(sequence: u64) -> SubscribeResponse {
    SubscribeResponse {
        protocol_version: pal_domain::PROTOCOL_VERSION,
        world_alias: "world-a".to_owned(),
        subject_id: SUBJECT.to_vec(),
        boot_id: [0x11; 16].to_vec(),
        sequence,
        rest_completed_at_unix_ms: 10_045,
        age_at_emit_ms: 25,
        position_x: sequence as f64,
        position_y: 2.0,
        position_z: 3.0,
        heading_degrees: Some(45.0),
        heading_source: HeadingSource::RotationZValidated as i32,
        trace_id: [0x33; 16].to_vec(),
        coordinate_space: CoordinateSpace::OfficialGameDataWorldV1 as i32,
        coordinate_profile_sha256: PROFILE.to_owned(),
    }
}

#[test]
fn ingest_owns_clock_evidence_and_invalid_or_expired_evidence_retains_last_position() {
    let (sender, mut source) = server_agent_channel("world-a", SUBJECT).unwrap();
    let mut ingest = TelemetryIngest::new(sender, 3, PROFILE, 500).unwrap();
    assert!(matches!(
        source.poll(1_000).unwrap(),
        Some(PositionSourceEvent::Connected { generation: 3 })
    ));

    let request = ClockProbeRequest {
        protocol_version: pal_domain::PROTOCOL_VERSION,
        nonce: vec![7; 16],
        client_send_unix_ms: 10_000,
    };
    let response = ClockProbeResponse {
        nonce: vec![7; 16],
        client_send_unix_ms: 10_000,
        agent_receive_unix_ms: 10_040,
        agent_send_unix_ms: 10_045,
    };
    ingest
        .accept_clock_probe(&request, &response, 10_100, 1_000)
        .unwrap();
    ingest.accept(envelope(1), 1_001).unwrap();
    let Some(PositionSourceEvent::Sample(first)) = source.poll(1_001).unwrap() else {
        panic!("expected first position");
    };
    assert_eq!(first.sequence(), 1);
    assert_eq!(first.clock().age_at_receive_upper_bound_ms, 125);

    let mut mismatch = response.clone();
    mismatch.nonce[0] ^= 1;
    assert!(
        ingest
            .accept_clock_probe(&request, &mismatch, 10_100, 2_000)
            .is_err()
    );
    assert!(matches!(
        source.poll(2_000).unwrap(),
        Some(PositionSourceEvent::ClockInvalid {
            generation: 3,
            reason: ClockInvalidReason::ProbeInvalid,
        })
    ));
    assert!(ingest.accept(envelope(2), 2_001).is_err());
    assert!(source.poll(2_001).unwrap().is_none());

    ingest
        .accept_clock_probe(&request, &response, 10_100, 3_000)
        .unwrap();
    assert!(
        ingest
            .accept(envelope(2), 3_000 + ESTIMATE_EXPIRY.as_millis() as u64 + 1)
            .is_err()
    );
    assert!(matches!(
        source.poll(94_001).unwrap(),
        Some(PositionSourceEvent::ClockInvalid {
            generation: 3,
            reason: ClockInvalidReason::EvidenceExpired,
        })
    ));
}
