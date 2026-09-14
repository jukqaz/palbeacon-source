use std::path::{Path, PathBuf};
use std::sync::Arc;

use pal_domain::{
    CoreState, CoreStateParts, DisplayMode, ExpandedMapView, Freshness, MiniMapView,
    OverlaySettings, PoiFilters, PositionSample, RotationMode, SampleClock,
};
use pal_map_pack_store::{MapPackStore, PoiKind};
use pal_render::{
    ActiveViewport, HeadingStatus, InterpolationPolicy, RenderCommandBuffer, SnapshotBuilder,
    ViewportLayout, ViewportMetrics,
};

const BUILD: &str = "24181527";
const SAMPLE_MAP_X: f64 = 511.717_283_268_912_2;
const SAMPLE_MAP_Y: f64 = 246.670_347_874_102_73;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("map-pack-valid")
}

fn open_pack(root: impl AsRef<Path>) -> Arc<MapPackStore> {
    MapPackStore::open(root, BUILD).expect("valid fixture pack")
}

fn layout() -> ViewportLayout {
    ViewportLayout::new(
        ViewportMetrics::new(320, 320, 1.0).unwrap(),
        ViewportMetrics::new(1024, 1024, 1.0).unwrap(),
    )
}

fn sample() -> PositionSample {
    PositionSample::new(
        "test-world",
        b"subject",
        b"boot-snapshot",
        7,
        11,
        400.0,
        -400.0,
        25.0,
        Some(135.0),
        SampleClock::received_with_age(0, 1_000),
    )
    .unwrap()
}

fn sample_at(x: f64, y: f64) -> PositionSample {
    PositionSample::new(
        "test-world",
        b"subject",
        b"boot-region-snapshot",
        7,
        12,
        x,
        y,
        25.0,
        Some(135.0),
        SampleClock::received_with_age(0, 1_000),
    )
    .unwrap()
}

struct StateSpec {
    freshness: Freshness,
    sample: Option<PositionSample>,
    display_mode: DisplayMode,
    rotation_mode: RotationMode,
    heading_available: bool,
    visible: bool,
    filters: PoiFilters,
    mini: MiniMapView,
    expanded: ExpandedMapView,
    settings_version: u64,
}

impl Default for StateSpec {
    fn default() -> Self {
        Self {
            freshness: Freshness::Live,
            sample: Some(sample()),
            display_mode: DisplayMode::MiniMap,
            rotation_mode: RotationMode::NorthUp,
            heading_available: true,
            visible: true,
            filters: PoiFilters::default(),
            mini: MiniMapView::new(900.0, 900.0, 1.5).unwrap(),
            expanded: ExpandedMapView::new(700.0, 600.0, 2.0).unwrap(),
            settings_version: 42,
        }
    }
}

fn core_state(spec: StateSpec) -> CoreState {
    let settings = OverlaySettings {
        enabled: spec.visible,
        display_mode: spec.display_mode,
        rotation_mode: spec.rotation_mode,
        poi_filters: spec.filters,
        ..OverlaySettings::default()
    };
    let connected = spec.freshness != Freshness::Offline;
    let interpolate_position = connected
        && spec.sample.is_some()
        && matches!(spec.freshness, Freshness::Live | Freshness::Delayed);
    CoreState::from_parts(CoreStateParts {
        settings,
        settings_version: spec.settings_version,
        window_snapshot: None,
        position_sample: spec.sample,
        freshness: spec.freshness,
        heading_available: spec.heading_available,
        connected,
        visible: spec.visible,
        interpolate_position,
        mini_map_view: spec.mini,
        expanded_map_view: spec.expanded,
    })
    .unwrap()
}

fn selected_ids(snapshot: &pal_render::RenderSnapshot) -> Vec<&str> {
    snapshot
        .visible_pois()
        .iter()
        .map(|handle| {
            snapshot
                .region_pack()
                .unwrap()
                .poi_index()
                .get(*handle)
                .unwrap()
                .id()
        })
        .collect()
}

