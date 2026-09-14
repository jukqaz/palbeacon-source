use pal_fullscreen_bridge::MAX_FRAME_PIXELS;
use pal_overlay_win::{
    HitShape, PhysicalRect, PhysicalSize, TopLeftLayoutError, top_left_overlay_layouts,
};

#[test]
fn keeps_reference_geometry_at_1600_by_900_and_96_dpi() {
    let client = PhysicalRect::new(-1_000, 40, 1_600, 900);

    let layouts =
        top_left_overlay_layouts(client, 96, 288).expect("reference surfaces fit the client");

    assert_eq!(
        layouts.mini().surface_shape,
        HitShape::RoundedRectangle {
            bounds: PhysicalRect::new(-984, 56, 288, 288),
            corner_radius_px: 144,
        }
    );
    assert_eq!(
        layouts.mini().map_viewport_rect,
        PhysicalRect::new(-984, 56, 288, 288)
    );
    assert_eq!(layouts.mini().control_rail_rect, None);
    assert_eq!(layouts.mini().viewport_size, PhysicalSize::new(288, 288));

    assert_eq!(
        layouts.expanded().surface_shape,
        HitShape::RoundedRectangle {
            bounds: PhysicalRect::new(-904, 94, 1408, 792),
            corner_radius_px: 13,
        }
    );
    assert_eq!(
        layouts.expanded().map_viewport_rect,
        PhysicalRect::new(-834, 94, 1338, 792)
    );
    assert_eq!(
        layouts.expanded().control_rail_rect,
        Some(PhysicalRect::new(-904, 94, 70, 792))
    );
    assert_eq!(
        layouts.expanded().viewport_size,
        PhysicalSize::new(1330, 787)
    );
}

#[test]
fn does_not_apply_monitor_dpi_twice_to_an_explicit_pixel_size() {
    let client = PhysicalRect::new(-1_000, 40, 1_600, 900);

    let layouts =
        top_left_overlay_layouts(client, 144, 288).expect("responsive surfaces fit the client");

    assert_eq!(
        layouts.mini().surface_shape.bounds(),
        PhysicalRect::new(-984, 56, 288, 288)
    );
    assert_eq!(
        layouts.expanded().surface_shape.bounds(),
        PhysicalRect::new(-904, 94, 1408, 792)
    );
    assert_eq!(
        layouts.expanded().control_rail_rect,
        Some(PhysicalRect::new(-904, 94, 70, 792))
    );
}

#[test]
fn shrinks_the_whole_visual_system_for_a_smaller_window() {
    let client = PhysicalRect::new(100, 200, 1_000, 700);

    let layouts =
        top_left_overlay_layouts(client, 144, 420).expect("responsive surfaces fit the client");

    assert_eq!(
        layouts.mini().surface_shape,
        HitShape::RoundedRectangle {
            bounds: PhysicalRect::new(110, 210, 266, 266),
            corner_radius_px: 133,
        }
    );
    assert_eq!(
        layouts.expanded().surface_shape,
        HitShape::RoundedRectangle {
            bounds: PhysicalRect::new(160, 242, 880, 616),
            corner_radius_px: 8,
        }
    );
    assert_eq!(
        layouts.expanded().control_rail_rect,
        Some(PhysicalRect::new(160, 242, 44, 616))
    );
    assert_eq!(
        layouts.expanded().viewport_size,
        PhysicalSize::new(836, 616)
    );
}

#[test]
fn caps_the_minimap_to_a_readable_fraction_of_a_windowed_client() {
    let client = PhysicalRect::new(0, 0, 1_366, 768);

    let layouts =
        top_left_overlay_layouts(client, 144, 420).expect("windowed layout must be supported");

    let mini = layouts.mini().surface_shape.bounds();
    assert_eq!(mini.width, 292);
    assert_eq!(mini.height, 292);
    assert!(mini.width * 100 <= client.width * 29);
    assert!(mini.height * 100 <= client.height * 39);
}

#[test]
fn keeps_the_requested_pixel_size_on_a_large_client() {
    let client = PhysicalRect::new(10, 20, 2_560, 1_440);

    let layouts =
        top_left_overlay_layouts(client, 144, 420).expect("large client supports the preference");

    assert_eq!(layouts.mini().surface_shape.bounds().width, 420);
    assert_eq!(layouts.mini().surface_shape.bounds().height, 420);
    assert_eq!(layouts.expanded().surface_shape.bounds().width, 2_253);
    assert_eq!(layouts.expanded().surface_shape.bounds().height, 1_267);
}

#[test]
fn centers_a_safe_large_surface_and_bounds_transfer_rasters_across_common_clients() {
    for (width, height) in [
        (1_366, 768),
        (1_600, 900),
        (1_920, 1_080),
        (2_560, 1_440),
        (3_440, 1_440),
        (3_840, 2_160),
    ] {
        let client = PhysicalRect::new(-320, 45, width, height);
        let layouts = top_left_overlay_layouts(client, 144, 288)
            .expect("validated desktop geometry must fit");
        let expanded = layouts.expanded();
        let bounds = expanded.surface_shape.bounds();

        assert!(bounds.width * 100 >= client.width * 87);
        assert!(bounds.width * 100 <= client.width * 89);
        assert!(bounds.height * 100 >= client.height * 87);
        assert!(bounds.height * 100 <= client.height * 89);

        let left_margin = i64::from(bounds.left) - i64::from(client.left);
        let top_margin = i64::from(bounds.top) - i64::from(client.top);
        let right_margin = i64::from(client.left) + i64::from(client.width)
            - (i64::from(bounds.left) + i64::from(bounds.width));
        let bottom_margin = i64::from(client.top) + i64::from(client.height)
            - (i64::from(bounds.top) + i64::from(bounds.height));
        assert!((left_margin - right_margin).abs() <= 1);
        assert!((top_margin - bottom_margin).abs() <= 1);

        let raster = expanded.viewport_size;
        let raster_pixels = u64::from(raster.width) * u64::from(raster.height);
        assert!(raster_pixels <= MAX_FRAME_PIXELS as u64);
        assert!(raster.width <= expanded.map_viewport_rect.width);
        assert!(raster.height <= expanded.map_viewport_rect.height);

        let logical_aspect = f64::from(expanded.map_viewport_rect.width)
            / f64::from(expanded.map_viewport_rect.height);
        let raster_aspect = f64::from(raster.width) / f64::from(raster.height);
        assert!((logical_aspect - raster_aspect).abs() < 0.003);
    }
}

#[test]
fn rejects_zero_dpi_instead_of_guessing_a_scale() {
    let client = PhysicalRect::new(10, 20, 1_920, 1_080);

    assert_eq!(
        top_left_overlay_layouts(client, 0, 288),
        Err(TopLeftLayoutError::ZeroDpi)
    );
}

#[test]
fn rejects_requested_pixel_sizes_outside_the_validated_range() {
    let client = PhysicalRect::new(10, 20, 1_920, 1_080);

    assert_eq!(
        top_left_overlay_layouts(client, 96, 179),
        Err(TopLeftLayoutError::MiniSizeOutOfRange)
    );
    assert_eq!(
        top_left_overlay_layouts(client, 96, 641),
        Err(TopLeftLayoutError::MiniSizeOutOfRange)
    );
}

#[test]
fn rejects_a_client_too_small_even_for_the_minimum_responsive_chrome() {
    assert_eq!(
        top_left_overlay_layouts(PhysicalRect::new(0, 0, 260, 180), 96, 288),
        Err(TopLeftLayoutError::InsufficientClientSpace)
    );
}
