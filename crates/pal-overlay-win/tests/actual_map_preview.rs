#![cfg(feature = "test-harness")]

use std::io::Cursor;
use std::sync::Arc;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use pal_domain::PoiFilters;
use pal_overlay_win::actual_map_preview::{
    ActualMapSurface, BmpDecodeError, CpuMapSurface, MapFileError, MapPoi, MapPoiIcon, MapPoiKind,
    MapRaster, MapView, WorldToImageTransform, WorldToImageTransformError,
    authoritative_main_map_world_to_image,
};
use pal_overlay_win::{PREVIEW_COLOR_KEY, PhysicalSize};

#[test]
fn decodes_a_bottom_up_24_bit_bmp_without_flipping_colors() {
    let bytes = bmp_2x2([0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0x00ff_ffff]);

    let map = MapRaster::decode_bmp(&bytes).expect("valid BMP");

    assert_eq!((map.width(), map.height()), (2, 2));
    assert_eq!(map.pixel(0, 0), Some(0x00ff_0000));
    assert_eq!(map.pixel(1, 0), Some(0x0000_ff00));
    assert_eq!(map.pixel(0, 1), Some(0x0000_00ff));
    assert_eq!(map.pixel(1, 1), Some(0x00ff_ffff));
}

#[test]
fn rejects_unsupported_or_truncated_bmp_data() {
    assert_eq!(
        MapRaster::decode_bmp(b"not a bitmap"),
        Err(BmpDecodeError::Truncated)
    );

    let mut compressed = bmp_2x2([1, 2, 3, 4]);
    compressed[30..34].copy_from_slice(&1_u32.to_le_bytes());
    assert_eq!(
        MapRaster::decode_bmp(&compressed),
        Err(BmpDecodeError::UnsupportedCompression)
    );
}

#[test]
fn file_loader_rejects_oversized_input_before_reading_it() {
    let path = std::env::temp_dir().join(format!(
        "pal-companion-oversized-map-{}.bmp",
        std::process::id()
    ));
    let file = std::fs::File::create(&path).expect("create sparse fixture");
    file.set_len(64 * 1_024 * 1_024 + 1)
        .expect("set sparse fixture length");

    let result = MapRaster::load_bmp_file(&path);
    let _ = std::fs::remove_file(path);

    assert_eq!(result, Err(MapFileError::FileTooLarge));
}

#[test]
fn bmp_decoder_rejects_dimensions_above_the_preview_budget() {
    let mut bytes = bmp_2x2([1, 2, 3, 4]);
    bytes[18..22].copy_from_slice(&4_097_i32.to_le_bytes());

    assert_eq!(
        MapRaster::decode_bmp(&bytes),
        Err(BmpDecodeError::Raster(
            pal_overlay_win::actual_map_preview::MapRasterError::DimensionsOutOfRange
        ))
    );
}

#[test]
fn authoritative_main_map_transform_maps_exact_bounds_and_center() {
    let transform = authoritative_main_map_world_to_image();
    let exact_points = [
        (349_400.0, -724_400.0, 0.0, 0.0),
        (349_400.0, 724_400.0, 1.0, 0.0),
        (-1_099_400.0, -724_400.0, 0.0, 1.0),
        (-1_099_400.0, 724_400.0, 1.0, 1.0),
        (-375_000.0, 0.0, 0.5, 0.5),
    ];

    for (world_x, world_y, expected_x, expected_y) in exact_points {
        let projected = transform.project(world_x, world_y);
        assert_close(projected.x(), expected_x);
        assert_close(projected.y(), expected_y);
    }
}

#[test]
fn exact_world_bounds_accept_edges_and_reject_other_regions() {
    let transform = authoritative_main_map_world_to_image();

    for (world_x, world_y) in [
        (349_400.0, -724_400.0),
        (349_400.0, 724_400.0),
        (-1_099_400.0, -724_400.0),
        (-1_099_400.0, 724_400.0),
        (-375_000.0, 0.0),
    ] {
        assert!(transform.project_within_bounds(world_x, world_y).is_some());
    }

    for (world_x, world_y) in [
        (349_400.000_001, 0.0),
        (-1_099_400.000_001, 0.0),
        (0.0, -724_400.000_001),
        (0.0, 724_400.000_001),
        (f64::NAN, 0.0),
        (0.0, f64::INFINITY),
    ] {
        assert_eq!(transform.project_within_bounds(world_x, world_y), None);
    }
}

