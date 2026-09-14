use pal_core_win::settings_store::{PatchResult, SettingsStore};
use pal_domain::{DisplayMode, InputMode, OverlaySettings, RotationMode};
use pal_protocol::{
    encode_overlay_settings,
    v2::{self, OverlaySettingField},
};

fn wire(settings: &OverlaySettings) -> v2::OverlaySettings {
    encode_overlay_settings(settings).unwrap()
}

#[test]
fn starts_at_version_one_and_conflict_returns_complete_current_value() {
    let initial = OverlaySettings::default();
    let mut store = SettingsStore::new(initial.clone()).unwrap();
    assert_eq!(store.version(), 1);
    assert_eq!(store.settings(), &initial);

    let mut candidate = initial.clone();
    candidate.input_mode = InputMode::PinnedInteractive;
    let before = store.snapshot();
    let result = store.apply_patch(
        99,
        wire(&candidate),
        &[OverlaySettingField::InputMode as i32],
    );

    assert_eq!(result, PatchResult::VersionConflict(before.clone()));
    assert_eq!(store.snapshot(), before);
}

#[test]
fn generic_field_mask_updates_ui_vertical_and_preserves_every_unlisted_value() {
    let mut store = SettingsStore::new(OverlaySettings::default()).unwrap();
    let before = store.snapshot();
    let mut candidate = before.settings().clone();
    candidate.input_mode = InputMode::PinnedInteractive;
    candidate.display_mode = DisplayMode::ExpandedMap;
    candidate.rotation_mode = RotationMode::HeadingUp;
    candidate.poi_filters.boss = false;
    candidate.opacity = 0.25;

    let result = store.apply_patch(
        1,
        wire(&candidate),
        &[
            OverlaySettingField::InputMode as i32,
            OverlaySettingField::DisplayMode as i32,
            OverlaySettingField::RotationMode as i32,
            OverlaySettingField::PoiFilters as i32,
        ],
    );
    let PatchResult::Applied(applied) = result else {
        panic!("valid patch must apply")
    };

    assert_eq!(applied.version(), 2);
    assert_eq!(applied.settings().input_mode, InputMode::PinnedInteractive);
    assert_eq!(applied.settings().display_mode, DisplayMode::ExpandedMap);
    assert_eq!(applied.settings().rotation_mode, RotationMode::HeadingUp);
    assert!(!applied.settings().poi_filters.boss);
    assert!(applied.settings().poi_filters.fast_travel);
    assert!(applied.settings().poi_filters.dungeon);
    assert_eq!(applied.settings().opacity, before.settings().opacity);
    assert_eq!(store.snapshot(), applied);
}

#[test]
fn every_rejected_patch_is_publicly_byte_for_byte_equivalent() {
    let mut store = SettingsStore::new(OverlaySettings::default()).unwrap();

    for (candidate, fields) in [
        (
            wire(store.settings()),
            vec![
                OverlaySettingField::InputMode as i32,
                OverlaySettingField::InputMode as i32,
            ],
        ),
        (
            wire(store.settings()),
            vec![OverlaySettingField::Unspecified as i32],
        ),
        (wire(store.settings()), vec![99]),
        (
            {
                let mut invalid = wire(store.settings());
                invalid.input_mode = 99;
                invalid
            },
            vec![OverlaySettingField::InputMode as i32],
        ),
        (
            {
                let mut invalid = wire(store.settings());
                invalid.opacity = f32::NAN;
                invalid
            },
            vec![OverlaySettingField::Opacity as i32],
        ),
    ] {
        let before = store.snapshot();
        assert!(matches!(
            store.apply_patch(before.version(), candidate, &fields),
            PatchResult::Rejected { .. }
        ));
        assert_eq!(store.snapshot(), before);
    }
}

#[test]
fn locked_and_minimap_can_be_restored_through_same_field_mask_path() {
    let initial = OverlaySettings {
        input_mode: InputMode::PinnedInteractive,
        display_mode: DisplayMode::ExpandedMap,
        ..OverlaySettings::default()
    };
    let mut store = SettingsStore::new(initial.clone()).unwrap();
    let mut candidate = initial;
    candidate.input_mode = InputMode::Locked;
    candidate.display_mode = DisplayMode::MiniMap;

    let result = store.apply_patch(
        1,
        wire(&candidate),
        &[
            OverlaySettingField::InputMode as i32,
            OverlaySettingField::DisplayMode as i32,
        ],
    );
    let PatchResult::Applied(applied) = result else {
        panic!("valid patch must apply")
    };
    assert_eq!(applied.settings().input_mode, InputMode::Locked);
    assert_eq!(applied.settings().display_mode, DisplayMode::MiniMap);
}

#[test]
fn unknown_future_field_number_is_rejected_without_mutation() {
    let mut store = SettingsStore::new(OverlaySettings::default()).unwrap();
    let before = store.snapshot();

    assert!(matches!(
        store.apply_patch(before.version(), wire(before.settings()), &[i32::MAX],),
        PatchResult::Rejected { .. }
    ));
    assert_eq!(store.snapshot(), before);
}
