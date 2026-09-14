use std::cell::RefCell;

use pal_domain::{InputMode, OverlayAction};
use pal_overlay_win::{
    ControlIntent, ControlIntentSink, HitShape, HitTest, InputController, InteractionTarget,
    LayoutHitTarget, OverlayLayout, PhysicalPoint, PhysicalRect, PhysicalSize, ResizeHandle,
    top_left_overlay_layouts,
};
use pal_render::{FilterAction, RailAction};

#[derive(Default)]
struct RecordingSink {
    intents: RefCell<Vec<ControlIntent>>,
}

impl ControlIntentSink for RecordingSink {
    type Error = std::convert::Infallible;

    fn submit(&self, intent: ControlIntent) -> Result<(), Self::Error> {
        self.intents.borrow_mut().push(intent);
        Ok(())
    }
}

fn rect(left: i32, top: i32, width: u32, height: u32) -> PhysicalRect {
    PhysicalRect::new(left, top, width, height)
}

fn point(x: i32, y: i32) -> PhysicalPoint {
    PhysicalPoint::new(x, y)
}

fn expanded_layout() -> OverlayLayout {
    top_left_overlay_layouts(rect(-1_000, 40, 1_600, 900), 96, 288)
        .expect("fixture layouts")
        .expanded()
}

#[test]
fn locked_is_transparent_even_at_the_surface_center() {
    let controller = InputController::new(InputMode::Locked, expanded_layout());

    assert_eq!(
        controller.wm_nchittest(point(-704, 236)),
        HitTest::Transparent
    );
    assert_eq!(
        controller.interaction_target(point(-704, 236)),
        InteractionTarget::None
    );
}

#[test]
fn interactive_rounded_surface_accepts_center_but_not_corner_cutout() {
    let controller = InputController::new(InputMode::PinnedInteractive, expanded_layout());

    assert_eq!(controller.wm_nchittest(point(-704, 236)), HitTest::Client);
    assert_eq!(
        controller.wm_nchittest(point(-983, 57)),
        HitTest::Transparent
    );
}

#[test]
fn expanded_interaction_distinguishes_map_and_control_rail() {
    let layout = expanded_layout();
    let controller = InputController::new(InputMode::PinnedInteractive, layout);
    let collapse =
        pal_overlay_win::gdi_correctness_renderer::rail_action_rect(layout, RailAction::Collapse)
            .expect("collapse control");
    let collapse_center = point(
        collapse.left + i32::try_from(collapse.width).unwrap() / 2,
        collapse.top + i32::try_from(collapse.height).unwrap() / 2,
    );
    let filter = pal_overlay_win::gdi_correctness_renderer::filter_button_rect(
        layout,
        FilterAction::FastTravel,
    )
    .expect("fast travel filter");
    let filter_center = point(
        filter.left + i32::try_from(filter.width).unwrap() / 2,
        filter.top + i32::try_from(filter.height).unwrap() / 2,
    );
    let map_center = point(
        layout.map_viewport_rect.left + i32::try_from(layout.map_viewport_rect.width).unwrap() / 2,
        layout.map_viewport_rect.top + i32::try_from(layout.map_viewport_rect.height).unwrap() / 2,
    );

    assert_eq!(
        controller.interaction_target(map_center),
        InteractionTarget::Map
    );
    assert_eq!(
        controller.interaction_target(collapse_center),
        InteractionTarget::ControlRail
    );
    assert_eq!(
        controller.interaction_target(filter_center),
        InteractionTarget::FilterStrip
    );
}

#[test]
fn map_and_rail_use_half_open_nonoverlapping_rectangles() {
    let layout = expanded_layout();
    let controller = InputController::new(InputMode::TemporaryInteractive, layout);
    let rail = layout.control_rail_rect.expect("expanded rail");
    let map_left = layout.map_viewport_rect.left;

    assert_eq!(
        controller.interaction_target(point(map_left - 1, rail.top + 200)),
        InteractionTarget::ControlRail
    );
    assert_eq!(
        controller.interaction_target(point(map_left, rail.top + 200)),
        InteractionTarget::Map
    );
    assert_eq!(
        controller.interaction_target(point(
            map_left + i32::try_from(layout.map_viewport_rect.width).unwrap(),
            rail.top + 200
        )),
        InteractionTarget::None
    );
}

#[test]
fn rounded_shape_uses_half_open_bounds_and_clamps_oversized_radius() {
    let bounds = rect(10, 20, 40, 20);
    let layout = OverlayLayout::new(
        bounds,
        HitShape::RoundedRectangle {
            bounds,
            corner_radius_px: 999,
        },
        bounds,
        None,
        PhysicalSize::new(40, 20),
    );
    let controller = InputController::new(InputMode::PinnedInteractive, layout);

    assert_eq!(controller.wm_nchittest(point(10, 20)), HitTest::Transparent);
    assert_eq!(controller.wm_nchittest(point(10, 30)), HitTest::Client);
    assert_eq!(controller.wm_nchittest(point(49, 30)), HitTest::Client);
    assert_eq!(controller.wm_nchittest(point(50, 30)), HitTest::Transparent);
    assert_eq!(controller.wm_nchittest(point(30, 40)), HitTest::Transparent);
}