#[test]
fn world_bounds_transform_rejects_non_finite_or_degenerate_bounds() {
    assert_eq!(
        WorldToImageTransform::from_world_bounds(f64::NAN, 0.0, 1.0, 1.0),
        Err(WorldToImageTransformError::NonFiniteBounds)
    );
    assert_eq!(
        WorldToImageTransform::from_world_bounds(1.0, 0.0, 1.0, 1.0),
        Err(WorldToImageTransformError::DegenerateBounds)
    );
}

#[test]
fn validated_affine_map_pixel_matrix_projects_into_normalized_coordinates() {
    let matrix = [[2.0, 3.0, 10.0], [-4.0, 5.0, 20.0]];
    let transform =
        WorldToImageTransform::from_affine_map_pixels(-10.0, -20.0, 30.0, 40.0, 200, 100, matrix)
            .expect("valid fitted affine transform");

    let projected = transform.project(7.0, 11.0);
    assert_close(projected.x(), 57.0 / 200.0);
    assert_close(projected.y(), 47.0 / 100.0);
    assert_eq!(transform.project_within_bounds(31.0, 11.0), None);
}

#[test]
fn affine_map_pixel_transform_rejects_invalid_numeric_boundaries() {
    let valid = [[2.0, 3.0, 10.0], [-4.0, 5.0, 20.0]];
    assert_eq!(
        WorldToImageTransform::from_affine_map_pixels(-10.0, -20.0, 30.0, 40.0, 0, 100, valid),
        Err(WorldToImageTransformError::InvalidMapDimensions)
    );
    assert_eq!(
        WorldToImageTransform::from_affine_map_pixels(
            -10.0,
            -20.0,
            30.0,
            40.0,
            200,
            100,
            [[f64::NAN, 0.0, 0.0], [0.0, 1.0, 0.0]]
        ),
        Err(WorldToImageTransformError::NonFiniteMatrix)
    );
    assert_eq!(
        WorldToImageTransform::from_affine_map_pixels(
            -10.0,
            -20.0,
            30.0,
            40.0,
            200,
            100,
            [[1.0, 2.0, 0.0], [2.0, 4.0, 0.0]]
        ),
        Err(WorldToImageTransformError::SingularMatrix)
    );
}

#[test]
fn real_map_surface_crops_rotates_and_preserves_source_colors_at_corners() {
    let map = coordinate_map(7, 7);
    let mut surface = CpuMapSurface::new(PhysicalSize::new(5, 5)).expect("surface");
    let view = MapView::new(3.0, 3.0, 1.0, -90.0).expect("view");

    surface.rasterize(&map, view);

    assert_ne!(surface.pixel(0, 0), Some(PREVIEW_COLOR_KEY));
    assert_ne!(surface.pixel(4, 4), Some(PREVIEW_COLOR_KEY));
    // A -90 degree map rotation means the screen-top sample comes from east of the center.
    assert_eq!(surface.pixel(2, 1), map.pixel(4, 3));
    assert_ne!(surface.pixel(2, 2), Some(0));
}

#[test]
fn map_sampling_smooths_fractional_positions_instead_of_pixelating_them() {
    let map = MapRaster::new(
        2,
        2,
        vec![0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0x00ff_ffff],
    )
    .expect("map");
    let mut surface = CpuMapSurface::new(PhysicalSize::new(1, 1)).expect("surface");

    surface.rasterize(
        &map,
        MapView::new(0.5, 0.5, 1.0, 0.0).expect("fractional view"),
    );

    assert_eq!(surface.pixel(0, 0), Some(0x0080_8080));
}

#[test]
fn minimap_surface_is_288_square_without_a_circular_pixel_mask() {
    let map = coordinate_map(512, 512);
    let mut surface = CpuMapSurface::new(PhysicalSize::new(288, 288)).expect("surface");
    let view = MapView::new(256.0, 256.0, 1.0, 0.0).expect("view");

    surface.rasterize(&map, view);

    assert_eq!(surface.size(), PhysicalSize::new(288, 288));
    assert_ne!(surface.pixel(0, 0), Some(PREVIEW_COLOR_KEY));
    assert_ne!(surface.pixel(287, 287), Some(PREVIEW_COLOR_KEY));
}

