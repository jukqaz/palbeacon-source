#![cfg(feature = "test-harness")]

use pal_domain::{DisplayMode, Freshness, InputMode};
use pal_overlay_win::actual_map_preview::CpuMapSurface;
use pal_overlay_win::gdi_correctness_renderer::{
    GdiCorrectnessRenderer, GdiPaintPlan, GdiRendererError, action_dock_rect, estimated_dpi,
};
use pal_overlay_win::renderer::{OverlayRenderer, RendererKind};
use pal_overlay_win::{HitShape, OverlayLayout, PhysicalPoint, PhysicalRect, PhysicalSize};
use pal_render::{
    FilterAction, GateBadge, RailAction, RailButtonState, StatusTone, build_chrome_plan,
};

#[test]
fn gdi_plan_contains_selected_chrome_and_no_debug_bands_or_crosshair() {
    let plan = GdiPaintPlan::from_layout(
        expanded_layout(),
        build_chrome_plan(
            DisplayMode::ExpandedMap,
            InputMode::PinnedInteractive,
            Freshness::Live,
            GateBadge::NotApproved,
        ),
    );

    assert_eq!(plan.renderer_kind(), RendererKind::GdiCorrectnessPreview);
    assert!(plan.has_rounded_border());
    assert!(plan.has_cardinals());
    assert!(plan.has_scale_bar());
    assert!(plan.has_compact_gate_badge());
    assert!(plan.has_control_rail());
    assert_eq!(plan.rail_glyph_count(), 5);
    assert!(plan.has_filter_strip());
    assert_eq!(plan.filter_button_count(), 8);
    assert!(!plan.has_center_crosshair());
    assert!(!plan.has_debug_bands());
}

#[test]
fn expanded_filter_panel_is_compact_and_stays_inside_the_map_viewport() {
    let plan = GdiPaintPlan::from_layout(
        expanded_layout(),
        build_chrome_plan(
            DisplayMode::ExpandedMap,
            InputMode::PinnedInteractive,
            Freshness::Live,
            GateBadge::Approved,
        ),
    );

    assert_eq!(
        plan.filter_panel_rect(),
        Some(PhysicalRect::new(86, 30, 260, 230))
    );
    assert_eq!(
        plan.filter_button_rect(FilterAction::FastTravel),
        Some(PhysicalRect::new(100, 88, 114, 34))
    );
    assert_eq!(
        plan.filter_button_rect(FilterAction::Dungeon),
        Some(PhysicalRect::new(222, 130, 114, 34))
    );
    assert_eq!(
        plan.filter_button_rect(FilterAction::Tower),
        Some(PhysicalRect::new(100, 172, 114, 34))
    );
    assert_eq!(
        plan.filter_button_rect(FilterAction::Salvage),
        Some(PhysicalRect::new(222, 214, 114, 34))
    );
}

#[test]
fn selected_filter_uses_cyan_without_recoloring_idle_filters() {
    let filters = pal_domain::PoiFilters {
        fast_travel: true,
        boss: false,
        ..pal_domain::PoiFilters::default()
    };
    let chrome = build_chrome_plan(
        DisplayMode::ExpandedMap,
        InputMode::PinnedInteractive,
        Freshness::Live,
        GateBadge::Approved,
    )
    .with_filter_selection(&filters);
    let plan = GdiPaintPlan::from_layout(expanded_layout(), chrome);

    assert_eq!(
        plan.filter_button_tone(FilterAction::FastTravel),
        Some(StatusTone::Cyan)
    );
    assert_eq!(
        plan.filter_button_tone(FilterAction::Boss),
        Some(StatusTone::Neutral)
    );
}

#[test]
fn map_controls_move_into_a_compact_bottom_dock() {
    let plan = GdiPaintPlan::from_layout(
        expanded_layout(),
        build_chrome_plan(
            DisplayMode::ExpandedMap,
            InputMode::PinnedInteractive,
            Freshness::Live,
            GateBadge::NotApproved,
        ),
    );

    assert_eq!(
        plan.rail_glyph_center(RailAction::Collapse),
        Some(PhysicalPoint::new(44, 44))
    );
    assert_eq!(
        plan.rail_glyph_center(RailAction::ZoomIn),
        Some(PhysicalPoint::new(148, 508))
    );
    assert_eq!(
        plan.rail_glyph_center(RailAction::ZoomOut),
        Some(PhysicalPoint::new(100, 508))
    );
    assert_eq!(
        plan.rail_glyph_center(RailAction::ToggleRotation),
        Some(PhysicalPoint::new(196, 508))
    );
    assert_eq!(
        plan.rail_glyph_center(RailAction::Lock),
        Some(PhysicalPoint::new(244, 508))
    );
    assert_eq!(
        action_dock_rect(expanded_layout()),
        Some(PhysicalRect::new(80, 488, 184, 40))
    );
}

