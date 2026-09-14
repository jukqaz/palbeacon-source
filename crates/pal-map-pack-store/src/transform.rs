use std::collections::BTreeSet;

use serde::Deserialize;
use thiserror::Error;

use crate::{MapPackError, MapRegion, TRANSFORM_SCHEMA_VERSION, parse_hash};

const PARITY_POINT_COUNT: usize = 25;
const PARITY_TOLERANCE_PX: f64 = 0.01;
const RELATIVE_DETERMINANT_EPSILON: f64 = 1.0e-12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldPoint {
    x: f64,
    y: f64,
}

impl WorldPoint {
    pub fn new(x: f64, y: f64) -> Result<Self, TransformError> {
        if !x.is_finite() {
            return Err(TransformError::WorldPointXNonFinite);
        }
        if !y.is_finite() {
            return Err(TransformError::WorldPointYNonFinite);
        }
        Ok(Self { x, y })
    }

    pub const fn x(self) -> f64 {
        self.x
    }

    pub const fn y(self) -> f64 {
        self.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapPoint {
    x: f64,
    y: f64,
}

impl MapPoint {
    pub fn new(x: f64, y: f64) -> Result<Self, TransformError> {
        if !x.is_finite() {
            return Err(TransformError::MapPointXNonFinite);
        }
        if !y.is_finite() {
            return Err(TransformError::MapPointYNonFinite);
        }
        Ok(Self { x, y })
    }

    pub const fn x(self) -> f64 {
        self.x
    }

    pub const fn y(self) -> f64 {
        self.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapRect {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl MapRect {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Result<Self, TransformError> {
        if ![min_x, min_y, max_x, max_y].into_iter().all(f64::is_finite) {
            return Err(TransformError::NonFiniteRectangle);
        }
        if min_x > max_x || min_y > max_y {
            return Err(TransformError::DegenerateRectangle);
        }
        Ok(Self {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }

    pub const fn min_x(self) -> f64 {
        self.min_x
    }

    pub const fn min_y(self) -> f64 {
        self.min_y
    }

    pub const fn max_x(self) -> f64 {
        self.max_x
    }

    pub const fn max_y(self) -> f64 {
        self.max_y
    }

    pub fn contains(self, point: MapPoint) -> bool {
        point.x >= self.min_x
            && point.x <= self.max_x
            && point.y >= self.min_y
            && point.y <= self.max_y
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum TransformError {
    #[error("transform matrix coefficients must be finite")]
    NonFiniteMatrix,
    #[error("transform matrix must be nonsingular at its coefficient scale")]
    SingularMatrix,
    #[error("world point x coordinate must be finite")]
    WorldPointXNonFinite,
    #[error("world point y coordinate must be finite")]
    WorldPointYNonFinite,
    #[error("map point x coordinate must be finite")]
    MapPointXNonFinite,
    #[error("map point y coordinate must be finite")]
    MapPointYNonFinite,
    #[error("map rectangle coordinates must be finite")]
    NonFiniteRectangle,
    #[error("map rectangle must be nondegenerate")]
    DegenerateRectangle,
}

impl TransformError {
    pub const fn field(self) -> Option<&'static str> {
        match self {
            Self::WorldPointXNonFinite => Some("world_point.x"),
            Self::WorldPointYNonFinite => Some("world_point.y"),
            Self::MapPointXNonFinite => Some("map_point.x"),
            Self::MapPointYNonFinite => Some("map_point.y"),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WorldToMapTransform {
    matrix: [[f64; 3]; 2],
    calibration: Option<CalibrationEvidence>,
}

impl WorldToMapTransform {
    pub fn new(matrix: [[f64; 3]; 2]) -> Result<Self, TransformError> {
        if !matrix.iter().flatten().copied().all(f64::is_finite) {
            return Err(TransformError::NonFiniteMatrix);
        }
        let linear_scale = matrix[0][0]
            .abs()
            .max(matrix[0][1].abs())
            .max(matrix[1][0].abs())
            .max(matrix[1][1].abs());
        if linear_scale == 0.0 {
            return Err(TransformError::SingularMatrix);
        }
        let normalized_determinant = (matrix[0][0] / linear_scale) * (matrix[1][1] / linear_scale)
            - (matrix[0][1] / linear_scale) * (matrix[1][0] / linear_scale);
        if !normalized_determinant.is_finite()
            || normalized_determinant.abs() <= RELATIVE_DETERMINANT_EPSILON
        {
            return Err(TransformError::SingularMatrix);
        }
        Ok(Self {
            matrix,
            calibration: None,
        })
    }

    pub fn project(&self, point: WorldPoint) -> Result<MapPoint, TransformError> {
        let x = self.matrix[0][0] * point.x + self.matrix[0][1] * point.y + self.matrix[0][2];
        let y = self.matrix[1][0] * point.x + self.matrix[1][1] * point.y + self.matrix[1][2];
        MapPoint::new(x, y)
    }

    pub const fn matrix(&self) -> [[f64; 3]; 2] {
        self.matrix
    }

    pub(crate) fn parse_region(
        bytes: &[u8],
        build: &str,
        region: &MapRegion,
    ) -> Result<Self, MapPackError> {
        let raw: RawTransformDocument =
            serde_json::from_slice(bytes).map_err(|_| MapPackError::ParseFailed {
                component: "transform",
            })?;
        if raw.schema_version != TRANSFORM_SCHEMA_VERSION {
            return invalid("schema_version");
        }
        if raw.game_build_id != build {
            return invalid("game_build_id");
        }
        if raw.map_id != region.map_id() || raw.region_id != region.region_id() {
            return invalid("region_identity");
        }
        if raw.map_width_px != region.map_width_px() || raw.map_height_px != region.map_height_px()
        {
            return invalid("map_dimensions");
        }
        let mut transform = Self::new(raw.matrix)?;
        if transform.matrix() != region.world_to_map_transform().matrix() {
            return invalid("manifest_matrix");
        }
        match (raw.transform_kind.as_str(), raw.calibration) {
            ("reference_fitted_affine_validated", Some(calibration))
                if region.map_id() == "MainMap" && region.region_id() == "FirstRegion" =>
            {
                transform.calibration = Some(CalibrationEvidence::validate(
                    calibration,
                    &transform,
                    region.map_width_px(),
                    region.map_height_px(),
                )?);
            }
            ("authoritative_world_bounds_validated", Some(calibration))
                if region.map_id() == "MainMap" && region.region_id() == "FirstRegion" =>
            {
                if transform.matrix() != authoritative_bounds_matrix(region) {
                    return invalid("authoritative_world_bounds_matrix");
                }
                transform.calibration = Some(CalibrationEvidence::validate(
                    calibration,
                    &transform,
                    region.map_width_px(),
                    region.map_height_px(),
                )?);
            }
            ("authoritative_region_bounds", None)
                if region.map_id() == "Tree" && region.region_id() == "DummyRegion" =>
            {
                if transform.matrix() != authoritative_bounds_matrix(region) {
                    return invalid("authoritative_region_bounds_matrix");
                }
            }
            _ => return invalid("transform_kind"),
        }
        Ok(transform)
    }
}

fn authoritative_bounds_matrix(region: &MapRegion) -> [[f64; 3]; 2] {
    let (min_x, min_y, max_x, max_y) = region.world_bounds();
    let map_x_scale = f64::from(region.map_width_px()) / (max_y - min_y);
    let map_y_scale = f64::from(region.map_height_px()) / (max_x - min_x);
    [
        [0.0, map_x_scale, -min_y * map_x_scale],
        [-map_y_scale, 0.0, max_x * map_y_scale],
    ]
}

#[derive(Clone, Debug)]
struct CalibrationEvidence {
    _landmark_count: u32,
    _reference_count: u32,
    _sealed_holdout_count: u32,
    _reference_median_error_px: f64,
    _reference_max_error_px: f64,
    _sealed_holdout_median_error_px: f64,
    _sealed_holdout_max_error_px: f64,
    _sealed_holdout_sha256: [u8; 32],
}

impl CalibrationEvidence {
    fn validate(
        raw: RawCalibrationEvidence,
        transform: &WorldToMapTransform,
        map_width_px: u32,
        map_height_px: u32,
    ) -> Result<Self, MapPackError> {
        let reference_sum = raw
            .reference_center_count
            .checked_add(raw.reference_north_west_count)
            .and_then(|value| value.checked_add(raw.reference_north_east_count))
            .and_then(|value| value.checked_add(raw.reference_south_west_count))
            .and_then(|value| value.checked_add(raw.reference_south_east_count));
        let sealed_holdout_sum = raw
            .sealed_holdout_center_count
            .checked_add(raw.sealed_holdout_north_west_count)
            .and_then(|value| value.checked_add(raw.sealed_holdout_north_east_count))
            .and_then(|value| value.checked_add(raw.sealed_holdout_south_west_count))
            .and_then(|value| value.checked_add(raw.sealed_holdout_south_east_count));
        let landmark_sum = raw.reference_count.checked_add(raw.sealed_holdout_count);
        if raw.landmark_count != 15
            || raw.reference_count != 10
            || raw.sealed_holdout_count != 5
            || raw.reference_center_count != 2
            || raw.reference_north_west_count != 2
            || raw.reference_north_east_count != 2
            || raw.reference_south_west_count != 2
            || raw.reference_south_east_count != 2
            || raw.sealed_holdout_center_count != 1
            || raw.sealed_holdout_north_west_count != 1
            || raw.sealed_holdout_north_east_count != 1
            || raw.sealed_holdout_south_west_count != 1
            || raw.sealed_holdout_south_east_count != 1
            || reference_sum != Some(raw.reference_count)
            || sealed_holdout_sum != Some(raw.sealed_holdout_count)
            || landmark_sum != Some(raw.landmark_count)
        {
            return invalid("calibration_counts");
        }
        if !valid_error_pair(raw.reference_median_error_px, raw.reference_max_error_px)
            || !valid_error_pair(
                raw.sealed_holdout_median_error_px,
                raw.sealed_holdout_max_error_px,
            )
        {
            return invalid("calibration_errors");
        }
        let sealed_holdout_sha256 = parse_hash(
            &raw.sealed_holdout_sha256,
            "calibration.sealed_holdout_sha256",
        )?;
        if sealed_holdout_sha256 == [0; 32] {
            return invalid("sealed_holdout_sha256");
        }
        if raw.parity_points.len() != PARITY_POINT_COUNT {
            return invalid("parity_points");
        }
        let mut world_points = BTreeSet::new();
        let mut expected_points = BTreeSet::new();
        let mut covered_grid_slots = BTreeSet::new();
        for parity in raw.parity_points {
            if ![
                parity.world_x,
                parity.world_y,
                parity.expected_map_x,
                parity.expected_map_y,
            ]
            .into_iter()
            .all(f64::is_finite)
            {
                return invalid("parity_points");
            }
            if parity.expected_map_x < 0.0
                || parity.expected_map_x > f64::from(map_width_px)
                || parity.expected_map_y < 0.0
                || parity.expected_map_y > f64::from(map_height_px)
            {
                return invalid("parity_points");
            }
            if !world_points.insert((
                coordinate_bits(parity.world_x),
                coordinate_bits(parity.world_y),
            )) || !expected_points.insert((
                coordinate_bits(parity.expected_map_x),
                coordinate_bits(parity.expected_map_y),
            )) {
                return invalid("parity_points");
            }

            let slot = grid_slot(
                parity.expected_map_x,
                parity.expected_map_y,
                map_width_px,
                map_height_px,
            )
            .ok_or(MapPackError::InvalidField {
                component: "transform",
                field: "parity_points",
            })?;
            if !covered_grid_slots.insert(slot) {
                return invalid("parity_points");
            }

            let projected = transform.project(WorldPoint::new(parity.world_x, parity.world_y)?)?;
            let error =
                (projected.x - parity.expected_map_x).hypot(projected.y - parity.expected_map_y);
            if !error.is_finite() || error > PARITY_TOLERANCE_PX {
                return invalid("parity_points");
            }
        }
        if covered_grid_slots.len() != PARITY_POINT_COUNT {
            return invalid("parity_points");
        }
        Ok(Self {
            _landmark_count: raw.landmark_count,
            _reference_count: raw.reference_count,
            _sealed_holdout_count: raw.sealed_holdout_count,
            _reference_median_error_px: raw.reference_median_error_px,
            _reference_max_error_px: raw.reference_max_error_px,
            _sealed_holdout_median_error_px: raw.sealed_holdout_median_error_px,
            _sealed_holdout_max_error_px: raw.sealed_holdout_max_error_px,
            _sealed_holdout_sha256: sealed_holdout_sha256,
        })
    }
}

fn valid_error_pair(median: f64, maximum: f64) -> bool {
    median.is_finite()
        && maximum.is_finite()
        && median >= 0.0
        && maximum >= 0.0
        && median <= 10.0
        && maximum <= 25.0
        && median <= maximum
}

fn coordinate_bits(value: f64) -> u64 {
    if value == 0.0 {
        0.0_f64.to_bits()
    } else {
        value.to_bits()
    }
}

fn grid_slot(
    expected_x: f64,
    expected_y: f64,
    map_width_px: u32,
    map_height_px: u32,
) -> Option<(u8, u8)> {
    let mut match_found = None;
    for grid_y in 0_u8..=4 {
        for grid_x in 0_u8..=4 {
            let slot_x = f64::from(map_width_px) * f64::from(grid_x) / 4.0;
            let slot_y = f64::from(map_height_px) * f64::from(grid_y) / 4.0;
            if (expected_x - slot_x).hypot(expected_y - slot_y) <= PARITY_TOLERANCE_PX {
                if match_found.is_some() {
                    return None;
                }
                match_found = Some((grid_x, grid_y));
            }
        }
    }
    match_found
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTransformDocument {
    schema_version: u32,
    game_build_id: String,
    map_id: String,
    region_id: String,
    transform_kind: String,
    map_width_px: u32,
    map_height_px: u32,
    matrix: [[f64; 3]; 2],
    calibration: Option<RawCalibrationEvidence>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCalibrationEvidence {
    landmark_count: u32,
    reference_count: u32,
    sealed_holdout_count: u32,
    reference_center_count: u32,
    reference_north_west_count: u32,
    reference_north_east_count: u32,
    reference_south_west_count: u32,
    reference_south_east_count: u32,
    sealed_holdout_center_count: u32,
    sealed_holdout_north_west_count: u32,
    sealed_holdout_north_east_count: u32,
    sealed_holdout_south_west_count: u32,
    sealed_holdout_south_east_count: u32,
    reference_median_error_px: f64,
    reference_max_error_px: f64,
    sealed_holdout_median_error_px: f64,
    sealed_holdout_max_error_px: f64,
    sealed_holdout_sha256: String,
    parity_points: Vec<RawParityPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawParityPoint {
    world_x: f64,
    world_y: f64,
    expected_map_x: f64,
    expected_map_y: f64,
}

fn invalid<T>(field: &'static str) -> Result<T, MapPackError> {
    Err(MapPackError::InvalidField {
        component: "transform",
        field,
    })
}
