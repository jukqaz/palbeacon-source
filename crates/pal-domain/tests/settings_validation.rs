use pal_domain::{
    CoreState, CoreStateParts, CoreStateValidationError, DisplayMode, ExpandedMapView, FpsProfile,
    Freshness, HIDDEN_UNVERIFIED_LAYER_IDS, HotkeyAction, HotkeyBindings, HotkeyChord,
    HotkeyModifiers, HotkeyValidationReason, InputMode, MiniMapView, OverlayAction,
    OverlaySettings, PoiFilters, PositionSample, RESOURCE_LAYER_IDS, RotationMode, SampleClock,
    SettingsValidationError, ViewValidationError, WindowSnapshot, reduce_overlay_action,
};

const fn rotation_mode_tag(mode: RotationMode) -> u8 {
    match mode {
        RotationMode::NorthUp => 0,
        RotationMode::HeadingUp => 1,
    }
}

const fn input_mode_tag(mode: InputMode) -> u8 {
    match mode {
        InputMode::Locked => 0,
        InputMode::TemporaryInteractive => 1,
        InputMode::PinnedInteractive => 2,
        InputMode::LayoutEdit => 3,
    }
}

const fn display_mode_tag(mode: DisplayMode) -> u8 {
    match mode {
        DisplayMode::MiniMap => 0,
        DisplayMode::ExpandedMap => 1,
    }
}

const fn fps_profile_tag(profile: FpsProfile) -> u8 {
    match profile {
        FpsProfile::Auto => 0,
        FpsProfile::Thirty => 1,
        FpsProfile::Sixty => 2,
    }
}

const fn hotkey_action_tag(action: HotkeyAction) -> u8 {
    match action {
        HotkeyAction::OverlayVisibility => 0,
        HotkeyAction::RotationToggle => 1,
        HotkeyAction::TemporaryInteraction => 2,
        HotkeyAction::InteractionLock => 3,
    }
}

const fn core_state_settings_error(
    error: CoreStateValidationError,
) -> Option<SettingsValidationError> {
    match error {
        CoreStateValidationError::Settings(error) => Some(error),
        CoreStateValidationError::SettingsVersionZero
        | CoreStateValidationError::ConnectionFreshnessMismatch
        | CoreStateValidationError::FreshnessPositionMissing
        | CoreStateValidationError::InterpolationStateInvalid
        | CoreStateValidationError::HeadingAvailabilityInvalid => None,
    }
}

#[test]
fn settings_defaults_are_safe_and_unassigned() {
    let settings = OverlaySettings::default();

    assert!(settings.enabled);
    assert!(settings.auto_show);
    assert_eq!(settings.rotation_mode, RotationMode::NorthUp);
    assert_eq!(settings.input_mode, InputMode::Locked);
    assert_eq!(settings.display_mode, DisplayMode::MiniMap);
    assert_eq!(settings.opacity, 0.90);
    assert_eq!(settings.diameter_px, 320);
    assert_eq!(settings.zoom, 1.0);
    assert_eq!(settings.normalized_x, 0.85);
    assert_eq!(settings.normalized_y, 0.20);
    assert_eq!(settings.fps_profile, FpsProfile::Auto);
    assert_eq!(settings.poi_filters, PoiFilters::default());
    assert!(settings.poi_filters.fast_travel);
    assert!(settings.poi_filters.boss);
    assert!(!settings.poi_filters.wanted);
    assert!(settings.poi_filters.dungeon);
    assert_eq!(
        settings.poi_filters.enabled_layer_ids,
        ["map-unlock", "poi", "tower"]
    );
    assert!(!RESOURCE_LAYER_IDS.contains(&"healing-spring"));
    assert_eq!(settings.hotkey_bindings, HotkeyBindings::default());
    assert!(settings.hotkey_bindings.overlay_visibility.is_none());
    assert!(settings.hotkey_bindings.rotation_toggle.is_none());
    assert!(settings.hotkey_bindings.temporary_interaction.is_none());
    assert!(settings.hotkey_bindings.interaction_lock.is_none());
    assert!(settings.validate().is_ok());
}

