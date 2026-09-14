use std::fs;
use std::path::PathBuf;

use pal_map_pack_store::{MapPoint, WorldPoint, WorldToMapTransform};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParityFixture {
    producer: String,
    schema_version: u32,
    matrix: [[f64; 3]; 2],
    points: Vec<ParityPoint>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParityPoint {
    world_x: f64,
    world_y: f64,
    expected_map_x: f64,
    expected_map_y: f64,
}

#[test]
fn csharp_coordinate_solver_matches_rust_at_twenty_five_points() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tools")
        .join("pal-map-pack")
        .join("tests")
        .join("fixtures")
        .join("transform-parity.json");
    let bytes = fs::read(path).expect("read C# transform parity fixture");
    let fixture: ParityFixture =
        serde_json::from_slice(&bytes).expect("parse strict parity fixture");
    assert_eq!(fixture.producer, "PalMapPack.CoordinateSolver");
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.points.len(), 25);

    let transform = WorldToMapTransform::new(fixture.matrix).expect("valid affine transform");
    for point in fixture.points {
        let projected = transform
            .project(WorldPoint::new(point.world_x, point.world_y).expect("finite world point"))
            .expect("finite projection");
        let expected =
            MapPoint::new(point.expected_map_x, point.expected_map_y).expect("finite map point");
        let error = ((projected.x() - expected.x()).powi(2)
            + (projected.y() - expected.y()).powi(2))
        .sqrt();
        assert!(
            error <= 0.01,
            "C#-to-Rust parity error {error}px exceeds 0.01px"
        );
    }
}
