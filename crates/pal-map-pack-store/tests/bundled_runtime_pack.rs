use std::path::Path;

use pal_map_pack_store::MapPackStore;

#[test]
fn bundled_runtime_pack_matches_contract_and_authenticates_every_tile() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let contract: serde_json::Value =
        serde_json::from_str(include_str!("../../../contracts/runtime-builds.json"))
            .expect("valid runtime build contract");
    let build = contract["current_game_build_id"]
        .as_str()
        .expect("string build identifier");
    let root = repo
        .join("assets/palbeacon/game/native-map-pack")
        .join(build);
    let store = MapPackStore::open_published(root, build).expect("approved bundled map pack");
    assert!(!store.map_regions().is_empty());
    for index in 0..store.map_regions().len() {
        let region = store.region_pack(index).expect("indexed region");
        assert!(!region.tile_index().all().is_empty());
        for tile in region.tile_index().all() {
            let bytes = region
                .read_verified_tile(tile.key())
                .expect("bundled tile matches its size and SHA-256");
            assert!(!bytes.is_empty());
        }
    }
}
