mod support;

use std::fs;

use pal_map_pack_store::{MapPackError, MapPackStore, TileKey};
use serde_json::json;
use support::{MAIN_INDEX, MAIN_TILES, TempPack, fixture_root, main_region_pack};

const BUILD: &str = "24181527";

#[test]
fn descriptors_are_canonical_by_level_y_x_independent_of_json_order() {
    let store = MapPackStore::open(fixture_root(), BUILD).unwrap();
    let keys: Vec<_> = main_region_pack(&store)
        .tile_index()
        .all()
        .iter()
        .map(|tile| tile.key())
        .collect();
    assert_eq!(
        keys,
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
            TileKey {
                level: 1,
                y: 0,
                x: 0
            },
        ]
    );
    assert!(
        main_region_pack(&store)
            .tile_descriptor(TileKey {
                level: 0,
                y: 1,
                x: 1
            })
            .is_some()
    );

    let pack = TempPack::copy_valid("tile-order");
    let mut index = pack.read_json(MAIN_INDEX);
    index["tiles"].as_array_mut().unwrap().reverse();
    pack.write_json(MAIN_INDEX, &index);
    pack.refresh_manifest_file_hash(MAIN_INDEX, "tile_index_sha256");
    let reordered = MapPackStore::open(pack.root(), BUILD).expect("JSON order is irrelevant");
    let reordered_keys: Vec<_> = main_region_pack(&reordered)
        .tile_index()
        .all()
        .iter()
        .map(|tile| tile.key())
        .collect();
    assert_eq!(reordered_keys, keys);
}

#[test]
fn refuses_duplicate_missing_and_orphan_tiles() {
    let duplicate = TempPack::copy_valid("tile-duplicate");
    let mut index = duplicate.read_json(MAIN_INDEX);
    let duplicate_tile = index["tiles"][0].clone();
    index["tiles"].as_array_mut().unwrap().push(duplicate_tile);
    duplicate.write_json(MAIN_INDEX, &index);
    duplicate.refresh_manifest_file_hash(MAIN_INDEX, "tile_index_sha256");
    assert!(MapPackStore::open(duplicate.root(), BUILD).is_err());

    let missing_entry = TempPack::copy_valid("tile-missing-entry");
    let mut index = missing_entry.read_json(MAIN_INDEX);
    index["tiles"].as_array_mut().unwrap().pop();
    missing_entry.write_json(MAIN_INDEX, &index);
    missing_entry.refresh_manifest_file_hash(MAIN_INDEX, "tile_index_sha256");
    assert!(MapPackStore::open(missing_entry.root(), BUILD).is_err());

    let missing_file = TempPack::copy_valid("tile-missing-file");
    fs::remove_file(missing_file.path(&format!("{MAIN_TILES}/0/1_1.jpg"))).unwrap();
    assert!(MapPackStore::open(missing_file.root(), BUILD).is_err());

    let orphan = TempPack::copy_valid("tile-orphan");
    fs::write(
        orphan.path(&format!("{MAIN_TILES}/0/orphan.jpg")),
        b"orphan",
    )
    .unwrap();
    assert!(MapPackStore::open(orphan.root(), BUILD).is_err());
}

#[test]
fn refuses_unsafe_tile_paths_and_invalid_rectangles() {
    for (label, path) in [
        ("tile-parent", "tiles/../manifest.json"),
        ("tile-absolute", "/tiles/0/0_0.jpg"),
        ("tile-drive", "C:/tiles/0/0_0.jpg"),
        ("tile-unc", "//server/share/tile.jpg"),
        ("tile-backslash", "tiles\\0\\0_0.jpg"),
    ] {
        let pack = TempPack::copy_valid(label);
        let mut index = pack.read_json(MAIN_INDEX);
        index["tiles"][0]["relative_path"] = json!(path);
        pack.write_json(MAIN_INDEX, &index);
        pack.refresh_manifest_file_hash(MAIN_INDEX, "tile_index_sha256");
        assert!(MapPackStore::open(pack.root(), BUILD).is_err());
    }

    let pack = TempPack::copy_valid("tile-rect");
    let mut index = pack.read_json(MAIN_INDEX);
    index["tiles"][0]["map_rect"]["max_x"] = json!(-1.0);
    pack.write_json(MAIN_INDEX, &index);
    pack.refresh_manifest_file_hash(MAIN_INDEX, "tile_index_sha256");
    assert!(MapPackStore::open(pack.root(), BUILD).is_err());
}

