use pal_core_win::settings_store::{ActionResult, SettingsStore};
use pal_domain::{DisplayMode, InputMode, OverlayAction, OverlaySettings, RESOURCE_LAYER_IDS};

#[test]
fn action_transition_is_versioned_and_returns_complete_effective_settings() {
    let mut store = SettingsStore::new(OverlaySettings::default()).expect("store");

    let ActionResult::Applied(expanded) = store.apply_action(1, OverlayAction::OpenExpanded) else {
        panic!("expanded action should apply");
    };
    assert_eq!(expanded.version(), 2);
    assert_eq!(expanded.settings().display_mode, DisplayMode::ExpandedMap);

    let ActionResult::Applied(interactive) = store.apply_action(2, OverlayAction::EnterInteractive)
    else {
        panic!("interactive action should apply");
    };
    assert_eq!(interactive.version(), 3);
    assert_eq!(
        interactive.settings().input_mode,
        InputMode::PinnedInteractive
    );
}

#[test]
fn stale_action_never_mutates_settings() {
    let mut store = SettingsStore::new(OverlaySettings::default()).expect("store");
    let before = store.snapshot();

    let ActionResult::VersionConflict(current) = store.apply_action(99, OverlayAction::ZoomIn)
    else {
        panic!("stale action should conflict");
    };
    assert_eq!(current, before);
    assert_eq!(store.snapshot(), before);
}

#[test]
fn no_op_action_does_not_consume_a_version() {
    let mut store = SettingsStore::new(OverlaySettings::default()).expect("store");

    let ActionResult::NoOp(current) = store.apply_action(1, OverlayAction::Show) else {
        panic!("showing an already-visible overlay should be a no-op");
    };
    assert_eq!(current.version(), 1);
    assert_eq!(store.version(), 1);
}

#[test]
fn filter_actions_toggle_base_and_group_filters_through_the_same_versioned_store() {
    let mut store = SettingsStore::new(OverlaySettings::default()).expect("store");

    let ActionResult::Applied(snapshot) = store.apply_action(1, OverlayAction::ToggleWanted) else {
        panic!("wanted filter action should apply");
    };
    assert!(snapshot.settings().poi_filters.wanted);
    assert!(snapshot.settings().poi_filters.fast_travel);
    assert!(snapshot.settings().poi_filters.boss);
    assert!(snapshot.settings().poi_filters.dungeon);

    let ActionResult::Applied(resources) = store.apply_action(2, OverlayAction::ToggleResources)
    else {
        panic!("resource filter action should apply");
    };
    assert_eq!(resources.version(), 3);
    assert!(RESOURCE_LAYER_IDS.iter().all(|layer_id| {
        resources
            .settings()
            .poi_filters
            .enabled_layer_ids
            .iter()
            .any(|enabled| enabled == layer_id)
    }));
}