#[test]
fn rail_glyphs_are_neutral_until_an_explicit_interaction_state_is_supplied() {
    let chrome = build_chrome_plan(
        DisplayMode::ExpandedMap,
        InputMode::PinnedInteractive,
        Freshness::Live,
        GateBadge::NotApproved,
    );
    let idle = GdiPaintPlan::from_layout(expanded_layout(), chrome);

    for action in chrome.rail_actions() {
        assert_eq!(idle.rail_glyph_tone(*action), Some(StatusTone::Neutral));
    }

    let hovered = GdiPaintPlan::from_layout(
        expanded_layout(),
        chrome.with_rail_button_state(RailAction::ZoomIn, RailButtonState::Hovered),
    );
    assert_eq!(
        hovered.rail_glyph_tone(RailAction::ZoomIn),
        Some(StatusTone::Amber)
    );
    assert_eq!(
        hovered.rail_glyph_tone(RailAction::ZoomOut),
        Some(StatusTone::Neutral)
    );
}

#[test]
fn approved_minimap_omits_gate_badge_and_control_rail() {
    let layout = minimap_layout();
    let plan = GdiPaintPlan::from_layout(
        layout,
        build_chrome_plan(
            DisplayMode::MiniMap,
            InputMode::Locked,
            Freshness::Live,
            GateBadge::Approved,
        ),
    );

    assert_eq!(plan.map_viewport_rect(), layout.map_viewport_rect);
    assert!(!plan.has_compact_gate_badge());
    assert!(!plan.has_control_rail());
    assert!(!plan.has_cardinals());
    assert!(!plan.has_scale_bar());
    assert_eq!(plan.rail_glyph_count(), 0);
    assert!(!plan.has_filter_strip());
    assert_eq!(plan.filter_button_count(), 0);
    assert!(plan.has_layered_minimap_bezel());
    assert_eq!(plan.minimap_bezel_layer_count(), 5);
    assert_eq!(plan.expanded_frame_layer_count(), 0);
}

#[test]
fn unapproved_minimap_keeps_only_the_amber_status_dot() {
    let layout = minimap_layout();
    let plan = GdiPaintPlan::from_layout(
        layout,
        build_chrome_plan(
            DisplayMode::MiniMap,
            InputMode::Locked,
            Freshness::Live,
            GateBadge::NotApproved,
        ),
    );

    assert_eq!(plan.status_tone(), StatusTone::Amber);
    assert!(!plan.has_compact_gate_badge());
    assert!(!plan.has_control_rail());
    assert!(!plan.has_cardinals());
    assert!(!plan.has_scale_bar());
    assert!(plan.has_layered_minimap_bezel());
    assert_eq!(plan.minimap_bezel_layer_count(), 5);
    assert_eq!(plan.expanded_frame_layer_count(), 0);
}

#[test]
fn expanded_map_keeps_the_rectangular_border_without_minimap_instrument_ticks() {
    let plan = GdiPaintPlan::from_layout(
        expanded_layout(),
        build_chrome_plan(
            DisplayMode::ExpandedMap,
            InputMode::PinnedInteractive,
            Freshness::Live,
            GateBadge::Approved,
        ),
    );

    assert!(!plan.has_layered_minimap_bezel());
    assert_eq!(plan.minimap_bezel_layer_count(), 0);
    assert_eq!(plan.expanded_frame_layer_count(), 3);
}

#[test]
fn circular_minimap_scales_chrome_from_its_reference_diameter_not_its_radius() {
    assert_eq!(estimated_dpi(minimap_layout()), 96);

    let bounds = PhysicalRect::new(16, 16, 360, 360);
    let large_minimap = OverlayLayout::new(
        PhysicalRect::new(0, 0, 1920, 1080),
        HitShape::RoundedRectangle {
            bounds,
            corner_radius_px: bounds.width / 2,
        },
        bounds,
        None,
        PhysicalSize::new(bounds.width, bounds.height),
    );
    assert_eq!(estimated_dpi(large_minimap), 120);
    assert_eq!(estimated_dpi(expanded_layout()), 96);
}

#[test]
fn renderer_rejects_a_cpu_surface_that_does_not_match_the_layout_viewport() {
    let layout = expanded_layout();
    let surface = CpuMapSurface::new(PhysicalSize::new(288, 288)).expect("mismatched surface");
    // SAFETY: the mismatch must be rejected before the renderer attempts to use the DC.
    let mut renderer =
        unsafe { GdiCorrectnessRenderer::for_paint(std::ptr::null_mut(), layout, 96) };
    let chrome = build_chrome_plan(
        DisplayMode::ExpandedMap,
        InputMode::Locked,
        Freshness::Live,
        GateBadge::Approved,
    );

    assert_eq!(
        renderer.present(&surface, chrome),
        Err(GdiRendererError::SurfaceSizeMismatch {
            expected: PhysicalSize::new(764, 520),
            actual: PhysicalSize::new(288, 288),
        })
    );
}

fn minimap_layout() -> OverlayLayout {
    let bounds = PhysicalRect::new(16, 16, 288, 288);
    OverlayLayout::new(
        PhysicalRect::new(0, 0, 640, 480),
        HitShape::RoundedRectangle {
            bounds,
            corner_radius_px: bounds.width / 2,
        },
        bounds,
        None,
        PhysicalSize::new(288, 288),
    )
}

fn expanded_layout() -> OverlayLayout {
    let bounds = PhysicalRect::new(16, 16, 820, 520);
    OverlayLayout::new(
        PhysicalRect::new(0, 0, 1_000, 700),
        HitShape::RoundedRectangle {
            bounds,
            corner_radius_px: 10,
        },
        PhysicalRect::new(72, 16, 764, 520),
        Some(PhysicalRect::new(16, 16, 56, 520)),
        PhysicalSize::new(764, 520),
    )
}
