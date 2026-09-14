use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;

use pal_domain::{
    CoreState, CoreStateParts, ExpandedMapView, Freshness, MiniMapView, OverlaySettings,
    PositionSample, SampleClock,
};
use pal_map_pack_store::{MapPackStore, MapPoint, WorldPoint};
use pal_render::{
    InterpolationPolicy, PlayerPose, RenderCommandBuffer, RenderSnapshot, SnapshotBuilder,
    ViewportLayout, ViewportMetrics, interpolate_player_pose,
};

const BUILD: &str = "24181527";

struct ThreadCountingAllocator;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn record_allocation() {
    COUNTING.with(|counting| {
        if counting.get() {
            ALLOCATIONS.with(|allocations| {
                allocations.set(allocations.get().saturating_add(1));
            });
        }
    });
}

unsafe impl GlobalAlloc for ThreadCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record_allocation();
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

fn allocation_count<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    ALLOCATIONS.with(|allocations| allocations.set(0));
    COUNTING.with(|counting| counting.set(true));
    let output = operation();
    COUNTING.with(|counting| counting.set(false));
    let count = ALLOCATIONS.with(Cell::get);
    (output, count)
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("map-pack-valid")
}

fn snapshot() -> Arc<RenderSnapshot> {
    let pack = MapPackStore::open(fixture_root(), BUILD).unwrap();
    let position = PositionSample::new(
        "test-world",
        b"subject",
        b"boot-zero-alloc",
        3,
        5,
        400.0,
        -400.0,
        25.0,
        Some(135.0),
        SampleClock::received_with_age(0, 1_000),
    )
    .unwrap();
    let settings = OverlaySettings::default();
    let state = CoreState::from_parts(CoreStateParts {
        settings,
        settings_version: 9,
        window_snapshot: None,
        position_sample: Some(position),
        freshness: Freshness::Live,
        heading_available: true,
        connected: true,
        visible: true,
        interpolate_position: true,
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

fn exercise_getters_and_interpolation(
    snapshot: &RenderSnapshot,
    from: PlayerPose,
    to: PlayerPose,
) -> PlayerPose {
    black_box(snapshot.pack());
    black_box(snapshot.build_id());
    black_box(snapshot.canonical_pack_hash());
    black_box(snapshot.settings_version());
    black_box(snapshot.visible());
    black_box(snapshot.display_mode());
    black_box(snapshot.freshness());
    black_box(snapshot.heading_status());
    black_box(snapshot.interpolation_policy());
    black_box(snapshot.player_pose());
    black_box(snapshot.active_viewport());
    black_box(snapshot.viewport_pose());
    black_box(snapshot.poi_filters());
    black_box(snapshot.visible_tiles());
    black_box(snapshot.visible_pois());
    if let Some(cursor) = black_box(snapshot.source_cursor()) {
        black_box(cursor.generation());
        black_box(cursor.boot_id());
        black_box(cursor.sequence());
    }
    black_box(interpolate_player_pose(
        from,
        to,
        0.25,
        InterpolationPolicy::Interpolate,
    ))
}

#[test]
fn warmed_snapshot_getters_and_pose_interpolation_allocate_exactly_zero() {
    let snapshot = snapshot();
    let from = snapshot.player_pose().unwrap();
    let to = PlayerPose::new(
        WorldPoint::new(500.0, -300.0).unwrap(),
        MapPoint::new(350.0, 350.0).unwrap(),
        30.0,
        Some(145.0),
    )
    .unwrap();

    black_box(exercise_getters_and_interpolation(&snapshot, from, to));
    let (pose, allocations) =
        allocation_count(|| exercise_getters_and_interpolation(&snapshot, from, to));

    black_box(pose);
    assert_eq!(
        allocations, 0,
        "warmed read/interpolation path allocated {allocations} times"
    );
}

#[test]
fn caller_preallocated_command_buffer_builds_and_rebuilds_without_allocating() {
    let snapshot = snapshot();
    let required = RenderCommandBuffer::required_capacity(&snapshot);
    let mut commands = RenderCommandBuffer::with_capacity(required);

    let (_, first_allocations) = allocation_count(|| commands.build_from(&snapshot));
    assert_eq!(
        first_allocations, 0,
        "caller-owned capacity must cover the first command build"
    );

    let (_, warmed_allocations) = allocation_count(|| commands.build_from(&snapshot));
    assert_eq!(
        warmed_allocations, 0,
        "warmed command rebuild allocated {warmed_allocations} times"
    );
    assert_eq!(commands.commands().len(), required);
}