#[test]
fn map_filter_selection_is_bounded_unique_and_ipc_safe() {
    let mut settings = OverlaySettings::default();
    settings.poi_filters.enabled_layer_ids = vec!["tower".to_owned(), "ore-quartz".to_owned()];
    settings.poi_filters.selected_pal_ids =
        vec!["SkyDragon".to_owned(), "BOSS_JetDragon".to_owned()];
    settings.poi_filters.night_only = true;
    settings
        .validate()
        .expect("valid synchronized filter state");

    settings
        .poi_filters
        .selected_pal_ids
        .push("SkyDragon".to_owned());
    assert_eq!(
        settings.validate().unwrap_err(),
        SettingsValidationError::DuplicateMapFilterId
    );

    settings.poi_filters.selected_pal_ids = vec!["invalid filter with spaces".to_owned()];
    assert_eq!(
        settings.validate().unwrap_err(),
        SettingsValidationError::InvalidMapFilterId
    );
}

#[test]
fn hidden_unverified_map_filters_are_rejected_and_can_be_sanitized() {
    let mut settings = OverlaySettings::default();
    settings.poi_filters.enabled_layer_ids = vec![
        "tower".to_owned(),
        "healing-spring".to_owned(),
        "captured-pal".to_owned(),
    ];
    assert_eq!(
        settings.validate().unwrap_err(),
        SettingsValidationError::HiddenUnverifiedMapFilterId
    );

    assert!(settings.poi_filters.remove_hidden_unverified_layers());
    assert_eq!(settings.poi_filters.enabled_layer_ids, ["tower"]);
    assert!(settings.validate().is_ok());
    assert!(HIDDEN_UNVERIFIED_LAYER_IDS.iter().all(|hidden| {
        !settings
            .poi_filters
            .enabled_layer_ids
            .iter()
            .any(|id| id == hidden)
    }));
}

#[test]
fn overlay_actions_are_deterministic_and_filter_toggles_preserve_other_state() {
    let initial = OverlaySettings::default();
    let expanded = reduce_overlay_action(&initial, OverlayAction::OpenExpanded);
    let interactive = reduce_overlay_action(&expanded, OverlayAction::EnterInteractive);
    let boss_off = reduce_overlay_action(&interactive, OverlayAction::ToggleBoss);

    assert_eq!(expanded.display_mode, DisplayMode::ExpandedMap);
    assert_eq!(interactive.input_mode, InputMode::PinnedInteractive);
    assert!(!boss_off.poi_filters.boss);
    assert!(boss_off.poi_filters.fast_travel);
    assert!(!boss_off.poi_filters.wanted);
    assert!(boss_off.poi_filters.dungeon);
    assert_eq!(
        reduce_overlay_action(&boss_off, OverlayAction::ToggleBoss)
            .poi_filters
            .boss,
        initial.poi_filters.boss
    );

    let tower_off = reduce_overlay_action(&interactive, OverlayAction::ToggleTower);
    assert!(
        !tower_off
            .poi_filters
            .enabled_layer_ids
            .contains(&"tower".to_owned())
    );
    let resources_on = reduce_overlay_action(&tower_off, OverlayAction::ToggleResources);
    assert!(RESOURCE_LAYER_IDS.iter().all(|layer_id| {
        resources_on
            .poi_filters
            .enabled_layer_ids
            .iter()
            .any(|enabled| enabled == layer_id)
    }));
    let resources_off = reduce_overlay_action(&resources_on, OverlayAction::ToggleResources);
    assert!(RESOURCE_LAYER_IDS.iter().all(|layer_id| {
        !resources_off
            .poi_filters
            .enabled_layer_ids
            .iter()
            .any(|enabled| enabled == layer_id)
    }));

    let collapsed = reduce_overlay_action(&interactive, OverlayAction::CloseExpanded);
    assert_eq!(collapsed.display_mode, DisplayMode::MiniMap);
    assert_eq!(collapsed.input_mode, InputMode::Locked);
}

#[test]
fn overlay_zoom_actions_clamp_at_the_validated_limits() {
    let maximum = OverlaySettings {
        zoom: 4.0,
        ..OverlaySettings::default()
    };
    let minimum = OverlaySettings {
        zoom: 0.5,
        ..OverlaySettings::default()
    };

    assert_eq!(
        reduce_overlay_action(&maximum, OverlayAction::ZoomIn).zoom,
        4.0
    );
    assert_eq!(
        reduce_overlay_action(&minimum, OverlayAction::ZoomOut).zoom,
        0.5
    );
}

