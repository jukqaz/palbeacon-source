use std::collections::BTreeSet;

use serde::Deserialize;

use crate::{
    CanonicalWriter, EXPECTED_CUE4PARSE_VERSION, MAP_PACK_SCHEMA_VERSION, MapPackError,
    POI_SCHEMA_VERSION, WorldToMapTransform, encode_hash, integrity, parse_hash,
};

const MAX_SOURCE_CONTAINERS: usize = 4_096;
const MAX_SMALL_STRING: usize = 128;
const MAX_MAP_REGIONS: usize = 64;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSourceContainerFingerprint {
    relative_name: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMapRegion {
    map_id: String,
    region_id: String,
    source_texture_path: String,
    world_min_x: f64,
    world_min_y: f64,
    world_max_x: f64,
    world_max_y: f64,
    block_size_x: f64,
    block_size_y: f64,
    grid_position_x: f64,
    grid_position_y: f64,
    priority: i32,
    map_width_px: u32,
    map_height_px: u32,
    map_asset_sha256: String,
    world_to_map_matrix: [[f64; 3]; 2],
    transform_relative_path: String,
    transform_sha256: String,
    tile_set_sha256: String,
    tile_index_sha256: String,
    tile_index_relative_path: String,
}

#[derive(Clone, Debug)]
pub struct MapRegion {
    map_id: String,
    region_id: String,
    source_texture_path: String,
    world_min_x: f64,
    world_min_y: f64,
    world_max_x: f64,
    world_max_y: f64,
    block_size_x: f64,
    block_size_y: f64,
    grid_position_x: f64,
    grid_position_y: f64,
    priority: i32,
    map_width_px: u32,
    map_height_px: u32,
    map_asset_sha256: [u8; 32],
    world_to_map_transform: WorldToMapTransform,
    transform_relative_path: String,
    transform_sha256: [u8; 32],
    tile_set_sha256: [u8; 32],
    tile_index_sha256: [u8; 32],
    tile_index_relative_path: String,
}

impl MapRegion {
    pub fn map_id(&self) -> &str {
        &self.map_id
    }

    pub fn region_id(&self) -> &str {
        &self.region_id
    }

    pub fn source_texture_path(&self) -> &str {
        &self.source_texture_path
    }

    pub const fn world_bounds(&self) -> (f64, f64, f64, f64) {
        (
            self.world_min_x,
            self.world_min_y,
            self.world_max_x,
            self.world_max_y,
        )
    }

    pub const fn block_size(&self) -> (f64, f64) {
        (self.block_size_x, self.block_size_y)
    }

    pub const fn grid_position(&self) -> (f64, f64) {
        (self.grid_position_x, self.grid_position_y)
    }

    pub const fn priority(&self) -> i32 {
        self.priority
    }

    pub const fn map_width_px(&self) -> u32 {
        self.map_width_px
    }

    pub const fn map_height_px(&self) -> u32 {
        self.map_height_px
    }

    pub const fn map_asset_sha256(&self) -> [u8; 32] {
        self.map_asset_sha256
    }

    pub const fn world_to_map_transform(&self) -> &WorldToMapTransform {
        &self.world_to_map_transform
    }

    pub fn transform_relative_path(&self) -> &str {
        &self.transform_relative_path
    }

    pub const fn transform_sha256(&self) -> &[u8; 32] {
        &self.transform_sha256
    }

    pub const fn tile_set_sha256(&self) -> &[u8; 32] {
        &self.tile_set_sha256
    }

    pub const fn tile_index_sha256(&self) -> &[u8; 32] {
        &self.tile_index_sha256
    }

    pub fn tile_index_relative_path(&self) -> &str {
        &self.tile_index_relative_path
    }

    pub(crate) fn path_prefix(&self) -> String {
        format!(
            "regions/{}/{}",
            self.map_id.to_ascii_lowercase(),
            self.region_id.to_ascii_lowercase()
        )
    }

    pub(crate) fn contains(&self, world_x: f64, world_y: f64) -> bool {
        world_x >= self.world_min_x
            && world_x <= self.world_max_x
            && world_y >= self.world_min_y
            && world_y <= self.world_max_y
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.world_min_x <= other.world_max_x
            && other.world_min_x <= self.world_max_x
            && self.world_min_y <= other.world_max_y
            && other.world_min_y <= self.world_max_y
    }
}

#[derive(Clone, Debug)]
pub struct SourceContainerFingerprint {
    relative_name: String,
    size_bytes: u64,
    sha256: [u8; 32],
}

impl SourceContainerFingerprint {
    pub fn relative_name(&self) -> &str {
        &self.relative_name
    }

    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub const fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMapPackManifest {
    game_build_id: String,
    schema_version: u32,
    source_input_set_sha256: String,
    source_containers: Vec<RawSourceContainerFingerprint>,
    source_contract_sha256: String,
    mapping_sha256: String,
    map_regions: Vec<RawMapRegion>,
    pois_sha256: String,
    cue4parse_version: String,
    tile_core_size_px: u32,
    tile_gutter_px: u32,
    extractor_version: String,
    extractor_commit: String,
    coordinate_transform_version: u32,
    poi_schema_version: u32,
    generated_at: String,
}

#[derive(Clone, Debug)]
pub struct MapPackManifest {
    game_build_id: String,
    schema_version: u32,
    source_input_set_sha256: [u8; 32],
    source_containers: Vec<SourceContainerFingerprint>,
    source_contract_sha256: [u8; 32],
    mapping_sha256: [u8; 32],
    map_regions: Vec<MapRegion>,
    pois_sha256: [u8; 32],
    cue4parse_version: String,
    tile_core_size_px: u32,
    tile_gutter_px: u32,
    extractor_version: String,
    extractor_commit: String,
    coordinate_transform_version: u32,
    poi_schema_version: u32,
    generated_at: String,
}

impl MapPackManifest {
    pub(crate) fn parse(bytes: &[u8], expected_build: &str) -> Result<Self, MapPackError> {
        let raw: RawMapPackManifest =
            serde_json::from_slice(bytes).map_err(|_| MapPackError::ParseFailed {
                component: "manifest",
            })?;
        if raw.schema_version != MAP_PACK_SCHEMA_VERSION {
            return invalid("schema_version");
        }
        if !valid_build_id(&raw.game_build_id) {
            return invalid("game_build_id");
        }
        if raw.game_build_id != expected_build {
            return Err(MapPackError::BuildMismatch {
                expected: expected_build.to_owned(),
                actual: raw.game_build_id,
            });
        }
        if raw.cue4parse_version != EXPECTED_CUE4PARSE_VERSION {
            return invalid("cue4parse_version");
        }
        if raw.coordinate_transform_version != crate::TRANSFORM_SCHEMA_VERSION {
            return invalid("coordinate_transform_version");
        }
        if raw.poi_schema_version != POI_SCHEMA_VERSION {
            return invalid("poi_schema_version");
        }
        if raw.tile_core_size_px != 512 {
            return invalid("tile_core_size_px");
        }
        if raw.tile_gutter_px != 2 {
            return invalid("tile_gutter_px");
        }
        validate_small_ascii(&raw.extractor_version, "extractor_version")?;
        validate_small_ascii(&raw.extractor_commit, "extractor_commit")?;
        validate_small_ascii(&raw.generated_at, "generated_at")?;
        if raw.source_containers.is_empty() || raw.source_containers.len() > MAX_SOURCE_CONTAINERS {
            return invalid("source_containers");
        }

        let mut source_containers = Vec::with_capacity(raw.source_containers.len());
        let mut names = BTreeSet::new();
        let mut folded_names = BTreeSet::new();
        for raw_container in raw.source_containers {
            integrity::validate_relative_name(
                &raw_container.relative_name,
                "source_containers.relative_name",
            )?;
            if raw_container.size_bytes == 0
                || !names.insert(raw_container.relative_name.clone())
                || !folded_names.insert(raw_container.relative_name.to_ascii_lowercase())
            {
                return Err(MapPackError::DuplicateEntry {
                    component: "source_container",
                });
            }
            source_containers.push(SourceContainerFingerprint {
                relative_name: raw_container.relative_name,
                size_bytes: raw_container.size_bytes,
                sha256: parse_hash(&raw_container.sha256, "source_containers.sha256")?,
            });
        }
        source_containers.sort_by(|left, right| {
            left.relative_name
                .as_bytes()
                .cmp(right.relative_name.as_bytes())
        });
        let source_input_set_sha256 =
            parse_hash(&raw.source_input_set_sha256, "source_input_set_sha256")?;
        if canonical_source_input_hash(&source_containers) != source_input_set_sha256 {
            return Err(MapPackError::HashMismatch {
                component: "source_input_set",
            });
        }

        Ok(Self {
            game_build_id: raw.game_build_id,
            schema_version: raw.schema_version,
            source_input_set_sha256,
            source_containers,
            source_contract_sha256: parse_hash(
                &raw.source_contract_sha256,
                "source_contract_sha256",
            )?,
            mapping_sha256: parse_hash(&raw.mapping_sha256, "mapping_sha256")?,
            map_regions: parse_map_regions(raw.map_regions)?,
            pois_sha256: parse_hash(&raw.pois_sha256, "pois_sha256")?,
            cue4parse_version: raw.cue4parse_version,
            tile_core_size_px: raw.tile_core_size_px,
            tile_gutter_px: raw.tile_gutter_px,
            extractor_version: raw.extractor_version,
            extractor_commit: raw.extractor_commit,
            coordinate_transform_version: raw.coordinate_transform_version,
            poi_schema_version: raw.poi_schema_version,
            generated_at: raw.generated_at,
        })
    }

    pub fn game_build_id(&self) -> &str {
        &self.game_build_id
    }
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
    pub const fn source_input_set_sha256(&self) -> [u8; 32] {
        self.source_input_set_sha256
    }
    pub fn source_input_set_sha256_hex(&self) -> String {
        encode_hash(self.source_input_set_sha256)
    }
    pub fn source_containers(&self) -> &[SourceContainerFingerprint] {
        &self.source_containers
    }
    pub fn map_regions(&self) -> &[MapRegion] {
        &self.map_regions
    }
    pub const fn pois_sha256(&self) -> &[u8; 32] {
        &self.pois_sha256
    }
    pub const fn tile_core_size_px(&self) -> u32 {
        self.tile_core_size_px
    }
    pub const fn tile_gutter_px(&self) -> u32 {
        self.tile_gutter_px
    }
    pub fn generated_at(&self) -> &str {
        &self.generated_at
    }

    pub fn select_map_region(
        &self,
        world_x: f64,
        world_y: f64,
    ) -> Result<&MapRegion, MapPackError> {
        select_map_region(&self.map_regions, world_x, world_y)
    }

    pub(crate) fn canonical_pack_hash(&self) -> [u8; 32] {
        let mut writer = CanonicalWriter::new(b"pal-map-pack-v2\0");
        writer.u32(self.schema_version);
        writer.string(&self.game_build_id);
        writer.hash(&self.source_input_set_sha256);
        writer.hash(&self.source_contract_sha256);
        writer.hash(&self.mapping_sha256);
        writer.u32(u32::try_from(self.map_regions.len()).expect("map region cap fits u32"));
        for region in &self.map_regions {
            writer.string(&region.map_id);
            writer.string(&region.region_id);
            writer.string(&region.source_texture_path);
            for value in [
                region.world_min_x,
                region.world_min_y,
                region.world_max_x,
                region.world_max_y,
                region.block_size_x,
                region.block_size_y,
                region.grid_position_x,
                region.grid_position_y,
            ] {
                writer.f64(value);
            }
            writer.i32(region.priority);
            writer.u32(region.map_width_px);
            writer.u32(region.map_height_px);
            writer.hash(&region.map_asset_sha256);
            for value in region.world_to_map_transform.matrix().into_iter().flatten() {
                writer.f64(value);
            }
            writer.string(&region.transform_relative_path);
            writer.hash(&region.transform_sha256);
            writer.hash(&region.tile_set_sha256);
            writer.hash(&region.tile_index_sha256);
            writer.string(&region.tile_index_relative_path);
        }
        writer.hash(&self.pois_sha256);
        writer.string(&self.cue4parse_version);
        writer.u32(self.tile_core_size_px);
        writer.u32(self.tile_gutter_px);
        writer.string(&self.extractor_version);
        writer.string(&self.extractor_commit);
        writer.u32(self.coordinate_transform_version);
        writer.u32(self.poi_schema_version);
        writer.finish()
    }
}

pub(crate) fn select_map_region(
    regions: &[MapRegion],
    world_x: f64,
    world_y: f64,
) -> Result<&MapRegion, MapPackError> {
    if !world_x.is_finite() || !world_y.is_finite() {
        return Err(MapPackError::InvalidMapRegionCoordinate);
    }
    let highest = regions
        .iter()
        .filter(|region| region.contains(world_x, world_y))
        .map(|region| region.priority)
        .max()
        .ok_or(MapPackError::OutsideMapRegions)?;
    let mut matches = regions
        .iter()
        .filter(|region| region.priority == highest && region.contains(world_x, world_y));
    let selected = matches.next().ok_or(MapPackError::OutsideMapRegions)?;
    if matches.next().is_some() {
        return Err(MapPackError::AmbiguousMapRegions);
    }
    Ok(selected)
}

fn parse_map_regions(raw_regions: Vec<RawMapRegion>) -> Result<Vec<MapRegion>, MapPackError> {
    if raw_regions.is_empty() || raw_regions.len() > MAX_MAP_REGIONS {
        return invalid("map_regions");
    }
    let mut regions = Vec::with_capacity(raw_regions.len());
    let mut identifiers = BTreeSet::new();
    let mut folded_identifiers = BTreeSet::new();
    for raw in raw_regions {
        validate_small_ascii(&raw.map_id, "map_regions.map_id")?;
        validate_small_ascii(&raw.region_id, "map_regions.region_id")?;
        if !raw.map_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || !raw
                .region_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return invalid("map_regions.identifiers");
        }
        validate_texture_path(&raw.source_texture_path)?;
        let geometry = [
            raw.world_min_x,
            raw.world_min_y,
            raw.world_max_x,
            raw.world_max_y,
            raw.block_size_x,
            raw.block_size_y,
            raw.grid_position_x,
            raw.grid_position_y,
        ];
        if geometry.iter().any(|value| !value.is_finite())
            || raw.world_min_x >= raw.world_max_x
            || raw.world_min_y >= raw.world_max_y
            || raw.block_size_x <= 0.0
            || raw.block_size_y <= 0.0
            || raw.priority < 0
            || raw.map_width_px == 0
            || raw.map_height_px == 0
            || raw.map_width_px > 8_192
            || raw.map_height_px > 8_192
        {
            return invalid("map_regions.geometry");
        }
        let identifier = format!("{}\0{}", raw.map_id, raw.region_id);
        if !identifiers.insert(identifier.clone())
            || !folded_identifiers.insert(identifier.to_ascii_lowercase())
        {
            return Err(MapPackError::DuplicateEntry {
                component: "map_region",
            });
        }
        let prefix = format!(
            "regions/{}/{}",
            raw.map_id.to_ascii_lowercase(),
            raw.region_id.to_ascii_lowercase()
        );
        let expected_transform = format!("{prefix}/transform.json");
        let expected_index = format!("{prefix}/tile-index.json");
        integrity::validate_relative_name(
            &raw.transform_relative_path,
            "map_regions.transform_relative_path",
        )?;
        integrity::validate_relative_name(
            &raw.tile_index_relative_path,
            "map_regions.tile_index_relative_path",
        )?;
        if raw.transform_relative_path != expected_transform
            || raw.tile_index_relative_path != expected_index
        {
            return invalid("map_regions.relative_paths");
        }
        regions.push(MapRegion {
            map_id: raw.map_id,
            region_id: raw.region_id,
            source_texture_path: raw.source_texture_path,
            world_min_x: raw.world_min_x,
            world_min_y: raw.world_min_y,
            world_max_x: raw.world_max_x,
            world_max_y: raw.world_max_y,
            block_size_x: raw.block_size_x,
            block_size_y: raw.block_size_y,
            grid_position_x: raw.grid_position_x,
            grid_position_y: raw.grid_position_y,
            priority: raw.priority,
            map_width_px: raw.map_width_px,
            map_height_px: raw.map_height_px,
            map_asset_sha256: parse_hash(&raw.map_asset_sha256, "map_regions.map_asset_sha256")?,
            world_to_map_transform: WorldToMapTransform::new(raw.world_to_map_matrix)?,
            transform_relative_path: raw.transform_relative_path,
            transform_sha256: parse_hash(&raw.transform_sha256, "map_regions.transform_sha256")?,
            tile_set_sha256: parse_hash(&raw.tile_set_sha256, "map_regions.tile_set_sha256")?,
            tile_index_sha256: parse_hash(&raw.tile_index_sha256, "map_regions.tile_index_sha256")?,
            tile_index_relative_path: raw.tile_index_relative_path,
        });
    }
    regions.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.map_id.as_bytes().cmp(right.map_id.as_bytes()))
            .then_with(|| left.region_id.as_bytes().cmp(right.region_id.as_bytes()))
    });
    if regions.iter().enumerate().any(|(index, region)| {
        regions[index + 1..]
            .iter()
            .any(|other| region.priority == other.priority && region.overlaps(other))
    }) {
        return Err(MapPackError::AmbiguousMapRegions);
    }
    validate_authoritative_regions(&regions)?;
    if regions
        .iter()
        .map(|region| region.map_asset_sha256)
        .collect::<BTreeSet<_>>()
        .len()
        != regions.len()
        || regions
            .iter()
            .map(|region| region.tile_set_sha256)
            .collect::<BTreeSet<_>>()
            .len()
            != regions.len()
    {
        return invalid("map_regions.distinct_content");
    }
    Ok(regions)
}