#[test]
fn expanded_surface_rasterizes_only_the_760_by_520_map_viewport() {
    let surface = CpuMapSurface::new(PhysicalSize::new(760, 520)).expect("surface");

    assert_eq!(surface.size(), PhysicalSize::new(760, 520));
    assert_eq!(surface.pixels().len(), 760 * 520);
}

#[test]
fn rasterization_reuses_the_preallocated_pixel_buffer() {
    let map = coordinate_map(32, 32);
    let mut surface = CpuMapSurface::new(PhysicalSize::new(24, 16)).expect("surface");
    let capacity = surface.pixel_capacity();
    let pointer = surface.pixels().as_ptr();

    surface.rasterize(
        &map,
        MapView::new(16.0, 16.0, 1.0, 0.0).expect("first view"),
    );
    surface.rasterize(
        &map,
        MapView::new(17.0, 16.0, 1.0, 15.0).expect("second view"),
    );

    assert_eq!(surface.pixel_capacity(), capacity);
    assert_eq!(surface.pixels().as_ptr(), pointer);
}

#[test]
fn exact_black_source_pixels_are_preserved() {
    let map = MapRaster::new(4, 4, vec![0; 16]).expect("black map");
    let mut surface = CpuMapSurface::new(PhysicalSize::new(4, 4)).expect("surface");

    surface.rasterize(
        &map,
        MapView::new(1.5, 1.5, 1.0, 0.0).expect("centered view"),
    );

    assert_eq!(surface.pixel(1, 1), Some(0x0000_0000));
    assert_eq!(surface.pixel(2, 2), Some(0x0000_0000));
}

#[test]
fn resize_changes_rectangular_storage_but_preserves_performance_counters() {
    let map = coordinate_map(16, 16);
    let mut surface = CpuMapSurface::new(PhysicalSize::new(8, 8)).expect("surface");
    surface.rasterize(&map, MapView::new(8.0, 8.0, 1.0, 0.0).expect("first view"));
    let counters = surface.performance_counters();

    assert_eq!(
        surface.resize(PhysicalSize::new(12, 6)),
        Ok(true),
        "a changed viewport must resize"
    );
    assert_eq!(surface.size(), PhysicalSize::new(12, 6));
    assert_eq!(surface.pixels().len(), 72);
    assert_eq!(surface.performance_counters(), counters);
    assert_eq!(surface.resize(PhysicalSize::new(12, 6)), Ok(false));
}

#[test]
fn rasterization_performance_counters() {
    let map = coordinate_map(7, 7);
    let mut surface = ActualMapSurface::new(5).expect("surface");
    let view = MapView::new(3.0, 3.0, 1.0, 0.0).expect("view");

    let initial = surface.performance_counters();
    assert_eq!(initial.rasterized_frames, 0);
    assert_eq!(initial.raster_cpu_timing_samples, 0);
    assert_eq!(initial.raster_cpu_timing_failures, 0);

    surface.rasterize(&map, view);

    let counters = surface.performance_counters();
    assert_eq!(counters.rasterized_frames, 1);
    assert_eq!(
        counters
            .raster_cpu_timing_samples
            .saturating_add(counters.raster_cpu_timing_failures),
        1
    );
    if counters.raster_cpu_timing_samples == 1 {
        assert_eq!(
            counters.raster_cpu_100ns_total,
            counters.last_raster_cpu_100ns
        );
    }
}

#[test]
fn player_arrow_rotation_points_right_at_ninety_degrees() {
    let map = coordinate_map(7, 7);
    let mut surface = ActualMapSurface::new(65).expect("surface");
    let view = MapView::with_rotations(3.0, 3.0, 1.0, 0.0, 90.0).expect("rotated player view");

    surface.rasterize(&map, view);

    assert_eq!(surface.pixel(44, 32), Some(0x0034_d7e6));
    assert_ne!(surface.pixel(32, 18), Some(0x0034_d7e6));
    assert_ne!(
        surface.pixel(52, 32),
        Some(0x0034_d7e6),
        "the player marker should be a compact compass needle, not a long rocket"
    );
    assert!(
        surface
            .pixels()
            .iter()
            .filter(|pixel| **pixel == 0x0034_d7e6)
            .count()
            >= 60,
        "the cyan forward needle must remain large enough to read over a moving map"
    );
    assert!(
        surface
            .pixels()
            .iter()
            .filter(|pixel| **pixel == 0x00f4_fbff)
            .count()
            >= 6,
        "the player marker needs a high-contrast body between its colored halves"
    );
    assert!(
        surface
            .pixels()
            .iter()
            .filter(|pixel| **pixel == 0x008f_e4e8)
            .count()
            >= 10,
        "the cool rear needle must make forward and backward orientation unambiguous"
    );
}