#[test]
fn snapshot_owns_complete_metadata_source_cursor_and_player_follow_view() {
    let pack = open_pack(fixture_root());
    let state = core_state(StateSpec::default());
    let mut builder = SnapshotBuilder::new(Arc::clone(&pack));

    let snapshot = builder.build(&state, layout()).unwrap();

    assert!(Arc::ptr_eq(snapshot.pack(), &pack));
    assert_eq!(snapshot.build_id(), BUILD);
    assert_eq!(snapshot.canonical_pack_hash(), pack.canonical_pack_hash());
    assert_eq!(snapshot.settings_version(), 42);
    assert!(snapshot.visible());
    assert_eq!(snapshot.display_mode(), DisplayMode::MiniMap);
    assert_eq!(snapshot.freshness(), Freshness::Live);
    assert_eq!(
        snapshot.interpolation_policy(),
        InterpolationPolicy::Interpolate
    );
    assert_eq!(snapshot.heading_status(), HeadingStatus::Available);
    assert_eq!(
        snapshot.poi_filters(),
        pal_map_pack_store::PoiFilterMask::from_kinds([
            pal_map_pack_store::PoiKind::FastTravel,
            pal_map_pack_store::PoiKind::Boss,
            pal_map_pack_store::PoiKind::Dungeon,
        ])
    );

    let cursor = snapshot.source_cursor().unwrap();
    assert_eq!(cursor.generation(), 7);
    assert_eq!(cursor.boot_id(), b"boot-snapshot");
    assert_eq!(cursor.sequence(), 11);

    let player = snapshot.player_pose().unwrap();
    assert_eq!(player.world().x(), 400.0);
    assert_eq!(player.world().y(), -400.0);
    assert_eq!(player.map().x(), SAMPLE_MAP_X);
    assert_eq!(player.map().y(), SAMPLE_MAP_Y);
    assert_eq!(player.z(), 25.0);
    assert_eq!(player.heading_degrees(), Some(135.0));

    let ActiveViewport::Mini(viewport) = snapshot.active_viewport() else {
        panic!("minimap mode must publish a circular minimap viewport");
    };
    assert_eq!(viewport.center(), player.map());
    assert_eq!(viewport.zoom(), 1.5);
    assert_eq!(snapshot.viewport_pose().map_rotation_degrees(), 0.0);
    assert_eq!(snapshot.viewport_pose().player_rotation_degrees(), 135.0);
    assert!(snapshot.visible_tiles().is_sorted());
    assert!(snapshot.visible_pois().is_sorted());
}

#[test]
fn overlap_position_uses_tree_transform_tiles_and_pois_without_main_map_fallback() {
    let pack = open_pack(fixture_root());
    let state = core_state(StateSpec {
        sample: Some(sample_at(348_000.0, -500_000.0)),
        ..StateSpec::default()
    });
    let snapshot = SnapshotBuilder::new(pack).build(&state, layout()).unwrap();

    let region = snapshot.region_pack().expect("sample selects a region");
    assert_eq!(region.region().map_id(), "Tree");
    assert_eq!(snapshot.player_pose().unwrap().map().x(), 953.2960441431612);
    assert_eq!(
        snapshot.player_pose().unwrap().map().y(),
        1022.0571391791034
    );
    assert!(
        snapshot
            .visible_tiles()
            .iter()
            .all(|key| region.tile_descriptor(*key).is_some())
    );
    assert!(snapshot.visible_pois().is_empty());
}