#[test]
fn setting_mode_matches_are_exhaustive_without_wildcards() {
    assert_eq!(rotation_mode_tag(RotationMode::NorthUp), 0);
    assert_eq!(rotation_mode_tag(RotationMode::HeadingUp), 1);
    assert_eq!(input_mode_tag(InputMode::Locked), 0);
    assert_eq!(input_mode_tag(InputMode::TemporaryInteractive), 1);
    assert_eq!(input_mode_tag(InputMode::PinnedInteractive), 2);
    assert_eq!(input_mode_tag(InputMode::LayoutEdit), 3);
    assert_eq!(display_mode_tag(DisplayMode::MiniMap), 0);
    assert_eq!(display_mode_tag(DisplayMode::ExpandedMap), 1);
    assert_eq!(fps_profile_tag(FpsProfile::Auto), 0);
    assert_eq!(fps_profile_tag(FpsProfile::Thirty), 1);
    assert_eq!(fps_profile_tag(FpsProfile::Sixty), 2);
    assert_eq!(hotkey_action_tag(HotkeyAction::OverlayVisibility), 0);
    assert_eq!(hotkey_action_tag(HotkeyAction::RotationToggle), 1);
    assert_eq!(hotkey_action_tag(HotkeyAction::TemporaryInteraction), 2);
    assert_eq!(hotkey_action_tag(HotkeyAction::InteractionLock), 3);
}

#[test]
fn settings_validation_accepts_inclusive_budget_limits() {
    for opacity in [0.20, 1.00] {
        let settings = OverlaySettings {
            opacity,
            ..OverlaySettings::default()
        };
        settings.validate().unwrap();
    }

    for zoom in [0.50, 4.00] {
        let settings = OverlaySettings {
            zoom,
            ..OverlaySettings::default()
        };
        settings.validate().unwrap();
    }

    for diameter_px in [180, 640] {
        let settings = OverlaySettings {
            diameter_px,
            ..OverlaySettings::default()
        };
        settings.validate().unwrap();
    }

    for normalized_x in [0.0, 1.0] {
        let settings = OverlaySettings {
            normalized_x,
            ..OverlaySettings::default()
        };
        settings.validate().unwrap();
    }

    for normalized_y in [0.0, 1.0] {
        let settings = OverlaySettings {
            normalized_y,
            ..OverlaySettings::default()
        };
        settings.validate().unwrap();
    }
}

#[test]
fn settings_validation_reports_the_out_of_budget_field() {
    let invalid_cases: [(OverlaySettings, &str); 10] = [
        (
            OverlaySettings {
                opacity: 0.19,
                ..OverlaySettings::default()
            },
            "opacity",
        ),
        (
            OverlaySettings {
                opacity: 1.01,
                ..OverlaySettings::default()
            },
            "opacity",
        ),
        (
            OverlaySettings {
                zoom: 0.49,
                ..OverlaySettings::default()
            },
            "zoom",
        ),
        (
            OverlaySettings {
                zoom: 4.01,
                ..OverlaySettings::default()
            },
            "zoom",
        ),
        (
            OverlaySettings {
                diameter_px: 179,
                ..OverlaySettings::default()
            },
            "diameter_px",
        ),
        (
            OverlaySettings {
                diameter_px: 641,
                ..OverlaySettings::default()
            },
            "diameter_px",
        ),
        (
            OverlaySettings {
                normalized_x: -0.01,
                ..OverlaySettings::default()
            },
            "normalized_x",
        ),
        (
            OverlaySettings {
                normalized_x: 1.01,
                ..OverlaySettings::default()
            },
            "normalized_x",
        ),
        (
            OverlaySettings {
                normalized_y: -0.01,
                ..OverlaySettings::default()
            },
            "normalized_y",
        ),
        (
            OverlaySettings {
                normalized_y: 1.01,
                ..OverlaySettings::default()
            },
            "normalized_y",
        ),
    ];

    for (settings, expected_field) in invalid_cases {
        assert_eq!(settings.validate().unwrap_err().field(), expected_field);
    }
}