#[test]
fn searched_view_can_render_a_live_player_away_from_the_search_target() {
    let background = 0x0012_3456;
    let map = MapRaster::new(65, 65, vec![background; 65 * 65]).expect("map");
    let mut surface = ActualMapSurface::new(65).expect("surface");
    let view = MapView::search_focus(32.0, 32.0, 1.0)
        .expect("search view")
        .with_player_map_pose(42.0, 32.0, 90.0)
        .expect("live player marker");

    surface.rasterize(&map, view);

    assert_eq!(view.center_x(), 32.0);
    assert_eq!(view.center_y(), 32.0);
    assert_eq!(view.player_map_pose(), Some((42.0, 32.0, 90.0)));
    assert!(
        surface.pixels().contains(&0x0034_d7e6),
        "the live player marker must remain visible while the expanded map is focused elsewhere"
    );
}

#[test]
fn player_marker_uses_a_compact_navigation_footprint() {
    let background = 0x0012_3456;
    let map = MapRaster::new(65, 65, vec![background; 65 * 65]).expect("map");
    let mut surface = ActualMapSurface::new(65).expect("surface");
    let view = MapView::with_rotations(32.0, 32.0, 1.0, 0.0, 0.0).expect("view");

    surface.rasterize(&map, view);

    let colored_pixels = surface
        .pixels()
        .iter()
        .enumerate()
        .filter(|(_, pixel)| matches!(**pixel, 0x0034_d7e6 | 0x008f_e4e8 | 0x00f4_fbff))
        .map(|(index, _)| ((index % 65) as i32, (index / 65) as i32))
        .collect::<Vec<_>>();
    let min_x = colored_pixels.iter().map(|(x, _)| *x).min().expect("x");
    let max_x = colored_pixels.iter().map(|(x, _)| *x).max().expect("x");
    let min_y = colored_pixels.iter().map(|(_, y)| *y).min().expect("y");
    let max_y = colored_pixels.iter().map(|(_, y)| *y).max().expect("y");

    assert!(
        max_x - min_x <= 16,
        "marker body must stay narrow enough to preserve nearby POIs"
    );
    assert!(
        max_y - min_y <= 25,
        "marker body must stay short enough to avoid a rocket silhouette"
    );
    assert!(
        min_y < 20,
        "the forward needle must visibly extend north of the center puck"
    );
    assert!(
        max_y > 36,
        "the rear needle must remain visible below the center puck"
    );
}

#[test]
fn exact_game_icon_uses_the_same_readable_screen_footprint_as_the_map_ui() {
    let background = 0x0012_3456;
    let map = MapRaster::new(96, 96, vec![background; 96 * 96]).expect("map");
    let view = MapView::new(48.0, 48.0, 1.0, 0.0).expect("view");
    let icon = Arc::new(
        MapPoiIcon::decode_png(include_bytes!(
            "../../../assets/palbeacon/game/map/icons/compass-fast-travel.png"
        ))
        .expect("bundled exact game UI icon"),
    );
    let marker = MapPoi::new(28.0, 28.0, MapPoiKind::FastTravel)
        .expect("fast travel marker")
        .with_icon(icon);
    let mut surface = ActualMapSurface::new(96).expect("surface");

    surface.rasterize_with_pois(
        &map,
        view,
        &[marker],
        &PoiFilters {
            fast_travel: true,
            boss: false,
            wanted: false,
            dungeon: false,
            ..PoiFilters::default()
        },
    );

    assert!(
        changed_pixels_in_region(&surface, 15, 15, 41, 41, background) >= 240,
        "the exact game icon and its compact contrast plate must not collapse into a tiny glyph"
    );
}

