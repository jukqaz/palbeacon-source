use pal_protocol::{
    PROTOCOL_VERSION,
    v2::{ClientHello, ClientRole, LocalEnvelope, ProtocolErrorCode, local_envelope::Payload},
};
use pal_windows_ipc::handshake::{HelloGate, HelloRejectionKind, SessionPolicy};

fn hello(role: ClientRole) -> LocalEnvelope {
    LocalEnvelope {
        protocol_version: PROTOCOL_VERSION,
        connection_id: vec![7; 16],
        message_id: 1,
        reply_to_message_id: None,
        payload: Some(Payload::ClientHello(ClientHello {
            role: role as i32,
            process_id: 42,
            minimum_protocol_version: PROTOCOL_VERSION,
            maximum_protocol_version: PROTOCOL_VERSION,
        })),
    }
}

#[test]
fn each_gate_accepts_only_its_configured_role() {
    assert!(
        HelloGate::new(SessionPolicy::management_ui())
            .evaluate(&hello(ClientRole::ManagementUi))
            .is_ok()
    );
    assert!(
        HelloGate::new(SessionPolicy::overlay())
            .evaluate(&hello(ClientRole::Overlay))
            .is_ok()
    );
    assert_eq!(
        HelloGate::new(SessionPolicy::management_ui())
            .evaluate(&hello(ClientRole::Overlay))
            .unwrap_err()
            .kind(),
        HelloRejectionKind::InvalidRole
    );
}

#[test]
fn accepts_valid_hello_only_once() {
    let mut gate = HelloGate::new(SessionPolicy::management_ui());
    let accepted = gate.evaluate(&hello(ClientRole::ManagementUi)).unwrap();
    assert_eq!(accepted.process_id(), 42);
    assert_eq!(accepted.connection_id(), &[7; 16]);
    assert!(gate.is_established());

    let second = gate.evaluate(&hello(ClientRole::ManagementUi)).unwrap_err();
    assert_eq!(second.kind(), HelloRejectionKind::SecondHello);
    assert_eq!(second.code(), ProtocolErrorCode::UnexpectedPayload);
    assert_eq!(second.offending_value(), "");
}

#[test]
fn rejects_v1_without_negotiating_a_legacy_session() {
    let mut legacy = hello(ClientRole::ManagementUi);
    legacy.protocol_version = 1;
    let mut gate = HelloGate::new(SessionPolicy::management_ui());
    let rejection = gate.evaluate(&legacy).unwrap_err();
    assert_eq!(rejection.kind(), HelloRejectionKind::IncompatibleVersion);
    assert_eq!(rejection.code(), ProtocolErrorCode::UnsupportedVersion);
    assert!(!gate.is_established());

    legacy.protocol_version = PROTOCOL_VERSION;
    let Some(Payload::ClientHello(value)) = legacy.payload.as_mut() else {
        unreachable!()
    };
    value.minimum_protocol_version = 1;
    value.maximum_protocol_version = 1;
    assert_eq!(
        gate.evaluate(&legacy).unwrap_err().kind(),
        HelloRejectionKind::IncompatibleVersion
    );
    assert!(!gate.is_established());
}

#[test]
fn rejects_roles_bad_identity_and_incompatible_ranges_with_sanitized_results() {
    for role in [ClientRole::Overlay, ClientRole::Unspecified] {
        let rejection = HelloGate::new(SessionPolicy::management_ui())
            .evaluate(&hello(role))
            .unwrap_err();
        assert_eq!(rejection.kind(), HelloRejectionKind::InvalidRole);
        assert_eq!(rejection.code(), ProtocolErrorCode::InvalidRole);
        assert_eq!(rejection.offending_value(), "");
    }

    let mut unknown = hello(ClientRole::ManagementUi);
    let Some(Payload::ClientHello(value)) = unknown.payload.as_mut() else {
        unreachable!()
    };
    value.role = 99;
    assert_eq!(
        HelloGate::new(SessionPolicy::management_ui())
            .evaluate(&unknown)
            .unwrap_err()
            .kind(),
        HelloRejectionKind::InvalidRole
    );

    let mut zero_pid = hello(ClientRole::ManagementUi);
    let Some(Payload::ClientHello(value)) = zero_pid.payload.as_mut() else {
        unreachable!()
    };
    value.process_id = 0;
    assert_eq!(
        HelloGate::new(SessionPolicy::management_ui())
            .evaluate(&zero_pid)
            .unwrap_err()
            .kind(),
        HelloRejectionKind::InvalidProcessId
    );

    let mut bad_connection = hello(ClientRole::ManagementUi);
    bad_connection.connection_id.pop();
    assert_eq!(
        HelloGate::new(SessionPolicy::management_ui())
            .evaluate(&bad_connection)
            .unwrap_err()
            .kind(),
        HelloRejectionKind::InvalidConnectionId
    );

    let mut zero_message = hello(ClientRole::ManagementUi);
    zero_message.message_id = 0;
    assert_eq!(
        HelloGate::new(SessionPolicy::management_ui())
            .evaluate(&zero_message)
            .unwrap_err()
            .kind(),
        HelloRejectionKind::InvalidMessageId
    );

    let mut incompatible = hello(ClientRole::ManagementUi);
    let Some(Payload::ClientHello(value)) = incompatible.payload.as_mut() else {
        unreachable!()
    };
    value.minimum_protocol_version = PROTOCOL_VERSION + 1;
    value.maximum_protocol_version = PROTOCOL_VERSION + 2;
    let rejection = HelloGate::new(SessionPolicy::management_ui())
        .evaluate(&incompatible)
        .unwrap_err();
    assert_eq!(rejection.kind(), HelloRejectionKind::IncompatibleVersion);
    assert_eq!(rejection.code(), ProtocolErrorCode::UnsupportedVersion);
    assert_eq!(rejection.offending_value(), "");
}

#[test]
fn rejects_every_non_hello_first_payload() {
    let mut envelope = hello(ClientRole::ManagementUi);
    envelope.payload = None;
    let rejection = HelloGate::new(SessionPolicy::management_ui())
        .evaluate(&envelope)
        .unwrap_err();
    assert_eq!(rejection.kind(), HelloRejectionKind::ExpectedHello);
    assert_eq!(rejection.code(), ProtocolErrorCode::UnexpectedPayload);
    assert_eq!(rejection.offending_value(), "");
}
