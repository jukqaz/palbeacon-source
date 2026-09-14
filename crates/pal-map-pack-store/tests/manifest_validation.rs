mod support;

use std::fs;
#[cfg(windows)]
use std::process::Command;

use pal_map_pack_store::{MapPackError, MapPackStore, ProvenanceStatus};
use serde_json::{Value, json};
use support::{
    MAIN_INDEX, MAIN_TRANSFORM, PublishedPack, TREE_INDEX, TempPack, assert_sanitized, fixture_root,
};

const BUILD: &str = "24181527";
type JsonMutation = (&'static str, Box<dyn Fn(&mut Value)>);

#[test]
fn opens_a_strict_current_build_pack_and_exposes_a_stable_hash() {
    let first = MapPackStore::open(fixture_root(), BUILD).expect("valid fixture opens");
    let second = MapPackStore::open(fixture_root(), BUILD).expect("valid fixture reopens");

    assert_eq!(first.manifest().game_build_id(), BUILD);
    assert_eq!(first.canonical_pack_hash(), second.canonical_pack_hash());
    assert_ne!(first.canonical_pack_hash(), [0; 32]);
    assert_eq!(first.provenance_status(), ProvenanceStatus::FormatBoundOnly);
    assert_eq!(first.map_regions().len(), 2);
    let tree = first
        .select_map_region(348_000.0, -500_000.0)
        .expect("higher-priority overlapping Tree region is selected");
    assert_eq!(tree.map_id(), "Tree");
    assert_eq!(tree.region_id(), "DummyRegion");
    assert_eq!(
        tree.source_texture_path(),
        "/Game/Pal/Texture/UI/Map/T_TreeMap.T_TreeMap"
    );
    assert_eq!(
        tree.world_bounds(),
        (347_351.5, -818_197.0, 689_148.5, -476_400.0)
    );
    assert_eq!(tree.block_size(), (1.0, 1.0));
    assert_eq!(tree.grid_position(), (0.0, 0.0));
    assert_eq!(tree.priority(), 1);
}

#[test]
fn map_region_selection_rejects_nonfinite_outside_and_ambiguous_packs() {
    let store = MapPackStore::open(fixture_root(), BUILD).unwrap();
    assert!(matches!(
        store.select_map_region(f64::NAN, 0.0),
        Err(MapPackError::InvalidMapRegionCoordinate)
    ));
    assert!(matches!(
        store.select_map_region(900_000.0, 900_000.0),
        Err(MapPackError::OutsideMapRegions)
    ));

    let tied = TempPack::copy_valid("region-tie");
    let mut manifest = tied.read_json("manifest.json");
    manifest["map_regions"][1]["priority"] = json!(0);
    tied.write_json("manifest.json", &manifest);
    assert!(matches!(
        MapPackStore::open(tied.root(), BUILD),
        Err(MapPackError::AmbiguousMapRegions)
    ));

    let missing = TempPack::copy_valid("regions-missing");
    let mut manifest = missing.read_json("manifest.json");
    manifest.as_object_mut().unwrap().remove("map_regions");
    missing.write_json("manifest.json", &manifest);
    assert!(MapPackStore::open(missing.root(), BUILD).is_err());
}

#[test]
fn authoritative_regions_must_bind_distinct_map_assets_and_tile_sets() {
    for (label, field) in [
        ("duplicate-map-asset", "map_asset_sha256"),
        ("duplicate-tile-set", "tile_set_sha256"),
    ] {
        let pack = TempPack::copy_valid(label);
        let mut manifest = pack.read_json("manifest.json");
        manifest["map_regions"][1][field] = manifest["map_regions"][0][field].clone();
        pack.write_json("manifest.json", &manifest);
        assert!(
            MapPackStore::open(pack.root(), BUILD).is_err(),
            "case {label}"
        );
    }
}

#[test]
fn legacy_global_map_fields_and_cross_region_paths_are_rejected() {
    for field in [
        "map_asset_sha256",
        "transform_sha256",
        "tile_set_sha256",
        "tile_index_sha256",
        "map_width_px",
        "map_height_px",
    ] {
        let pack = TempPack::copy_valid(field);
        let mut manifest = pack.read_json("manifest.json");
        manifest[field] = json!(if field.ends_with("_px") {
            "1024"
        } else {
            "aaaaaaaa"
        });
        pack.write_json("manifest.json", &manifest);
        assert!(
            MapPackStore::open(pack.root(), BUILD).is_err(),
            "field {field}"
        );
    }

    let crossed = TempPack::copy_valid("crossed-region-path");
    let mut manifest = crossed.read_json("manifest.json");
    manifest["map_regions"][1]["tile_index_relative_path"] = json!(MAIN_INDEX);
    crossed.write_json("manifest.json", &manifest);
    assert!(MapPackStore::open(crossed.root(), BUILD).is_err());
}

#[test]
fn published_store_resolves_only_a_strict_hash_bound_active_pointer() {
    let published = PublishedPack::valid("valid");
    let store = MapPackStore::open_published(published.root(), BUILD)
        .expect("strict active pointer resolves");
    assert_eq!(store.manifest().game_build_id(), BUILD);

    let escaped = PublishedPack::valid("escaped");
    let mut pointer: Value =
        serde_json::from_slice(&fs::read(escaped.pointer_path()).unwrap()).unwrap();
    pointer["relative_version_path"] = json!("../map-pack-valid");
    fs::write(
        escaped.pointer_path(),
        serde_json::to_vec_pretty(&pointer).unwrap(),
    )
    .unwrap();
    assert!(MapPackStore::open_published(escaped.root(), BUILD).is_err());

    let tampered = PublishedPack::valid("hash");
    let mut pointer: Value =
        serde_json::from_slice(&fs::read(tampered.pointer_path()).unwrap()).unwrap();
    pointer["manifest_sha256"] =
        json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    fs::write(
        tampered.pointer_path(),
        serde_json::to_vec_pretty(&pointer).unwrap(),
    )
    .unwrap();
    assert!(MapPackStore::open_published(tampered.root(), BUILD).is_err());
}

#[test]
fn published_store_rejects_a_versions_parent_link_that_escapes_the_dataset_root() {
    let published = PublishedPack::valid("versions-parent-link");
    let outside = PublishedPack::valid("versions-parent-link-outside");
    let versions = published.root().join(".versions");
    fs::remove_dir_all(&versions).expect("remove original versions directory");
    create_directory_link(&outside.root().join(".versions"), &versions);

    assert!(matches!(
        MapPackStore::open_published(published.root(), BUILD),
        Err(MapPackError::ReparsePoint { .. } | MapPackError::PathEscape { .. })
    ));

    remove_directory_link(&versions);
}

#[cfg(windows)]
fn create_directory_link(target: &std::path::Path, link: &std::path::Path) {
    let status = Command::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .status()
        .expect("launch mklink junction command");
    assert!(status.success(), "create directory junction");
}

#[cfg(windows)]
fn remove_directory_link(link: &std::path::Path) {
    fs::remove_dir(link).expect("remove directory junction");
}

#[cfg(unix)]
fn create_directory_link(target: &std::path::Path, link: &std::path::Path) {
    std::os::unix::fs::symlink(target, link).expect("create directory symlink");
}

#[cfg(unix)]
fn remove_directory_link(link: &std::path::Path) {
    fs::remove_file(link).expect("remove directory symlink");
}

#[test]
fn canonical_pack_hash_excludes_generated_at_but_binds_provenance() {
    let original = MapPackStore::open(fixture_root(), BUILD).unwrap();

    let timestamp = TempPack::copy_valid("timestamp");
    let mut manifest = timestamp.read_json("manifest.json");
    manifest["generated_at"] = json!("2099-12-31T23:59:59Z");
    timestamp.write_json("manifest.json", &manifest);
    let timestamp = MapPackStore::open(timestamp.root(), BUILD).unwrap();
    assert_eq!(
        timestamp.canonical_pack_hash(),
        original.canonical_pack_hash()
    );

    let provenance = TempPack::copy_valid("provenance");
    let mut manifest = provenance.read_json("manifest.json");
    manifest["mapping_sha256"] =
        json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    provenance.write_json("manifest.json", &manifest);
    let provenance = MapPackStore::open(provenance.root(), BUILD).unwrap();
    assert_ne!(
        provenance.canonical_pack_hash(),
        original.canonical_pack_hash()
    );
}

#[test]
fn canonical_hash_contract_has_a_golden_vector() {
    let store = MapPackStore::open(fixture_root(), BUILD).unwrap();
    let encoded = support::lowercase_hex(store.canonical_pack_hash());
    assert_eq!(
        encoded,
        "ab470ec052957b06a9088d580bcfcf1f5ab8df677581fe7be9def4ef89c8f7f1"
    );
    assert_eq!(
        store.manifest().source_input_set_sha256_hex(),
        "18807b49ac0b33ca9888e523d642634e51c7f6fb9041d66b4bf77c7d75b920fa"
    );
}

#[test]
fn refuses_another_build_without_old_build_fallback() {
    let error = MapPackStore::open(fixture_root(), "99999999").unwrap_err();
    assert!(matches!(
        error,
        MapPackError::BuildMismatch { ref expected, ref actual }
            if expected == "99999999" && actual == BUILD
    ));
}

#[test]
fn refuses_missing_manifest_and_partial_staging() {
    let pack = TempPack::copy_valid("missing-manifest");
    fs::remove_file(pack.path("manifest.json")).expect("remove manifest");
    let error = MapPackStore::open(pack.root(), BUILD).unwrap_err();
    assert_sanitized(&error, pack.root());

    let pack = TempPack::copy_valid("partial-staging");
    fs::remove_file(pack.path("pois.json")).expect("remove POIs");
    let error = MapPackStore::open(pack.root(), BUILD).unwrap_err();
    assert_sanitized(&error, pack.root());
}

#[test]
fn refuses_unknown_missing_and_mismatched_manifest_fields() {
    let cases: Vec<JsonMutation> = vec![
        ("unknown", Box::new(|value| value["surprise"] = json!(true))),
        (
            "missing",
            Box::new(|value| {
                value.as_object_mut().unwrap().remove("mapping_sha256");
            }),
        ),
        (
            "schema",
            Box::new(|value| value["schema_version"] = json!(1)),
        ),
        (
            "transform-schema",
            Box::new(|value| value["coordinate_transform_version"] = json!(1)),
        ),
        (
            "poi-schema",
            Box::new(|value| value["poi_schema_version"] = json!(1)),
        ),
        (
            "cue",
            Box::new(|value| value["cue4parse_version"] = json!("wrong")),
        ),
        (
            "build-empty",
            Box::new(|value| value["game_build_id"] = json!("")),
        ),
        (
            "build-nondigit",
            Box::new(|value| value["game_build_id"] = json!("24x")),
        ),
        (
            "width-zero",
            Box::new(|value| value["map_width_px"] = json!(0)),
        ),
        (
            "width-large",
            Box::new(|value| value["map_width_px"] = json!(8193)),
        ),
        (
            "core",
            Box::new(|value| value["tile_core_size_px"] = json!(256)),
        ),
        (
            "gutter",
            Box::new(|value| value["tile_gutter_px"] = json!(1)),
        ),
    ];

    for (label, mutate) in cases {
        let pack = TempPack::copy_valid(label);
        let mut manifest = pack.read_json("manifest.json");
        mutate(&mut manifest);
        pack.write_json("manifest.json", &manifest);
        let error = MapPackStore::open(pack.root(), BUILD).unwrap_err();
        assert_sanitized(&error, pack.root());
    }
}

#[test]
fn refuses_uppercase_short_and_nonhex_hashes() {
    for (label, hash) in [
        (
            "uppercase",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ),
        ("short", "aa"),
        (
            "nonhex",
            "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg",
        ),
    ] {
        let pack = TempPack::copy_valid(label);
        let mut manifest = pack.read_json("manifest.json");
        manifest["mapping_sha256"] = json!(hash);
        pack.write_json("manifest.json", &manifest);
        let error = MapPackStore::open(pack.root(), BUILD).unwrap_err();
        assert_sanitized(&error, pack.root());
    }
}

#[test]
fn canonical_source_container_digest_is_order_independent() {
    let pack = TempPack::copy_valid("container-order");
    let mut manifest = pack.read_json("manifest.json");
    manifest["source_containers"]
        .as_array_mut()
        .unwrap()
        .reverse();
    pack.write_json("manifest.json", &manifest);

    let reordered = MapPackStore::open(pack.root(), BUILD).expect("input order is irrelevant");
    let original = MapPackStore::open(fixture_root(), BUILD).expect("valid fixture opens");
    assert_eq!(
        reordered.canonical_pack_hash(),
        original.canonical_pack_hash()
    );
}

#[test]
fn refuses_tampered_source_container_fingerprints_and_duplicate_names() {
    for (label, mutate) in [
        ("container-size", 0_u8),
        ("container-hash", 1_u8),
        ("container-duplicate", 2_u8),
    ] {
        let pack = TempPack::copy_valid(label);
        let mut manifest = pack.read_json("manifest.json");
        match mutate {
            0 => manifest["source_containers"][0]["size_bytes"] = json!(999),
            1 => {
                manifest["source_containers"][0]["sha256"] =
                    json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            }
            _ => {
                let duplicate = manifest["source_containers"][0].clone();
                manifest["source_containers"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
        }
        pack.write_json("manifest.json", &manifest);
        let error = MapPackStore::open(pack.root(), BUILD).unwrap_err();
        assert_sanitized(&error, pack.root());
    }
}

#[test]
fn refuses_unsafe_source_container_paths() {
    for (label, unsafe_name) in [
        ("parent", "../pakchunk0.pak"),
        ("dot", "Pal/./pakchunk0.pak"),
        ("empty-component", "Pal//pakchunk0.pak"),
        ("absolute", "/Pal/pakchunk0.pak"),
        ("drive", "C:/Pal/pakchunk0.pak"),
        ("unc", "//server/share/pakchunk0.pak"),
        ("alternate-separator", "Pal\\..\\pakchunk0.pak"),
        ("colon", "Pal/pak:chunk0.pak"),
        ("trailing-slash", "Pal/pakchunk0.pak/"),
        ("leading-space", " Pal/pakchunk0.pak"),
        ("trailing-space", "Pal/pakchunk0.pak "),
        ("trailing-dot", "Pal/pakchunk0.pak."),
        ("dos-device", "Pal/CON.pak"),
        ("nul", "Pal/nu\u{0}l.pak"),
        ("less-than", "Pal/pak<chunk.pak"),
        ("greater-than", "Pal/pak>chunk.pak"),
        ("quote", "Pal/pak\"chunk.pak"),
        ("pipe", "Pal/pak|chunk.pak"),
        ("question", "Pal/pak?chunk.pak"),
        ("star", "Pal/pak*chunk.pak"),
        ("conin", "Pal/CONIN$.pak"),
        ("conout", "Pal/CONOUT$.pak"),
        ("clock", "Pal/CLOCK$.pak"),
    ] {
        let pack = TempPack::copy_valid(label);
        let mut manifest = pack.read_json("manifest.json");
        manifest["source_containers"][0]["relative_name"] = json!(unsafe_name);
        pack.write_json("manifest.json", &manifest);
        let error = MapPackStore::open(pack.root(), BUILD).unwrap_err();
        assert_sanitized(&error, pack.root());
    }
}

#[test]
fn refuses_source_path_case_collisions_and_bounded_field_overflow() {
    let collision = TempPack::copy_valid("case-collision");
    let mut manifest = collision.read_json("manifest.json");
    let mut duplicate = manifest["source_containers"][0].clone();
    duplicate["relative_name"] = json!(
        manifest["source_containers"][0]["relative_name"]
            .as_str()
            .unwrap()
            .to_ascii_uppercase()
    );
    manifest["source_containers"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    collision.write_json("manifest.json", &manifest);
    assert!(MapPackStore::open(collision.root(), BUILD).is_err());

    let long_path = TempPack::copy_valid("long-path");
    let mut manifest = long_path.read_json("manifest.json");
    manifest["source_containers"][0]["relative_name"] = json!("a".repeat(241));
    long_path.write_json("manifest.json", &manifest);
    assert!(MapPackStore::open(long_path.root(), BUILD).is_err());

    let oversized = TempPack::copy_valid("oversized-manifest");
    let mut bytes = fs::read(oversized.path("manifest.json")).unwrap();
    bytes.extend(std::iter::repeat_n(b' ', 1_100_000));
    fs::write(oversized.path("manifest.json"), bytes).unwrap();
    assert!(MapPackStore::open(oversized.root(), BUILD).is_err());
}

#[test]
fn refuses_symlink_or_reparse_escape_when_the_platform_can_create_one() {
    let pack = TempPack::copy_valid("reparse");
    let outside = TempPack::copy_valid("reparse-outside");
    fs::remove_file(pack.path(MAIN_TRANSFORM)).expect("remove original transform");

    #[cfg(windows)]
    let linked =
        std::os::windows::fs::symlink_file(outside.path(MAIN_TRANSFORM), pack.path(MAIN_TRANSFORM))
            .is_ok();
    #[cfg(unix)]
    let linked =
        std::os::unix::fs::symlink(outside.path(MAIN_TRANSFORM), pack.path(MAIN_TRANSFORM)).is_ok();
    #[cfg(not(any(windows, unix)))]
    let linked = false;

    if linked {
        let error = MapPackStore::open(pack.root(), BUILD).unwrap_err();
        assert_sanitized(&error, pack.root());
    }
}

#[test]
fn refuses_one_byte_mutation_of_each_runtime_verifiable_asset() {
    for (label, relative) in [
        ("main-transform", MAIN_TRANSFORM),
        ("tree-transform", "regions/tree/dummyregion/transform.json"),
        ("pois", "pois.json"),
        ("main-tile-index", MAIN_INDEX),
        ("tree-tile-index", TREE_INDEX),
        (
            "main-tile",
            concat!("regions/mainmap/firstregion/tiles", "/0/0_0.jpg"),
        ),
        ("tree-tile", "regions/tree/dummyregion/tiles/0/0_0.jpg"),
    ] {
        let pack = TempPack::copy_valid(label);
        let path = pack.path(relative);
        let mut bytes = fs::read(&path).expect("read mutation target");
        bytes[0] ^= 1;
        fs::write(path, bytes).expect("mutate target");
        let error = MapPackStore::open(pack.root(), BUILD).unwrap_err();
        assert_sanitized(&error, pack.root());
    }
}

#[test]
fn public_errors_never_echo_the_absolute_root_or_raw_json() {
    let pack = TempPack::copy_valid("sanitized");
    fs::write(
        pack.path("manifest.json"),
        br#"{"game_build_id":"synthetic-fast-secret","raw":"do-not-echo"}"#,
    )
    .expect("write malformed manifest");
    let error = MapPackStore::open(pack.root(), BUILD).unwrap_err();
    assert_sanitized(&error, pack.root());
    assert!(!error.to_string().contains("do-not-echo"));
    let debug = format!("{error:?}");
    assert!(!debug.contains(&pack.root().display().to_string()));
    assert!(!debug.contains("do-not-echo"));
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        let text = cause.to_string();
        assert!(!text.contains(&pack.root().display().to_string()));
        assert!(!text.contains("do-not-echo"));
        source = cause.source();
    }
}
