use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

use crate::{
    CanonicalWriter, MAP_PACK_SCHEMA_VERSION, MapPackError, MapPackManifest, MapRect, MapRegion,
    TILE_INDEX_SCHEMA_VERSION, integrity, parse_hash,
};

const MAX_TILES: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileKey {
    pub level: u8,
    pub y: u32,
    pub x: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileLevel {
    level: u8,
    width_px: u32,
    height_px: u32,
    grid_width: u32,
    grid_height: u32,
}

impl TileLevel {
    pub const fn level(self) -> u8 {
        self.level
    }

    pub const fn width_px(self) -> u32 {
        self.width_px
    }

    pub const fn height_px(self) -> u32 {
        self.height_px
    }

    pub const fn grid_width(self) -> u32 {
        self.grid_width
    }

    pub const fn grid_height(self) -> u32 {
        self.grid_height
    }
}

#[derive(Clone, Debug)]
pub struct TileDescriptor {
    key: TileKey,
    relative_path: String,
    size_bytes: u64,
    sha256: [u8; 32],
    map_rect: MapRect,
}

impl TileDescriptor {
    pub const fn key(&self) -> TileKey {
        self.key
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub const fn sha256(&self) -> [u8; 32] {
        self.sha256
    }

    pub const fn map_rect(&self) -> MapRect {
        self.map_rect
    }
}

#[derive(Clone, Debug)]
pub struct TileIndex {
    levels: Vec<TileLevel>,
    entries: Vec<TileDescriptor>,
}

impl TileIndex {
    pub fn levels(&self) -> &[TileLevel] {
        &self.levels
    }

    pub fn all(&self) -> &[TileDescriptor] {
        &self.entries
    }

    pub fn get(&self, key: TileKey) -> Option<&TileDescriptor> {
        self.entries
            .binary_search_by_key(&key, TileDescriptor::key)
            .ok()
            .map(|index| &self.entries[index])
    }

    pub(crate) fn parse_and_verify(
        bytes: &[u8],
        root: &Path,
        manifest: &MapPackManifest,
        region: &MapRegion,
    ) -> Result<Self, MapPackError> {
        let raw: RawTileIndex =
            serde_json::from_slice(bytes).map_err(|_| MapPackError::ParseFailed {
                component: "tile_index",
            })?;
        if raw.schema_version != TILE_INDEX_SCHEMA_VERSION
            || raw.schema_version != MAP_PACK_SCHEMA_VERSION
        {
            return invalid("schema_version");
        }
        if raw.game_build_id != manifest.game_build_id() {
            return invalid("game_build_id");
        }
        if raw.map_id != region.map_id() || raw.region_id != region.region_id() {
            return invalid("region_identity");
        }
        if raw.map_width_px != region.map_width_px() || raw.map_height_px != region.map_height_px()
        {
            return invalid("map_dimensions");
        }
        if raw.tile_core_size_px != manifest.tile_core_size_px()
            || raw.tile_gutter_px != manifest.tile_gutter_px()
        {
            return invalid("tile_geometry");
        }

        let expected_levels = expected_pyramid(
            region.map_width_px(),
            region.map_height_px(),
            manifest.tile_core_size_px(),
        )?;
        if raw.level_count as usize != raw.levels.len() || raw.levels.len() != expected_levels.len()
        {
            return invalid("level_count");
        }
        let mut levels_by_key = BTreeMap::new();
        for raw_level in raw.levels {
            let level = TileLevel {
                level: raw_level.level,
                width_px: raw_level.width_px,
                height_px: raw_level.height_px,
                grid_width: raw_level.grid_width,
                grid_height: raw_level.grid_height,
            };
            if levels_by_key.insert(level.level, level).is_some() {
                return Err(MapPackError::DuplicateEntry {
                    component: "tile_level",
                });
            }
        }
        for expected in &expected_levels {
            if levels_by_key.get(&expected.level) != Some(expected) {
                return invalid("levels");
            }
        }

        let expected_tile_count: usize = expected_levels
            .iter()
            .map(|level| (level.grid_width as usize) * (level.grid_height as usize))
            .sum();
        if raw.tile_count as usize != raw.tiles.len()
            || raw.tiles.len() != expected_tile_count
            || raw.tiles.len() > MAX_TILES
        {
            return invalid("tile_count");
        }

        let mut expected_keys = BTreeSet::new();
        for level in &expected_levels {
            for y in 0..level.grid_height {
                for x in 0..level.grid_width {
                    expected_keys.insert(TileKey {
                        level: level.level,
                        y,
                        x,
                    });
                }
            }
        }

        let mut seen_keys = BTreeSet::new();
        let mut seen_paths = BTreeSet::new();
        let mut folded_paths = BTreeSet::new();
        let mut entries = Vec::with_capacity(raw.tiles.len());
        for raw_tile in raw.tiles {
            let key = TileKey {
                level: raw_tile.level,
                y: raw_tile.y,
                x: raw_tile.x,
            };
            if !seen_keys.insert(key) {
                return Err(MapPackError::DuplicateEntry {
                    component: "tile_key",
                });
            }
            if !expected_keys.contains(&key) {
                return invalid("tiles.key");
            }
            integrity::validate_relative_name(&raw_tile.relative_path, "tiles.relative_path")?;
            let expected_path = format!(
                "{}/tiles/{}/{}_{}.jpg",
                region.path_prefix(),
                key.level,
                key.y,
                key.x
            );
            if raw_tile.relative_path != expected_path {
                return invalid("tiles.relative_path");
            }
            if !seen_paths.insert(raw_tile.relative_path.clone())
                || !folded_paths.insert(raw_tile.relative_path.to_ascii_lowercase())
            {
                return Err(MapPackError::DuplicateEntry {
                    component: "tile_path",
                });
            }
            if raw_tile.size_bytes == 0 || raw_tile.size_bytes > integrity::MAX_TILE_BYTES {
                return Err(MapPackError::SizeLimit { component: "tile" });
            }
            let sha256 = parse_hash(&raw_tile.sha256, "tiles.sha256")?;
            let map_rect = MapRect::new(
                raw_tile.map_rect.min_x,
                raw_tile.map_rect.min_y,
                raw_tile.map_rect.max_x,
                raw_tile.map_rect.max_y,
            )?;
            let expected_rect = expected_map_rect(key, manifest.tile_core_size_px(), region)?;
            if map_rect != expected_rect {
                return invalid("tiles.map_rect");
            }
            entries.push(TileDescriptor {
                key,
                relative_path: raw_tile.relative_path,
                size_bytes: raw_tile.size_bytes,
                sha256,
                map_rect,
            });
        }
        if seen_keys != expected_keys {
            return Err(MapPackError::MissingTile);
        }
        entries.sort_by_key(TileDescriptor::key);

        for descriptor in &entries {
            integrity::verify_streaming_file(
                root,
                &descriptor.relative_path,
                "tile",
                descriptor.size_bytes,
                &descriptor.sha256,
            )?;
        }
        let expected_directories: BTreeSet<_> = expected_levels
            .iter()
            .map(|level| format!("{}/tiles/{}", region.path_prefix(), level.level))
            .collect();
        integrity::validate_tile_tree(
            root,
            &format!("{}/tiles", region.path_prefix()),
            &seen_paths,
            &expected_directories,
        )?;

        if canonical_tile_set_hash(&entries) != *region.tile_set_sha256() {
            return Err(MapPackError::HashMismatch {
                component: "tile_set",
            });
        }
        Ok(Self {
            levels: expected_levels,
            entries,
        })
    }
}

fn expected_pyramid(
    mut width: u32,
    mut height: u32,
    core: u32,
) -> Result<Vec<TileLevel>, MapPackError> {
    let mut levels = Vec::new();
    for level in 0_u8..=31 {
        levels.push(TileLevel {
            level,
            width_px: width,
            height_px: height,
            grid_width: width.div_ceil(core),
            grid_height: height.div_ceil(core),
        });
        if width <= core && height <= core {
            return Ok(levels);
        }
        width = width.div_ceil(2);
        height = height.div_ceil(2);
    }
    invalid("levels")
}

fn expected_map_rect(
    key: TileKey,
    tile_core_size_px: u32,
    region: &MapRegion,
) -> Result<MapRect, MapPackError> {
    let scale = 1_u64
        .checked_shl(u32::from(key.level))
        .ok_or(MapPackError::InvalidField {
            component: "tile_index",
            field: "tiles.level",
        })?;
    let span =
        u64::from(tile_core_size_px)
            .checked_mul(scale)
            .ok_or(MapPackError::InvalidField {
                component: "tile_index",
                field: "tiles.level",
            })?;
    let min_x = u64::from(key.x) * span;
    let min_y = u64::from(key.y) * span;
    let max_x = (min_x + span).min(u64::from(region.map_width_px()));
    let max_y = (min_y + span).min(u64::from(region.map_height_px()));
    MapRect::new(min_x as f64, min_y as f64, max_x as f64, max_y as f64)
        .map_err(MapPackError::Transform)
}

fn canonical_tile_set_hash(entries: &[TileDescriptor]) -> [u8; 32] {
    let mut writer = CanonicalWriter::new(b"pal-tile-set-v1\0");
    writer.u32(u32::try_from(entries.len()).expect("tile cap fits u32"));
    for entry in entries {
        writer.u8(entry.key.level);
        writer.u32(entry.key.y);
        writer.u32(entry.key.x);
        writer.string(&entry.relative_path);
        writer.u64(entry.size_bytes);
        writer.hash(&entry.sha256);
    }
    writer.finish()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTileIndex {
    schema_version: u32,
    game_build_id: String,
    map_id: String,
    region_id: String,
    map_width_px: u32,
    map_height_px: u32,
    tile_core_size_px: u32,
    tile_gutter_px: u32,
    level_count: u32,
    tile_count: u32,
    levels: Vec<RawTileLevel>,
    tiles: Vec<RawTileDescriptor>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTileLevel {
    level: u8,
    width_px: u32,
    height_px: u32,
    grid_width: u32,
    grid_height: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTileDescriptor {
    level: u8,
    y: u32,
    x: u32,
    relative_path: String,
    size_bytes: u64,
    sha256: String,
    map_rect: RawMapRect,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMapRect {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

fn invalid<T>(field: &'static str) -> Result<T, MapPackError> {
    Err(MapPackError::InvalidField {
        component: "tile_index",
        field,
    })
}
