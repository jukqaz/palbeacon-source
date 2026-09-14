mod support;

use pal_map_pack_store::{CullingQuery, MapPackStore, MapRect, PoiFilterMask, PoiKind};
use serde_json::json;
use support::{TempPack, fixture_root, main_region_pack};

const BUILD: &str = "24181527";

#[test]
fn canonical_poi_order_and_handles_do_not_depend_on_json_order() {
    let original = MapPackStore::open(fixture_root(), BUILD).unwrap();
    let original_ids: Vec<_> = main_region_pack(&original)
        .poi_index()
        .all()
        .iter()
        .map(|poi| poi.id())
        .collect();
    assert_eq!(original_ids, ["fast-1", "boss-1", "dungeon-1"]);

    let pack = TempPack::copy_valid("poi-order");
    let mut pois = pack.read_json("pois.json");
    pois["pois"].as_array_mut().unwrap().reverse();
    pack.write_json("pois.json", &pois);
    pack.refresh_manifest_file_hash("pois.json", "pois_sha256");
    let reordered = MapPackStore::open(pack.root(), BUILD).expect("JSON order is irrelevant");
    let reordered_ids: Vec<_> = main_region_pack(&reordered)
        .poi_index()
        .all()
        .iter()
        .map(|poi| poi.id())
        .collect();
    assert_eq!(reordered_ids, original_ids);
}

#[test]
fn filters_and_culls_pois_with_boundary_inclusion() {
    let store = MapPackStore::open(fixture_root(), BUILD).unwrap();
    let query = CullingQuery::new(
        MapRect::new(
            511.717_283_268_912_2,
            246.670_347_874_102_7,
            512.0,
            247.094_422_970_734_42,
        )
        .unwrap(),
    );
    let mut output = Vec::new();

    let main = main_region_pack(&store);
    main.poi_index()
        .visible_into(&query, PoiFilterMask::none(), &mut output);
    assert!(output.is_empty());

    main.poi_index().visible_into(
        &query,
        PoiFilterMask::from_kinds([PoiKind::FastTravel, PoiKind::Dungeon]),
        &mut output,
    );
    let ids: Vec<_> = output
        .iter()
        .map(|handle| main.poi_index().get(*handle).unwrap().id())
        .collect();
    assert_eq!(ids, ["fast-1", "dungeon-1"]);

    main.poi_index()
        .visible_into(&query, PoiFilterMask::all(), &mut output);
    assert_eq!(output.len(), 3);
}

#[test]
fn optional_entity_id_is_validated_and_exposed_for_portrait_resolution() {
    let pack = TempPack::copy_valid("poi-entity-id");
    let mut pois = pack.read_json("pois.json");
    pois["pois"][1]["entity_id"] = json!("Anubis");
    pack.write_json("pois.json", &pois);
    pack.refresh_manifest_file_hash("pois.json", "pois_sha256");

    let store = MapPackStore::open(pack.root(), BUILD).expect("valid entity id");
    let boss = main_region_pack(&store)
        .poi_index()
        .all()
        .iter()
        .find(|poi| poi.kind() == PoiKind::Boss)
        .expect("boss");
    assert_eq!(boss.entity_id(), Some("Anubis"));

    let invalid = TempPack::copy_valid("poi-invalid-entity-id");
    let mut invalid_pois = invalid.read_json("pois.json");
    invalid_pois["pois"][1]["entity_id"] = json!("../Anubis");
    invalid.write_json("pois.json", &invalid_pois);
    invalid.refresh_manifest_file_hash("pois.json", "pois_sha256");
    assert!(MapPackStore::open(invalid.root(), BUILD).is_err());
}

#[test]
fn wanted_targets_are_a_distinct_filterable_kind() {
    let pack = TempPack::copy_valid("poi-wanted-kind");
    let mut pois = pack.read_json("pois.json");
    let mut wanted = pois["pois"][1].clone();
    wanted["id"] = json!("wanted-1");
    wanted["kind"] = json!("wanted");
    pois["pois"].as_array_mut().unwrap().push(wanted);
    pois["poi_count"] = json!(4);
    pack.write_json("pois.json", &pois);
    pack.refresh_manifest_file_hash("pois.json", "pois_sha256");

    let store = MapPackStore::open(pack.root(), BUILD).expect("wanted POI kind");
    let main = main_region_pack(&store);
    let wanted = main
        .poi_index()
        .all()
        .iter()
        .find(|poi| poi.kind() == PoiKind::Wanted)
        .expect("wanted target");
    assert_eq!(wanted.id(), "wanted-1");

    let query = CullingQuery::new(MapRect::new(0.0, 0.0, 1024.0, 1024.0).unwrap());
    let mut output = Vec::new();
    main.poi_index().visible_into(
        &query,
        PoiFilterMask::from_kinds([PoiKind::Wanted]),
        &mut output,
    );
    assert_eq!(output.len(), 1);
    assert_eq!(main.poi_index().get(output[0]).unwrap().id(), "wanted-1");
}