#[test]
fn requires_the_exact_contiguous_pyramid_and_exact_paths() {
    for (label, case) in [
        ("missing-level", 0_u8),
        ("extra-level", 1),
        ("wrong-level-width", 2),
        ("wrong-grid", 3),
        ("wrong-count", 4),
        ("wrong-path", 5),
        ("duplicate-path", 6),
        ("wrong-rect", 7),
        ("large-tile", 8),
    ] {
        let pack = TempPack::copy_valid(label);
        let mut index = pack.read_json(MAIN_INDEX);
        match case {
            0 => {
                index["levels"].as_array_mut().unwrap().remove(0);
            }
            1 => index["levels"].as_array_mut().unwrap().push(json!({
                "level": 2, "width_px": 256, "height_px": 256,
                "grid_width": 1, "grid_height": 1
            })),
            2 => index["levels"][0]["width_px"] = json!(511),
            3 => index["levels"][1]["grid_width"] = json!(1),
            4 => index["tile_count"] = json!(6),
            5 => {
                index["tiles"][0]["relative_path"] =
                    json!("regions/mainmap/firstregion/tiles/0/01_1.jpg")
            }
            6 => index["tiles"][1]["relative_path"] = index["tiles"][0]["relative_path"].clone(),
            7 => index["tiles"][0]["map_rect"]["min_x"] = json!(511.0),
            _ => index["tiles"][0]["size_bytes"] = json!(16_777_217_u64),
        }
        pack.write_json(MAIN_INDEX, &index);
        pack.refresh_manifest_file_hash(MAIN_INDEX, "tile_index_sha256");
        assert!(
            MapPackStore::open(pack.root(), BUILD).is_err(),
            "case {label}"
        );
    }
}

#[test]
fn tile_document_is_strict_and_matches_build_dimensions_and_counts() {
    for (label, pointer, value) in [
        ("schema", "/schema_version", json!(1)),
        ("build", "/game_build_id", json!("99999999")),
        ("map", "/map_id", json!("Tree")),
        ("region", "/region_id", json!("DummyRegion")),
        ("width", "/map_width_px", json!(512)),
        ("height", "/map_height_px", json!(512)),
        ("core", "/tile_core_size_px", json!(256)),
        ("gutter", "/tile_gutter_px", json!(1)),
        ("levels", "/level_count", json!(3)),
    ] {
        let pack = TempPack::copy_valid(label);
        let mut index = pack.read_json(MAIN_INDEX);
        *index.pointer_mut(pointer).unwrap() = value;
        pack.write_json(MAIN_INDEX, &index);
        pack.refresh_manifest_file_hash(MAIN_INDEX, "tile_index_sha256");
        assert!(MapPackStore::open(pack.root(), BUILD).is_err());
    }

    let pack = TempPack::copy_valid("unknown-tile-field");
    let mut index = pack.read_json(MAIN_INDEX);
    index["unexpected"] = json!(true);
    pack.write_json(MAIN_INDEX, &index);
    pack.refresh_manifest_file_hash(MAIN_INDEX, "tile_index_sha256");
    assert!(MapPackStore::open(pack.root(), BUILD).is_err());
}

#[test]
fn refuses_nonregular_tile_and_extra_case_variant_file() {
    let nonregular = TempPack::copy_valid("nonregular-tile");
    let nonregular_path = nonregular.path(&format!("{MAIN_TILES}/0/0_0.jpg"));
    fs::remove_file(&nonregular_path).unwrap();
    fs::create_dir(&nonregular_path).unwrap();
    assert!(MapPackStore::open(nonregular.root(), BUILD).is_err());

    let extra = TempPack::copy_valid("case-variant-orphan");
    let path = extra.path(&format!("{MAIN_TILES}/0/ORPHAN.jpg"));
    if fs::write(&path, b"orphan").is_ok() {
        assert!(MapPackStore::open(extra.root(), BUILD).is_err());
    }
}

