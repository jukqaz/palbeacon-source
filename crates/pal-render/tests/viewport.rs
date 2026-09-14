use pal_domain::RotationMode;
use pal_map_pack_store::{MapPoint, WorldPoint};
use pal_render::{
    ActiveViewport, ExpandedMapViewport, HeadingStatus, InterpolationPolicy, MiniMapViewport,
    PlayerPose, ScreenPoint, ViewportLayout, ViewportMetrics, ViewportValidationError,
    compute_viewport_pose, interpolate_player_pose,
};

const EPSILON: f64 = 1.0e-9;

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= EPSILON,
        "{actual} != {expected}"
    );
}

fn metrics(width: u32, height: u32, base_mpp: f64) -> ViewportMetrics {
    ViewportMetrics::new(width, height, base_mpp).expect("valid viewport metrics")
}

fn player_pose(
    world_x: f64,
    world_y: f64,
    map_x: f64,
    map_y: f64,
    z: f64,
    heading: Option<f32>,
) -> PlayerPose {
    PlayerPose::new(
        WorldPoint::new(world_x, world_y).unwrap(),
        MapPoint::new(map_x, map_y).unwrap(),
        z,
        heading,
    )
    .unwrap()
}

#[test]
fn metrics_and_zoom_accept_exact_bounds_and_reject_invalid_values() {
    assert!(ViewportMetrics::new(1, 1, f64::MIN_POSITIVE).is_ok());
    assert!(ViewportMetrics::new(16_384, 16_384, 1.0).is_ok());
    for (result, expected) in [
        (
            ViewportMetrics::new(0, 1, 1.0),
            ViewportValidationError::OutputWidthOutOfRange,
        ),
        (
            ViewportMetrics::new(1, 0, 1.0),
            ViewportValidationError::OutputHeightOutOfRange,
        ),
        (
            ViewportMetrics::new(16_385, 1, 1.0),
            ViewportValidationError::OutputWidthOutOfRange,
        ),
        (
            ViewportMetrics::new(1, 16_385, 1.0),
            ViewportValidationError::OutputHeightOutOfRange,
        ),
        (
            ViewportMetrics::new(1, 1, 0.0),
            ViewportValidationError::MapPixelsPerScreenPixelInvalid,
        ),
        (
            ViewportMetrics::new(1, 1, f64::NAN),
            ViewportValidationError::MapPixelsPerScreenPixelInvalid,
        ),
        (
            ViewportMetrics::new(1, 1, f64::MAX),
            ViewportValidationError::MapPixelsPerScreenPixelInvalid,
        ),
    ] {
        assert_eq!(result, Err(expected));
    }

    let center = MapPoint::new(100.0, 200.0).unwrap();
    assert!(MiniMapViewport::new(center, 0.5, metrics(320, 320, 1.0)).is_ok());
    assert!(MiniMapViewport::new(center, 4.0, metrics(320, 320, 1.0)).is_ok());
    assert_eq!(
        MiniMapViewport::new(center, 0.499, metrics(320, 320, 1.0)),
        Err(ViewportValidationError::ZoomOutOfRange)
    );
    assert_eq!(
        ExpandedMapViewport::new(center, f32::NAN, metrics(800, 600, 1.0)),
        Err(ViewportValidationError::ZoomOutOfRange)
    );
}

#[test]
fn north_up_and_heading_up_are_exact_for_cardinal_headings() {
    for heading in [0.0_f32, 90.0, 180.0, 270.0] {
        let north = compute_viewport_pose(RotationMode::NorthUp, Some(heading));
        assert_eq!(north.map_rotation_degrees(), 0.0);
        assert_eq!(north.player_rotation_degrees(), heading);
        assert_eq!(north.heading_status(), HeadingStatus::Available);

        let heading_up = compute_viewport_pose(RotationMode::HeadingUp, Some(heading));
        assert_eq!(heading_up.map_rotation_degrees(), -heading);
        assert_eq!(heading_up.player_rotation_degrees(), 0.0);
        assert_eq!(heading_up.heading_status(), HeadingStatus::Available);
    }
}

#[test]
fn unavailable_or_nonfinite_heading_falls_back_to_north_up() {
    for heading in [None, Some(f32::NAN), Some(f32::INFINITY)] {
        let pose = compute_viewport_pose(RotationMode::HeadingUp, heading);
        assert_eq!(pose.map_rotation_degrees(), 0.0);
        assert_eq!(pose.player_rotation_degrees(), 0.0);
        assert_eq!(pose.heading_status(), HeadingStatus::Unavailable);
    }
}

#[test]
fn player_pose_rejects_nonfinite_z_and_heading() {
    let world = WorldPoint::new(0.0, 0.0).unwrap();
    let map = MapPoint::new(0.0, 0.0).unwrap();
    assert_eq!(
        PlayerPose::new(world, map, f64::NAN, None),
        Err(ViewportValidationError::PlayerZNonFinite)
    );
    assert_eq!(
        PlayerPose::new(world, map, 0.0, Some(f32::INFINITY)),
        Err(ViewportValidationError::PlayerHeadingNonFinite)
    );
}

