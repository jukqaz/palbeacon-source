#![cfg(windows)]

use std::{
    thread,
    time::{Duration, Instant},
};

use pal_core_win::{control_session::ControlSession, ipc_runtime::CoreIpcRuntime};
use pal_domain::{OverlayAction, OverlaySettings};
use pal_overlay_win::{
    control_client::{ControlClientEvent, ControlClientSubmit, OverlayControlClientRuntime},
    control_intent::ControlIntent,
};

fn wait_for(
    client: &OverlayControlClientRuntime,
    predicate: impl Fn(&ControlClientEvent) -> bool,
) -> ControlClientEvent {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Some(event) = client.take_latest()
            && predicate(&event)
        {
            return event;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for the expected overlay control client event");
}

#[test]
fn overlay_control_client_queries_core_and_applies_a_versioned_action() {
    let endpoint = format!(
        r"\\.\pipe\PalBeacon.Overlay.Client.Test.{}.{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("unnamed")
    );
    let session = ControlSession::new(OverlaySettings::default()).unwrap();
    let core = CoreIpcRuntime::start_at(&endpoint, session.shared_store()).unwrap();
    let client = OverlayControlClientRuntime::start_at(&endpoint).unwrap();

    let connected = wait_for(&client, |event| {
        matches!(event, ControlClientEvent::Connected { .. })
    });
    let ControlClientEvent::Connected {
        settings_version,
        settings,
    } = connected
    else {
        unreachable!("wait predicate only accepts connected events")
    };
    assert_eq!(settings_version, 1);
    assert!(settings.poi_filters.fast_travel);

    assert_eq!(
        client.submit(ControlIntent::Action {
            expected_settings_version: settings_version,
            action: OverlayAction::ToggleFastTravel,
        }),
        ControlClientSubmit::Queued
    );
    let applied = wait_for(&client, |event| {
        matches!(event, ControlClientEvent::SettingsApplied { .. })
    });
    let ControlClientEvent::SettingsApplied {
        settings_version,
        settings,
    } = applied
    else {
        unreachable!("wait predicate only accepts applied events")
    };
    assert_eq!(settings_version, 2);
    assert!(!settings.poi_filters.fast_travel);

    drop(client);
    core.shutdown().unwrap();
}

#[test]
fn overlay_control_client_replaces_only_poi_filters_through_core_authority() {
    let endpoint = format!(
        r"\\.\pipe\PalBeacon.Overlay.Filter.Test.{}.{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("unnamed")
    );
    let initial = OverlaySettings {
        zoom: 2.25,
        ..OverlaySettings::default()
    };
    let session = ControlSession::new(initial).unwrap();
    let core = CoreIpcRuntime::start_at(&endpoint, session.shared_store()).unwrap();
    let client = OverlayControlClientRuntime::start_at(&endpoint).unwrap();

    let connected = wait_for(&client, |event| {
        matches!(event, ControlClientEvent::Connected { .. })
    });
    let ControlClientEvent::Connected {
        settings_version, ..
    } = connected
    else {
        unreachable!("wait predicate only accepts connected events")
    };
    let filters = pal_domain::PoiFilters {
        fast_travel: false,
        boss: true,
        wanted: true,
        dungeon: false,
        enabled_layer_ids: vec!["egg".to_owned(), "chromite".to_owned()],
        selected_pal_ids: vec!["SkyDragon".to_owned()],
        night_only: true,
    };
    assert_eq!(
        client.submit(ControlIntent::ReplacePoiFilters {
            expected_settings_version: settings_version,
            filters: filters.clone(),
        }),
        ControlClientSubmit::Queued
    );

    let applied = wait_for(&client, |event| {
        matches!(event, ControlClientEvent::SettingsApplied { .. })
    });
    let ControlClientEvent::SettingsApplied {
        settings_version,
        settings,
    } = applied
    else {
        unreachable!("wait predicate only accepts applied events")
    };
    assert_eq!(settings_version, 2);
    assert_eq!(settings.poi_filters, filters);
    assert_eq!(settings.zoom, 2.25);

    drop(client);
    core.shutdown().unwrap();
}