#[test]
fn boss_without_a_verified_portrait_uses_a_neutral_plate_and_small_danger_accent() {
    let background = 0x0012_3456;
    let map = MapRaster::new(96, 96, vec![background; 96 * 96]).expect("map");
    let view = MapView::new(48.0, 48.0, 1.0, 0.0).expect("view");
    let marker = MapPoi::new(28.0, 28.0, MapPoiKind::Boss).expect("boss marker");
    let mut surface = ActualMapSurface::new(96).expect("surface");

    surface.rasterize_with_pois(
        &map,
        view,
        &[marker],
        &PoiFilters {
            fast_travel: false,
            boss: true,
            wanted: false,
            dungeon: false,
            ..PoiFilters::default()
        },
    );

    assert_eq!(surface.pixel(28, 28), Some(0x0014_2830));
    assert!(surface.pixels().contains(&0x00d7_e8e5));
    let danger_pixels = surface
        .pixels()
        .iter()
        .filter(|pixel| **pixel == 0x00d9_4052)
        .count();
    assert!(
        (1..=24).contains(&danger_pixels),
        "boss danger color must remain a small accent, got {danger_pixels} pixels"
    );
}

#[test]
fn verified_boss_portrait_replaces_the_neutral_plate_without_a_red_disc() {
    let background = 0x0012_3456;
    let map = MapRaster::new(96, 96, vec![background; 96 * 96]).expect("map");
    let view = MapView::new(48.0, 48.0, 1.0, 0.0).expect("view");
    let portrait = Arc::new(MapPoiIcon::decode_png(&square_icon_png(63, 5)).expect("portrait"));
    let marker = MapPoi::new(28.0, 28.0, MapPoiKind::Boss)
        .expect("boss marker")
        .with_icon(portrait);
    let mut surface = ActualMapSurface::new(96).expect("surface");

    surface.rasterize_with_pois(
        &map,
        view,
        &[marker],
        &PoiFilters {
            fast_travel: false,
            boss: true,
            wanted: false,
            dungeon: false,
            ..PoiFilters::default()
        },
    );

    assert_eq!(surface.pixel(28, 28), Some(0x00f5_d237));
    assert!(surface.pixels().contains(&0x00d7_e8e5));
    assert!(
        surface
            .pixels()
            .iter()
            .filter(|pixel| **pixel == 0x00d9_4052)
            .count()
            <= 24
    );
}

#[test]
fn supplemental_markers_stay_on_the_expanded_map() {
    let background = 0x0012_3456;
    let map = MapRaster::new(1_024, 1_024, vec![background; 1_024 * 1_024]).expect("map");
    let view = MapView::new(512.0, 512.0, 1.0, 0.0).expect("view");
    let icon = Arc::new(MapPoiIcon::decode_png(&square_icon_png(63, 8)).expect("icon"));
    let marker = MapPoi::new(412.0, 512.0, MapPoiKind::Supplemental)
        .expect("resource marker")
        .with_supplemental_filter("tower")
        .expect("known layer")
        .with_icon(icon);
    let filters = PoiFilters {
        fast_travel: false,
        boss: false,
        wanted: false,
        dungeon: false,
        enabled_layer_ids: vec!["tower".to_owned()],
        ..PoiFilters::default()
    };
    let mut minimap = CpuMapSurface::new(PhysicalSize::new(360, 360)).expect("minimap");
    let mut expanded = CpuMapSurface::new(PhysicalSize::new(960, 720)).expect("expanded map");

    minimap.rasterize_with_pois(&map, view, std::slice::from_ref(&marker), &filters);
    expanded.rasterize_with_pois(&map, view, &[marker], &filters);

    let minimap_pixels = changed_pixels_in_region(&minimap, 40, 140, 120, 220, background);
    let expanded_pixels = changed_pixels_in_region(&expanded, 320, 300, 440, 420, background);
    assert_eq!(minimap_pixels, 0, "the exact-filter minimap stays clean");
    assert!(
        expanded_pixels > 0,
        "the expanded map retains supplemental layers"
    );
}

