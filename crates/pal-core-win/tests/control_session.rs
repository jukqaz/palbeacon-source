use pal_core_win::control_session::ControlSession;
use pal_domain::{DisplayMode, InputMode, OverlaySettings};
use pal_protocol::{
    FrameCodec, PROTOCOL_VERSION, decode_overlay_settings, encode_overlay_settings,
    v2::{
        ClientHello, ClientRole, LocalEnvelope, OverlayCommand, OverlayCommandKind,
        OverlayCommandStatus, OverlaySettingField, ProtocolErrorCode, SettingsApplyStatus,
        SettingsPatch, SettingsQuery, local_envelope::Payload,
    },
};
use pal_windows_ipc::framed_stream::{FramedStream, StreamError};

fn envelope(connection: u8, message_id: u64, payload: Payload) -> LocalEnvelope {
    LocalEnvelope {
        protocol_version: PROTOCOL_VERSION,
        connection_id: vec![connection; 16],
        message_id,
        reply_to_message_id: None,
        payload: Some(payload),
    }
}

fn hello(connection: u8, message_id: u64) -> LocalEnvelope {
    hello_as(connection, message_id, ClientRole::ManagementUi)
}

fn hello_as(connection: u8, message_id: u64, role: ClientRole) -> LocalEnvelope {
    envelope(
        connection,
        message_id,
        Payload::ClientHello(ClientHello {
            role: role as i32,
            process_id: 100,
            minimum_protocol_version: PROTOCOL_VERSION,
            maximum_protocol_version: PROTOCOL_VERSION,
        }),
    )
}

#[test]
fn overlay_role_can_apply_semantic_actions_and_only_patch_poi_filters() {
    let mut session = ControlSession::new(OverlaySettings::default()).unwrap();
    assert!(send(&mut session, &hello_as(14, 1, ClientRole::Overlay)).is_none());

    let response = send(
        &mut session,
        &envelope(
            14,
            2,
            Payload::OverlayCommand(OverlayCommand {
                trace_id: vec![15; 16],
                command: OverlayCommandKind::OpenExpanded as i32,
                expected_settings_version: 1,
            }),
        ),
    )
    .expect("semantic command response");
    let Some(Payload::OverlayCommandResult(result)) = response.payload else {
        panic!("overlay command must produce OverlayCommandResult")
    };
    assert_eq!(result.status, OverlayCommandStatus::Applied as i32);
    assert_eq!(result.settings_version, 2);
    let effective = decode_overlay_settings(result.effective_settings.unwrap()).unwrap();
    assert_eq!(effective.display_mode, DisplayMode::ExpandedMap);

    let mut changed = effective;
    changed.poi_filters.fast_travel = true;
    changed.poi_filters.selected_pal_ids = vec!["SkyDragon".to_owned()];
    let response = send(
        &mut session,
        &patch(14, 3, 16, 2, &changed, &[OverlaySettingField::PoiFilters]),
    )
    .expect("overlay filter patch acknowledgement");
    let Some(Payload::SettingsAck(ack)) = response.payload else {
        panic!("overlay POI filter patch must be acknowledged")
    };
    assert_eq!(ack.status, SettingsApplyStatus::Applied as i32);
    assert_eq!(ack.applied_version, 3);
    assert_eq!(
        session.settings_snapshot().settings().poi_filters,
        changed.poi_filters
    );

    changed.opacity = 0.5;
    let response = send(
        &mut session,
        &patch(14, 4, 17, 3, &changed, &[OverlaySettingField::Opacity]),
    )
    .expect("wrong-field patch rejection");
    let Some(Payload::Error(error)) = response.payload else {
        panic!("overlay non-filter settings patch must be rejected")
    };
    assert_eq!(error.code, ProtocolErrorCode::UnexpectedPayload as i32);
    assert_eq!(session.settings_snapshot().settings().opacity, 0.9);
}

fn patch(
    connection: u8,
    message_id: u64,
    trace: u8,
    expected: u64,
    settings: &OverlaySettings,
    fields: &[OverlaySettingField],
) -> LocalEnvelope {
    envelope(
        connection,
        message_id,
        Payload::SettingsPatch(SettingsPatch {
            trace_id: vec![trace; 16],
            expected_settings_version: expected,
            candidate: Some(encode_overlay_settings(settings).unwrap()),
            changed_fields: fields.iter().map(|value| *value as i32).collect(),
        }),
    )
}

fn send(session: &mut ControlSession, value: &LocalEnvelope) -> Option<LocalEnvelope> {
    session
        .handle_envelope(value.clone())
        .unwrap()
        .map(|response| FrameCodec::decode(&response).unwrap())
}

