use pal_protocol::v2::{
    ClockProbeRequest, ClockProbeResponse, CoordinateSpace, GetDescriptorResponse, HeadingSource,
    SubscribeResponse,
};
use pal_state::{PositionSource, PositionSourceEvent, server_agent_channel};
use pal_telemetry::{
    MAX_DECODED_MESSAGE_SIZE, TelemetryIngest, ValidationError, decode_envelope,
    validate_clock_probe_request, validate_clock_probe_response, validate_descriptor,
    validate_envelope,
};

const SUBJECT: [u8; 32] = [0x42; 32];
const BOOT: [u8; 16] = [0x11; 16];
const TRACE: [u8; 16] = [0x33; 16];
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROFILE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn descriptor() -> GetDescriptorResponse {
    GetDescriptorResponse {
        protocol_version: pal_domain::PROTOCOL_VERSION,
        agent_version: "0.1.0".to_owned(),
        rest_server_version: "v0.6".to_owned(),
        server_fingerprint: HASH.to_owned(),
        gate_profile_sha256: HASH.to_owned(),
        selected_interval_ms: 500,
        world_alias: "world-a".to_owned(),
        coordinate_space: CoordinateSpace::OfficialGameDataWorldV1 as i32,
        coordinate_profile_sha256: PROFILE.to_owned(),
    }
}

fn envelope() -> SubscribeResponse {
    SubscribeResponse {
        protocol_version: pal_domain::PROTOCOL_VERSION,
        world_alias: "world-a".to_owned(),
        subject_id: SUBJECT.to_vec(),
        boot_id: BOOT.to_vec(),
        sequence: 1,
        rest_completed_at_unix_ms: 1_700_000_000_000,
        age_at_emit_ms: 10,
        position_x: 1.0,
        position_y: 2.0,
        position_z: 3.0,
        heading_degrees: Some(45.0),
        heading_source: HeadingSource::RotationZValidated as i32,
        trace_id: TRACE.to_vec(),
        coordinate_space: CoordinateSpace::OfficialGameDataWorldV1 as i32,
        coordinate_profile_sha256: PROFILE.to_owned(),
    }
}

#[test]
fn accepts_only_exact_live_wire_contract() {
    validate_descriptor(&descriptor(), PROFILE).unwrap();
    validate_envelope(&envelope(), PROFILE).unwrap();

    let mut cases = Vec::new();
    let mut value = envelope();
    value.protocol_version = 1;
    cases.push(value);
    let mut value = envelope();
    value.protocol_version += 1;
    cases.push(value);
    let mut value = envelope();
    value.world_alias = String::new();
    cases.push(value);
    let mut value = envelope();
    value.subject_id.pop();
    cases.push(value);
    let mut value = envelope();
    value.boot_id.pop();
    cases.push(value);
    let mut value = envelope();
    value.trace_id.pop();
    cases.push(value);
    let mut value = envelope();
    value.sequence = 0;
    cases.push(value);
    let mut value = envelope();
    value.position_x = f64::NAN;
    cases.push(value);
    let mut value = envelope();
    value.heading_degrees = Some(360.0);
    cases.push(value);
    let mut value = envelope();
    value.heading_source = HeadingSource::Unspecified as i32;
    cases.push(value);
    let mut value = envelope();
    value.heading_degrees = None;
    cases.push(value);
    let mut value = envelope();
    value.coordinate_space = CoordinateSpace::Unspecified as i32;
    cases.push(value);
    let mut value = envelope();
    value.coordinate_profile_sha256 = HASH.to_owned();
    cases.push(value);

    for invalid in cases {
        assert!(validate_envelope(&invalid, PROFILE).is_err());
    }
}

#[test]
fn descriptor_requires_canonical_hashes_interval_and_coordinate_profile() {
    for mutate in [
        |value: &mut GetDescriptorResponse| value.server_fingerprint.make_ascii_uppercase(),
        |value: &mut GetDescriptorResponse| value.gate_profile_sha256.truncate(63),
        |value: &mut GetDescriptorResponse| value.selected_interval_ms = 250,
        |value: &mut GetDescriptorResponse| {
            value.coordinate_space = CoordinateSpace::Unspecified as i32
        },
        |value: &mut GetDescriptorResponse| value.coordinate_profile_sha256 = HASH.to_owned(),
    ] {
        let mut invalid = descriptor();
        mutate(&mut invalid);
        assert!(validate_descriptor(&invalid, PROFILE).is_err());
    }
}

#[test]
fn oversized_frame_is_rejected_before_decode() {
    let bytes = vec![0_u8; MAX_DECODED_MESSAGE_SIZE + 1];
    assert_eq!(
        decode_envelope(&bytes),
        Err(ValidationError::MessageTooLarge)
    );
}

#[test]
fn valid_frame_is_published_into_existing_server_agent_sender() {
    let (sender, mut source) = server_agent_channel("world-a", SUBJECT).unwrap();
    let mut ingest = TelemetryIngest::new(sender, 9, PROFILE, 500).unwrap();
    assert!(matches!(
        source.poll(50).unwrap(),
        Some(PositionSourceEvent::Connected { generation: 9 })
    ));

    let request = ClockProbeRequest {
        protocol_version: pal_domain::PROTOCOL_VERSION,
        nonce: vec![7; 16],
        client_send_unix_ms: 1_000,
    };
    let response = ClockProbeResponse {
        nonce: vec![7; 16],
        client_send_unix_ms: 1_000,
        agent_receive_unix_ms: 1_020,
        agent_send_unix_ms: 1_025,
    };
    ingest
        .accept_clock_probe(&request, &response, 1_080, 49)
        .unwrap();
    ingest.accept(envelope(), 50).unwrap();
    let Some(PositionSourceEvent::Sample(sample)) = source.poll(50).unwrap() else {
        panic!("expected accepted sample");
    };
    assert_eq!(sample.sequence(), 1);
    assert_eq!(sample.clock().age_at_receive_upper_bound_ms, 90);
    assert_eq!(sample.clock().received_at_monotonic_ms, 50);

    ingest.disconnect();
    assert!(matches!(
        source.poll(50).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 9 })
    ));
}

#[test]
fn clock_nonce_is_exactly_sixteen_bytes() {
    for nonce in [vec![], vec![7; 15], vec![7; 17], vec![7; 1_024]] {
        assert_eq!(
            validate_clock_probe_request(&ClockProbeRequest {
                protocol_version: pal_domain::PROTOCOL_VERSION,
                nonce: nonce.clone(),
                client_send_unix_ms: 1,
            }),
            Err(ValidationError::ClockNonce)
        );
        assert_eq!(
            validate_clock_probe_response(&ClockProbeResponse {
                nonce,
                client_send_unix_ms: 1,
                agent_receive_unix_ms: 1,
                agent_send_unix_ms: 1,
            }),
            Err(ValidationError::ClockNonce)
        );
    }
}
