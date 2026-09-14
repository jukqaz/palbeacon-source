use pal_domain::{DisplayMode, Freshness, InputMode, PoiFilters};
use pal_render::{
    FilterAction, GateBadge, OVERLAY_VISUAL_V1, RailAction, RailButtonState, StatusTone,
    build_chrome_plan, filter_button_tone, rail_button_tone,
};

#[test]
fn visual_v1_pins_the_selected_dip_geometry_and_palette() {
    let spec = OVERLAY_VISUAL_V1;
    assert_eq!(spec.client_inset_dip, 16);
    assert_eq!(spec.mini_width_dip, 288);
    assert_eq!(spec.mini_height_dip, 288);
    assert_eq!(spec.expanded_width_dip, 1120);
    assert_eq!(spec.expanded_height_dip, 630);
    assert_eq!(spec.expanded_rail_width_dip, 56);
    assert_eq!(spec.expanded_header_height_dip, 0);
    assert_eq!(spec.expanded_panel_width_dip, 260);
    assert_eq!(spec.expanded_panel_inset_dip, 14);
    assert_eq!(spec.expanded_search_row_height_dip, 50);
    assert_eq!(spec.corner_radius_dip, 10);
    assert_eq!(spec.border_dip, 1);
    assert_eq!(spec.minimap_bezel_body_dip, 6);
    assert_eq!(spec.minimap_bezel_inner_inset_dip, 9);
    assert_eq!(spec.minimap_tick_length_dip, 8);
    assert_eq!(spec.rail_button_dip, 40);
    assert_eq!(spec.rail_inset_dip, 8);
    assert_eq!(spec.filter_button_width_dip, 114);
    assert_eq!(spec.filter_button_height_dip, 34);
    assert_eq!(spec.filter_button_gap_dip, 8);
    assert_eq!(spec.filter_strip_inset_dip, 12);
    assert_eq!(spec.status_dot_dip, 8);
    assert_eq!(
        spec.palette.neutral_backdrop.rgba(),
        [0x07, 0x0d, 0x14, 0xee]
    );
    assert_eq!(spec.palette.neutral_rail.rgba(), [0x0b, 0x17, 0x21, 0xf4]);
    assert_eq!(spec.palette.neutral_border.rgba(), [0x42, 0x6f, 0x7a, 0xe8]);
    assert_eq!(
        spec.palette.minimap_bezel_shadow.rgba(),
        [0x02, 0x07, 0x0b, 0xff]
    );
    assert_eq!(
        spec.palette.minimap_bezel_outer.rgba(),
        [0x0a, 0x17, 0x1e, 0xff]
    );
    assert_eq!(
        spec.palette.minimap_bezel_body.rgba(),
        [0x18, 0x30, 0x3a, 0xff]
    );
    assert_eq!(
        spec.palette.minimap_bezel_highlight.rgba(),
        [0x8f, 0xd6, 0xdc, 0xff]
    );
    assert_eq!(
        spec.palette.minimap_bezel_inner.rgba(),
        [0x2d, 0x55, 0x5f, 0xff]
    );
    assert_eq!(
        spec.palette.minimap_tick_muted.rgba(),
        [0x5a, 0x7d, 0x85, 0xff]
    );
    assert_eq!(spec.palette.neutral_text.rgba(), [0xee, 0xf4, 0xf7, 0xff]);
    assert_eq!(spec.palette.neutral_muted.rgba(), [0xa7, 0xb3, 0xbe, 0xff]);
    assert_eq!(spec.palette.cyan_live.rgba(), [0x34, 0xd7, 0xe6, 0xff]);
    assert_eq!(spec.palette.cyan_live.rgb(), [0x34, 0xd7, 0xe6]);
    assert_eq!(spec.palette.amber_warning.rgba(), [0xf2, 0xb8, 0x4b, 0xff]);
    assert_eq!(spec.palette.amber_warning.rgb(), [0xf2, 0xb8, 0x4b]);
}

#[test]
fn filter_actions_expose_full_game_terms_and_compact_overlay_labels() {
    let expected = [
        (FilterAction::FastTravel, "참수리 상", "이동"),
        (FilterAction::Boss, "필드 보스", "보스"),
        (FilterAction::Wanted, "지명수배", "현상"),
        (FilterAction::Dungeon, "던전", "던전"),
        (FilterAction::Tower, "탑", "탑"),
        (FilterAction::Egg, "팰의 알", "알"),
        (FilterAction::Resources, "자원", "자원"),
        (FilterAction::Salvage, "인양", "인양"),
    ];

    for (action, full, compact) in expected {
        assert_eq!(action.label_ko(), full);
        assert_eq!(action.compact_label_ko(), compact);
        assert!(action.imgui_label_ko().starts_with(compact));
        assert!(action.imgui_label_ko().contains("##filter_"));
    }
}

