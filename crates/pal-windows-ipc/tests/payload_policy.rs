use pal_protocol::{
    PROTOCOL_VERSION,
    v2::{
        ClientHello, ClientRole, CompanionRequest, LocalEnvelope, ProtocolErrorCode,
        local_envelope::Payload,
    },
};
use pal_windows_ipc::handshake::HelloGate;
use pal_windows_ipc::handshake::SessionPolicy;

fn envelope(message_id: u64, payload: Payload) -> LocalEnvelope {
    LocalEnvelope {
        protocol_version: PROTOCOL_VERSION,
        connection_id: vec![3; 16],
        message_id,
        reply_to_message_id: None,
        payload: Some(payload),
    }
}

#[test]
fn overlay_role_cannot_send_product_queries() {
    let request = Payload::ProductRequest(CompanionRequest::default());

    assert!(!SessionPolicy::overlay().permits(&request));
    assert!(SessionPolicy::management_ui().permits(&request));
}

#[test]
fn established_overlay_gate_rejects_product_queries() {
    let mut gate = HelloGate::new(SessionPolicy::overlay());
    gate.evaluate(&envelope(
        1,
        Payload::ClientHello(ClientHello {
            role: ClientRole::Overlay as i32,
            process_id: 42,
            minimum_protocol_version: PROTOCOL_VERSION,
            maximum_protocol_version: PROTOCOL_VERSION,
        }),
    ))
    .unwrap();

    let rejection = gate
        .validate_established(&envelope(
            2,
            Payload::ProductRequest(CompanionRequest::default()),
        ))
        .unwrap_err();

    assert_eq!(rejection.code(), ProtocolErrorCode::UnexpectedPayload);
    assert_eq!(rejection.field(), "payload");
}
