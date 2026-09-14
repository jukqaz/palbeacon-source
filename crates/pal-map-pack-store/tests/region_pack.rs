mod support;

use pal_map_pack_store::{MapPackStore, TileKey, WorldPoint};
use support::fixture_root;

const BUILD: &str = "24181527";

#[test]
fn overlapping_world_coordinate_selects_and_reads_the_tree_region_pack() {
    let store = MapPackStore::open(fixture_root(), BUILD).expect("multi-region fixture opens");
    let tree = store
        .select_region_pack(348_000.0, -500_000.0)
        .expect("higher-priority Tree region is selected");

    assert_eq!(tree.region().map_id(), "Tree");
    assert_eq!(tree.region().region_id(), "DummyRegion");
    assert_eq!(
        tree.transform()
            .project(WorldPoint::new(348_000.0, -500_000.0).unwrap())
            .unwrap()
            .x(),
        953.2960441431612
    );
    assert_eq!(tree.tile_index().all().len(), 5);
    assert_eq!(
        tree.read_verified_tile(TileKey {
            level: 0,
            y: 0,
            x: 0,
        })
        .expect("Tree tile authenticates"),
        b"synthetic tree 0,0\n"
    );
}

#[test]
fn main_map_coordinate_selects_the_main_map_tile_set() {
    let store = MapPackStore::open(fixture_root(), BUILD).expect("multi-region fixture opens");
    let main = store
        .select_region_pack(0.0, 0.0)
        .expect("MainMap region is selected");

    assert_eq!(main.region().map_id(), "MainMap");
    assert_eq!(main.tile_index().all().len(), 5);
    assert_ne!(
        main.read_verified_tile(TileKey {
            level: 0,
            y: 0,
            x: 0,
        })
        .unwrap(),
        b"synthetic tree 0,0\n"
    );
}
