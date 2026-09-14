mod support;

use pal_map_pack_store::{MapPackStore, MapPoint, TransformError, WorldPoint, WorldToMapTransform};
use serde_json::json;
use support::{MAIN_TRANSFORM, TREE_TRANSFORM, TempPack};

const BUILD: &str = "24181527";

#[test]
fn public_points_reject_each_nonfinite_coordinate_with_an_exact_field() {
    for (error, expected, field) in [
        (
            WorldPoint::new(f64::NAN, 0.0).unwrap_err(),
            TransformError::WorldPointXNonFinite,
            "world_point.x",
        ),
        (
            WorldPoint::new(0.0, f64::INFINITY).unwrap_err(),
            TransformError::WorldPointYNonFinite,
            "world_point.y",
        ),
        (
            MapPoint::new(f64::NEG_INFINITY, 0.0).unwrap_err(),
            TransformError::MapPointXNonFinite,
            "map_point.x",
        ),
        (
            MapPoint::new(0.0, f64::NAN).unwrap_err(),
            TransformError::MapPointYNonFinite,
            "map_point.y",
        ),
    ] {
        assert_eq!(error, expected);
        assert_eq!(error.field(), Some(field));
    }
}

#[test]
fn applies_the_affine_world_to_map_transform() {
    let transform = WorldToMapTransform::new([[0.5, 0.0, 100.0], [0.0, -0.5, 200.0]])
        .expect("valid affine transform");
    assert_eq!(
        transform
            .project(WorldPoint::new(20.0, 40.0).unwrap())
            .unwrap(),
        MapPoint::new(110.0, 180.0).unwrap()
    );
    assert_eq!(transform.matrix(), [[0.5, 0.0, 100.0], [0.0, -0.5, 200.0]]);
}

