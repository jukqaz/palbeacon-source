use std::path::PathBuf;

use pal_domain::RotationMode;
use pal_map_pack_store::{MapPackStore, MapPoint, MapRegionPack, PoiFilterMask, PoiKind, TileKey};
use pal_render::{
    ActiveViewport, ExpandedMapViewport, MiniMapViewport, ViewportMetrics, compute_viewport_pose,
    select_lod_level, select_visible_pois_into, select_visible_tiles_into,
};

const BUILD: &str = "24181527";

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("map-pack-valid")
}

fn metrics(width: u32, height: u32, base_mpp: f64) -> ViewportMetrics {
    ViewportMetrics::new(width, height, base_mpp).unwrap()
}

fn point(x: f64, y: f64) -> MapPoint {
    MapPoint::new(x, y).unwrap()
}

fn north_up_pose() -> pal_render::ViewportPose {
    compute_viewport_pose(RotationMode::NorthUp, Some(0.0))
}

fn main_pack(store: &MapPackStore) -> &MapRegionPack {
    store.select_region_pack(0.0, 0.0).unwrap()
}

fn selected_ids<'a>(
    store: &'a MapPackStore,
    handles: &[pal_map_pack_store::PoiHandle],
) -> Vec<&'a str> {
    handles
        .iter()
        .map(|handle| main_pack(store).poi_index().get(*handle).unwrap().id())
        .collect()
}

fn poi_point(store: &MapPackStore, id: &str) -> MapPoint {
    main_pack(store)
        .poi_index()
        .all()
        .iter()
        .find(|poi| poi.id() == id)
        .expect("fixture POI")
        .map()
}

#[test]
fn lod_uses_floor_boundaries_and_clamps_to_available_levels() {
    let store = MapPackStore::open(fixture_root(), BUILD).unwrap();
    let levels = main_pack(&store).tile_index().levels();

    let zoomed_in =
        MiniMapViewport::new(point(0.0, 0.0), 4.0, metrics(1, 1, f64::MIN_POSITIVE)).unwrap();
    let zoomed_out =
        MiniMapViewport::new(point(0.0, 0.0), 0.5, metrics(1, 1, f64::MAX / 4.0)).unwrap();

    assert_eq!(
        select_lod_level(levels, zoomed_in.effective_map_pixels_per_screen_pixel()),
        Some(0)
    );
    assert_eq!(select_lod_level(levels, 1.999), Some(0));
    assert_eq!(select_lod_level(levels, 2.0), Some(1));
    assert_eq!(
        select_lod_level(levels, zoomed_out.effective_map_pixels_per_screen_pixel()),
        Some(1)
    );
}

#[test]
fn minimap_culling_includes_inside_and_boundary_but_excludes_aabb_corner() {
    let store = MapPackStore::open(fixture_root(), BUILD).unwrap();
    let fast = poi_point(&store, "fast-1");
    let dungeon = poi_point(&store, "dungeon-1");
    let delta = fast.x() - dungeon.x();
    let center = point(fast.x() - 1.4 * delta, fast.y() - 0.5 * delta);
    let radius = (0.4_f64.hypot(1.5)) * delta;
    let viewport = ActiveViewport::Mini(
        MiniMapViewport::new(center, 1.0, metrics(200, 200, radius / 100.0)).unwrap(),
    );
    let mut output = Vec::new();

    select_visible_pois_into(
        main_pack(&store).poi_index(),
        viewport,
        north_up_pose(),
        main_pack(&store).region().map_width_px(),
        main_pack(&store).region().map_height_px(),
        PoiFilterMask::all(),
        &mut output,
    );

    assert_eq!(selected_ids(&store, &output), ["fast-1", "dungeon-1"]);
}

#[test]
fn expanded_culling_uses_the_rotated_rectangle_with_inclusive_boundary() {
    let store = MapPackStore::open(fixture_root(), BUILD).unwrap();
    let fast = poi_point(&store, "fast-1");
    let dungeon = poi_point(&store, "dungeon-1");
    let delta = fast.x() - dungeon.x();
    let viewport = ActiveViewport::Expanded(
        ExpandedMapViewport::new(fast, 1.0, metrics(199, 400, 2.0_f64.sqrt() * delta / 100.0))
            .unwrap(),
    );
    let pose = compute_viewport_pose(RotationMode::HeadingUp, Some(315.0));
    let mut output = Vec::new();

    select_visible_pois_into(
        main_pack(&store).poi_index(),
        viewport,
        pose,
        main_pack(&store).region().map_width_px(),
        main_pack(&store).region().map_height_px(),
        PoiFilterMask::all(),
        &mut output,
    );

    assert_eq!(selected_ids(&store, &output), ["fast-1", "boss-1"]);
}