#[test]
fn main_to_tree_to_outside_transition_publishes_a_cleared_nonrenderable_snapshot() {
    let pack = open_pack(fixture_root());
    let mut builder = SnapshotBuilder::new(pack);

    let main = builder
        .build(&core_state(StateSpec::default()), layout())
        .unwrap();
    assert_eq!(main.region_pack().unwrap().region().map_id(), "MainMap");

    let tree = builder
        .build(
            &core_state(StateSpec {
                sample: Some(sample_at(348_000.0, -500_000.0)),
                ..StateSpec::default()
            }),
            layout(),
        )
        .unwrap();
    assert_eq!(tree.region_pack().unwrap().region().map_id(), "Tree");

    let outside = builder
        .build(
            &core_state(StateSpec {
                sample: Some(sample_at(900_000.0, 900_000.0)),
                ..StateSpec::default()
            }),
            layout(),
        )
        .expect("outside coordinate clears instead of retaining the prior Tree snapshot");
    assert!(outside.region_pack().is_none());
    assert!(!outside.visible());
    assert!(outside.player_pose().is_none());
    assert!(outside.visible_tiles().is_empty());
    assert!(outside.visible_pois().is_empty());
    assert_eq!(RenderCommandBuffer::required_capacity(&outside), 0);
    let mut commands = RenderCommandBuffer::with_capacity(8);
    commands.build_from(&outside);
    assert!(commands.commands().is_empty());
}

#[test]
fn freshness_controls_interpolation_without_dropping_retained_pose() {
    let pack = open_pack(fixture_root());
    let mut builder = SnapshotBuilder::new(pack);

    for (freshness, expected) in [
        (Freshness::Live, InterpolationPolicy::Interpolate),
        (Freshness::Delayed, InterpolationPolicy::Interpolate),
        (Freshness::Stale, InterpolationPolicy::Freeze),
        (Freshness::Offline, InterpolationPolicy::Freeze),
    ] {
        let state = core_state(StateSpec {
            freshness,
            heading_available: matches!(freshness, Freshness::Live | Freshness::Delayed),
            ..StateSpec::default()
        });
        let snapshot = builder.build(&state, layout()).unwrap();
        assert_eq!(snapshot.interpolation_policy(), expected);
        assert!(snapshot.player_pose().is_some());
        assert_eq!(snapshot.active_viewport().center().x(), SAMPLE_MAP_X);
        assert_eq!(snapshot.active_viewport().center().y(), SAMPLE_MAP_Y);
    }
}

#[test]
fn minimap_uses_stored_center_only_without_a_sample_and_expanded_view_is_independent() {
    let pack = open_pack(fixture_root());
    let mut builder = SnapshotBuilder::new(pack);
    let no_sample = core_state(StateSpec {
        freshness: Freshness::Stale,
        sample: None,
        heading_available: false,
        mini: MiniMapView::new(12.0, 34.0, 1.25).unwrap(),
        ..StateSpec::default()
    });
    let fallback = builder.build(&no_sample, layout()).unwrap();
    assert_eq!(fallback.active_viewport().center().x(), 12.0);
    assert_eq!(fallback.active_viewport().center().y(), 34.0);
    assert_eq!(fallback.player_pose(), None);
    assert_eq!(fallback.source_cursor(), None);

    let expanded = core_state(StateSpec {
        display_mode: DisplayMode::ExpandedMap,
        expanded: ExpandedMapView::new(700.0, 600.0, 2.5).unwrap(),
        ..StateSpec::default()
    });
    let expanded = builder.build(&expanded, layout()).unwrap();
    let ActiveViewport::Expanded(viewport) = expanded.active_viewport() else {
        panic!("expanded mode must publish a rectangular viewport");
    };
    assert_eq!(viewport.center().x(), 700.0);
    assert_eq!(viewport.center().y(), 600.0);
    assert_eq!(viewport.zoom(), 2.5);
    assert_eq!(expanded.player_pose().unwrap().map().x(), SAMPLE_MAP_X);
    assert_eq!(expanded.player_pose().unwrap().map().y(), SAMPLE_MAP_Y);
}

