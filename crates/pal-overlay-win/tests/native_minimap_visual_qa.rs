#![cfg(all(windows, feature = "approved-map-pack-runtime"))]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use image::RgbImage;
use pal_domain::PoiFilters;
use pal_overlay_win::actual_map_preview::{
    ActualMapSurface, MapPoi, MapPoiIcon, MapPoiKind, MapRaster, MapView,
};
use pal_overlay_win::{OverlayWindowHost, PhysicalRect, PumpOutcome, top_left_overlay_layouts};
use pal_windows::{
    FakeWindowBackend, GameWindowTracker, MonitorId, PhysicalClientRect, TrackedWindow,
    WindowEvent, WindowId, WindowObservation, WindowObservationParts,
};
use serde_json::Value;
use windows_sys::Win32::Graphics::Gdi::UpdateWindow;

const CURRENT_BUILD: &str = "24575825";
const MAP_SIDE: u32 = 2_048;
const TILE_SIDE: u32 = 512;
const TILE_GUTTER: u32 = 2;

#[test]
#[ignore = "manual Win32 visual QA; set PALBEACON_NATIVE_QA_MODE=north-up|heading-up|stale|fast-travel-only"]
fn renders_current_exact_build_minimap_in_the_native_window() {
    let mode = env::var("PALBEACON_NATIVE_QA_MODE").unwrap_or_else(|_| "north-up".to_owned());
    assert!(matches!(
        mode.as_str(),
        "north-up" | "heading-up" | "stale" | "fast-travel-only"
    ));
    let hold_seconds = env::var("PALBEACON_NATIVE_QA_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(30)
        .clamp(1, 120);

    let root = repository_root();
    let map_root = root.join("assets/palbeacon/game/map");
    assert_eq!(
        manifest_build(&root.join("contracts/map/v1/manifest.json")),
        CURRENT_BUILD
    );
    assert_eq!(
        manifest_build(&map_root.join("tile-index.v1.json")),
        CURRENT_BUILD
    );

    let map = load_exact_map(&map_root.join("tiles"));
    let pois = load_exact_pois(&map_root);
    assert_eq!(pois.len(), 432);

    let client = PhysicalRect::new(0, 0, 1_920, 1_080);
    let layouts = top_left_overlay_layouts(client, 96, 320).expect("FHD minimap layout");
    let mut host = OverlayWindowHost::create().expect("native overlay window");
    host.apply_layouts(layouts).expect("native layout");
    host.attach(&tracked_window(client))
        .expect("visible tracked client");
    host.apply_approved_map_surface(
        ActualMapSurface::new(layouts.mini().viewport_size).expect("bounded surface"),
    )
    .expect("approved surface");
    let filters = if mode == "fast-travel-only" {
        PoiFilters {
            fast_travel: true,
            boss: false,
            wanted: false,
            dungeon: false,
            ..PoiFilters::default()
        }
    } else {
        PoiFilters {
            fast_travel: true,
            boss: true,
            wanted: true,
            dungeon: true,
            ..PoiFilters::default()
        }
    };
    host.set_poi_filters(filters.clone())
        .expect("exact POI filters");

    let mut view = match mode.as_str() {
        "north-up" | "fast-travel-only" => {
            MapView::with_rotations(1_024.0, 1_024.0, 2.6, 0.0, 34.0)
        }
        "heading-up" | "stale" => MapView::with_rotations(1_024.0, 1_024.0, 2.6, -34.0, 0.0),
        _ => unreachable!(),
    }
    .expect("finite exact map view");
    if mode == "stale" {
        view = view.without_player_marker();
    }
    assert_eq!(
        host.update_approved_map_surface_with_pois(&map, view, &pois, &filters),
        Ok(true)
    );
    assert_ne!(unsafe { UpdateWindow(host.raw_hwnd()) }, 0);

    let bounds = layouts.mini().surface_shape.bounds();
    println!(
        "READY mode={mode} build={CURRENT_BUILD} hwnd={} rect={},{},{},{} pois={}",
        host.raw_hwnd() as usize,
        bounds.left,
        bounds.top,
        bounds.width,
        bounds.height,
        pois.len()
    );
    let deadline = Instant::now() + Duration::from_secs(hold_seconds);
    while Instant::now() < deadline {
        assert_eq!(host.pump_messages(), Ok(PumpOutcome::Continue));
        thread::sleep(Duration::from_millis(16));
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn manifest_build(path: &Path) -> String {
    let value: Value =
        serde_json::from_slice(&fs::read(path).expect("manifest bytes")).expect("manifest JSON");
    value
        .get("game_build_id")
        .and_then(Value::as_str)
        .expect("game_build_id")
        .to_owned()
}

fn load_exact_map(tile_root: &Path) -> MapRaster {
    let mut pixels = vec![0_u32; (MAP_SIDE * MAP_SIDE) as usize];
    for tile_y in 0..4 {
        for tile_x in 0..4 {
            let tile_path = tile_root.join(format!("{tile_y}_{tile_x}.jpg"));
            let tile = image::open(&tile_path)
                .unwrap_or_else(|error| panic!("decode {}: {error}", tile_path.display()))
                .to_rgb8();
            assert_eq!(
                tile.dimensions(),
                (TILE_SIDE + TILE_GUTTER * 2, TILE_SIDE + TILE_GUTTER * 2)
            );
            copy_tile(&mut pixels, &tile, tile_x, tile_y);
        }
    }
    MapRaster::new(MAP_SIDE, MAP_SIDE, pixels).expect("current exact map raster")
}

fn copy_tile(destination: &mut [u32], tile: &RgbImage, tile_x: u32, tile_y: u32) {
    for y in 0..TILE_SIDE {
        for x in 0..TILE_SIDE {
            let pixel = tile.get_pixel(x + TILE_GUTTER, y + TILE_GUTTER);
            let destination_x = tile_x * TILE_SIDE + x;
            let destination_y = tile_y * TILE_SIDE + y;
            let index = (destination_y * MAP_SIDE + destination_x) as usize;
            destination[index] =
                (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2]);
        }
    }
}

fn load_exact_pois(map_root: &Path) -> Vec<MapPoi> {
    let value: Value =
        serde_json::from_slice(&fs::read(map_root.join("pois.v1.json")).expect("exact POI bytes"))
            .expect("exact POI JSON");
    assert_eq!(
        value.get("game_build_id").and_then(Value::as_str),
        Some(CURRENT_BUILD)
    );

    let fast_travel = Arc::new(
        MapPoiIcon::decode_png(
            &fs::read(map_root.join("icons/compass-fast-travel.png")).expect("fast travel icon"),
        )
        .expect("decoded fast travel icon"),
    );
    let boss = Arc::new(
        MapPoiIcon::decode_webp(
            &fs::read(map_root.join("icons/boss-category.webp")).expect("boss icon"),
        )
        .expect("decoded boss icon"),
    );
    let dungeon = Arc::new(
        MapPoiIcon::decode_png(
            &fs::read(map_root.join("icons/compass-dungeon.png")).expect("dungeon icon"),
        )
        .expect("decoded dungeon icon"),
    );
    let wanted = Arc::new(
        MapPoiIcon::decode_png(
            &fs::read(map_root.join("icons/compass-bounty.png")).expect("wanted icon"),
        )
        .expect("decoded wanted icon"),
    );

    value
        .get("pois")
        .and_then(Value::as_array)
        .expect("POI rows")
        .iter()
        .map(|row| {
            let kind = match row.get("kind").and_then(Value::as_str).expect("POI kind") {
                "fast_travel" => MapPoiKind::FastTravel,
                "boss" => MapPoiKind::Boss,
                "dungeon" => MapPoiKind::Dungeon,
                "wanted" => MapPoiKind::Wanted,
                value => panic!("unsupported exact POI kind: {value}"),
            };
            let icon = match kind {
                MapPoiKind::FastTravel => Arc::clone(&fast_travel),
                MapPoiKind::Boss => Arc::clone(&boss),
                MapPoiKind::Dungeon => Arc::clone(&dungeon),
                MapPoiKind::Wanted => Arc::clone(&wanted),
                _ => unreachable!(),
            };
            MapPoi::new(
                row.get("map_x").and_then(Value::as_f64).expect("map_x"),
                row.get("map_y").and_then(Value::as_f64).expect("map_y"),
                kind,
            )
            .expect("finite exact POI")
            .with_icon(icon)
        })
        .collect()
}

fn tracked_window(client: PhysicalRect) -> TrackedWindow {
    let observation = WindowObservation::for_test(WindowObservationParts {
        id: WindowId::from_raw(7),
        monitor_id: MonitorId::from_raw(9),
        process_id: 42,
        image_path: r"C:\Palworld-Win64-Shipping.exe".to_owned(),
        window_title: "visual QA fixture".to_owned(),
        client_rect: PhysicalClientRect::new(client.left, client.top, client.width, client.height),
        dpi: 96,
        visible: true,
        top_level: true,
        foreground: true,
        minimized: false,
        inspectable: true,
    });
    let mut tracker = GameWindowTracker::new(FakeWindowBackend::with_windows([observation]));
    match tracker.poll().expect("tracking") {
        Some(WindowEvent::Attached(window)) => window,
        event => panic!("expected attached QA window, got {event:?}"),
    }
}