#[test]
fn legacy_pal_selection_stays_off_the_exact_filter_minimap() {
    let background = 0x0012_3456;
    let map = MapRaster::new(1_024, 1_024, vec![background; 1_024 * 1_024]).expect("map");
    let view = MapView::new(512.0, 512.0, 1.0, 0.0).expect("view");
    let icon = Arc::new(MapPoiIcon::decode_png(&square_icon_png(63, 8)).expect("Pal icon"));
    let marker = MapPoi::new(412.0, 512.0, MapPoiKind::PalSpawn)
        .expect("pal spawn")
        .with_pal_spawn_filter(Arc::from("Anubis"), false)
        .with_icon(icon);
    let filters = PoiFilters {
        fast_travel: false,
        boss: false,
        wanted: false,
        dungeon: false,
        selected_pal_ids: vec!["Anubis".to_owned()],
        ..PoiFilters::default()
    };
    let mut minimap = CpuMapSurface::new(PhysicalSize::new(360, 360)).expect("minimap");
    let mut expanded = CpuMapSurface::new(PhysicalSize::new(960, 720)).expect("expanded map");

    minimap.rasterize_with_pois(&map, view, std::slice::from_ref(&marker), &filters);
    expanded.rasterize_with_pois(&map, view, &[marker], &filters);

    assert_eq!(
        changed_pixels_in_region(&minimap, 40, 140, 120, 220, background),
        0,
        "legacy pal selections must not leak into the exact-filter minimap"
    );
    assert!(
        changed_pixels_in_region(&expanded, 320, 300, 440, 420, background) > 0,
        "the expanded map retains the exact pal selection"
    );
}

#[test]
fn transparent_source_padding_does_not_shrink_poi_artwork() {
    let background = 0x0012_3456;
    let map = MapRaster::new(96, 96, vec![background; 96 * 96]).expect("map");
    let view = MapView::new(48.0, 48.0, 1.0, 0.0).expect("view");
    let tight_icon =
        Arc::new(MapPoiIcon::decode_png(&square_icon_png(11, 1)).expect("tight synthetic icon"));
    let padded_icon =
        Arc::new(MapPoiIcon::decode_png(&square_icon_png(63, 27)).expect("padded synthetic icon"));
    let tight_marker = MapPoi::new(48.0, 48.0, MapPoiKind::Supplemental)
        .expect("tight marker")
        .with_supplemental_filter("tower")
        .expect("known layer")
        .with_icon(tight_icon);
    let padded_marker = MapPoi::new(48.0, 48.0, MapPoiKind::Supplemental)
        .expect("padded marker")
        .with_supplemental_filter("tower")
        .expect("known layer")
        .with_icon(padded_icon);
    let filters = PoiFilters {
        fast_travel: false,
        boss: false,
        wanted: false,
        dungeon: false,
        enabled_layer_ids: vec!["tower".to_owned()],
        ..PoiFilters::default()
    };
    let mut tight_surface = CpuMapSurface::new(PhysicalSize::new(128, 96)).expect("tight surface");
    let mut padded_surface =
        CpuMapSurface::new(PhysicalSize::new(128, 96)).expect("padded surface");

    tight_surface.rasterize_with_pois(&map, view, &[tight_marker], &filters);
    padded_surface.rasterize_with_pois(&map, view, &[padded_marker], &filters);

    let tight_pixels = changed_pixels_in_region(&tight_surface, 48, 32, 80, 64, background);
    let padded_pixels = changed_pixels_in_region(&padded_surface, 48, 32, 80, 64, background);
    assert!(
        tight_pixels.abs_diff(padded_pixels) <= 12,
        "transparent source padding must not change the on-screen footprint: tight={tight_pixels}, padded={padded_pixels}"
    );
}

#[test]
fn poi_filters_change_only_the_requested_marker_kinds() {
    let map = MapRaster::new(64, 64, vec![0x0012_3456; 64 * 64]).expect("map");
    let view = MapView::new(32.0, 32.0, 1.0, 0.0).expect("view");
    let pois = [
        MapPoi::new(20.0, 20.0, MapPoiKind::FastTravel).expect("fast travel"),
        MapPoi::new(44.0, 20.0, MapPoiKind::Boss).expect("boss"),
        MapPoi::new(20.0, 44.0, MapPoiKind::Dungeon).expect("dungeon"),
        MapPoi::new(44.0, 44.0, MapPoiKind::Wanted).expect("wanted target"),
    ];
    let mut surface = ActualMapSurface::new(64).expect("surface");
    surface.rasterize_with_pois(
        &map,
        view,
        &pois,
        &PoiFilters {
            fast_travel: true,
            boss: false,
            wanted: false,
            dungeon: false,
            ..PoiFilters::default()
        },
    );
    assert!(surface.pixels().contains(&0x0000_d8ef));
    assert!(!surface.pixels().contains(&0x00d9_4052));
    assert_eq!(surface.pixel(6, 44), Some(0x0012_3456));

    surface.rasterize_with_pois(
        &map,
        view,
        &pois,
        &PoiFilters {
            fast_travel: false,
            boss: true,
            wanted: false,
            dungeon: true,
            ..PoiFilters::default()
        },
    );
    assert!(!surface.pixels().contains(&0x0000_d8ef));
    assert!(surface.pixels().contains(&0x0014_2830));
    assert!(surface.pixels().contains(&0x00d9_4052));
    assert_eq!(surface.pixel(20, 44), Some(0x0015_202b));

    surface.rasterize_with_pois(
        &map,
        view,
        &pois,
        &PoiFilters {
            fast_travel: false,
            boss: false,
            wanted: true,
            dungeon: false,
            ..PoiFilters::default()
        },
    );
    assert_eq!(surface.pixel(44, 44), Some(0x00d9_4052));
    assert_eq!(surface.pixel(44, 20), Some(0x0012_3456));
}

