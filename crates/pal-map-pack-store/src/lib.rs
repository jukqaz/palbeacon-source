//! Strict, immutable runtime boundary for locally extracted map packs.
//!
//! Task 5A validates synthetic metadata and runtime-verifiable files only. Source-contract,
//! mapping and original-map hashes remain provenance fields whose source content is proven by the
//! extractor gate. `std` path validation narrows races but cannot eliminate all TOCTOU attacks;
//! every tile read therefore reopens the file, rejects reparse points and rechecks size/hash.

mod integrity;
mod manifest;
mod poi;
mod tile_index;
mod transform;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub use manifest::{MapPackManifest, MapRegion, SourceContainerFingerprint};
pub use poi::{CullingQuery, Poi, PoiFilterMask, PoiHandle, PoiIndex, PoiKind};
pub use tile_index::{TileDescriptor, TileIndex, TileKey, TileLevel};
pub use transform::{MapPoint, MapRect, TransformError, WorldPoint, WorldToMapTransform};

pub const MAP_PACK_SCHEMA_VERSION: u32 = 2;
pub const TRANSFORM_SCHEMA_VERSION: u32 = 2;
pub const POI_SCHEMA_VERSION: u32 = 2;
pub const TILE_INDEX_SCHEMA_VERSION: u32 = 2;
pub const EXPECTED_CUE4PARSE_VERSION: &str = "1.2.2.202607";
const MAX_ACTIVE_POINTER_BYTES: u64 = 16 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawActivePackPointer {
    schema_version: u32,
    game_build_id: String,
    relative_version_path: String,
    manifest_sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvenanceStatus {
    /// Provenance hashes are strictly formatted and bound into the pack identity, but their
    /// original external inputs are not available to the runtime for recomputation.
    FormatBoundOnly,
}

#[derive(Debug, Error)]
pub enum MapPackError {
    #[error("expected build identifier is invalid")]
    InvalidExpectedBuild,
    #[error("map pack build mismatch: expected {expected}, found {actual}")]
    BuildMismatch { expected: String, actual: String },
    #[error("required map pack component is missing: {component}")]
    MissingComponent { component: &'static str },
    #[error("map pack component could not be read: {component}")]
    ReadFailed { component: &'static str },
    #[error("map pack component exceeds its size limit: {component}")]
    SizeLimit { component: &'static str },
    #[error("map pack JSON is invalid: {component}")]
    ParseFailed { component: &'static str },
    #[error("map pack field is invalid: {component}.{field}")]
    InvalidField {
        component: &'static str,
        field: &'static str,
    },
    #[error("map pack hash is malformed: {field}")]
    InvalidHash { field: &'static str },
    #[error("map pack content hash mismatch: {component}")]
    HashMismatch { component: &'static str },
    #[error("map pack relative path is unsafe: {field}")]
    UnsafePath { field: &'static str },
    #[error("map pack path crosses a link or reparse point: {component}")]
    ReparsePoint { component: &'static str },
    #[error("map pack path escapes its root: {component}")]
    PathEscape { component: &'static str },
    #[error("map pack contains a duplicate entry: {component}")]
    DuplicateEntry { component: &'static str },
    #[error("map region selection coordinate is non-finite")]
    InvalidMapRegionCoordinate,
    #[error("world coordinate is outside every map region")]
    OutsideMapRegions,
    #[error("map regions have an ambiguous highest-priority overlap")]
    AmbiguousMapRegions,
    #[error("map pack is missing an indexed tile")]
    MissingTile,
    #[error("map pack does not index tile {level}/{y}_{x}")]
    UnknownTile { level: u8, y: u32, x: u32 },
    #[error("map pack contains an unindexed or nonregular tile")]
    OrphanTile,
    #[error("map pack transform is invalid: {0}")]
    Transform(#[from] TransformError),
}

pub struct MapPackStore {
    manifest: MapPackManifest,
    region_packs: Vec<MapRegionPack>,
    canonical_pack_hash: [u8; 32],
}

pub struct MapRegionPack {
    root: PathBuf,
    region: MapRegion,
    transform: WorldToMapTransform,
    poi_index: PoiIndex,
    tile_index: TileIndex,
}

impl fmt::Debug for MapRegionPack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MapRegionPack")
            .field("map_id", &self.region.map_id())
            .field("region_id", &self.region.region_id())
            .field("poi_count", &self.poi_index.all().len())
            .field("tile_count", &self.tile_index.all().len())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for MapPackStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MapPackStore")
            .field("game_build_id", &self.manifest.game_build_id())
            .field("region_count", &self.region_packs.len())
            .finish_non_exhaustive()
    }
}

impl MapPackStore {
    pub fn open_published(
        dataset_root: impl AsRef<Path>,
        expected_build: &str,
    ) -> Result<Arc<Self>, MapPackError> {
        if !manifest::valid_build_id(expected_build) {
            return Err(MapPackError::InvalidExpectedBuild);
        }
        let dataset_root = integrity::validate_root(dataset_root.as_ref())?;
        let pointer_name = format!("{expected_build}.active.json");
        let pointer_bytes = integrity::read_bounded(
            &dataset_root,
            &pointer_name,
            "active_pointer",
            MAX_ACTIVE_POINTER_BYTES,
        )?;
        let pointer: RawActivePackPointer =
            serde_json::from_slice(&pointer_bytes).map_err(|_| MapPackError::ParseFailed {
                component: "active_pointer",
            })?;
        if pointer.schema_version != 1 || pointer.game_build_id != expected_build {
            return Err(MapPackError::InvalidField {
                component: "active_pointer",
                field: "identity",
            });
        }
        integrity::validate_relative_name(
            &pointer.relative_version_path,
            "active_pointer.relative_version_path",
        )?;
        let Some(version_name) = pointer.relative_version_path.strip_prefix(".versions/") else {
            return Err(MapPackError::UnsafePath {
                field: "active_pointer.relative_version_path",
            });
        };
        if version_name.is_empty() || version_name.contains('/') {
            return Err(MapPackError::UnsafePath {
                field: "active_pointer.relative_version_path",
            });
        }
        let expected_manifest_hash =
            parse_hash(&pointer.manifest_sha256, "active_pointer.manifest_sha256")?;
        let version_root = integrity::resolve_existing_directory(
            &dataset_root,
            &pointer.relative_version_path,
            "version_root",
        )?;
        let manifest_bytes = integrity::read_bounded(
            &version_root,
            "manifest.json",
            "manifest",
            integrity::MAX_MANIFEST_BYTES,
        )?;
        let actual_manifest_hash: [u8; 32] = Sha256::digest(&manifest_bytes).into();
        if actual_manifest_hash != expected_manifest_hash {
            return Err(MapPackError::HashMismatch {
                component: "active_pointer",
            });
        }
        Self::open(version_root, expected_build)
    }

    pub fn open(root: impl AsRef<Path>, expected_build: &str) -> Result<Arc<Self>, MapPackError> {
        if !manifest::valid_build_id(expected_build) {
            return Err(MapPackError::InvalidExpectedBuild);
        }

        let root = integrity::validate_root(root.as_ref())?;
        let manifest_bytes = integrity::read_bounded(
            &root,
            "manifest.json",
            "manifest",
            integrity::MAX_MANIFEST_BYTES,
        )?;
        let manifest = MapPackManifest::parse(&manifest_bytes, expected_build)?;

        let poi_bytes = integrity::read_hashed(
            &root,
            "pois.json",
            "pois",
            integrity::MAX_POIS_BYTES,
            manifest.pois_sha256(),
        )?;
        let poi_indexes = PoiIndex::parse_by_region(
            &poi_bytes,
            manifest.game_build_id(),
            manifest.map_regions(),
        )?;

        let mut region_packs = Vec::with_capacity(manifest.map_regions().len());
        for (region, poi_index) in manifest.map_regions().iter().cloned().zip(poi_indexes) {
            let transform_bytes = integrity::read_hashed(
                &root,
                region.transform_relative_path(),
                "transform",
                integrity::MAX_TRANSFORM_BYTES,
                region.transform_sha256(),
            )?;
            let transform = WorldToMapTransform::parse_region(
                &transform_bytes,
                manifest.game_build_id(),
                &region,
            )?;
            let tile_index_bytes = integrity::read_hashed(
                &root,
                region.tile_index_relative_path(),
                "tile_index",
                integrity::MAX_TILE_INDEX_BYTES,
                region.tile_index_sha256(),
            )?;
            let tile_index =
                TileIndex::parse_and_verify(&tile_index_bytes, &root, &manifest, &region)?;
            region_packs.push(MapRegionPack {
                root: root.clone(),
                region,
                transform,
                poi_index,
                tile_index,
            });
        }

        let canonical_pack_hash = manifest.canonical_pack_hash();
        Ok(Arc::new(Self {
            manifest,
            region_packs,
            canonical_pack_hash,
        }))
    }

    pub const fn manifest(&self) -> &MapPackManifest {
        &self.manifest
    }

    pub fn map_regions(&self) -> &[MapRegion] {
        self.manifest.map_regions()
    }

    pub fn select_map_region(
        &self,
        world_x: f64,
        world_y: f64,
    ) -> Result<&MapRegion, MapPackError> {
        self.manifest.select_map_region(world_x, world_y)
    }

    pub fn select_region_pack(
        &self,
        world_x: f64,
        world_y: f64,
    ) -> Result<&MapRegionPack, MapPackError> {
        let index = self.select_region_pack_index(world_x, world_y)?;
        self.region_pack(index)
            .ok_or(MapPackError::MissingComponent {
                component: "map_region_pack",
            })
    }

    pub fn select_region_pack_index(
        &self,
        world_x: f64,
        world_y: f64,
    ) -> Result<usize, MapPackError> {
        let selected = self.manifest.select_map_region(world_x, world_y)?;
        self.region_packs
            .iter()
            .position(|pack| {
                pack.region.map_id() == selected.map_id()
                    && pack.region.region_id() == selected.region_id()
            })
            .ok_or(MapPackError::MissingComponent {
                component: "map_region_pack",
            })
    }

    pub fn region_pack(&self, index: usize) -> Option<&MapRegionPack> {
        self.region_packs.get(index)
    }

    pub const fn canonical_pack_hash(&self) -> [u8; 32] {
        self.canonical_pack_hash
    }

    pub const fn provenance_status(&self) -> ProvenanceStatus {
        ProvenanceStatus::FormatBoundOnly
    }
}

impl MapRegionPack {
    pub const fn region(&self) -> &MapRegion {
        &self.region
    }

    pub const fn transform(&self) -> &WorldToMapTransform {
        &self.transform
    }

    pub const fn poi_index(&self) -> &PoiIndex {
        &self.poi_index
    }

    pub const fn tile_index(&self) -> &TileIndex {
        &self.tile_index
    }

    pub fn tile_descriptor(&self, key: TileKey) -> Option<&TileDescriptor> {
        self.tile_index.get(key)
    }

    /// Reopens and authenticates a tile against this selected region's index on every call.
    pub fn read_verified_tile(&self, key: TileKey) -> Result<Vec<u8>, MapPackError> {
        let descriptor = self.tile_descriptor(key).ok_or(MapPackError::UnknownTile {
            level: key.level,
            y: key.y,
            x: key.x,
        })?;
        integrity::read_verified_bytes(
            &self.root,
            descriptor.relative_path(),
            "tile",
            descriptor.size_bytes(),
            &descriptor.sha256(),
        )
    }
}

pub(crate) fn parse_hash(value: &str, field: &'static str) -> Result<[u8; 32], MapPackError> {
    if value.len() != 64
        || !value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(MapPackError::InvalidHash { field });
    }
    let mut decoded = [0_u8; 32];
    for (index, output) in decoded.iter_mut().enumerate() {
        let high = hex_nibble(value.as_bytes()[index * 2]);
        let low = hex_nibble(value.as_bytes()[index * 2 + 1]);
        *output = (high << 4) | low;
    }
    Ok(decoded)
}

pub(crate) fn encode_hash(hash: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in hash {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

const fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => 0,
    }
}

pub(crate) struct CanonicalWriter {
    hasher: Sha256,
}

impl CanonicalWriter {
    pub fn new(domain: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(domain);
        Self { hasher }
    }

    pub fn u8(&mut self, value: u8) {
        self.hasher.update([value]);
    }

    pub fn u32(&mut self, value: u32) {
        self.hasher.update(value.to_le_bytes());
    }

    pub fn u64(&mut self, value: u64) {
        self.hasher.update(value.to_le_bytes());
    }

    pub fn i32(&mut self, value: i32) {
        self.hasher.update(value.to_le_bytes());
    }

    pub fn f64(&mut self, value: f64) {
        self.hasher.update(value.to_bits().to_le_bytes());
    }

    pub fn string(&mut self, value: &str) {
        self.u32(u32::try_from(value.len()).expect("validated strings fit u32"));
        self.hasher.update(value.as_bytes());
    }

    pub fn hash(&mut self, value: &[u8; 32]) {
        self.hasher.update(value);
    }

    pub fn finish(self) -> [u8; 32] {
        self.hasher.finalize().into()
    }
}