#[test]
fn settings_validation_rejects_non_finite_floats() {
    let invalid_cases = [
        (
            OverlaySettings {
                opacity: f32::NAN,
                ..OverlaySettings::default()
            },
            "opacity",
        ),
        (
            OverlaySettings {
                zoom: f32::INFINITY,
                ..OverlaySettings::default()
            },
            "zoom",
        ),
        (
            OverlaySettings {
                normalized_x: f32::NEG_INFINITY,
                ..OverlaySettings::default()
            },
            "normalized_x",
        ),
        (
            OverlaySettings {
                normalized_y: f32::NAN,
                ..OverlaySettings::default()
            },
            "normalized_y",
        ),
    ];

    for (settings, expected_field) in invalid_cases {
        assert_eq!(settings.validate().unwrap_err().field(), expected_field);
    }
}

#[test]
fn hotkey_validation_reports_each_public_binding_path() {
    let mut zero_key = OverlaySettings::default();
    zero_key.hotkey_bindings.overlay_visibility = Some(HotkeyChord {
        modifiers: HotkeyModifiers::default(),
        virtual_key: 0,
    });
    assert_eq!(
        zero_key.validate().unwrap_err().field(),
        "hotkey_bindings.overlay_visibility"
    );

    let mut unmodified_k = OverlaySettings::default();
    unmodified_k.hotkey_bindings.rotation_toggle = Some(HotkeyChord {
        modifiers: HotkeyModifiers::default(),
        virtual_key: 0x4B,
    });
    assert_eq!(
        unmodified_k.validate().unwrap_err().field(),
        "hotkey_bindings.rotation_toggle"
    );

    unmodified_k.hotkey_bindings.rotation_toggle = Some(HotkeyChord {
        modifiers: HotkeyModifiers {
            control: true,
            ..HotkeyModifiers::default()
        },
        virtual_key: 0x4B,
    });
    unmodified_k.validate().unwrap();

    for function_key in [0x70, 0x87] {
        let mut standalone_function_key = OverlaySettings::default();
        standalone_function_key.hotkey_bindings.overlay_visibility = Some(HotkeyChord {
            modifiers: HotkeyModifiers::default(),
            virtual_key: function_key,
        });
        standalone_function_key.validate().unwrap();
    }

    for game_key in [0x30, 0x41, 0x5A] {
        let mut unmodified_game_key = OverlaySettings::default();
        unmodified_game_key.hotkey_bindings.overlay_visibility = Some(HotkeyChord {
            modifiers: HotkeyModifiers::default(),
            virtual_key: game_key,
        });
        assert_eq!(
            unmodified_game_key.validate().unwrap_err(),
            SettingsValidationError::InvalidHotkey {
                action: HotkeyAction::OverlayVisibility,
                reason: HotkeyValidationReason::UnmodifiedGameKey,
            }
        );
    }

    let mut modified_game_key = OverlaySettings::default();
    modified_game_key.hotkey_bindings.overlay_visibility = Some(HotkeyChord {
        modifiers: HotkeyModifiers {
            alt: true,
            shift: true,
            ..HotkeyModifiers::default()
        },
        virtual_key: 0x41,
    });
    modified_game_key.validate().unwrap();

    let mut invalid_interaction_lock = OverlaySettings::default();
    invalid_interaction_lock.hotkey_bindings.interaction_lock = Some(HotkeyChord {
        modifiers: HotkeyModifiers::default(),
        virtual_key: 0,
    });
    assert_eq!(
        invalid_interaction_lock.validate().unwrap_err().field(),
        "hotkey_bindings.interaction_lock"
    );

    let mut invalid_temporary_interaction = OverlaySettings::default();
    invalid_temporary_interaction
        .hotkey_bindings
        .temporary_interaction = Some(HotkeyChord {
        modifiers: HotkeyModifiers::default(),
        virtual_key: 0x4B,
    });
    assert_eq!(
        invalid_temporary_interaction
            .validate()
            .unwrap_err()
            .field(),
        "hotkey_bindings.temporary_interaction"
    );
}

