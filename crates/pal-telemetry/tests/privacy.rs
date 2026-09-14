use pal_protocol::v2::{CoordinateSpace, HeadingSource, SubscribeResponse};
use pal_state::server_agent_channel;
use pal_telemetry::{
    ClockProbeObservation, MonotonicEpoch, NetworkConsumerConfig, NetworkConsumerError,
    NetworkFailureClass, SequenceCursor, TelemetryIngest, validate_envelope,
};
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};

const PROFILE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn valid_tls_material() -> (String, String, String) {
    let mut ca_parameters = CertificateParams::default();
    ca_parameters.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = CertifiedIssuer::self_signed(ca_parameters, KeyPair::generate().unwrap()).unwrap();
    let client_key = KeyPair::generate().unwrap();
    let client_certificate = CertificateParams::default()
        .signed_by(&client_key, &ca)
        .unwrap();
    (
        ca.pem(),
        client_certificate.pem(),
        client_key.serialize_pem(),
    )
}

#[test]
fn validation_and_transport_debug_output_redacts_sensitive_values() {
    let subject = [0x42; 32];
    let (sender, _) = server_agent_channel("world-a", subject).unwrap();
    let ingest = TelemetryIngest::new(sender, 1, PROFILE, 500).unwrap();
    assert_eq!(format!("{ingest:?}"), "[REDACTED TELEMETRY INGEST]");
    let cursor = SequenceCursor::new([0x11; 16]);
    assert_eq!(format!("{cursor:?}"), "[REDACTED SEQUENCE CURSOR]");
    let observation = ClockProbeObservation {
        expected_nonce: vec![0xde; 16],
        echoed_nonce: vec![0xde; 16],
        client_send_unix_ms: 1,
        agent_receive_unix_ms: 2,
        agent_send_unix_ms: 3,
        client_receive_unix_ms: 4,
    };
    assert_eq!(
        format!("{observation:?}"),
        "[REDACTED CLOCK PROBE OBSERVATION]"
    );
    let (ca, certificate, private_key) = valid_tls_material();
    let config = NetworkConsumerConfig::new(
        "https://secret-endpoint.invalid",
        "localhost",
        ca.as_bytes(),
        certificate.as_bytes(),
        private_key.as_bytes(),
        "world-a",
        subject,
        PROFILE,
        500,
    )
    .unwrap();
    let config_debug = format!("{config:?}");
    assert_eq!(config_debug, "[REDACTED NETWORK CONSUMER CONFIG]");

    let invalid = SubscribeResponse {
        protocol_version: pal_domain::PROTOCOL_VERSION,
        world_alias: "world-a".to_owned(),
        subject_id: subject.to_vec(),
        boot_id: [0x11; 16].to_vec(),
        sequence: 1,
        rest_completed_at_unix_ms: 1,
        age_at_emit_ms: 0,
        position_x: 12345.6789,
        position_y: 98765.4321,
        position_z: 55555.5,
        heading_degrees: Some(12.5),
        heading_source: HeadingSource::Unspecified as i32,
        trace_id: [0x33; 16].to_vec(),
        coordinate_space: CoordinateSpace::OfficialGameDataWorldV1 as i32,
        coordinate_profile_sha256: PROFILE.to_owned(),
    };
    let error = validate_envelope(&invalid, PROFILE).unwrap_err();
    let rendered = format!("{error:?} {error}");
    for forbidden in [
        "12345.6789",
        "98765.4321",
        "55555.5",
        "42424242",
        "11111111",
        "33333333",
        PROFILE,
        "world-a",
        "secret-endpoint",
        "secret-ca",
        "secret-cert",
        "secret-key",
        "dededede",
    ] {
        assert!(
            !format!("{rendered} {config_debug} {observation:?}").contains(forbidden),
            "leaked {forbidden}"
        );
    }
}

#[test]
fn network_consumer_configuration_requires_tls() {
    let result = NetworkConsumerConfig::new(
        "http://127.0.0.1:1234",
        "localhost",
        b"ca",
        b"cert",
        b"key",
        "world-a",
        [0x42; 32],
        PROFILE,
        500,
    );
    assert_eq!(result.unwrap_err(), NetworkConsumerError::Configuration);
}

#[test]
fn malformed_local_tls_material_is_a_terminal_configuration_error() {
    let result = NetworkConsumerConfig::new(
        "https://127.0.0.1:1234",
        "localhost",
        b"-----BEGIN CERTIFICATE-----\ninvalid\n-----END CERTIFICATE-----\n",
        b"-----BEGIN CERTIFICATE-----\ninvalid\n-----END CERTIFICATE-----\n",
        b"-----BEGIN PRIVATE KEY-----\ninvalid\n-----END PRIVATE KEY-----\n",
        "world-a",
        [0x42; 32],
        PROFILE,
        500,
    );

    assert_eq!(result.unwrap_err(), NetworkConsumerError::Configuration);
}

#[test]
fn a_client_certificate_and_private_key_mismatch_is_rejected_before_connect() {
    let (ca, certificate, _) = valid_tls_material();
    let (_, _, wrong_private_key) = valid_tls_material();
    let result = NetworkConsumerConfig::new(
        "https://127.0.0.1:1234",
        "localhost",
        ca.as_bytes(),
        certificate.as_bytes(),
        wrong_private_key.as_bytes(),
        "world-a",
        [0x42; 32],
        PROFILE,
        500,
    );

    assert_eq!(result.unwrap_err(), NetworkConsumerError::Configuration);
}

#[test]
fn reconnect_policy_fails_closed_for_identity_validation_and_state_errors() {
    for error in [
        NetworkConsumerError::Configuration,
        NetworkConsumerError::RpcTerminal,
        NetworkConsumerError::MessageTooLarge,
        NetworkConsumerError::Identity,
        NetworkConsumerError::Validation,
        NetworkConsumerError::Clock,
        NetworkConsumerError::Ingest,
    ] {
        assert_eq!(error.class(), NetworkFailureClass::Terminal);
    }
    for error in [
        NetworkConsumerError::Transport,
        NetworkConsumerError::Rpc,
        NetworkConsumerError::Disconnected,
    ] {
        assert_eq!(error.class(), NetworkFailureClass::Transient);
    }
}

#[test]
fn a_shared_monotonic_epoch_preserves_startup_offset() {
    let origin = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(3))
        .expect("valid earlier instant");
    let epoch = MonotonicEpoch::new_at(origin);

    assert!(epoch.elapsed_ms() >= 3_000);
}