#[test]
fn layout_edit_exposes_move_center_and_resize_handles_on_outer_surface() {
    let layout = expanded_layout();
    let controller = InputController::new(InputMode::LayoutEdit, layout);
    let bounds = layout.surface_shape.bounds();

    assert_eq!(
        controller.layout_hit_target(point(
            bounds.left + i32::try_from(bounds.width).unwrap() / 2,
            bounds.top + i32::try_from(bounds.height).unwrap() / 2,
        )),
        LayoutHitTarget::Move
    );
    assert_eq!(
        controller.layout_hit_target(point(bounds.left + 8, bounds.top + 8)),
        LayoutHitTarget::Resize(ResizeHandle::NorthWest)
    );
    assert_eq!(
        controller.layout_hit_target(point(
            bounds.left + i32::try_from(bounds.width).unwrap() - 8,
            bounds.top + i32::try_from(bounds.height).unwrap() - 8,
        )),
        LayoutHitTarget::Resize(ResizeHandle::SouthEast)
    );
    assert_eq!(
        controller.layout_hit_target(point(bounds.left - 1, bounds.top - 1)),
        LayoutHitTarget::None
    );
}

#[test]
fn unrepresentable_native_rounded_geometry_fails_closed_without_overflow() {
    let bounds = rect(i32::MIN, i32::MIN, u32::MAX, u32::MAX);
    let layout = OverlayLayout::new(
        bounds,
        HitShape::RoundedRectangle {
            bounds,
            corner_radius_px: 18,
        },
        bounds,
        None,
        PhysicalSize::new(u32::MAX, u32::MAX),
    );
    let controller = InputController::new(InputMode::PinnedInteractive, layout);

    assert_eq!(
        controller.wm_nchittest(point(i32::MIN, -1)),
        HitTest::Transparent
    );
}

#[test]
fn locked_mode_emits_no_rail_or_wheel_intents() {
    let mut controller = InputController::new(InputMode::Locked, expanded_layout());
    controller.set_effective_settings_version(7);
    let sink = RecordingSink::default();

    assert!(
        !controller
            .handle_left_click(point(-196, 132), &sink)
            .unwrap()
    );
    assert!(
        !controller
            .handle_wheel(point(-700, 200), 120, &sink)
            .unwrap()
    );
    assert!(sink.intents.borrow().is_empty());
}

#[test]
fn interactive_rail_and_map_wheel_emit_versioned_semantic_intents() {
    let layout = expanded_layout();
    let mut controller = InputController::new(InputMode::PinnedInteractive, layout);
    controller.set_effective_settings_version(7);
    let sink = RecordingSink::default();
    let zoom_in =
        pal_overlay_win::gdi_correctness_renderer::rail_action_rect(layout, RailAction::ZoomIn)
            .expect("zoom in control");
    let zoom_in_center = point(
        zoom_in.left + i32::try_from(zoom_in.width).unwrap() / 2,
        zoom_in.top + i32::try_from(zoom_in.height).unwrap() / 2,
    );
    let map_center = point(
        layout.map_viewport_rect.left + i32::try_from(layout.map_viewport_rect.width).unwrap() / 2,
        layout.map_viewport_rect.top + i32::try_from(layout.map_viewport_rect.height).unwrap() / 2,
    );

    assert!(controller.handle_left_click(zoom_in_center, &sink).unwrap());
    assert!(controller.handle_wheel(map_center, -120, &sink).unwrap());

    assert_eq!(
        sink.intents.borrow().as_slice(),
        &[
            ControlIntent::Action {
                expected_settings_version: 7,
                action: OverlayAction::ZoomIn,
            },
            ControlIntent::Action {
                expected_settings_version: 7,
                action: OverlayAction::ZoomOut,
            },
        ]
    );
}

#[test]
fn expanded_filter_strip_emits_only_the_selected_base_filter_action() {
    let mut controller = InputController::new(InputMode::PinnedInteractive, expanded_layout());
    controller.set_effective_settings_version(11);
    let sink = RecordingSink::default();
    let fast_travel = pal_overlay_win::gdi_correctness_renderer::filter_button_rect(
        expanded_layout(),
        FilterAction::FastTravel,
    )
    .expect("fast travel filter");
    let point = PhysicalPoint::new(
        fast_travel.left + i32::try_from(fast_travel.width).unwrap() / 2,
        fast_travel.top + i32::try_from(fast_travel.height).unwrap() / 2,
    );

    assert!(controller.handle_left_click(point, &sink).unwrap());
    assert!(!controller.handle_wheel(point, 120, &sink).unwrap());
    assert_eq!(
        sink.intents.borrow().as_slice(),
        &[ControlIntent::Action {
            expected_settings_version: 11,
            action: OverlayAction::ToggleFastTravel,
        }]
    );
}

#[test]
fn expanded_filter_strip_emits_a_second_row_resource_action() {
    let mut controller = InputController::new(InputMode::PinnedInteractive, expanded_layout());
    controller.set_effective_settings_version(12);
    let sink = RecordingSink::default();
    let resources = pal_overlay_win::gdi_correctness_renderer::filter_button_rect(
        expanded_layout(),
        FilterAction::Resources,
    )
    .expect("resources filter");
    let point = PhysicalPoint::new(
        resources.left + i32::try_from(resources.width).unwrap() / 2,
        resources.top + i32::try_from(resources.height).unwrap() / 2,
    );

    assert!(controller.handle_left_click(point, &sink).unwrap());
    assert_eq!(
        sink.intents.borrow().as_slice(),
        &[ControlIntent::Action {
            expected_settings_version: 12,
            action: OverlayAction::ToggleResources,
        }]
    );
}

#[test]
fn rail_hit_boxes_do_not_consume_empty_rail_space() {
    let controller = InputController::new(InputMode::PinnedInteractive, expanded_layout());
    let sink = RecordingSink::default();

    assert!(
        !controller
            .handle_left_click(point(-452, 285), &sink)
            .unwrap()
    );
    assert!(sink.intents.borrow().is_empty());
}