#[test]
fn sample_heading_is_ignored_unless_core_state_marks_it_available() {
    let pack = open_pack(fixture_root());
    let mut builder = SnapshotBuilder::new(pack);
    let unavailable = core_state(StateSpec {
        rotation_mode: RotationMode::HeadingUp,
        heading_available: false,
        ..StateSpec::default()
    });
    let unavailable = builder.build(&unavailable, layout()).unwrap();
    assert_eq!(unavailable.player_pose().unwrap().heading_degrees(), None);
    assert_eq!(unavailable.heading_status(), HeadingStatus::Unavailable);
    assert_eq!(unavailable.viewport_pose().map_rotation_degrees(), 0.0);
    assert_eq!(unavailable.viewport_pose().player_rotation_degrees(), 0.0);

    let available = core_state(StateSpec {
        rotation_mode: RotationMode::HeadingUp,
        ..StateSpec::default()
    });
    let available = builder.build(&available, layout()).unwrap();
    assert_eq!(
        available.player_pose().unwrap().heading_degrees(),
        Some(135.0)
    );
    assert_eq!(available.heading_status(), HeadingStatus::Available);
    assert_eq!(available.viewport_pose().map_rotation_degrees(), -135.0);
}

#[test]
fn same_inputs_are_deterministic_and_hidden_keeps_the_semantic_set() {
    let pack = open_pack(fixture_root());
    let mut builder = SnapshotBuilder::new(pack);
    let state = core_state(StateSpec::default());
    let first = builder.build(&state, layout()).unwrap();
    let second = builder.build(&state, layout()).unwrap();

    assert_eq!(first.build_id(), second.build_id());
    assert_eq!(first.canonical_pack_hash(), second.canonical_pack_hash());
    assert_eq!(first.settings_version(), second.settings_version());
    assert_eq!(first.source_cursor(), second.source_cursor());
    assert_eq!(first.display_mode(), second.display_mode());
    assert_eq!(first.freshness(), second.freshness());
    assert_eq!(first.player_pose(), second.player_pose());
    assert_eq!(first.active_viewport(), second.active_viewport());
    assert_eq!(first.viewport_pose(), second.viewport_pose());
    assert_eq!(first.poi_filters(), second.poi_filters());
    assert_eq!(first.visible_tiles(), second.visible_tiles());
    assert_eq!(first.visible_pois(), second.visible_pois());

    let hidden = core_state(StateSpec {
        visible: false,
        ..StateSpec::default()
    });
    let hidden = builder.build(&hidden, layout()).unwrap();
    assert!(!hidden.visible());
    assert_eq!(hidden.visible_tiles(), first.visible_tiles());
    assert_eq!(hidden.visible_pois(), first.visible_pois());
}

#[test]
fn all_domain_filter_combinations_map_to_expected_visible_pois() {
    let pack = open_pack(fixture_root());
    let mut builder = SnapshotBuilder::new(pack);
    let cases: &[(PoiFilters, &[&str])] = &[
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
            &["fast-1"],
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
            &["boss-1"],
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
            &["dungeon-1"],
        ),
        (
            PoiFilters {
                fast_travel: true,
                boss: true,
                wanted: false,
                dungeon: false,
                enabled_layer_ids: Vec::new(),
                selected_pal_ids: Vec::new(),
                night_only: false,
            },
            &["fast-1", "boss-1"],
        ),
        (
            PoiFilters {
                fast_travel: true,
                boss: false,
                wanted: false,
                dungeon: true,
                enabled_layer_ids: Vec::new(),
                selected_pal_ids: Vec::new(),
                night_only: false,
            },
            &["fast-1", "dungeon-1"],
        ),
        (
            PoiFilters {
                fast_travel: false,
                boss: true,
                wanted: false,
                dungeon: true,
                enabled_layer_ids: Vec::new(),
                selected_pal_ids: Vec::new(),
                night_only: false,
            },
            &["boss-1", "dungeon-1"],
        ),
        (PoiFilters::default(), &["fast-1", "boss-1", "dungeon-1"]),
    ];

    for (filters, expected) in cases {
        let state = core_state(StateSpec {
            display_mode: DisplayMode::ExpandedMap,
            filters: filters.clone(),
            expanded: ExpandedMapView::new(512.0, 512.0, 1.0).unwrap(),
            ..StateSpec::default()
        });
        let snapshot = builder.build(&state, layout()).unwrap();
        assert_eq!(selected_ids(&snapshot), *expected);
        for (kind, enabled) in [
            (PoiKind::FastTravel, filters.fast_travel),
            (PoiKind::Boss, filters.boss),
            (PoiKind::Wanted, filters.wanted),
            (PoiKind::Dungeon, filters.dungeon),
        ] {
            assert_eq!(snapshot.poi_filters().contains(kind), enabled);
        }
    }
}