#[test]
fn poi_filters_cover_none_each_pair_and_all_without_reopening_the_pack() {
    let store = MapPackStore::open(fixture_root(), BUILD).unwrap();
    let viewport = ActiveViewport::Expanded(
        ExpandedMapViewport::new(point(512.0, 512.0), 1.0, metrics(1024, 1024, 1.0)).unwrap(),
    );
    let cases: &[(PoiFilterMask, &[&str])] = &[
        (PoiFilterMask::none(), &[]),
        (
            PoiFilterMask::from_kinds([PoiKind::FastTravel]),
            &["fast-1"],
        ),
        (PoiFilterMask::from_kinds([PoiKind::Boss]), &["boss-1"]),
        (
            PoiFilterMask::from_kinds([PoiKind::Dungeon]),
            &["dungeon-1"],
        ),
        (
            PoiFilterMask::from_kinds([PoiKind::FastTravel, PoiKind::Boss]),
            &["fast-1", "boss-1"],
        ),
        (
            PoiFilterMask::from_kinds([PoiKind::FastTravel, PoiKind::Dungeon]),
            &["fast-1", "dungeon-1"],
        ),
        (
            PoiFilterMask::from_kinds([PoiKind::Boss, PoiKind::Dungeon]),
            &["boss-1", "dungeon-1"],
        ),
        (PoiFilterMask::all(), &["fast-1", "boss-1", "dungeon-1"]),
    ];
    let mut output = Vec::new();

    for (filters, expected) in cases {
        select_visible_pois_into(
            main_pack(&store).poi_index(),
            viewport,
            north_up_pose(),
            main_pack(&store).region().map_width_px(),
            main_pack(&store).region().map_height_px(),
            *filters,
            &mut output,
        );
        assert_eq!(selected_ids(&store, &output), *expected);
    }
}

#[test]
fn tile_selection_uses_the_conservative_current_level_aabb() {
    let store = MapPackStore::open(fixture_root(), BUILD).unwrap();
    let viewport = ActiveViewport::Mini(
        MiniMapViewport::new(point(400.0, 400.0), 1.0, metrics(300, 300, 1.0)).unwrap(),
    );
    let mut output = vec![TileKey {
        level: 9,
        y: 9,
        x: 9,
    }];

    select_visible_tiles_into(
        main_pack(&store).tile_index(),
        viewport,
        north_up_pose(),
        main_pack(&store).region().map_width_px(),
        main_pack(&store).region().map_height_px(),
        &mut output,
    );

    assert_eq!(
        output,
        [
            TileKey {
                level: 0,
                y: 0,
                x: 0
            },
            TileKey {
                level: 0,
                y: 0,
                x: 1
            },
            TileKey {
                level: 0,
                y: 1,
                x: 0
            },
            TileKey {
                level: 0,
                y: 1,
                x: 1
            },
        ]
    );
}

#[test]
fn map_edge_clamps_and_fully_outside_viewports_clear_reused_outputs() {
    let store = MapPackStore::open(fixture_root(), BUILD).unwrap();
    let edge = ActiveViewport::Mini(
        MiniMapViewport::new(point(10.0, 10.0), 1.0, metrics(100, 100, 1.0)).unwrap(),
    );
    let outside = ActiveViewport::Mini(
        MiniMapViewport::new(point(-500.0, -500.0), 1.0, metrics(100, 100, 1.0)).unwrap(),
    );
    let mut tiles = Vec::new();
    let mut pois = Vec::new();

    select_visible_tiles_into(
        main_pack(&store).tile_index(),
        edge,
        north_up_pose(),
        main_pack(&store).region().map_width_px(),
        main_pack(&store).region().map_height_px(),
        &mut tiles,
    );
    assert_eq!(
        tiles,
        [TileKey {
            level: 0,
            y: 0,
            x: 0
        }]
    );

    select_visible_tiles_into(
        main_pack(&store).tile_index(),
        outside,
        north_up_pose(),
        main_pack(&store).region().map_width_px(),
        main_pack(&store).region().map_height_px(),
        &mut tiles,
    );
    select_visible_pois_into(
        main_pack(&store).poi_index(),
        outside,
        north_up_pose(),
        main_pack(&store).region().map_width_px(),
        main_pack(&store).region().map_height_px(),
        PoiFilterMask::all(),
        &mut pois,
    );
    assert!(tiles.is_empty());
    assert!(pois.is_empty());
}