#[test]
fn dense_pois_are_decluttered_in_screen_space() {
    let background = 0x0012_3456;
    let map = MapRaster::new(96, 64, vec![background; 96 * 64]).expect("map");
    let view = MapView::new(48.0, 32.0, 1.0, 0.0).expect("view");
    let pois = [
        MapPoi::new(20.0, 20.0, MapPoiKind::Boss).expect("first boss"),
        MapPoi::new(38.0, 20.0, MapPoiKind::Boss).expect("nearby boss"),
    ];
    let mut surface = CpuMapSurface::new(PhysicalSize::new(96, 64)).expect("surface");

    surface.rasterize_with_pois(
        &map,
        view,
        &pois,
        &PoiFilters {
            fast_travel: false,
            boss: true,
            wanted: false,
            dungeon: false,
            ..PoiFilters::default()
        },
    );

    assert_ne!(surface.pixel(20, 20), Some(background));
    assert_eq!(
        surface.pixel(38, 20),
        Some(background),
        "the second marker is hidden instead of becoming an overlapping blob"
    );
}

#[test]
fn circular_minimap_keeps_edge_marker_art_inside_the_visible_bezel() {
    let background = 0x0012_3456;
    let map = MapRaster::new(96, 96, vec![background; 96 * 96]).expect("map");
    let view = MapView::new(48.0, 48.0, 1.0, 0.0).expect("view");
    let edge_boss = MapPoi::new(0.0, 48.0, MapPoiKind::Boss).expect("edge boss");
    let mut surface = ActualMapSurface::new(96).expect("surface");

    surface.rasterize_with_pois(
        &map,
        view,
        &[edge_boss],
        &PoiFilters {
            fast_travel: false,
            boss: true,
            wanted: false,
            dungeon: false,
            ..PoiFilters::default()
        },
    );

    assert_eq!(
        changed_pixels_in_region(&surface, 0, 36, 2, 60, background),
        0,
        "the circular window must not clip marker art at its edge"
    );
    assert!(
        changed_pixels_in_region(&surface, 3, 36, 24, 60, background) > 0,
        "the edge marker is retained inside the visible minimap"
    );
}

#[test]
fn supplemental_filter_mask_switches_game_derived_markers_without_reloading_icons() {
    let background = 0x0012_3456;
    let map = MapRaster::new(96, 96, vec![background; 96 * 96]).expect("map");
    let view = MapView::new(48.0, 48.0, 1.0, 0.0).expect("view");
    let icon = Arc::new(
        MapPoiIcon::decode_png(include_bytes!(
            "../../../assets/palbeacon/game/map/icons/compass-fast-travel.png"
        ))
        .expect("bundled exact game UI icon"),
    );
    let marker = MapPoi::new(24.0, 24.0, MapPoiKind::Supplemental)
        .expect("supplemental marker")
        .with_supplemental_filter("tower")
        .expect("known layer")
        .with_icon(icon);
    let mut surface = CpuMapSurface::new(PhysicalSize::new(128, 96)).expect("surface");

    surface.rasterize_with_pois(
        &map,
        view,
        std::slice::from_ref(&marker),
        &PoiFilters {
            fast_travel: false,
            boss: false,
            wanted: false,
            dungeon: false,
            enabled_layer_ids: vec!["tower".to_owned()],
            ..PoiFilters::default()
        },
    );
    let visible_region = changed_pixels_in_region(&surface, 26, 10, 54, 38, background);
    assert!(visible_region > 0, "enabled supplemental icon must render");

    surface.rasterize_with_pois(
        &map,
        view,
        &[marker],
        &PoiFilters {
            fast_travel: false,
            boss: false,
            wanted: false,
            dungeon: false,
            enabled_layer_ids: vec!["ore-quartz".to_owned()],
            ..PoiFilters::default()
        },
    );
    assert_eq!(
        changed_pixels_in_region(&surface, 26, 10, 54, 38, background),
        0,
        "a disabled supplemental layer must not leave stale pixels"
    );
}