#[test]
fn shortest_heading_and_coordinates_interpolate_with_clamped_progress() {
    let from = player_pose(0.0, 10.0, 100.0, 200.0, 1.0, Some(359.0));
    let to = player_pose(100.0, 30.0, 300.0, 600.0, 11.0, Some(1.0));
    for (t, expected_heading) in [
        (0.0, 359.0),
        (0.25, 359.5),
        (0.5, 0.0),
        (0.75, 0.5),
        (1.0, 1.0),
    ] {
        let pose = interpolate_player_pose(from, to, t, InterpolationPolicy::Interpolate);
        assert_close(pose.world().x(), 100.0 * f64::from(t));
        assert_close(pose.world().y(), 10.0 + 20.0 * f64::from(t));
        assert_close(pose.map().x(), 100.0 + 200.0 * f64::from(t));
        assert_close(pose.map().y(), 200.0 + 400.0 * f64::from(t));
        assert_close(pose.z(), 1.0 + 10.0 * f64::from(t));
        assert_eq!(pose.heading_degrees(), Some(expected_heading));
    }

    assert_eq!(
        interpolate_player_pose(from, to, -1.0, InterpolationPolicy::Interpolate).world(),
        from.world()
    );
    assert_eq!(
        interpolate_player_pose(from, to, 2.0, InterpolationPolicy::Interpolate).world(),
        to.world()
    );
    assert_eq!(
        interpolate_player_pose(from, to, f32::NAN, InterpolationPolicy::Interpolate),
        to
    );
}

#[test]
fn freeze_returns_target_exactly_and_missing_endpoint_heading_uses_target() {
    let from = player_pose(0.0, 0.0, 0.0, 0.0, 0.0, Some(90.0));
    let to_without_heading = player_pose(10.0, 20.0, 30.0, 40.0, 50.0, None);
    for t in [f32::NEG_INFINITY, -1.0, 0.5, 2.0, f32::NAN] {
        assert_eq!(
            interpolate_player_pose(from, to_without_heading, t, InterpolationPolicy::Freeze),
            to_without_heading
        );
        assert_eq!(
            interpolate_player_pose(
                from,
                to_without_heading,
                t,
                InterpolationPolicy::Interpolate
            )
            .heading_degrees(),
            None
        );
    }

    let from_without_heading = player_pose(0.0, 0.0, 0.0, 0.0, 0.0, None);
    let target = player_pose(10.0, 20.0, 30.0, 40.0, 50.0, Some(45.0));
    assert_eq!(
        interpolate_player_pose(
            from_without_heading,
            target,
            0.0,
            InterpolationPolicy::Interpolate
        )
        .heading_degrees(),
        Some(45.0)
    );
}

#[test]
fn non_square_camera_round_trips_at_right_angle_rotation() {
    let viewport = ExpandedMapViewport::new(
        MapPoint::new(500.0, 400.0).unwrap(),
        2.0,
        metrics(800, 400, 4.0),
    )
    .unwrap();
    let map = MapPoint::new(620.0, 310.0).unwrap();
    let screen = viewport.map_to_screen(map, 90.0).unwrap();
    let round_trip = viewport.screen_to_map(screen, 90.0).unwrap();
    assert_close(round_trip.x(), map.x());
    assert_close(round_trip.y(), map.y());

    let center_screen = viewport.map_to_screen(viewport.center(), -37.0).unwrap();
    assert_eq!(center_screen, ScreenPoint::new(400.0, 200.0).unwrap());
}

#[test]
fn broad_phase_bounds_clamp_to_map_and_return_none_when_fully_outside() {
    let map_size = (1024_u32, 1024_u32);
    let clamped = ExpandedMapViewport::new(
        MapPoint::new(10.0, 10.0).unwrap(),
        1.0,
        metrics(200, 100, 2.0),
    )
    .unwrap()
    .broad_phase_bounds(map_size.0, map_size.1, 0.0)
    .unwrap();
    assert_eq!(clamped.min_x(), 0.0);
    assert_eq!(clamped.min_y(), 0.0);
    assert_eq!(clamped.max_x(), 210.0);
    assert_eq!(clamped.max_y(), 110.0);

    let outside = MiniMapViewport::new(
        MapPoint::new(-500.0, -500.0).unwrap(),
        1.0,
        metrics(100, 100, 1.0),
    )
    .unwrap();
    assert_eq!(outside.broad_phase_bounds(map_size.0, map_size.1), None);
}

#[test]
fn expanded_broad_phase_saturates_overflowing_corner_projection() {
    let huge_scale = f64::MAX / 2.0;
    let huge_center = f64::MAX / 2.0;
    let viewport = ExpandedMapViewport::new(
        MapPoint::new(huge_center, huge_center).unwrap(),
        0.5,
        metrics(1, 1, huge_scale),
    )
    .unwrap();

    assert_eq!(
        viewport.screen_to_map(ScreenPoint::new(1.0, 1.0).unwrap(), 45.0),
        Err(ViewportValidationError::ProjectionNonFinite),
        "the public inverse projection must keep rejecting non-finite results"
    );

    let bounds = viewport
        .broad_phase_bounds(1024, 1024, 45.0)
        .expect("the finite map lies inside the enormous rotated viewport");

    assert_eq!(bounds.min_x(), 0.0);
    assert_eq!(bounds.min_y(), 0.0);
    assert_eq!(bounds.max_x(), 1024.0);
    assert_eq!(bounds.max_y(), 1024.0);
}

#[test]
fn layout_keeps_mini_and_expanded_metrics_independent() {
    let mini = metrics(320, 320, 1.0);
    let expanded = metrics(1280, 720, 2.0);
    let layout = ViewportLayout::new(mini, expanded);
    assert_eq!(layout.mini_metrics(), mini);
    assert_eq!(layout.expanded_metrics(), expanded);

    let center = MapPoint::new(100.0, 100.0).unwrap();
    let mini_active = ActiveViewport::Mini(MiniMapViewport::new(center, 1.0, mini).unwrap());
    let expanded_active =
        ActiveViewport::Expanded(ExpandedMapViewport::new(center, 3.0, expanded).unwrap());
    assert_eq!(mini_active.zoom(), 1.0);
    assert_eq!(expanded_active.zoom(), 3.0);
}