fn validate_authoritative_regions(regions: &[MapRegion]) -> Result<(), MapPackError> {
    let exact = [
        (
            "MainMap",
            "FirstRegion",
            "/Game/Pal/Texture/UI/Map/T_WorldMap.T_WorldMap",
            -1_099_400.0,
            -724_400.0,
            349_400.0,
            724_400.0,
            0,
        ),
        (
            "Tree",
            "DummyRegion",
            "/Game/Pal/Texture/UI/Map/T_TreeMap.T_TreeMap",
            347_351.5,
            -818_197.0,
            689_148.5,
            -476_400.0,
            1,
        ),
    ];
    if regions.len() != exact.len() {
        return invalid("map_regions.authoritative_rows");
    }
    for (region, expected) in regions.iter().zip(exact) {
        if region.map_id != expected.0
            || region.region_id != expected.1
            || region.source_texture_path != expected.2
            || region.world_min_x != expected.3
            || region.world_min_y != expected.4
            || region.world_max_x != expected.5
            || region.world_max_y != expected.6
            || region.block_size_x != 1.0
            || region.block_size_y != 1.0
            || region.grid_position_x != 0.0
            || region.grid_position_y != 0.0
            || region.priority != expected.7
        {
            return invalid("map_regions.authoritative_rows");
        }
    }
    Ok(())
}

fn validate_texture_path(value: &str) -> Result<(), MapPackError> {
    if value.len() > 240
        || !value.starts_with("/Game/")
        || value.contains('\\')
        || value.bytes().any(|byte| !byte.is_ascii_graphic())
    {
        return invalid("map_regions.source_texture_path");
    }
    Ok(())
}

pub(crate) fn valid_build_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 32 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn canonical_source_input_hash(containers: &[SourceContainerFingerprint]) -> [u8; 32] {
    let mut writer = CanonicalWriter::new(b"pal-source-input-v1\0");
    writer.u32(u32::try_from(containers.len()).expect("container cap fits u32"));
    for container in containers {
        writer.string(&container.relative_name);
        writer.u64(container.size_bytes);
        writer.hash(&container.sha256);
    }
    writer.finish()
}

fn validate_small_ascii(value: &str, field: &'static str) -> Result<(), MapPackError> {
    if value.is_empty()
        || value.len() > MAX_SMALL_STRING
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return invalid(field);
    }
    Ok(())
}

fn invalid<T>(field: &'static str) -> Result<T, MapPackError> {
    Err(MapPackError::InvalidField {
        component: "manifest",
        field,
    })
}
