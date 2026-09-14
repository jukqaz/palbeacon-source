use std::path::PathBuf;
use std::sync::Arc;

use pal_domain::{
    CoreState, CoreStateParts, DisplayMode, ExpandedMapView, Freshness, MiniMapView,
    OverlaySettings, PoiFilters, PositionSample, RotationMode, SampleClock,
};
use pal_map_pack_store::{MapPackStore, PoiKind};
use pal_render::{
    RenderCommand, RenderCommandBuffer, SnapshotBuilder, ViewportLayout, ViewportMetrics,
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

fn snapshot(rotation_mode: RotationMode) -> Arc<pal_render::RenderSnapshot> {
    snapshot_with_filters(rotation_mode, PoiFilters::default())
}

fn snapshot_with_filters(
    rotation_mode: RotationMode,
    poi_filters: PoiFilters,
) -> Arc<pal_render::RenderSnapshot> {
    snapshot_with_options(rotation_mode, poi_filters, true, true)
}

fn snapshot_with_options(
    rotation_mode: RotationMode,
    poi_filters: PoiFilters,
    visible: bool,
    include_sample: bool,
) -> Arc<pal_render::RenderSnapshot> {
    snapshot_with_options_at(
        rotation_mode,
        poi_filters,
        visible,
        include_sample,
        400.0,
        -400.0,
    )
}

fn snapshot_with_options_at(
    rotation_mode: RotationMode,
    poi_filters: PoiFilters,
    visible: bool,
    include_sample: bool,
    world_x: f64,
    world_y: f64,
) -> Arc<pal_render::RenderSnapshot> {
    let pack = MapPackStore::open(fixture_root(), BUILD).unwrap();
    let sample = PositionSample::new(
        "test-world",
        b"subject",
        b"boot-render-commands",
        3,
        5,
        world_x,
        world_y,
        25.0,
        Some(135.0),
        SampleClock::received_with_age(0, 1_000),
    )
    .unwrap();
    let state = CoreState::from_parts(CoreStateParts {
        settings: OverlaySettings {
            enabled: visible,
            display_mode: DisplayMode::ExpandedMap,
            rotation_mode,
            poi_filters,
            ..OverlaySettings::default()
        },
        settings_version: 9,
        window_snapshot: None,
        position_sample: include_sample.then_some(sample),
        freshness: if include_sample {
            Freshness::Live
        } else {
            Freshness::Stale
        },
        heading_available: include_sample,
        connected: true,
        visible,
        interpolate_position: include_sample,
        mini_map_view: MiniMapView::new(0.0, 0.0, 1.0).unwrap(),
        expanded_map_view: ExpandedMapView::new(512.0, 512.0, 1.0).unwrap(),
    })
    .unwrap();
    let layout = ViewportLayout::new(
        ViewportMetrics::new(320, 320, 1.0).unwrap(),
        ViewportMetrics::new(1024, 1024, 1.0).unwrap(),
    );
    SnapshotBuilder::new(pack).build(&state, layout).unwrap()
}

#[test]
fn main_and_tree_tile_commands_have_noncolliding_region_bound_identities() {
    let main = snapshot(RotationMode::NorthUp);
    let tree = snapshot_with_options_at(
        RotationMode::NorthUp,
        PoiFilters::default(),
        true,
        true,
        348_000.0,
        -500_000.0,
    );
    let mut main_commands = RenderCommandBuffer::with_capacity(32);
    let mut tree_commands = RenderCommandBuffer::with_capacity(32);
    main_commands.build_from(&main);
    tree_commands.build_from(&tree);

    let main_tile = main_commands
        .commands()
        .iter()
        .find_map(|command| match command {
            RenderCommand::MapTile(tile) => Some(*tile),
            _ => None,
        })
        .expect("MainMap emits a tile");
    let tree_tile = tree_commands
        .commands()
        .iter()
        .find_map(|command| match command {
            RenderCommand::MapTile(tile) if tile.key() == main_tile.key() => Some(*tile),
            _ => None,
        })
        .expect("Tree emits the same local tile coordinate");

    assert_eq!(
        main_tile.identity().pack_hash(),
        tree_tile.identity().pack_hash()
    );
    assert_ne!(
        main_tile.identity().region_index(),
        tree_tile.identity().region_index()
    );
    assert_ne!(main_tile.identity(), tree_tile.identity());
}

#[test]
fn north_up_emits_one_map_transform_raw_tiles_and_a_centered_heading_arrow() {
    let snapshot = snapshot(RotationMode::NorthUp);
    let mut buffer = RenderCommandBuffer::with_capacity(32);

    buffer.build_from(&snapshot);

    assert_eq!(
        buffer
            .commands()
            .iter()
            .filter(|command| matches!(command, RenderCommand::MapTransform(_)))
            .count(),
        1
    );
    let RenderCommand::MapTransform(transform) = buffer.commands()[0] else {
        panic!("the map transform must be the first command");
    };
    assert_eq!(transform.map_rotation_degrees(), 0.0);
    assert_eq!(transform.viewport(), snapshot.active_viewport());

    let tile_keys: Vec<_> = buffer
        .commands()
        .iter()
        .filter_map(|command| match command {
            RenderCommand::MapTile(tile) => Some(tile.key()),
            _ => None,
        })
        .collect();
    assert_eq!(tile_keys, snapshot.visible_tiles());

    let RenderCommand::PlayerArrow(arrow) = buffer.commands().last().unwrap() else {
        panic!("the centered player arrow must be the last command");
    };
    assert_eq!(arrow.screen_position().x(), 512.0);
    assert_eq!(arrow.screen_position().y(), 512.0);
    assert_eq!(arrow.rotation_degrees(), 135.0);
}

#[test]
fn heading_up_rotates_only_the_map_transform_and_keeps_the_player_arrow_up() {
    let snapshot = snapshot(RotationMode::HeadingUp);
    let mut buffer = RenderCommandBuffer::with_capacity(32);

    buffer.build_from(&snapshot);

    let RenderCommand::MapTransform(transform) = buffer.commands()[0] else {
        panic!("the map transform must be the first command");
    };
    assert_eq!(transform.map_rotation_degrees(), -135.0);
    for command in buffer.commands() {
        if let RenderCommand::MapTile(tile) = command {
            assert_eq!(
                tile.map_rect(),
                snapshot
                    .region_pack()
                    .unwrap()
                    .tile_descriptor(tile.key())
                    .unwrap()
                    .map_rect(),
                "tiles stay in their raw map coordinates; only the map transform rotates"
            );
        }
    }
    let RenderCommand::PlayerArrow(arrow) = buffer.commands().last().unwrap() else {
        panic!("the centered player arrow must be the last command");
    };
    assert_eq!(arrow.rotation_degrees(), 0.0);
}

#[test]
fn poi_filter_combinations_emit_exact_stably_ordered_commands() {
    let cases: &[(PoiFilters, &[PoiKind])] = &[
        (
            PoiFilters {
                fast_travel: false,
                boss: false,
                wanted: false,
                dungeon: false,
                enabled_layer_ids: Vec::new(),
                selected_pal_ids: Vec::new(),
                night_only: false,
            },
            &[],
        ),
        (
            PoiFilters {
                fast_travel: true,
                boss: false,
                wanted: false,
                dungeon: false,
                enabled_layer_ids: Vec::new(),
                selected_pal_ids: Vec::new(),
                night_only: false,
            },
            &[PoiKind::FastTravel],
        ),
        (
            PoiFilters {
                fast_travel: false,
                boss: true,
                wanted: false,
                dungeon: false,
                enabled_layer_ids: Vec::new(),
                selected_pal_ids: Vec::new(),
                night_only: false,
            },
            &[PoiKind::Boss],
        ),
        (
            PoiFilters {
                fast_travel: false,
                boss: false,
                wanted: false,
                dungeon: true,
                enabled_layer_ids: Vec::new(),
                selected_pal_ids: Vec::new(),
                night_only: false,
            },
            &[PoiKind::Dungeon],
        ),
        (
            PoiFilters::default(),
            &[PoiKind::FastTravel, PoiKind::Boss, PoiKind::Dungeon],
        ),
    ];

    for (filters, expected_kinds) in cases {
        let snapshot = snapshot_with_filters(RotationMode::NorthUp, filters.clone());
        let mut buffer = RenderCommandBuffer::with_capacity(32);
        buffer.build_from(&snapshot);

        let actual: Vec<_> = buffer
            .commands()
            .iter()
            .filter_map(|command| match command {
                RenderCommand::Poi(poi) => Some(poi.kind()),
                _ => None,
            })
            .collect();
        assert_eq!(actual, *expected_kinds);
        assert_eq!(
            buffer.commands().len(),
            2 + snapshot.visible_tiles().len() + expected_kinds.len()
        );
        for command in buffer.commands() {
            if let RenderCommand::Poi(poi) = command {
                let source = snapshot
                    .region_pack()
                    .unwrap()
                    .poi_index()
                    .get(poi.handle())
                    .unwrap();
                assert_eq!(poi.identity().pack_hash(), snapshot.canonical_pack_hash());
                assert_eq!(
                    Some(poi.identity().region_index()),
                    snapshot.active_region_index()
                );
                assert_eq!(poi.kind(), source.kind());
                assert_eq!(poi.map_position(), source.map());
            }
        }

        let first = buffer.commands().to_vec();
        buffer.build_from(&snapshot);
        assert_eq!(buffer.commands(), first);
    }
}

#[test]
fn hidden_or_regionless_snapshots_emit_nothing() {
    let hidden = snapshot_with_options(RotationMode::NorthUp, PoiFilters::default(), false, true);
    let mut buffer = RenderCommandBuffer::with_capacity(32);
    buffer.build_from(&hidden);
    assert!(buffer.commands().is_empty());
    assert_eq!(RenderCommandBuffer::required_capacity(&hidden), 0);

    let no_player =
        snapshot_with_options(RotationMode::NorthUp, PoiFilters::default(), true, false);
    buffer.build_from(&no_player);
    assert!(
        buffer
            .commands()
            .iter()
            .all(|command| { !matches!(command, RenderCommand::PlayerArrow(_)) })
    );
    assert!(no_player.region_pack().is_none());
    assert!(buffer.commands().is_empty());
    assert_eq!(
        buffer.commands().len(),
        RenderCommandBuffer::required_capacity(&no_player)
    );
}