#[test]
fn refuses_missing_kind_duplicate_id_invalid_coordinates_build_and_verification() {
    let cases = [
        ("missing-kind", 0_u8),
        ("duplicate-id", 1),
        ("empty-id", 2),
        ("nonfinite", 3),
        ("out-of-bounds", 4),
        ("wrong-build", 5),
        ("unverified", 6),
        ("parity", 7),
    ];

    for (label, case) in cases {
        let pack = TempPack::copy_valid(label);
        let mut pois = pack.read_json("pois.json");
        match case {
            0 => {
                pois["pois"].as_array_mut().unwrap().pop();
            }
            1 => pois["pois"][1]["id"] = pois["pois"][0]["id"].clone(),
            2 => pois["pois"][0]["id"] = json!(""),
            3 => pois["pois"][0]["world_x"] = json!(1.0e308),
            4 => pois["pois"][0]["map_x"] = json!(2000.0),
            5 => pois["pois"][0]["source_build_id"] = json!("99999999"),
            6 => pois["pois"][0]["verified"] = json!(false),
            _ => pois["pois"][0]["map_x"] = json!(100.02),
        }
        pack.write_json("pois.json", &pois);
        pack.refresh_manifest_file_hash("pois.json", "pois_sha256");
        assert!(
            MapPackStore::open(pack.root(), BUILD).is_err(),
            "case {label}"
        );
    }
}

#[test]
fn poi_document_is_strict_counted_bounded_and_uses_euclidean_parity() {
    for (label, case) in [
        ("schema", 0_u8),
        ("build", 1),
        ("region", 2),
        ("count", 3),
        ("unknown", 4),
        ("non-ascii-id", 5),
        ("long-id", 6),
        ("long-name", 7),
        ("euclidean-parity", 8),
    ] {
        let pack = TempPack::copy_valid(label);
        let mut pois = pack.read_json("pois.json");
        match case {
            0 => pois["schema_version"] = json!(1),
            1 => pois["game_build_id"] = json!("99999999"),
            2 => pois["pois"][0]["region_id"] = json!("MissingRegion"),
            3 => pois["poi_count"] = json!(4),
            4 => pois["unexpected"] = json!(true),
            5 => pois["pois"][0]["id"] = json!("던전"),
            6 => pois["pois"][0]["id"] = json!("a".repeat(129)),
            7 => pois["pois"][0]["display_name"] = json!("a".repeat(257)),
            _ => {
                pois["pois"][0]["map_x"] = json!(0.008);
                pois["pois"][0]["map_y"] = json!(300.008);
            }
        }
        pack.write_json("pois.json", &pois);
        pack.refresh_manifest_file_hash("pois.json", "pois_sha256");
        assert!(
            MapPackStore::open(pack.root(), BUILD).is_err(),
            "case {label}"
        );
    }
}

#[test]
fn poi_in_an_overlap_must_belong_to_the_unique_highest_priority_region() {
    let pack = TempPack::copy_valid("poi-overlap-owner");
    let matrix = [
        [0.0, 1024.0 / 1_448_800.0, 724_400.0 * 1024.0 / 1_448_800.0],
        [-1024.0 / 1_448_800.0, 0.0, 349_400.0 * 1024.0 / 1_448_800.0],
    ];

    let mut transform = pack.read_json("regions/mainmap/firstregion/transform.json");
    transform["matrix"] = json!(matrix);
    transform["calibration"]["parity_points"] = json!(main_map_parity_points(matrix));
    pack.write_json("regions/mainmap/firstregion/transform.json", &transform);
    pack.refresh_manifest_file_hash(
        "regions/mainmap/firstregion/transform.json",
        "transform_sha256",
    );

    let mut manifest = pack.read_json("manifest.json");
    manifest["map_regions"][0]["world_to_map_matrix"] = json!(matrix);
    pack.write_json("manifest.json", &manifest);

    let mut pois = pack.read_json("pois.json");
    for poi in pois["pois"].as_array_mut().unwrap() {
        let world_x = poi["world_x"].as_f64().unwrap();
        let world_y = poi["world_y"].as_f64().unwrap();
        let (map_x, map_y) = project(matrix, world_x, world_y);
        poi["map_x"] = json!(map_x);
        poi["map_y"] = json!(map_y);
    }
    let overlap = &mut pois["pois"][0];
    overlap["world_x"] = json!(348_000.0);
    overlap["world_y"] = json!(-500_000.0);
    let (map_x, map_y) = project(matrix, 348_000.0, -500_000.0);
    overlap["map_x"] = json!(map_x);
    overlap["map_y"] = json!(map_y);
    assert_eq!(overlap["map_id"], json!("MainMap"));
    pack.write_json("pois.json", &pois);
    pack.refresh_manifest_file_hash("pois.json", "pois_sha256");

    assert!(
        MapPackStore::open(pack.root(), BUILD).is_err(),
        "the overlapping coordinate is owned by higher-priority Tree, not claimed MainMap"
    );
}

fn main_map_parity_points(matrix: [[f64; 3]; 2]) -> Vec<serde_json::Value> {
    let mut points = Vec::with_capacity(25);
    for grid_y in 0..=4 {
        for grid_x in 0..=4 {
            let expected_x = 1024.0 * f64::from(grid_x) / 4.0;
            let expected_y = 1024.0 * f64::from(grid_y) / 4.0;
            let world_y = (expected_x - matrix[0][2]) / matrix[0][1];
            let world_x = (expected_y - matrix[1][2]) / matrix[1][0];
            points.push(json!({
                "world_x": world_x,
                "world_y": world_y,
                "expected_map_x": expected_x,
                "expected_map_y": expected_y,
            }));
        }
    }
    points
}

fn project(matrix: [[f64; 3]; 2], world_x: f64, world_y: f64) -> (f64, f64) {
    (
        matrix[0][0] * world_x + matrix[0][1] * world_y + matrix[0][2],
        matrix[1][0] * world_x + matrix[1][1] * world_y + matrix[1][2],
    )
}