#[test]
fn dispatches_no_mutation_before_valid_hello() {
    let mut session = ControlSession::new(OverlaySettings::default()).unwrap();
    let before = session.settings_snapshot();
    let mut changed = before.settings().clone();
    changed.input_mode = InputMode::PinnedInteractive;

    let response = send(
        &mut session,
        &patch(
            3,
            7,
            9,
            before.version(),
            &changed,
            &[OverlaySettingField::InputMode],
        ),
    )
    .unwrap();
    let Some(Payload::Error(error)) = response.payload else {
        panic!("pre-hello payload must produce ProtocolError")
    };
    assert_eq!(error.code, ProtocolErrorCode::UnexpectedPayload as i32);
    assert_eq!(error.offending_value, "");
    assert_eq!(response.reply_to_message_id, Some(7));
    assert_eq!(session.settings_snapshot(), before);
}

#[test]
fn framed_patch_returns_correlated_complete_ack() {
    let mut session = ControlSession::new(OverlaySettings::default()).unwrap();
    assert!(send(&mut session, &hello(4, 1)).is_none());
    let before = session.settings_snapshot();
    let mut changed = before.settings().clone();
    changed.input_mode = InputMode::PinnedInteractive;
    changed.display_mode = DisplayMode::ExpandedMap;

    let response = send(
        &mut session,
        &patch(
            4,
            2,
            8,
            before.version(),
            &changed,
            &[
                OverlaySettingField::InputMode,
                OverlaySettingField::DisplayMode,
            ],
        ),
    )
    .unwrap();
    assert_eq!(response.connection_id, vec![4; 16]);
    assert_ne!(response.message_id, 0);
    assert_eq!(response.reply_to_message_id, Some(2));
    let Some(Payload::SettingsAck(ack)) = response.payload else {
        panic!("settings request must produce SettingsAck")
    };
    assert_eq!(ack.trace_id, vec![8; 16]);
    assert_eq!(ack.status, SettingsApplyStatus::Applied as i32);
    assert_eq!(ack.applied_version, 2);
    let effective = decode_overlay_settings(ack.effective_settings.unwrap()).unwrap();
    assert_eq!(effective, *session.settings_snapshot().settings());
}

#[test]
fn settings_query_returns_current_version_without_mutation() {
    let initial = OverlaySettings {
        opacity: 0.73,
        diameter_px: 384,
        ..OverlaySettings::default()
    };
    let mut session = ControlSession::new(initial.clone()).unwrap();
    assert!(send(&mut session, &hello(12, 1)).is_none());

    let response = send(
        &mut session,
        &envelope(
            12,
            2,
            Payload::SettingsQuery(SettingsQuery {
                trace_id: vec![13; 16],
            }),
        ),
    )
    .unwrap();
    let Some(Payload::SettingsSnapshot(snapshot)) = response.payload else {
        panic!("settings query must produce SettingsSnapshot")
    };
    assert_eq!(snapshot.trace_id, vec![13; 16]);
    assert_eq!(snapshot.settings_version, 1);
    assert_eq!(
        decode_overlay_settings(snapshot.settings.unwrap()).unwrap(),
        initial
    );
    assert_eq!(session.settings_snapshot().version(), 1);
}

#[test]
fn disconnect_locks_without_hiding_and_trusted_reconnect_can_patch() {
    let initial = OverlaySettings {
        input_mode: InputMode::PinnedInteractive,
        display_mode: DisplayMode::ExpandedMap,
        enabled: true,
        ..OverlaySettings::default()
    };
    let mut session = ControlSession::new(initial).unwrap();
    assert!(send(&mut session, &hello(5, 1)).is_none());

    session.disconnect().unwrap();
    let locked = session.settings_snapshot();
    assert_eq!(locked.settings().input_mode, InputMode::Locked);
    assert_eq!(locked.settings().display_mode, DisplayMode::ExpandedMap);
    assert!(locked.settings().enabled);
    assert_eq!(locked.version(), 2);

    let shared = session.shared_store();
    let mut reconnect = ControlSession::from_shared(shared);
    assert!(send(&mut reconnect, &hello(6, 1)).is_none());
    let mut candidate = locked.settings().clone();
    candidate.input_mode = InputMode::PinnedInteractive;
    let response = send(
        &mut reconnect,
        &patch(
            6,
            2,
            10,
            locked.version(),
            &candidate,
            &[OverlaySettingField::InputMode],
        ),
    )
    .unwrap();
    let Some(Payload::SettingsAck(ack)) = response.payload else {
        panic!("trusted reconnect patch must be acknowledged")
    };
    assert_eq!(ack.status, SettingsApplyStatus::Applied as i32);
    assert_eq!(
        reconnect.settings_snapshot().settings().input_mode,
        InputMode::PinnedInteractive
    );
}