#[test]
fn browse_view_does_not_invent_a_player_marker() {
    let map = coordinate_map(7, 7);
    let mut surface = CpuMapSurface::new(PhysicalSize::new(5, 5)).expect("surface");

    surface.rasterize(&map, MapView::browse(3.0, 3.0, 1.0).expect("browse view"));

    assert_eq!(surface.pixel(2, 2), map.pixel(3, 3));
}

#[test]
fn reviewed_messenger_of_love_layer_has_a_runtime_filter_bit() {
    let marker = MapPoi::new(24.0, 24.0, MapPoiKind::Supplemental)
        .expect("supplemental marker")
        .with_supplemental_filter("messenger-of-love");

    assert!(marker.is_some());
}

#[test]
fn map_drag_uses_the_inverse_surface_transform_and_preserves_the_player_marker() {
    let view = MapView::with_rotations(500.0, 500.0, 2.0, 90.0, 15.0)
        .expect("rotated view")
        .with_player_map_pose(450.0, 550.0, 25.0)
        .expect("player marker");

    let panned = view
        .panned_by_raster_delta(20.0, 10.0, 200, 100, 1_000, 1_000)
        .expect("pan");

    assert_close(panned.center_x(), 480.0);
    assert_close(panned.center_y(), 540.0);
    assert_eq!(panned.player_map_pose(), Some((450.0, 550.0, 25.0)));
}

#[test]
fn map_drag_clamps_the_rotated_viewport_inside_the_map() {
    let view = MapView::new(150.0, 150.0, 1.0, 0.0).expect("view");

    let panned = view
        .panned_by_raster_delta(10_000.0, 10_000.0, 200, 100, 1_000, 800)
        .expect("clamped pan");

    assert_close(panned.center_x(), 100.0);
    assert_close(panned.center_y(), 50.0);
}

fn square_icon_png(canvas_side: u32, inset: u32) -> Vec<u8> {
    let mut image = RgbaImage::from_pixel(canvas_side, canvas_side, Rgba([0, 0, 0, 0]));
    let end = canvas_side - inset;
    for y in inset..end {
        for x in inset..end {
            image.put_pixel(x, y, Rgba([245, 210, 55, 255]));
        }
    }
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .expect("PNG fixture encodes");
    bytes.into_inner()
}

fn changed_pixels_in_region(
    surface: &ActualMapSurface,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
    background: u32,
) -> usize {
    let mut changed = 0;
    for y in top..=bottom {
        for x in left..=right {
            if surface.pixel(x, y).is_some_and(|pixel| pixel != background) {
                changed += 1;
            }
        }
    }
    changed
}

fn coordinate_map(width: u32, height: u32) -> MapRaster {
    let pixels = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| {
                let red = (x + 1) & 0xff;
                let green = (y + 1) & 0xff;
                (red << 16) | (green << 8) | 1
            })
        })
        .collect();
    MapRaster::new(width, height, pixels).expect("valid coordinate map")
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= f64::EPSILON * 4.0,
        "expected {expected}, got {actual}"
    );
}

fn bmp_2x2(top_down_colors: [u32; 4]) -> Vec<u8> {
    const HEADER: usize = 54;
    const ROW_BYTES: usize = 8;
    let mut bytes = vec![0_u8; HEADER + ROW_BYTES * 2];
    let file_size = bytes.len() as u32;
    bytes[0..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&file_size.to_le_bytes());
    bytes[10..14].copy_from_slice(&(HEADER as u32).to_le_bytes());
    bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&2_i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&2_i32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&(ROW_BYTES as u32 * 2).to_le_bytes());

    for (file_row, source_y) in [1_usize, 0].into_iter().enumerate() {
        for x in 0..2 {
            let color = top_down_colors[source_y * 2 + x];
            let offset = HEADER + file_row * ROW_BYTES + x * 3;
            bytes[offset] = (color & 0xff) as u8;
            bytes[offset + 1] = ((color >> 8) & 0xff) as u8;
            bytes[offset + 2] = ((color >> 16) & 0xff) as u8;
        }
    }
    bytes
}