#[test]
fn rejects_nonfinite_singular_near_singular_and_overflowing_transforms() {
    for matrix in [
        [[f64::NAN, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [[f64::INFINITY, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [[1.0, 2.0, 0.0], [2.0, 4.0, 0.0]],
        [[1.0, 0.0, 0.0], [0.0, 1.0e-13, 0.0]],
    ] {
        assert!(WorldToMapTransform::new(matrix).is_err());
    }

    let transform = WorldToMapTransform::new([[1.0e308, 0.0, 0.0], [0.0, 1.0e308, 0.0]])
        .expect("finite nonsingular matrix");
    assert!(
        transform
            .project(WorldPoint::new(2.0, 0.0).unwrap())
            .is_err()
    );
}

#[test]
fn determinant_rejection_is_scale_relative() {
    assert!(WorldToMapTransform::new([[1.0e9, 0.0, 0.0], [0.0, 1.0e-4, 0.0]]).is_err());
    assert!(WorldToMapTransform::new([[1.0e9, 0.0, 0.0], [0.0, 1.0e9, 0.0]]).is_ok());
}

#[test]
fn validates_calibration_count_quadrants_and_error_boundaries() {
    let at_error_boundaries = TempPack::copy_valid("calibration-error-boundaries");
    let mut transform = at_error_boundaries.read_json(MAIN_TRANSFORM);
    for pointer in [
        "/calibration/reference_median_error_px",
        "/calibration/sealed_holdout_median_error_px",
    ] {
        *transform.pointer_mut(pointer).unwrap() = json!(10.0);
    }
    for pointer in [
        "/calibration/reference_max_error_px",
        "/calibration/sealed_holdout_max_error_px",
    ] {
        *transform.pointer_mut(pointer).unwrap() = json!(25.0);
    }
    at_error_boundaries.write_json(MAIN_TRANSFORM, &transform);
    at_error_boundaries.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");
    MapPackStore::open(at_error_boundaries.root(), BUILD)
        .expect("10px/25px boundaries are inclusive for both evidence sets");

    let cases = [
        ("landmarks", "/calibration/landmark_count", json!(9)),
        (
            "reference-center",
            "/calibration/reference_center_count",
            json!(1),
        ),
        (
            "reference-north-west",
            "/calibration/reference_north_west_count",
            json!(1),
        ),
        (
            "sealed-holdout-center",
            "/calibration/sealed_holdout_center_count",
            json!(0),
        ),
        (
            "sealed-holdout-south-east",
            "/calibration/sealed_holdout_south_east_count",
            json!(0),
        ),
        (
            "reference-median",
            "/calibration/reference_median_error_px",
            json!(10.0001),
        ),
        (
            "reference-max",
            "/calibration/reference_max_error_px",
            json!(25.0001),
        ),
        (
            "sealed-holdout-median",
            "/calibration/sealed_holdout_median_error_px",
            json!(10.0001),
        ),
        (
            "sealed-holdout-max",
            "/calibration/sealed_holdout_max_error_px",
            json!(25.0001),
        ),
        (
            "reference-median-exceeds-max",
            "/calibration/reference_median_error_px",
            json!(1.0),
        ),
        (
            "sealed-holdout-median-exceeds-max",
            "/calibration/sealed_holdout_median_error_px",
            json!(1.0),
        ),
        (
            "reference-count-sum",
            "/calibration/reference_count",
            json!(6),
        ),
        (
            "sealed-holdout-count-sum",
            "/calibration/sealed_holdout_count",
            json!(6),
        ),
    ];
    for (label, pointer, replacement) in cases {
        let pack = TempPack::copy_valid(label);
        let mut transform = pack.read_json(MAIN_TRANSFORM);
        *transform.pointer_mut(pointer).unwrap() = replacement;
        pack.write_json(MAIN_TRANSFORM, &transform);
        pack.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");
        assert!(
            MapPackStore::open(pack.root(), BUILD).is_err(),
            "case {label}"
        );
    }
}

#[test]
fn rejects_internally_consistent_extra_calibration_samples() {
    let pack = TempPack::copy_valid("extra-calibration-reference");
    let mut transform = pack.read_json(MAIN_TRANSFORM);
    transform["calibration"]["landmark_count"] = json!(16);
    transform["calibration"]["reference_count"] = json!(11);
    transform["calibration"]["reference_center_count"] = json!(3);
    pack.write_json(MAIN_TRANSFORM, &transform);
    pack.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");

    assert!(MapPackStore::open(pack.root(), BUILD).is_err());
}

#[test]
fn sealed_holdout_evidence_hash_is_strict_lowercase_sha256() {
    for (label, value) in [
        ("short", json!("00")),
        (
            "all-zero",
            json!("0000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "upper",
            json!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        ),
        (
            "non-hex",
            json!("gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg"),
        ),
    ] {
        let pack = TempPack::copy_valid(label);
        let mut transform = pack.read_json(MAIN_TRANSFORM);
        transform["calibration"]["sealed_holdout_sha256"] = value;
        pack.write_json(MAIN_TRANSFORM, &transform);
        pack.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");
        assert!(MapPackStore::open(pack.root(), BUILD).is_err());
    }
}

#[test]
fn transform_document_is_strict_and_matches_build_and_dimensions() {
    for (label, pointer, value) in [
        ("schema", "/schema_version", json!(1)),
        ("build", "/game_build_id", json!("99999999")),
        ("width", "/map_width_px", json!(512)),
        ("height", "/map_height_px", json!(512)),
    ] {
        let pack = TempPack::copy_valid(label);
        let mut transform = pack.read_json(MAIN_TRANSFORM);
        *transform.pointer_mut(pointer).unwrap() = value;
        pack.write_json(MAIN_TRANSFORM, &transform);
        pack.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");
        assert!(MapPackStore::open(pack.root(), BUILD).is_err());
    }

    let pack = TempPack::copy_valid("unknown-transform-field");
    let mut transform = pack.read_json(MAIN_TRANSFORM);
    transform["unexpected"] = json!(true);
    pack.write_json(MAIN_TRANSFORM, &transform);
    pack.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");
    assert!(MapPackStore::open(pack.root(), BUILD).is_err());
}

#[test]
fn requires_exactly_twenty_five_projection_parity_points() {
    for (label, remove, extra) in [("short", true, false), ("long", false, true)] {
        let pack = TempPack::copy_valid(label);
        let mut transform = pack.read_json(MAIN_TRANSFORM);
        let points = transform["calibration"]["parity_points"]
            .as_array_mut()
            .unwrap();
        if remove {
            points.pop();
        }
        if extra {
            points.push(points[0].clone());
        }
        pack.write_json(MAIN_TRANSFORM, &transform);
        pack.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");
        assert!(MapPackStore::open(pack.root(), BUILD).is_err());
    }
}

#[test]
fn parity_error_at_point_zero_one_pixels_is_inclusive_but_more_is_rejected() {
    let at_boundary = TempPack::copy_valid("parity-boundary");
    let mut transform = at_boundary.read_json(MAIN_TRANSFORM);
    let expected = transform["calibration"]["parity_points"][0]["expected_map_x"]
        .as_f64()
        .unwrap();
    transform["calibration"]["parity_points"][0]["expected_map_x"] = json!(expected + 0.01);
    at_boundary.write_json(MAIN_TRANSFORM, &transform);
    at_boundary.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");
    MapPackStore::open(at_boundary.root(), BUILD).expect("0.01px parity error is inclusive");

    let beyond = TempPack::copy_valid("parity-beyond");
    let mut transform = beyond.read_json(MAIN_TRANSFORM);
    let expected = transform["calibration"]["parity_points"][0]["expected_map_x"]
        .as_f64()
        .unwrap();
    transform["calibration"]["parity_points"][0]["expected_map_x"] = json!(expected + 0.0101);
    beyond.write_json(MAIN_TRANSFORM, &transform);
    beyond.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");
    assert!(MapPackStore::open(beyond.root(), BUILD).is_err());
}

#[test]
fn parity_evidence_rejects_duplicate_points_and_clustered_grid_slots() {
    let duplicate = TempPack::copy_valid("parity-duplicate");
    let mut transform = duplicate.read_json(MAIN_TRANSFORM);
    transform["calibration"]["parity_points"][1] =
        transform["calibration"]["parity_points"][0].clone();
    duplicate.write_json(MAIN_TRANSFORM, &transform);
    duplicate.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");
    assert!(MapPackStore::open(duplicate.root(), BUILD).is_err());

    let clustered = TempPack::copy_valid("parity-clustered");
    let mut transform = clustered.read_json(MAIN_TRANSFORM);
    transform["calibration"]["parity_points"][1] = json!({
        "world_x": -199.99,
        "world_y": -1648.0,
        "expected_map_x": 0.005,
        "expected_map_y": 1024.0
    });
    clustered.write_json(MAIN_TRANSFORM, &transform);
    clustered.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");
    assert!(MapPackStore::open(clustered.root(), BUILD).is_err());
}

#[test]
fn transform_kind_identity_and_optional_calibration_are_region_specific() {
    let legacy_main = TempPack::copy_valid("legacy-main-transform-kind");
    let mut main = legacy_main.read_json(MAIN_TRANSFORM);
    main["transform_kind"] = json!("calibrated_affine");
    legacy_main.write_json(MAIN_TRANSFORM, &main);
    legacy_main.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");
    assert!(MapPackStore::open(legacy_main.root(), BUILD).is_err());

    let main_as_bounds = TempPack::copy_valid("main-transform-kind");
    let mut main = main_as_bounds.read_json(MAIN_TRANSFORM);
    main["transform_kind"] = json!("authoritative_region_bounds");
    main["calibration"] = serde_json::Value::Null;
    main_as_bounds.write_json(MAIN_TRANSFORM, &main);
    main_as_bounds.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");
    assert!(MapPackStore::open(main_as_bounds.root(), BUILD).is_err());

    let tree_as_calibrated = TempPack::copy_valid("tree-transform-kind");
    let main = tree_as_calibrated.read_json(MAIN_TRANSFORM);
    let mut tree = tree_as_calibrated.read_json(TREE_TRANSFORM);
    tree["transform_kind"] = json!("calibrated_affine");
    tree["calibration"] = main["calibration"].clone();
    tree_as_calibrated.write_json(TREE_TRANSFORM, &tree);
    tree_as_calibrated.refresh_manifest_file_hash(TREE_TRANSFORM, "transform_sha256");
    assert!(MapPackStore::open(tree_as_calibrated.root(), BUILD).is_err());

    let wrong_identity = TempPack::copy_valid("tree-transform-identity");
    let mut tree = wrong_identity.read_json(TREE_TRANSFORM);
    tree["region_id"] = json!("FirstRegion");
    wrong_identity.write_json(TREE_TRANSFORM, &tree);
    wrong_identity.refresh_manifest_file_hash(TREE_TRANSFORM, "transform_sha256");
    assert!(MapPackStore::open(wrong_identity.root(), BUILD).is_err());
}

#[test]
fn validated_reference_fitted_affine_can_differ_from_authoritative_bounds() {
    let pack = TempPack::copy_valid("reference-fitted-affine");
    let matrix = [
        [0.000_073, 0.000_409, 318.25],
        [-0.000_351, 0.000_061, 147.75],
    ];
    let mut transform = pack.read_json(MAIN_TRANSFORM);
    transform["transform_kind"] = json!("reference_fitted_affine_validated");
    transform["matrix"] = json!(matrix);
    transform["calibration"]["parity_points"] = json!(parity_points(matrix));
    pack.write_json(MAIN_TRANSFORM, &transform);
    pack.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");

    let mut manifest = pack.read_json("manifest.json");
    manifest["map_regions"][0]["world_to_map_matrix"] = json!(matrix);
    pack.write_json("manifest.json", &manifest);
    let mut pois = pack.read_json("pois.json");
    for poi in pois["pois"].as_array_mut().expect("fixture POI array") {
        let world_x = poi["world_x"].as_f64().expect("fixture POI world x");
        let world_y = poi["world_y"].as_f64().expect("fixture POI world y");
        poi["map_x"] = json!(matrix[0][0] * world_x + matrix[0][1] * world_y + matrix[0][2]);
        poi["map_y"] = json!(matrix[1][0] * world_x + matrix[1][1] * world_y + matrix[1][2]);
    }
    pack.write_json("pois.json", &pois);
    pack.refresh_manifest_file_hash("pois.json", "pois_sha256");

    let store = MapPackStore::open(pack.root(), BUILD)
        .expect("validated reference-fitted affine should open");
    assert_eq!(
        store.map_regions()[0].world_to_map_transform().matrix(),
        matrix
    );
}

#[test]
fn reference_fitted_affine_still_requires_calibration_evidence() {
    let pack = TempPack::copy_valid("reference-fitted-without-evidence");
    let mut transform = pack.read_json(MAIN_TRANSFORM);
    transform["transform_kind"] = json!("reference_fitted_affine_validated");
    transform["calibration"] = serde_json::Value::Null;
    pack.write_json(MAIN_TRANSFORM, &transform);
    pack.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");

    assert!(MapPackStore::open(pack.root(), BUILD).is_err());
}

#[test]
fn tree_bounds_matrix_is_recomputed_instead_of_trusting_matching_manifest_bytes() {
    let pack = TempPack::copy_valid("tree-transform-recomputed");
    let mut transform = pack.read_json(TREE_TRANSFORM);
    transform["matrix"][0][2] = json!(2451.0);
    pack.write_json(TREE_TRANSFORM, &transform);
    pack.refresh_manifest_file_hash(TREE_TRANSFORM, "transform_sha256");

    let mut manifest = pack.read_json("manifest.json");
    manifest["map_regions"][1]["world_to_map_matrix"] = transform["matrix"].clone();
    pack.write_json("manifest.json", &manifest);
    assert!(MapPackStore::open(pack.root(), BUILD).is_err());
}

fn parity_points(matrix: [[f64; 3]; 2]) -> Vec<serde_json::Value> {
    let [a, b, c] = matrix[0];
    let [d, e, f] = matrix[1];
    let determinant = a * e - b * d;
    let mut points = Vec::with_capacity(25);
    for grid_y in 0..=4 {
        for grid_x in 0..=4 {
            let map_x = 1024.0 * f64::from(grid_x) / 4.0;
            let map_y = 1024.0 * f64::from(grid_y) / 4.0;
            let translated_x = map_x - c;
            let translated_y = map_y - f;
            points.push(json!({
                "world_x": (e * translated_x - b * translated_y) / determinant,
                "world_y": (-d * translated_x + a * translated_y) / determinant,
                "expected_map_x": map_x,
                "expected_map_y": map_y,
            }));
        }
    }
    points
}

#[test]
fn validated_main_bounds_matrix_is_recomputed_instead_of_trusting_matching_manifest_bytes() {
    let pack = TempPack::copy_valid("main-transform-recomputed");
    let mut transform = pack.read_json(MAIN_TRANSFORM);
    transform["matrix"][0][2] = json!(513.0);
    pack.write_json(MAIN_TRANSFORM, &transform);
    pack.refresh_manifest_file_hash(MAIN_TRANSFORM, "transform_sha256");

    let mut manifest = pack.read_json("manifest.json");
    manifest["map_regions"][0]["world_to_map_matrix"] = transform["matrix"].clone();
    pack.write_json("manifest.json", &manifest);
    assert!(MapPackStore::open(pack.root(), BUILD).is_err());
}