#[test]
fn unauthenticated_disconnect_is_noop_after_malformed_and_rejected_input() {
    let initial = OverlaySettings {
        input_mode: InputMode::PinnedInteractive,
        display_mode: DisplayMode::ExpandedMap,
        ..OverlaySettings::default()
    };
    let mut session = ControlSession::new(initial).unwrap();
    let before = session.settings_snapshot();

    let mut malformed = FramedStream::new();
    malformed.push(&[1, 0, 0, 0, 0xff]).unwrap();
    assert_eq!(
        malformed.next_message::<LocalEnvelope>(),
        Err(StreamError::Malformed)
    );

    let mut changed = before.settings().clone();
    changed.input_mode = InputMode::Locked;
    assert!(
        send(
            &mut session,
            &patch(
                3,
                7,
                9,
                before.version(),
                &changed,
                &[OverlaySettingField::InputMode],
            ),
        )
        .is_some()
    );

    let mut invalid_hello = hello(3, 8);
    let Some(Payload::ClientHello(value)) = invalid_hello.payload.as_mut() else {
        unreachable!()
    };
    value.role = ClientRole::Unspecified as i32;
    let response = send(&mut session, &invalid_hello).unwrap();
    let Some(Payload::Error(error)) = response.payload else {
        panic!("unspecified hello role must be rejected")
    };
    assert_eq!(error.code, ProtocolErrorCode::InvalidRole as i32);

    let disconnected = session.disconnect().unwrap();
    assert_eq!(disconnected, before);
    assert_eq!(session.settings_snapshot(), before);
}

#[test]
fn stream_output_dispatches_partial_multiple_frames_to_framed_ack() {
    let mut session = ControlSession::new(OverlaySettings::default()).unwrap();
    let before = session.settings_snapshot();
    let mut changed = before.settings().clone();
    changed.input_mode = InputMode::PinnedInteractive;
    let hello_frame = FrameCodec::encode(&hello(11, 1)).unwrap();
    let patch_frame = FrameCodec::encode(&patch(
        11,
        2,
        12,
        before.version(),
        &changed,
        &[OverlaySettingField::InputMode],
    ))
    .unwrap();
    let input = [hello_frame, patch_frame].concat();
    let mut stream = FramedStream::new();

    stream.push(&input[..2]).unwrap();
    stream.push(&input[2..9]).unwrap();
    stream.push(&input[9..]).unwrap();

    let mut responses = Vec::new();
    while let Some(envelope) = stream.next_message::<LocalEnvelope>().unwrap() {
        if let Some(response) = session.handle_envelope(envelope).unwrap() {
            responses.push(FrameCodec::decode::<LocalEnvelope>(&response).unwrap());
        }
    }

    assert_eq!(responses.len(), 1);
    let response = responses.pop().unwrap();
    assert_eq!(response.reply_to_message_id, Some(2));
    let Some(Payload::SettingsAck(ack)) = response.payload else {
        panic!("streamed settings patch must produce SettingsAck")
    };
    assert_eq!(ack.trace_id, vec![12; 16]);
    assert_eq!(ack.status, SettingsApplyStatus::Applied as i32);
}

#[test]
fn stream_output_dispatches_prehello_request_to_framed_protocol_error() {
    let mut session = ControlSession::new(OverlaySettings::default()).unwrap();
    let before = session.settings_snapshot();
    let frame = FrameCodec::encode(&patch(
        13,
        7,
        14,
        before.version(),
        before.settings(),
        &[OverlaySettingField::InputMode],
    ))
    .unwrap();
    let mut stream = FramedStream::new();

    stream.push(&frame[..3]).unwrap();
    stream.push(&frame[3..]).unwrap();
    let envelope = stream.next_message::<LocalEnvelope>().unwrap().unwrap();
    let framed_response = session.handle_envelope(envelope).unwrap().unwrap();
    let response: LocalEnvelope = FrameCodec::decode(&framed_response).unwrap();

    assert_eq!(response.reply_to_message_id, Some(7));
    let Some(Payload::Error(error)) = response.payload else {
        panic!("pre-hello streamed request must produce ProtocolError")
    };
    assert_eq!(error.code, ProtocolErrorCode::UnexpectedPayload as i32);
    assert_eq!(error.offending_value, "");
    assert_eq!(session.settings_snapshot(), before);
}