#[test]
fn mini_chrome_has_no_rail_and_uses_cyan_only_for_live_state() {
    let plan = build_chrome_plan(
        DisplayMode::MiniMap,
        InputMode::Locked,
        Freshness::Live,
        GateBadge::Approved,
    );
    assert_eq!(plan.status_tone(), StatusTone::Cyan);
    assert!(plan.rail_actions().is_empty());
    assert!(plan.filter_actions().is_empty());
    assert!(!plan.shows_cardinals());
    assert!(!plan.shows_scale_bar());

    for freshness in [Freshness::Delayed, Freshness::Stale, Freshness::Offline] {
        let plan = build_chrome_plan(
            DisplayMode::MiniMap,
            InputMode::Locked,
            freshness,
            GateBadge::Approved,
        );
        assert_eq!(plan.status_tone(), StatusTone::Amber);
    }
}

#[test]
fn development_gate_warning_is_compact_amber_and_cannot_be_suppressed() {
    let plan = build_chrome_plan(
        DisplayMode::ExpandedMap,
        InputMode::PinnedInteractive,
        Freshness::Live,
        GateBadge::NotApproved,
    );
    assert_eq!(plan.status_tone(), StatusTone::Amber);
    assert_eq!(plan.gate_badge(), GateBadge::NotApproved);
    assert_eq!(
        plan.rail_actions(),
        &[
            RailAction::Collapse,
            RailAction::ZoomIn,
            RailAction::ZoomOut,
            RailAction::ToggleRotation,
            RailAction::Lock,
        ]
    );
    assert!(plan.shows_cardinals());
    assert!(plan.shows_scale_bar());
    assert_eq!(
        plan.filter_actions(),
        &[
            FilterAction::FastTravel,
            FilterAction::Boss,
            FilterAction::Wanted,
            FilterAction::Dungeon,
            FilterAction::Tower,
            FilterAction::Egg,
            FilterAction::Resources,
            FilterAction::Salvage,
        ]
    );
}

#[test]
fn only_selected_rail_state_uses_the_amber_action_tone() {
    assert_eq!(rail_button_tone(RailButtonState::Idle), StatusTone::Neutral);
    assert_eq!(
        rail_button_tone(RailButtonState::Hovered),
        StatusTone::Amber
    );
    assert_eq!(
        rail_button_tone(RailButtonState::Pressed),
        StatusTone::Amber
    );
    assert_eq!(rail_button_tone(RailButtonState::Active), StatusTone::Amber);
}

#[test]
fn filter_selection_uses_cyan_while_pointer_feedback_stays_amber() {
    assert_eq!(
        filter_button_tone(RailButtonState::Idle),
        StatusTone::Neutral
    );
    assert_eq!(
        filter_button_tone(RailButtonState::Hovered),
        StatusTone::Amber
    );
    assert_eq!(
        filter_button_tone(RailButtonState::Pressed),
        StatusTone::Amber
    );
    assert_eq!(
        filter_button_tone(RailButtonState::Active),
        StatusTone::Cyan
    );
}

#[test]
fn expanded_chrome_defaults_every_rail_action_to_idle() {
    let plan = build_chrome_plan(
        DisplayMode::ExpandedMap,
        InputMode::PinnedInteractive,
        Freshness::Live,
        GateBadge::Approved,
    );

    for action in plan.rail_actions() {
        assert_eq!(plan.rail_button_state(*action), RailButtonState::Idle);
    }
}

#[test]
fn chrome_records_only_explicit_rail_interaction_state() {
    let plan = build_chrome_plan(
        DisplayMode::ExpandedMap,
        InputMode::PinnedInteractive,
        Freshness::Live,
        GateBadge::Approved,
    )
    .with_rail_button_state(RailAction::ZoomIn, RailButtonState::Hovered);

    assert_eq!(
        plan.rail_button_state(RailAction::ZoomIn),
        RailButtonState::Hovered
    );
    assert_eq!(
        plan.rail_button_state(RailAction::Collapse),
        RailButtonState::Idle
    );
    assert_eq!(
        plan.rail_button_state(RailAction::Lock),
        RailButtonState::Idle
    );
}

#[test]
fn filter_strip_reflects_only_the_core_selected_filters() {
    let filters = PoiFilters {
        fast_travel: true,
        boss: false,
        wanted: true,
        dungeon: false,
        ..PoiFilters::default()
    };
    let plan = build_chrome_plan(
        DisplayMode::ExpandedMap,
        InputMode::PinnedInteractive,
        Freshness::Live,
        GateBadge::Approved,
    )
    .with_filter_selection(&filters);

    assert_eq!(
        plan.filter_button_state(FilterAction::FastTravel),
        RailButtonState::Active
    );
    assert_eq!(
        plan.filter_button_state(FilterAction::Boss),
        RailButtonState::Idle
    );
    assert_eq!(
        plan.filter_button_state(FilterAction::Wanted),
        RailButtonState::Active
    );
    assert_eq!(
        plan.filter_button_state(FilterAction::Dungeon),
        RailButtonState::Idle
    );
    assert_eq!(
        plan.filter_button_state(FilterAction::Tower),
        RailButtonState::Active
    );
    assert_eq!(
        plan.filter_button_state(FilterAction::Egg),
        RailButtonState::Idle
    );
    assert_eq!(
        plan.filter_button_state(FilterAction::Resources),
        RailButtonState::Idle
    );
    assert_eq!(
        plan.filter_button_state(FilterAction::Salvage),
        RailButtonState::Idle
    );
}
