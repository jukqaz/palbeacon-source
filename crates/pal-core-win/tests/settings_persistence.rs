use std::{thread, time::Duration};

use pal_core_win::{
    control_session::ControlSession, settings_persistence::SettingsPersistenceRuntime,
};
use pal_domain::{InputMode, OverlaySettings};
use pal_overlay_control::{OverlayControlDocument, read_control, write_control};
use pal_protocol::{
    PROTOCOL_VERSION, encode_overlay_settings,
    v2::{
        ClientHello, ClientRole, LocalEnvelope, OverlaySettingField, SettingsPatch,
        local_envelope::Payload,
    },
};
use tempfile::tempdir;

fn envelope(message_id: u64, payload: Payload) -> LocalEnvelope {
    LocalEnvelope {
        protocol_version: PROTOCOL_VERSION,
        connection_id: vec![7; 16],
        message_id,
        reply_to_message_id: None,
        payload: Some(payload),
    }
}

#[test]
fn applied_and_disconnect_safety_settings_are_persisted() {
    let directory = tempdir().expect("tempdir");
    let path = directory.path().join("overlay-control.json");
    write_control(&path, &OverlayControlDocument::default()).expect("seed");

    let mut session = ControlSession::new(OverlaySettings::default()).expect("session");
    let persistence =
        SettingsPersistenceRuntime::start(session.shared_store(), path.clone()).expect("runtime");
    session
        .handle_envelope(envelope(
            1,
            Payload::ClientHello(ClientHello {
                role: ClientRole::ManagementUi as i32,
                process_id: std::process::id(),
                minimum_protocol_version: PROTOCOL_VERSION,
                maximum_protocol_version: PROTOCOL_VERSION,
            }),
        ))
        .expect("hello");

    let candidate = OverlaySettings {
        input_mode: InputMode::PinnedInteractive,
        opacity: 0.67,
        ..OverlaySettings::default()
    };
    session
        .handle_envelope(envelope(
            2,
            Payload::SettingsPatch(SettingsPatch {
                trace_id: vec![8; 16],
                expected_settings_version: 1,
                candidate: Some(encode_overlay_settings(&candidate).expect("wire")),
                changed_fields: vec![
                    OverlaySettingField::InputMode as i32,
                    OverlaySettingField::Opacity as i32,
                ],
            }),
        ))
        .expect("patch");
    wait_for(&path, |document| {
        document.settings.input_mode == pal_overlay_control::ControlInputMode::PinnedInteractive
            && (document.settings.opacity - 0.67).abs() < f32::EPSILON
    });

    session.disconnect().expect("disconnect");
    wait_for(&path, |document| {
        document.settings.input_mode == pal_overlay_control::ControlInputMode::Locked
    });
    persistence.shutdown().expect("shutdown");
}

fn wait_for(path: &std::path::Path, predicate: impl Fn(&OverlayControlDocument) -> bool) {
    for _ in 0..100 {
        let document = read_control(path).expect("read persisted settings");
        if predicate(&document) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("settings were not persisted before the test deadline");
}