#[test]
fn visible_windows_require_physical_size_and_dpi() {
    for (width, height, dpi, expected_field) in [
        (0, 720, 96, "client_width"),
        (1_280, 0, 96, "client_height"),
        (1_280, 720, 0, "dpi"),
    ] {
        let error =
            WindowSnapshot::new(42, 10, 20, width, height, dpi, true, true, false).unwrap_err();
        assert_eq!(error.field(), expected_field);
    }
}

#[test]
fn hidden_windows_may_have_no_current_client_geometry() {
    let snapshot = WindowSnapshot::new(42, 10, 20, 0, 0, 0, false, false, true).unwrap();

    assert_eq!(snapshot.process_id(), 42);
    assert_eq!(snapshot.client_left(), 10);
    assert_eq!(snapshot.client_top(), 20);
    assert_eq!(snapshot.client_width(), 0);
    assert_eq!(snapshot.client_height(), 0);
    assert_eq!(snapshot.dpi(), 0);
    assert!(!snapshot.visible());
    assert!(!snapshot.active());
    assert!(snapshot.minimized());
}

#[test]
fn core_state_validates_settings_and_exposes_read_only_access() {
    let window = WindowSnapshot::new(42, 10, 20, 1_280, 720, 144, true, true, false).unwrap();
    let state = CoreState::new(
        OverlaySettings::default(),
        Some(window.clone()),
        None,
        Freshness::Offline,
        false,
    )
    .unwrap();

    assert_eq!(state.settings(), &OverlaySettings::default());
    assert_eq!(state.window_snapshot(), Some(&window));
    assert!(state.position_sample().is_none());
    assert_eq!(state.freshness(), Freshness::Offline);
    assert!(!state.heading_available());

    let error = CoreState::new(
        OverlaySettings {
            opacity: 1.1,
            ..OverlaySettings::default()
        },
        None,
        None,
        Freshness::Live,
        true,
    )
    .unwrap_err();
    assert_eq!(error.field(), "opacity");
    assert_eq!(core_state_settings_error(error).unwrap().field(), "opacity");
}

#[test]
fn map_views_validate_finite_centers_and_inclusive_zoom() {
    for zoom in [0.50, 4.00] {
        assert!(MiniMapView::new(1.0, -1.0, zoom).is_ok());
        assert!(ExpandedMapView::new(1.0, -1.0, zoom).is_ok());
    }

    for (error, field) in [
        (
            MiniMapView::new(f64::NAN, 0.0, 1.0).unwrap_err(),
            "mini_map_view.center_x",
        ),
        (
            MiniMapView::new(0.0, f64::INFINITY, 1.0).unwrap_err(),
            "mini_map_view.center_y",
        ),
        (
            MiniMapView::new(0.0, 0.0, 4.01).unwrap_err(),
            "mini_map_view.zoom",
        ),
        (
            ExpandedMapView::new(f64::NEG_INFINITY, 0.0, 1.0).unwrap_err(),
            "expanded_map_view.center_x",
        ),
        (
            ExpandedMapView::new(0.0, f64::NAN, 1.0).unwrap_err(),
            "expanded_map_view.center_y",
        ),
        (
            ExpandedMapView::new(0.0, 0.0, 0.49).unwrap_err(),
            "expanded_map_view.zoom",
        ),
    ] {
        assert_eq!(error.field(), field);
    }
}

#[test]
fn complete_core_state_parts_are_validated_and_read_only() {
    let parts = CoreStateParts {
        settings: OverlaySettings::default(),
        settings_version: 4,
        window_snapshot: None,
        position_sample: None,
        freshness: Freshness::Stale,
        heading_available: false,
        connected: true,
        visible: true,
        interpolate_position: false,
        mini_map_view: MiniMapView::new(2.0, 3.0, 1.5).unwrap(),
        expanded_map_view: ExpandedMapView::new(4.0, 5.0, 2.5).unwrap(),
    };
    let state = CoreState::from_parts(parts.clone()).unwrap();
    assert_eq!(state.settings_version(), 4);
    assert!(state.connected());
    assert!(state.visible());
    assert!(!state.interpolate_position());
    assert_eq!(state.mini_map_view(), &parts.mini_map_view);
    assert_eq!(state.expanded_map_view(), &parts.expanded_map_view);

    let error = CoreState::from_parts(CoreStateParts {
        settings_version: 0,
        ..parts
    })
    .unwrap_err();
    assert_eq!(error, CoreStateValidationError::SettingsVersionZero);
    assert_eq!(error.field(), "settings_version");
}