#[test]
fn scratch_reuse_and_pack_swap_cannot_mutate_or_reinterpret_old_snapshots() {
    let first_pack = open_pack(fixture_root());
    let second_pack = open_pack(fixture_root());
    assert!(!Arc::ptr_eq(&first_pack, &second_pack));
    let mut builder = SnapshotBuilder::new(Arc::clone(&first_pack));

    let full_state = core_state(StateSpec {
        display_mode: DisplayMode::ExpandedMap,
        expanded: ExpandedMapView::new(512.0, 512.0, 1.0).unwrap(),
        ..StateSpec::default()
    });
    let old = builder.build(&full_state, layout()).unwrap();
    let old_tiles = old.visible_tiles().to_vec();
    let old_pois = old.visible_pois().to_vec();

    let empty_state = core_state(StateSpec {
        freshness: Freshness::Stale,
        sample: None,
        heading_available: false,
        mini: MiniMapView::new(-500.0, -500.0, 1.0).unwrap(),
        filters: PoiFilters {
            fast_travel: false,
            boss: false,
            wanted: false,
            dungeon: false,
            enabled_layer_ids: Vec::new(),
            selected_pal_ids: Vec::new(),
            night_only: false,
        },
        ..StateSpec::default()
    });
    let empty = builder.build(&empty_state, layout()).unwrap();
    assert!(empty.visible_tiles().is_empty());
    assert!(empty.visible_pois().is_empty());
    assert_eq!(old.visible_tiles(), old_tiles);
    assert_eq!(old.visible_pois(), old_pois);

    let replaced = builder.replace_pack(Arc::clone(&second_pack));
    assert!(Arc::ptr_eq(&replaced, &first_pack));
    let new = builder.build(&full_state, layout()).unwrap();
    assert!(Arc::ptr_eq(old.pack(), &first_pack));
    assert!(Arc::ptr_eq(new.pack(), &second_pack));
    assert_eq!(selected_ids(&old), selected_ids(&new));
}

#[test]
fn filter_changes_reuse_the_open_pack_without_touching_its_files() {
    let temporary_root = std::env::temp_dir().join(format!(
        "pal-render-snapshot-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("unnamed")
    ));
    if temporary_root.exists() {
        std::fs::remove_dir_all(&temporary_root).unwrap();
    }
    copy_tree(&fixture_root(), &temporary_root);
    let pack = open_pack(&temporary_root);
    std::fs::remove_dir_all(&temporary_root).unwrap();

    let mut builder = SnapshotBuilder::new(Arc::clone(&pack));
    let all = builder
        .build(
            &core_state(StateSpec {
                display_mode: DisplayMode::ExpandedMap,
                expanded: ExpandedMapView::new(512.0, 512.0, 1.0).unwrap(),
                ..StateSpec::default()
            }),
            layout(),
        )
        .unwrap();
    let none = builder
        .build(
            &core_state(StateSpec {
                display_mode: DisplayMode::ExpandedMap,
                filters: PoiFilters {
                    fast_travel: false,
                    boss: false,
                    wanted: false,
                    dungeon: false,
                    enabled_layer_ids: Vec::new(),
                    selected_pal_ids: Vec::new(),
                    night_only: false,
                },
                expanded: ExpandedMapView::new(512.0, 512.0, 1.0).unwrap(),
                ..StateSpec::default()
            }),
            layout(),
        )
        .unwrap();

    assert!(Arc::ptr_eq(all.pack(), &pack));
    assert!(Arc::ptr_eq(none.pack(), &pack));
    assert_eq!(selected_ids(&all), ["fast-1", "boss-1", "dungeon-1"]);
    assert!(none.visible_pois().is_empty());
    assert_eq!(all.visible_tiles(), none.visible_tiles());
}

fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