#[test]
fn verified_tile_reads_recheck_mutation_missing_reparse_and_unknown_keys() {
    let mutated = TempPack::copy_valid("read-mutated");
    let store = MapPackStore::open(mutated.root(), BUILD).unwrap();
    fs::write(
        mutated.path(&format!("{MAIN_TILES}/0/0_0.jpg")),
        b"changed tile bytes\n",
    )
    .unwrap();
    assert!(matches!(
        main_region_pack(&store).read_verified_tile(TileKey {
            level: 0,
            y: 0,
            x: 0
        }),
        Err(MapPackError::HashMismatch { component: "tile" })
    ));

    let missing = TempPack::copy_valid("read-missing");
    let store = MapPackStore::open(missing.root(), BUILD).unwrap();
    fs::remove_file(missing.path(&format!("{MAIN_TILES}/0/0_0.jpg"))).unwrap();
    assert!(
        main_region_pack(&store)
            .read_verified_tile(TileKey {
                level: 0,
                y: 0,
                x: 0
            })
            .is_err()
    );

    let unknown = MapPackStore::open(fixture_root(), BUILD).unwrap();
    assert!(matches!(
        main_region_pack(&unknown).read_verified_tile(TileKey {
            level: 9,
            y: 9,
            x: 9
        }),
        Err(MapPackError::UnknownTile {
            level: 9,
            y: 9,
            x: 9
        })
    ));

    let linked = TempPack::copy_valid("read-reparse");
    let outside = TempPack::copy_valid("read-reparse-outside");
    let store = MapPackStore::open(linked.root(), BUILD).unwrap();
    fs::remove_file(linked.path(&format!("{MAIN_TILES}/0/0_0.jpg"))).unwrap();
    #[cfg(windows)]
    let link_created = std::os::windows::fs::symlink_file(
        outside.path(&format!("{MAIN_TILES}/0/0_0.jpg")),
        linked.path(&format!("{MAIN_TILES}/0/0_0.jpg")),
    )
    .is_ok();
    #[cfg(unix)]
    let link_created = std::os::unix::fs::symlink(
        outside.path(&format!("{MAIN_TILES}/0/0_0.jpg")),
        linked.path(&format!("{MAIN_TILES}/0/0_0.jpg")),
    )
    .is_ok();
    #[cfg(not(any(windows, unix)))]
    let link_created = false;
    if link_created {
        assert!(
            main_region_pack(&store)
                .read_verified_tile(TileKey {
                    level: 0,
                    y: 0,
                    x: 0
                })
                .is_err()
        );
    }

    let debug = format!("{store:?}");
    assert!(!debug.contains(&linked.root().display().to_string()));
}

#[test]
fn deep_and_numerous_orphan_entries_are_bounded() {
    let deep = TempPack::copy_valid("deep-orphan");
    fs::create_dir_all(deep.path(&format!("{MAIN_TILES}/0/deep/nested"))).unwrap();
    fs::write(
        deep.path(&format!("{MAIN_TILES}/0/deep/nested/orphan.jpg")),
        b"orphan",
    )
    .unwrap();
    assert!(matches!(
        MapPackStore::open(deep.root(), BUILD),
        Err(MapPackError::OrphanTile)
    ));

    let numerous = TempPack::copy_valid("numerous-orphan");
    for index in 0..64 {
        fs::write(
            numerous.path(&format!("{MAIN_TILES}/0/orphan-{index}.jpg")),
            b"orphan",
        )
        .unwrap();
    }
    assert!(matches!(
        MapPackStore::open(numerous.root(), BUILD),
        Err(MapPackError::SizeLimit { component: "tiles" })
    ));
}