#[test]
fn core_state_parts_reject_inconsistent_runtime_flags() {
    let valid = CoreStateParts {
        settings: OverlaySettings::default(),
        settings_version: 1,
        window_snapshot: None,
        position_sample: None,
        freshness: Freshness::Offline,
        heading_available: false,
        connected: false,
        visible: true,
        interpolate_position: false,
        mini_map_view: MiniMapView::default(),
        expanded_map_view: ExpandedMapView::default(),
    };

    let error = CoreState::from_parts(CoreStateParts {
        connected: true,
        ..valid.clone()
    })
    .unwrap_err();
    assert_eq!(error, CoreStateValidationError::ConnectionFreshnessMismatch);
    assert_eq!(error.field(), "freshness");

    let error = CoreState::from_parts(CoreStateParts {
        interpolate_position: true,
        ..valid.clone()
    })
    .unwrap_err();
    assert_eq!(error, CoreStateValidationError::InterpolationStateInvalid);
    assert_eq!(error.field(), "interpolate_position");

    let error = CoreState::from_parts(CoreStateParts {
        heading_available: true,
        ..valid
    })
    .unwrap_err();
    assert_eq!(error, CoreStateValidationError::HeadingAvailabilityInvalid);
    assert_eq!(error.field(), "heading_available");
}

fn runtime_position() -> PositionSample {
    PositionSample::new(
        "world",
        b"subject",
        b"boot",
        1,
        1,
        1.0,
        2.0,
        3.0,
        Some(45.0),
        SampleClock::received_with_age(0, 100),
    )
    .unwrap()
}

fn runtime_parts(freshness: Freshness) -> CoreStateParts {
    CoreStateParts {
        settings: OverlaySettings::default(),
        settings_version: 1,
        window_snapshot: None,
        position_sample: None,
        freshness,
        heading_available: false,
        connected: freshness != Freshness::Offline,
        visible: true,
        interpolate_position: false,
        mini_map_view: MiniMapView::default(),
        expanded_map_view: ExpandedMapView::default(),
    }
}

#[test]
fn live_and_delayed_core_state_require_a_position_sample() {
    for freshness in [Freshness::Live, Freshness::Delayed] {
        let error = CoreState::from_parts(runtime_parts(freshness)).unwrap_err();
        assert_eq!(error.field(), "position_sample");
    }
}

#[test]
fn interpolation_flag_is_exactly_equivalent_to_runtime_eligibility() {
    for freshness in [Freshness::Live, Freshness::Delayed] {
        let mut eligible = runtime_parts(freshness);
        eligible.position_sample = Some(runtime_position());
        let error = CoreState::from_parts(eligible.clone()).unwrap_err();
        assert_eq!(error.field(), "interpolate_position");

        eligible.interpolate_position = true;
        CoreState::from_parts(eligible).unwrap();
    }

    let stale_without_sample = runtime_parts(Freshness::Stale);
    CoreState::from_parts(stale_without_sample.clone()).unwrap();
    CoreState::from_parts(CoreStateParts {
        position_sample: Some(runtime_position()),
        ..stale_without_sample
    })
    .unwrap();

    let offline = runtime_parts(Freshness::Offline);
    CoreState::from_parts(CoreStateParts {
        position_sample: Some(runtime_position()),
        ..offline
    })
    .unwrap();
}

const fn view_error_field(error: ViewValidationError) -> &'static str {
    match error {
        ViewValidationError::MiniMapCenterXNonFinite
        | ViewValidationError::MiniMapCenterYNonFinite
        | ViewValidationError::MiniMapZoomOutOfRange
        | ViewValidationError::ExpandedMapCenterXNonFinite
        | ViewValidationError::ExpandedMapCenterYNonFinite
        | ViewValidationError::ExpandedMapZoomOutOfRange => error.field(),
    }
}

#[test]
fn view_error_variants_remain_exhaustively_matched() {
    assert_eq!(
        view_error_field(ViewValidationError::ExpandedMapZoomOutOfRange),
        "expanded_map_view.zoom"
    );
}
