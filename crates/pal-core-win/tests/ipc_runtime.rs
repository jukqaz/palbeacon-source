#![cfg(windows)]

use std::time::Duration;

use pal_core_win::{control_session::ControlSession, ipc_runtime::CoreIpcRuntime};
use pal_domain::{OverlayAction, OverlaySettings};
use pal_protocol::{
    PROTOCOL_VERSION, decode_overlay_settings, encode_overlay_action, encode_overlay_settings,
    v2::{
        ClientHello, ClientRole, LocalEnvelope, OverlayCommand, OverlayCommandStatus,
        OverlaySettingField, SettingsApplyStatus, SettingsPatch, SettingsQuery,
        local_envelope::Payload,
    },
};
use pal_windows_ipc::client::PipeClient;

fn hello(role: ClientRole) -> LocalEnvelope {
    LocalEnvelope {
        protocol_version: PROTOCOL_VERSION,
        connection_id: vec![4; 16],
        message_id: 1,
        reply_to_message_id: None,
        payload: Some(Payload::ClientHello(ClientHello {
            role: role as i32,
            process_id: std::process::id(),
            minimum_protocol_version: PROTOCOL_VERSION,
            maximum_protocol_version: PROTOCOL_VERSION,
        })),
    }
}

#[test]
fn core_runtime_accepts_a_management_ui_client_and_shuts_down_cleanly() {
    let endpoint = format!(
        r"\\.\pipe\PalBeacon.Core.Test.{}.{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("unnamed")
    );
    let session = ControlSession::new(OverlaySettings::default()).unwrap();
    let runtime = CoreIpcRuntime::start_at(&endpoint, session.shared_store()).unwrap();
    let mut client = PipeClient::connect(
        &endpoint,
        hello(ClientRole::ManagementUi),
        Duration::from_secs(2),
    )
    .unwrap();
    let candidate = OverlaySettings {
        opacity: 0.7,
        ..OverlaySettings::default()
    };
    let request = LocalEnvelope {
        protocol_version: PROTOCOL_VERSION,
        connection_id: vec![4; 16],
        message_id: 2,
        reply_to_message_id: None,
        payload: Some(Payload::SettingsPatch(SettingsPatch {
            trace_id: vec![8; 16],
            expected_settings_version: 1,
            candidate: Some(encode_overlay_settings(&candidate).unwrap()),
            changed_fields: vec![OverlaySettingField::Opacity as i32],
        })),
    };
    let response = client.request(request, Duration::from_secs(2)).unwrap();
    let Some(Payload::SettingsAck(ack)) = response.payload else {
        panic!("settings request must receive a settings acknowledgement")
    };
    assert_eq!(ack.status, SettingsApplyStatus::Applied as i32);
    assert_eq!(ack.applied_version, 2);

    drop(client);
    runtime.shutdown().unwrap();
}

#[test]
fn core_runtime_accepts_an_overlay_semantic_command_and_returns_effective_settings() {
    let endpoint = format!(
        r"\\.\pipe\PalBeacon.Core.Overlay.Test.{}.{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("unnamed")
    );
    let session = ControlSession::new(OverlaySettings::default()).unwrap();
    let runtime = CoreIpcRuntime::start_at(&endpoint, session.shared_store()).unwrap();
    let mut client = PipeClient::connect(
        &endpoint,
        hello(ClientRole::Overlay),
        Duration::from_secs(2),
    )
    .unwrap();
    let trace_id = vec![9; 16];
    let request = LocalEnvelope {
        protocol_version: PROTOCOL_VERSION,
        connection_id: vec![4; 16],
        message_id: 2,
        reply_to_message_id: None,
        payload: Some(Payload::OverlayCommand(OverlayCommand {
            trace_id: trace_id.clone(),
            command: encode_overlay_action(OverlayAction::ToggleFastTravel) as i32,
            expected_settings_version: 1,
        })),
    };

    let response = client.request(request, Duration::from_secs(2)).unwrap();
    let Some(Payload::OverlayCommandResult(result)) = response.payload else {
        panic!("overlay action must receive an overlay command result")
    };
    assert_eq!(result.trace_id, trace_id);
    assert_eq!(result.status, OverlayCommandStatus::Applied as i32);
    assert_eq!(result.settings_version, 2);
    let effective = decode_overlay_settings(
        result
            .effective_settings
            .expect("applied commands must return complete effective settings"),
    )
    .unwrap();
    assert!(!effective.poi_filters.fast_travel);

    let snapshot_response = client
        .request(
            LocalEnvelope {
                protocol_version: PROTOCOL_VERSION,
                connection_id: vec![4; 16],
                message_id: 3,
                reply_to_message_id: None,
                payload: Some(Payload::SettingsQuery(SettingsQuery {
                    trace_id: vec![10; 16],
                })),
            },
            Duration::from_secs(2),
        )
        .unwrap();
    let Some(Payload::SettingsSnapshot(snapshot)) = snapshot_response.payload else {
        panic!("overlay must be able to query the Core-authoritative settings snapshot")
    };
    assert_eq!(snapshot.settings_version, 2);
    assert_eq!(
        decode_overlay_settings(
            snapshot
                .settings
                .expect("settings snapshot must include complete settings"),
        )
        .unwrap(),
        effective
    );

    drop(client);
    runtime.shutdown().unwrap();
}
