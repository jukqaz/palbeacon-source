use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use image::{ImageFormat, RgbImage};
use pal_domain::RESOURCE_LAYER_IDS;
use pal_map_pack_store::{MapPackError, MapPackStore, PoiKind, TileKey};
use pal_render::FilterAction;
use serde::Deserialize;
use thiserror::Error;

use crate::actual_map_preview::{
    MapPoi, MapPoiIcon, MapPoiKind, MapRaster, MapRasterError, WorldToImageTransform,
    WorldToImageTransformError,
};

const MAIN_MAP_ID: &str = "MainMap";
const MAIN_REGION_ID: &str = "FirstRegion";
const MAX_PAL_PORTRAIT_BYTES: u64 = 1_048_576;
const MAX_SPAWN_INDEX_BYTES: u64 = 4 * 1_048_576;
const MAX_SUPPLEMENTAL_LAYER_BYTES: u64 = 2 * 1_048_576;
const MAX_POI_TERMINOLOGY_BYTES: u64 = 512 * 1024;

#[derive(Debug, Error)]
pub enum ApprovedMapSurfaceError {
    #[error("published map pack failed strict verification: {0}")]
    MapPack(#[from] MapPackError),
    #[error("approved map pack is missing MainMap/FirstRegion")]
    MissingMainMap,
    #[error("approved map dimensions exceed the overlay raster budget")]
    MapDimensionsOutOfRange,
    #[error("approved level-zero tile is missing")]
    MissingLevelZeroTile,
    #[error("approved tile JPEG could not be decoded")]
    TileDecode,
    #[error("approved tile JPEG dimensions do not match the manifest")]
    TileDimensions,
    #[error("approved tile geometry overflows the destination raster")]
    TileGeometry,
    #[error("approved map raster is invalid: {0}")]
    Raster(#[from] MapRasterError),
    #[error("approved world-to-map projection is invalid: {0}")]
    Transform(#[from] WorldToImageTransformError),
    #[error("overlay Pal spawn index is invalid")]
    SpawnIndex,
    #[error("overlay supplemental map layer index is invalid")]
    SupplementalLayerIndex,
    #[error("published overlay map icon is missing or invalid for layer '{layer_id}'")]
    MapIconAsset { layer_id: String },
}

#[derive(Clone, Debug)]
pub struct PreparedApprovedMainMap {
    build_id: String,
    canonical_pack_sha256: [u8; 32],
    surface: PreparedApprovedMapRegion,
}

#[derive(Clone, Debug)]
pub struct PreparedApprovedMapPack {
    build_id: String,
    canonical_pack_sha256: [u8; 32],
    regions: Vec<PreparedApprovedMapRegion>,
}

#[derive(Clone, Debug)]
pub struct PreparedApprovedMapRegion {
    map_id: String,
    region_id: String,
    priority: i32,
    transform_sha256: [u8; 32],
    raster: Arc<MapRaster>,
    world_to_image: WorldToImageTransform,
    pois: Vec<MapPoi>,
    search_entries: Vec<MapSearchEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MapSearchKind {
    Pal,
    Poi,
    Resource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MapSearchTarget {
    Pal(String),
    Poi(MapPoiKind),
    Layer(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapSearchEntry {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: MapSearchKind,
    pub map_id: String,
    pub region_id: String,
    pub map_x: f64,
    pub map_y: f64,
    pub target: MapSearchTarget,
}

impl PreparedApprovedMapPack {
    /// Opens and authenticates every published region in the exact-build map pack.
    ///
    /// Region priority is preserved so overlapping World Tree coordinates select the same
    /// surface as `MapPackStore::select_region_pack`.
    pub fn open_published(
        dataset_root: impl AsRef<Path>,
        expected_build: &str,
    ) -> Result<Self, ApprovedMapSurfaceError> {
        let dataset_root = dataset_root.as_ref();
        let store = MapPackStore::open_published(dataset_root, expected_build)?;
        let mut portrait_cache = HashMap::<String, Arc<MapPoiIcon>>::new();
        let mut regions = Vec::with_capacity(store.map_regions().len());
        for region_index in 0..store.map_regions().len() {
            regions.push(prepare_region(
                dataset_root,
                &store,
                region_index,
                &mut portrait_cache,
            )?);
        }
        append_pal_spawn_markers(
            dataset_root,
            expected_build,
            &mut regions,
            &mut portrait_cache,
        )?;
        append_supplemental_markers(
            dataset_root,
            expected_build,
            &mut regions,
            &mut portrait_cache,
        )?;
        if !regions
            .iter()
            .any(|region| region.map_id == MAIN_MAP_ID && region.region_id == MAIN_REGION_ID)
        {
            return Err(ApprovedMapSurfaceError::MissingMainMap);
        }
        Ok(Self {
            build_id: store.manifest().game_build_id().to_owned(),
            canonical_pack_sha256: store.canonical_pack_hash(),
            regions,
        })
    }

    pub fn build_id(&self) -> &str {
        &self.build_id
    }

    pub const fn canonical_pack_sha256(&self) -> [u8; 32] {
        self.canonical_pack_sha256
    }

    pub fn regions(&self) -> &[PreparedApprovedMapRegion] {
        &self.regions
    }

    pub fn select_region_index(&self, world_x: f64, world_y: f64) -> Option<usize> {
        if !world_x.is_finite() || !world_y.is_finite() {
            return None;
        }
        let highest_priority = self
            .regions
            .iter()
            .filter(|region| region.contains_world(world_x, world_y))
            .map(|region| region.priority)
            .max()?;
        let mut matches = self.regions.iter().enumerate().filter(|(_, region)| {
            region.priority == highest_priority && region.contains_world(world_x, world_y)
        });
        let (index, _) = matches.next()?;
        matches.next().is_none().then_some(index)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpawnSearchIndex {
    schema_version: u32,
    game_build_id: String,
    verified: bool,
    placement_unit: String,
    group_unit: String,
    placement_count: usize,
    resolved_placement_count: usize,
    spawn_group_count: usize,
    species_count: usize,
    placements: Vec<SpawnPlacementRecord>,
    species: HashMap<String, Vec<SpawnSpeciesGroupRecord>>,
}

type SpawnPlacementRecord = (String, String, f64, f64, f64, String, String);
type SpawnSpeciesGroupRecord = (
    String,
    String,
    u32,
    String,
    String,
    bool,
    u32,
    u32,
    u32,
    u32,
);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SupplementalLayerIndex {
    schema_version: u32,
    game_build_id: String,
    dataset_version: String,
    coordinate_contract: String,
    provenance: serde_json::Value,
    layers: Vec<SupplementalLayer>,
    regions: Vec<SupplementalRegion>,
}

#[derive(Deserialize)]
struct PoiTerminologyIndex {
    schema_version: u32,
    language: String,
    game_build_id: String,
    terms: Vec<PoiTerminologyTerm>,
}

#[derive(Deserialize)]
struct PoiTerminologyTerm {
    id: String,
    label_ko: String,
    #[serde(default = "default_filter_visibility")]
    filter_visibility: String,
    #[serde(default)]
    merged_into: Option<String>,
}

fn default_filter_visibility() -> String {
    "visible".to_owned()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SupplementalLayer {
    index: u32,
    id: String,
    label: String,
    group: String,
    default_enabled: bool,
    minimum_scale: f64,
    dense: bool,
    point_count: usize,
}

#[derive(Deserialize)]
struct KoreanPalName {
    localized_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SupplementalRegion {
    map_id: String,
    region_id: String,
    point_count: usize,
    points: Vec<(u32, f64, f64, f64, f64)>,
}

fn append_supplemental_markers(
    dataset_root: &Path,
    expected_build: &str,
    regions: &mut [PreparedApprovedMapRegion],
    icon_cache: &mut HashMap<String, Arc<MapPoiIcon>>,
) -> Result<(), ApprovedMapSurfaceError> {
    let path = dataset_root.join("overlay").join("layers.v1.json");
    if !path.is_file() {
        return Ok(());
    }
    let terminology = load_poi_terminology(dataset_root, expected_build)?;
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| ApprovedMapSurfaceError::SupplementalLayerIndex)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_SUPPLEMENTAL_LAYER_BYTES
    {
        return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .map_err(|_| ApprovedMapSurfaceError::SupplementalLayerIndex)?,
    );
    File::open(path)
        .map_err(|_| ApprovedMapSurfaceError::SupplementalLayerIndex)?
        .take(MAX_SUPPLEMENTAL_LAYER_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ApprovedMapSurfaceError::SupplementalLayerIndex)?;
    if bytes.len() as u64 > MAX_SUPPLEMENTAL_LAYER_BYTES {
        return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
    }
    let index: SupplementalLayerIndex = serde_json::from_slice(&bytes)
        .map_err(|_| ApprovedMapSurfaceError::SupplementalLayerIndex)?;
    if index.schema_version != 1
        || index.game_build_id != expected_build
        || index.dataset_version.is_empty()
        || index.coordinate_contract != "pal-companion-map-region-projection-v1"
        || index.layers.is_empty()
        || index.layers.len() > 64
        || index.regions.is_empty()
    {
        return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
    }
    let _ = index.provenance;

    let mut layer_ids = Vec::<String>::with_capacity(index.layers.len());
    let mut layer_labels = Vec::<Option<String>>::with_capacity(index.layers.len());
    let mut public_layer_ids = Vec::<Option<String>>::with_capacity(index.layers.len());
    let mut public_layer_kinds = Vec::<Option<MapPoiKind>>::with_capacity(index.layers.len());
    let mut layer_icons = Vec::<Option<Arc<MapPoiIcon>>>::with_capacity(index.layers.len());
    let mut declared_counts = vec![0_usize; index.layers.len()];
    for (expected_index, layer) in index.layers.into_iter().enumerate() {
        if layer.index != u32::try_from(expected_index).expect("at most 64 layers")
            || !valid_filter_id(&layer.id)
            || layer.label.is_empty()
            || layer.group.is_empty()
            || !layer.minimum_scale.is_finite()
        {
            return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
        }
        let _ = (layer.default_enabled, layer.dense);
        let Some(source_term) = terminology.get(&layer.id) else {
            return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
        };
        let public_layer = if source_term.filter_visibility == "hidden_until_verified" {
            None
        } else {
            public_supplemental_layer(&layer.id, &terminology)
        };
        let (public_kind, public_id, public_label) = match public_layer {
            Some((kind, id, label)) => (Some(kind), Some(id.to_owned()), Some(label.to_owned())),
            None if source_term.filter_visibility == "hidden_until_verified" => (None, None, None),
            None => return Err(ApprovedMapSurfaceError::SupplementalLayerIndex),
        };
        layer_labels.push(public_label);
        layer_icons.push(match (public_kind, public_id.as_deref()) {
            // Exact entity portraits are attached by the verified core POI index. A generic boss
            // must keep the neutral renderer fallback instead of borrowing another character.
            (Some(MapPoiKind::Boss), _) => None,
            (Some(_), Some(id)) => Some(required_supplemental_layer_icon(
                dataset_root,
                id,
                icon_cache,
            )?),
            _ => None,
        });
        public_layer_ids.push(public_id);
        public_layer_kinds.push(public_kind);
        layer_ids.push(layer.id);
        declared_counts[expected_index] = layer.point_count;
    }

    let mut observed_counts = vec![0_usize; layer_ids.len()];
    for source_region in index.regions {
        if source_region.map_id.is_empty()
            || source_region.region_id.is_empty()
            || source_region.point_count != source_region.points.len()
        {
            return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
        }
        for (layer_index, world_x, world_y, map_x, map_y) in source_region.points {
            let layer_index = usize::try_from(layer_index)
                .map_err(|_| ApprovedMapSurfaceError::SupplementalLayerIndex)?;
            if layer_ids.get(layer_index).is_none() {
                return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
            }
            if !world_x.is_finite()
                || !world_y.is_finite()
                || !map_x.is_finite()
                || !map_y.is_finite()
            {
                return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
            }
            observed_counts[layer_index] += 1;
            let Some(public_layer_id) =
                public_layer_ids.get(layer_index).and_then(Option::as_deref)
            else {
                continue;
            };
            let Some(public_kind) = public_layer_kinds.get(layer_index).copied().flatten() else {
                return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
            };
            let Some(region_index) = select_region_index(regions, world_x, world_y) else {
                continue;
            };
            let region = &mut regions[region_index];
            let Some(projected) = region
                .world_to_image
                .project_within_bounds(world_x, world_y)
            else {
                continue;
            };
            let Some(mut marker) = MapPoi::new(projected.x(), projected.y(), public_kind) else {
                return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
            };
            if public_kind == MapPoiKind::Supplemental {
                let Some(filtered) = marker.with_supplemental_filter(public_layer_id) else {
                    return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
                };
                marker = filtered;
            }
            if let Some(icon) = layer_icons.get(layer_index).and_then(Option::as_ref) {
                marker = marker.with_icon(Arc::clone(icon));
            }
            if !region
                .pois
                .iter()
                .any(|existing| same_public_poi_identity(existing, &marker))
            {
                region.pois.push(marker);
            }
        }
    }
    if observed_counts != declared_counts {
        return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
    }
    for (layer_index, public_id) in public_layer_ids.iter().enumerate() {
        let Some(public_id) = public_id.as_ref() else {
            continue;
        };
        let Some(public_kind) = public_layer_kinds[layer_index] else {
            return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
        };
        let kind = if RESOURCE_LAYER_IDS.contains(&public_id.as_str()) {
            MapSearchKind::Resource
        } else {
            MapSearchKind::Poi
        };
        for region in regions.iter_mut() {
            let points = region
                .pois
                .iter()
                .filter(|poi| {
                    if public_kind == MapPoiKind::Boss {
                        poi.kind() == MapPoiKind::Boss
                    } else {
                        poi.kind() == MapPoiKind::Supplemental
                            && poi.filter_id() == Some(public_id.as_str())
                    }
                })
                .collect::<Vec<_>>();
            let Some((map_x, map_y)) = centroid(&points) else {
                continue;
            };
            let label = layer_labels
                .get(layer_index)
                .and_then(|label| label.clone())
                .unwrap_or_else(|| public_id.clone());
            if region.search_entries.iter().any(|entry| {
                entry.target == MapSearchTarget::Layer(public_id.clone()) && entry.title == label
            }) {
                continue;
            }
            region.search_entries.push(MapSearchEntry {
                id: format!("layer:{}:{}:{}", region.map_id, region.region_id, public_id),
                title: label,
                subtitle: if kind == MapSearchKind::Resource {
                    "자원 위치".to_owned()
                } else {
                    "지도 위치".to_owned()
                },
                kind,
                map_id: region.map_id.clone(),
                region_id: region.region_id.clone(),
                map_x,
                map_y,
                target: MapSearchTarget::Layer(public_id.clone()),
            });
        }
    }
    Ok(())
}

fn load_poi_terminology(
    dataset_root: &Path,
    expected_build: &str,
) -> Result<HashMap<String, PoiTerminologyTerm>, ApprovedMapSurfaceError> {
    let path = dataset_root
        .join("overlay")
        .join("poi-terminology.ko.v1.json");
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| ApprovedMapSurfaceError::SupplementalLayerIndex)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_POI_TERMINOLOGY_BYTES
    {
        return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .map_err(|_| ApprovedMapSurfaceError::SupplementalLayerIndex)?,
    );
    File::open(path)
        .map_err(|_| ApprovedMapSurfaceError::SupplementalLayerIndex)?
        .take(MAX_POI_TERMINOLOGY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ApprovedMapSurfaceError::SupplementalLayerIndex)?;
    if bytes.len() as u64 > MAX_POI_TERMINOLOGY_BYTES {
        return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
    }
    let index: PoiTerminologyIndex = serde_json::from_slice(&bytes)
        .map_err(|_| ApprovedMapSurfaceError::SupplementalLayerIndex)?;
    let build_matches = index.game_build_id == expected_build
        || index.game_build_id == format!("steam:{expected_build}");
    if index.schema_version != 1
        || index.language != "ko"
        || !build_matches
        || index.terms.is_empty()
        || index.terms.len() > 64
    {
        return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
    }
    let mut terms = HashMap::with_capacity(index.terms.len());
    for term in index.terms {
        if !valid_filter_id(&term.id)
            || term.label_ko.is_empty()
            || term.label_ko.len() > 256
            || term.label_ko.chars().any(char::is_control)
            || !matches!(
                term.filter_visibility.as_str(),
                "visible" | "merged" | "hidden_until_verified"
            )
            || (term.filter_visibility == "merged" && term.merged_into.is_none())
            || (term.filter_visibility != "merged" && term.merged_into.is_some())
            || term
                .merged_into
                .as_deref()
                .is_some_and(|target| !valid_filter_id(target))
        {
            return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
        }
        if terms.insert(term.id.clone(), term).is_some() {
            return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
        }
    }
    if terms.values().any(|term| {
        term.merged_into
            .as_ref()
            .is_some_and(|target| !terms.contains_key(target))
    }) {
        return Err(ApprovedMapSurfaceError::SupplementalLayerIndex);
    }
    Ok(terms)
}

fn public_supplemental_layer<'a>(
    source_id: &str,
    terms: &'a HashMap<String, PoiTerminologyTerm>,
) -> Option<(MapPoiKind, &'a str, &'a str)> {
    let source = terms.get(source_id)?;
    if source.filter_visibility == "hidden_until_verified" {
        return None;
    }
    let public_id = source.merged_into.as_deref().unwrap_or(source.id.as_str());
    let public = terms.get(public_id)?;
    let kind = if public.id == "boss" {
        MapPoiKind::Boss
    } else {
        MapPoiKind::Supplemental
    };
    Some((kind, public.id.as_str(), public.label_ko.as_str()))
}

fn same_public_poi_identity(left: &MapPoi, right: &MapPoi) -> bool {
    left.kind() == right.kind()
        && left.filter_id() == right.filter_id()
        && rounded_coordinate_key(left.map_x()) == rounded_coordinate_key(right.map_x())
        && rounded_coordinate_key(left.map_y()) == rounded_coordinate_key(right.map_y())
}

fn rounded_coordinate_key(value: f64) -> i64 {
    (value * 1_000.0).round() as i64
}

fn append_pal_spawn_markers(
    dataset_root: &Path,
    expected_build: &str,
    regions: &mut [PreparedApprovedMapRegion],
    portrait_cache: &mut HashMap<String, Arc<MapPoiIcon>>,
) -> Result<(), ApprovedMapSurfaceError> {
    let path = dataset_root.join("overlay").join("spawns.search.v1.json");
    if !path.is_file() {
        return Ok(());
    }
    let metadata =
        std::fs::symlink_metadata(&path).map_err(|_| ApprovedMapSurfaceError::SpawnIndex)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_SPAWN_INDEX_BYTES
    {
        return Err(ApprovedMapSurfaceError::SpawnIndex);
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len()).map_err(|_| ApprovedMapSurfaceError::SpawnIndex)?,
    );
    File::open(path)
        .map_err(|_| ApprovedMapSurfaceError::SpawnIndex)?
        .take(MAX_SPAWN_INDEX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ApprovedMapSurfaceError::SpawnIndex)?;
    if bytes.len() as u64 > MAX_SPAWN_INDEX_BYTES {
        return Err(ApprovedMapSurfaceError::SpawnIndex);
    }
    let index: SpawnSearchIndex = serde_json::from_slice(&bytes).map_err(|error| {
        eprintln!("overlay Pal spawn index parse failed: {error}");
        ApprovedMapSurfaceError::SpawnIndex
    })?;
    let expected_qualified_build = format!("steam:{expected_build}");
    let observed_spawn_rule_count = index.species.values().map(Vec::len).sum::<usize>();
    let metadata_valid = index.schema_version == 1
        && index.verified
        && (index.game_build_id == expected_build
            || index.game_build_id == expected_qualified_build)
        && index.placement_unit == "one row in DT_PalSpawnerPlacement"
        && index.group_unit == "one weighted row in DT_PalWildSpawner"
        && index.placement_count >= index.resolved_placement_count
        && index.resolved_placement_count == index.placements.len()
        && index.spawn_group_count > 0
        && index.spawn_group_count <= observed_spawn_rule_count
        && index.species_count == index.species.len()
        && !index.placements.is_empty()
        && !index.species.is_empty();
    if !metadata_valid {
        eprintln!(
            "overlay Pal spawn index metadata mismatch: declared placements={}/{}, \
             observed={}, declared groups={}, observed rules={}, declared species={}, observed={}",
            index.placement_count,
            index.resolved_placement_count,
            index.placements.len(),
            index.spawn_group_count,
            observed_spawn_rule_count,
            index.species_count,
            index.species.len()
        );
        return Err(ApprovedMapSurfaceError::SpawnIndex);
    }

    let korean_names = load_korean_pal_names(dataset_root);
    let mut placements_by_spawner =
        HashMap::<String, Vec<(&str, f64, f64)>>::with_capacity(index.placements.len());
    for (placement_id, spawner, world_x, world_y, world_z, placement_type, spawner_type) in
        &index.placements
    {
        if placement_id.is_empty()
            || spawner.is_empty()
            || !world_x.is_finite()
            || !world_y.is_finite()
            || !world_z.is_finite()
            || placement_type.is_empty()
            || spawner_type.is_empty()
        {
            return Err(ApprovedMapSurfaceError::SpawnIndex);
        }
        placements_by_spawner
            .entry(spawner.clone())
            .or_default()
            .push((placement_id, *world_x, *world_y));
    }

    let mut seen = HashSet::<(String, String, bool)>::new();
    for (species_id, rules) in index.species {
        if !valid_entity_id(&species_id) {
            return Err(ApprovedMapSurfaceError::SpawnIndex);
        }
        let icon = load_cached_pal_portrait(dataset_root, &species_id, portrait_cache);
        let display_name = korean_names
            .get(&species_id)
            .filter(|value| !value.is_empty())
            .cloned()
            .unwrap_or_else(|| species_id.clone());
        let species_id: Arc<str> = Arc::from(species_id.as_str());
        let mut region_points = HashMap::<usize, Vec<(f64, f64)>>::new();
        for (
            spawner,
            spawner_type,
            _weight,
            only_time,
            only_weather,
            _world_tree_aura,
            min_level,
            max_level,
            min_count,
            max_count,
        ) in rules
        {
            if spawner.is_empty()
                || spawner_type.is_empty()
                || only_weather.is_empty()
                || min_level > max_level
                || min_count > max_count
            {
                return Err(ApprovedMapSurfaceError::SpawnIndex);
            }
            let night_spawn = only_time == "Night";
            for &(placement_id, world_x, world_y) in
                placements_by_spawner.get(&spawner).into_iter().flatten()
            {
                if !seen.insert((species_id.to_string(), placement_id.to_owned(), night_spawn)) {
                    continue;
                }
                let Some(region_index) = select_region_index(regions, world_x, world_y) else {
                    continue;
                };
                let region = &mut regions[region_index];
                let Some(map_point) = region
                    .world_to_image
                    .project_within_bounds(world_x, world_y)
                else {
                    continue;
                };
                let mut marker = MapPoi::new(map_point.x(), map_point.y(), MapPoiKind::PalSpawn)
                    .expect("verified projection produces finite coordinates")
                    .with_pal_spawn_filter(Arc::clone(&species_id), night_spawn);
                if let Some(icon) = &icon {
                    marker = marker.with_icon(Arc::clone(icon));
                }
                region_points
                    .entry(region_index)
                    .or_default()
                    .push((map_point.x(), map_point.y()));
                region.pois.push(marker);
            }
        }
        for (region_index, points) in region_points {
            let region = &mut regions[region_index];
            let (map_x, map_y) = centroid_xy(&points).expect("spawn result has at least one point");
            region.search_entries.push(MapSearchEntry {
                id: format!("pal:{}:{}:{}", region.map_id, region.region_id, species_id),
                title: display_name.clone(),
                subtitle: format!("팰 출현 지역 · {species_id}"),
                kind: MapSearchKind::Pal,
                map_id: region.map_id.clone(),
                region_id: region.region_id.clone(),
                map_x,
                map_y,
                target: MapSearchTarget::Pal(species_id.to_string()),
            });
        }
    }
    Ok(())
}

fn load_korean_pal_names(dataset_root: &Path) -> HashMap<String, String> {
    let candidates = [
        dataset_root.join("overlay").join("pals.ko.v1.json"),
        dataset_root
            .parent()
            .unwrap_or(dataset_root)
            .join("save-parser-data")
            .join("l10n")
            .join("ko")
            .join("pals.json"),
    ];
    for path in candidates {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() == 0
            || metadata.len() > MAX_SUPPLEMENTAL_LAYER_BYTES
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(records) = serde_json::from_slice::<HashMap<String, KoreanPalName>>(&bytes) else {
            continue;
        };
        return records
            .into_iter()
            .map(|(id, record)| (id, record.localized_name))
            .collect();
    }
    HashMap::new()
}

fn centroid(points: &[&MapPoi]) -> Option<(f64, f64)> {
    if points.is_empty() {
        return None;
    }
    Some((
        points.iter().map(|point| point.map_x()).sum::<f64>() / points.len() as f64,
        points.iter().map(|point| point.map_y()).sum::<f64>() / points.len() as f64,
    ))
}

fn centroid_xy(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    if points.is_empty() {
        return None;
    }
    Some((
        points.iter().map(|point| point.0).sum::<f64>() / points.len() as f64,
        points.iter().map(|point| point.1).sum::<f64>() / points.len() as f64,
    ))
}

fn select_region_index(
    regions: &[PreparedApprovedMapRegion],
    world_x: f64,
    world_y: f64,
) -> Option<usize> {
    let highest_priority = regions
        .iter()
        .filter(|region| region.contains_world(world_x, world_y))
        .map(PreparedApprovedMapRegion::priority)
        .max()?;
    let mut matches = regions.iter().enumerate().filter(|(_, region)| {
        region.priority() == highest_priority && region.contains_world(world_x, world_y)
    });
    let (index, _) = matches.next()?;
    matches.next().is_none().then_some(index)
}

fn load_cached_pal_portrait(
    dataset_root: &Path,
    species_id: &str,
    cache: &mut HashMap<String, Arc<MapPoiIcon>>,
) -> Option<Arc<MapPoiIcon>> {
    for candidate in [
        species_id,
        species_id.strip_prefix("BOSS_").unwrap_or(species_id),
        species_id.strip_prefix("RAID_").unwrap_or(species_id),
        species_id.strip_prefix("PREDATOR_").unwrap_or(species_id),
    ] {
        if let Some(icon) = cache.get(candidate) {
            return Some(Arc::clone(icon));
        }
        if let Some(icon) = load_pal_portrait(dataset_root, candidate) {
            cache.insert(candidate.to_owned(), Arc::clone(&icon));
            return Some(icon);
        }
    }
    None
}

fn valid_entity_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_filter_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':'))
}

impl PreparedApprovedMainMap {
    /// Opens only a published, exact-build map pack and prepares its authenticated MainMap data.
    ///
    /// `MapPackStore::open_published` verifies the active pointer, manifest, transform evidence,
    /// every tile hash and POI parity before this method decodes a single raster tile.
    pub fn open_published(
        dataset_root: impl AsRef<Path>,
        expected_build: &str,
    ) -> Result<Self, ApprovedMapSurfaceError> {
        let dataset_root = dataset_root.as_ref();
        let store = MapPackStore::open_published(dataset_root, expected_build)?;
        let region_index = store
            .map_regions()
            .iter()
            .enumerate()
            .find(|(_, region)| {
                region.map_id() == MAIN_MAP_ID && region.region_id() == MAIN_REGION_ID
            })
            .map(|(index, _)| index)
            .ok_or(ApprovedMapSurfaceError::MissingMainMap)?;
        let mut portrait_cache = HashMap::<String, Arc<MapPoiIcon>>::new();
        let surface = prepare_region(dataset_root, &store, region_index, &mut portrait_cache)?;

        Ok(Self {
            build_id: store.manifest().game_build_id().to_owned(),
            canonical_pack_sha256: store.canonical_pack_hash(),
            surface,
        })
    }

    pub fn build_id(&self) -> &str {
        &self.build_id
    }

    pub const fn canonical_pack_sha256(&self) -> [u8; 32] {
        self.canonical_pack_sha256
    }

    pub const fn transform_sha256(&self) -> [u8; 32] {
        self.surface.transform_sha256
    }

    pub fn raster(&self) -> &MapRaster {
        &self.surface.raster
    }

    pub fn raster_arc(&self) -> Arc<MapRaster> {
        Arc::clone(&self.surface.raster)
    }

    pub const fn world_to_image(&self) -> WorldToImageTransform {
        self.surface.world_to_image
    }

    pub fn pois(&self) -> &[MapPoi] {
        &self.surface.pois
    }
}

impl PreparedApprovedMapRegion {
    pub fn map_id(&self) -> &str {
        &self.map_id
    }

    pub fn region_id(&self) -> &str {
        &self.region_id
    }

    pub const fn priority(&self) -> i32 {
        self.priority
    }

    pub const fn transform_sha256(&self) -> [u8; 32] {
        self.transform_sha256
    }

    pub fn raster(&self) -> &MapRaster {
        &self.raster
    }

    pub fn raster_arc(&self) -> Arc<MapRaster> {
        Arc::clone(&self.raster)
    }

    pub const fn world_to_image(&self) -> WorldToImageTransform {
        self.world_to_image
    }

    pub fn pois(&self) -> &[MapPoi] {
        &self.pois
    }

    pub fn search_entries(&self) -> &[MapSearchEntry] {
        &self.search_entries
    }

    pub fn contains_world(&self, world_x: f64, world_y: f64) -> bool {
        self.world_to_image
            .project_within_bounds(world_x, world_y)
            .is_some()
    }
}

fn prepare_region(
    dataset_root: &Path,
    store: &MapPackStore,
    region_index: usize,
    portrait_cache: &mut HashMap<String, Arc<MapPoiIcon>>,
) -> Result<PreparedApprovedMapRegion, ApprovedMapSurfaceError> {
    let region = store
        .map_regions()
        .get(region_index)
        .ok_or(ApprovedMapSurfaceError::MissingMainMap)?;
    let region_pack = store
        .region_pack(region_index)
        .ok_or(ApprovedMapSurfaceError::MissingMainMap)?;
    let width = region.map_width_px();
    let height = region.map_height_px();
    if width == 0 || height == 0 || width > 4_096 || height > 4_096 {
        return Err(ApprovedMapSurfaceError::MapDimensionsOutOfRange);
    }

    let raster = stitch_level_zero_tiles(
        region_pack,
        width,
        height,
        store.manifest().tile_core_size_px(),
        store.manifest().tile_gutter_px(),
    )?;
    let (min_world_x, min_world_y, max_world_x, max_world_y) = region.world_bounds();
    let world_to_image = WorldToImageTransform::from_affine_map_pixels(
        min_world_x,
        min_world_y,
        max_world_x,
        max_world_y,
        width,
        height,
        region_pack.transform().matrix(),
    )?;
    let mut search_entries = Vec::new();
    let pois = region_pack
        .poi_index()
        .all()
        .iter()
        .map(|poi| {
            let kind = match poi.kind() {
                PoiKind::FastTravel => MapPoiKind::FastTravel,
                PoiKind::Boss => MapPoiKind::Boss,
                PoiKind::Wanted => MapPoiKind::Wanted,
                PoiKind::Dungeon => MapPoiKind::Dungeon,
            };
            let mut marker = MapPoi::new(poi.map().x(), poi.map().y(), kind)
                .expect("strict map-pack validation guarantees finite POI coordinates");
            let category_icon = load_map_category_icon(dataset_root, kind, portrait_cache);
            if kind == MapPoiKind::Boss
                && let Some(entity_id) = poi.entity_id()
            {
                let icon = portrait_cache.get(entity_id).cloned().or_else(|| {
                    let decoded = load_pal_portrait(dataset_root, entity_id)?;
                    portrait_cache.insert(entity_id.to_owned(), Arc::clone(&decoded));
                    Some(decoded)
                });
                if let Some(icon) = icon {
                    marker = marker.with_icon(icon);
                }
            }
            if kind == MapPoiKind::Wanted
                && let Some(icon) =
                    load_wanted_portrait(dataset_root, poi.display_name(), portrait_cache)
            {
                marker = marker.with_icon(icon);
            }
            if marker.icon().is_none()
                && let Some(icon) = category_icon
            {
                marker = marker.with_icon(icon);
            }
            search_entries.push(MapSearchEntry {
                id: poi.id().to_owned(),
                title: poi.display_name().to_owned(),
                subtitle: poi_kind_label(kind).to_owned(),
                kind: MapSearchKind::Poi,
                map_id: region.map_id().to_owned(),
                region_id: region.region_id().to_owned(),
                map_x: poi.map().x(),
                map_y: poi.map().y(),
                target: MapSearchTarget::Poi(kind),
            });
            marker
        })
        .collect();
    Ok(PreparedApprovedMapRegion {
        map_id: region.map_id().to_owned(),
        region_id: region.region_id().to_owned(),
        priority: region.priority(),
        transform_sha256: *region.transform_sha256(),
        raster: Arc::new(raster),
        world_to_image,
        pois,
        search_entries,
    })
}

const fn poi_kind_label(kind: MapPoiKind) -> &'static str {
    match kind {
        MapPoiKind::FastTravel => FilterAction::FastTravel.label_ko(),
        MapPoiKind::Boss => FilterAction::Boss.label_ko(),
        MapPoiKind::Wanted => FilterAction::Wanted.label_ko(),
        MapPoiKind::Dungeon => FilterAction::Dungeon.label_ko(),
        MapPoiKind::Supplemental => "지도 위치",
        MapPoiKind::PalSpawn => "팰 출현 지역",
    }
}

fn load_map_category_icon(
    dataset_root: &Path,
    kind: MapPoiKind,
    cache: &mut HashMap<String, Arc<MapPoiIcon>>,
) -> Option<Arc<MapPoiIcon>> {
    let (file_name, format) = map_category_icon_spec(kind)?;
    let cache_key = format!("@map:{file_name}");
    if let Some(icon) = cache.get(&cache_key) {
        return Some(Arc::clone(icon));
    }
    let path = dataset_root.join("icons").join("map").join(file_name);
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_PAL_PORTRAIT_BYTES
    {
        return None;
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).ok()?);
    File::open(path)
        .ok()?
        .take(MAX_PAL_PORTRAIT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_PAL_PORTRAIT_BYTES {
        return None;
    }
    let icon = match format {
        ImageFormat::Png => MapPoiIcon::decode_png(&bytes),
        ImageFormat::WebP => MapPoiIcon::decode_webp(&bytes),
        _ => None,
    }
    .map(Arc::new)?;
    cache.insert(cache_key, Arc::clone(&icon));
    Some(icon)
}

const fn map_category_icon_spec(kind: MapPoiKind) -> Option<(&'static str, ImageFormat)> {
    match kind {
        MapPoiKind::FastTravel => Some(("compass-fast-travel.png", ImageFormat::Png)),
        MapPoiKind::Wanted => Some(("compass-bounty.png", ImageFormat::Png)),
        MapPoiKind::Dungeon => Some(("compass-dungeon.png", ImageFormat::Png)),
        MapPoiKind::Boss | MapPoiKind::Supplemental | MapPoiKind::PalSpawn => None,
    }
}

const WANTED_PORTRAIT_FILES: [(&str, &str); 33] = [
    ("현상범 에고", "t_boss_npc_believer.webp"),
    ("현상범 브릭", "t_boss_npc_believer_fat.webp"),
    ("현상범 램", "t_boss_npc_darktrader.webp"),
    ("현상범 위스퍼", "t_boss_npc_female_desertpeople.webp"),
    ("현상범 플레어", "t_boss_npc_female.webp"),
    ("현상범 사이렌", "t_boss_npc_female02.webp"),
    ("현상범 턴코트", "t_boss_npc_female03.webp"),
    ("현상범 제이드", "t_boss_npc_female_soldier.webp"),
    ("현상범 대즐", "t_boss_npc_female_soldier02.webp"),
    ("현상범 알로하", "t_boss_npc_female_soldier03.webp"),
    ("현상범 님블", "t_boss_npc_female_soldier04.webp"),
    ("현상범 섀도", "t_boss_npc_firecult.webp"),
    ("현상범 그릴", "t_boss_npc_hunter_fat.webp"),
    ("현상범 호크", "t_boss_npc_hunter.webp"),
    ("현상범 팬텀", "t_boss_npc_male_desertpeople.webp"),
    ("현상범 어친", "t_boss_npc_male_ninjaelite.webp"),
    ("현상범 다이너", "t_boss_npc_male.webp"),
    ("현상범 퀼", "t_boss_npc_male02.webp"),
    ("현상범 스쿳", "t_boss_npc_male03.webp"),
    ("현상범 마이트", "t_boss_npc_male.webp"),
    ("현상범 크래시", "t_boss_npc_male_soldier.webp"),
    ("현상범 다트", "t_boss_npc_male_soldier02.webp"),
    ("현상범 클린트", "t_boss_npc_male_soldier03.webp"),
    ("현상범 라쏘", "t_boss_npc_male_soldier04.webp"),
    ("현상범 스킴", "t_boss_npc_male_trader01.webp"),
    ("현상범 미믹", "t_boss_npc_male_trader02.webp"),
    ("현상범 빌리", "t_boss_npc_male_trader03.webp"),
    ("현상범 펌블", "t_boss_npc_male_ninja.webp"),
    ("현상범 윕", "t_boss_npc_police.webp"),
    ("현상범 핀치", "t_boss_npc_police_old.webp"),
    ("현상범 위스크", "t_boss_npc_male_scientist.webp"),
    ("현상범 노우", "t_boss_npc_viking.webp"),
    ("현상범 캐시", "t_boss_npc_vikingelite.webp"),
];

fn load_wanted_portrait(
    dataset_root: &Path,
    display_name: &str,
    cache: &mut HashMap<String, Arc<MapPoiIcon>>,
) -> Option<Arc<MapPoiIcon>> {
    let file_name = WANTED_PORTRAIT_FILES
        .iter()
        .find_map(|(name, file)| (*name == display_name).then_some(*file))?;
    let cache_key = format!("@wanted:{file_name}");
    if let Some(icon) = cache.get(&cache_key) {
        return Some(Arc::clone(icon));
    }
    let path = dataset_root.join("icons").join("wanted").join(file_name);
    let icon = load_map_icon_file(&path, ImageFormat::WebP)?;
    cache.insert(cache_key, Arc::clone(&icon));
    Some(icon)
}

fn load_supplemental_layer_icon(
    dataset_root: &Path,
    layer_id: &str,
    cache: &mut HashMap<String, Arc<MapPoiIcon>>,
) -> Option<Arc<MapPoiIcon>> {
    if !valid_filter_id(layer_id) {
        return None;
    }
    let cache_key = format!("@layer:{layer_id}");
    if let Some(icon) = cache.get(&cache_key) {
        return Some(Arc::clone(icon));
    }
    for (extension, format) in [("png", ImageFormat::Png), ("webp", ImageFormat::WebP)] {
        let path = dataset_root
            .join("icons")
            .join("layers")
            .join(format!("{layer_id}.{extension}"));
        let Some(icon) = load_map_icon_file(&path, format) else {
            continue;
        };
        cache.insert(cache_key, Arc::clone(&icon));
        return Some(icon);
    }
    None
}

fn required_supplemental_layer_icon(
    dataset_root: &Path,
    layer_id: &str,
    cache: &mut HashMap<String, Arc<MapPoiIcon>>,
) -> Result<Arc<MapPoiIcon>, ApprovedMapSurfaceError> {
    load_supplemental_layer_icon(dataset_root, layer_id, cache).ok_or_else(|| {
        ApprovedMapSurfaceError::MapIconAsset {
            layer_id: layer_id.to_owned(),
        }
    })
}

fn load_map_icon_file(path: &Path, format: ImageFormat) -> Option<Arc<MapPoiIcon>> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_PAL_PORTRAIT_BYTES
    {
        return None;
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).ok()?);
    File::open(path)
        .ok()?
        .take(MAX_PAL_PORTRAIT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_PAL_PORTRAIT_BYTES {
        return None;
    }
    match format {
        ImageFormat::Png => MapPoiIcon::decode_png(&bytes),
        ImageFormat::WebP => MapPoiIcon::decode_webp(&bytes),
        _ => None,
    }
    .map(Arc::new)
}

fn load_pal_portrait(dataset_root: &Path, entity_id: &str) -> Option<Arc<MapPoiIcon>> {
    if entity_id.is_empty()
        || entity_id.len() > 128
        || !entity_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    let path = dataset_root
        .join("icons")
        .join("pals")
        .join(format!("T_{entity_id}_icon_normal.webp"));
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_PAL_PORTRAIT_BYTES
    {
        return None;
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).ok()?);
    File::open(path)
        .ok()?
        .take(MAX_PAL_PORTRAIT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_PAL_PORTRAIT_BYTES {
        return None;
    }
    MapPoiIcon::decode_webp(&bytes).map(Arc::new)
}

fn stitch_level_zero_tiles(
    region_pack: &pal_map_pack_store::MapRegionPack,
    width: u32,
    height: u32,
    core: u32,
    gutter: u32,
) -> Result<MapRaster, ApprovedMapSurfaceError> {
    let level = region_pack
        .tile_index()
        .levels()
        .iter()
        .copied()
        .find(|level| level.level() == 0)
        .ok_or(ApprovedMapSurfaceError::MissingLevelZeroTile)?;
    if level.width_px() != width || level.height_px() != height || core == 0 {
        return Err(ApprovedMapSurfaceError::TileGeometry);
    }
    let output_side = core
        .checked_add(
            gutter
                .checked_mul(2)
                .ok_or(ApprovedMapSurfaceError::TileGeometry)?,
        )
        .ok_or(ApprovedMapSurfaceError::TileGeometry)?;
    let pixel_count = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or(ApprovedMapSurfaceError::TileGeometry)?;
    let mut pixels = vec![0_u32; pixel_count];

    for tile_y in 0..level.grid_height() {
        for tile_x in 0..level.grid_width() {
            let bytes = region_pack.read_verified_tile(TileKey {
                level: 0,
                y: tile_y,
                x: tile_x,
            })?;
            let decoded = decode_jpeg_tile(&bytes, output_side)?;
            copy_tile_core(
                &decoded,
                gutter,
                core,
                tile_x,
                tile_y,
                width,
                height,
                &mut pixels,
            )?;
        }
    }
    MapRaster::new(width, height, pixels).map_err(ApprovedMapSurfaceError::Raster)
}

fn decode_jpeg_tile(bytes: &[u8], expected_side: u32) -> Result<RgbImage, ApprovedMapSurfaceError> {
    let image = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg)
        .map_err(|_| ApprovedMapSurfaceError::TileDecode)?;
    if image.width() != expected_side || image.height() != expected_side {
        return Err(ApprovedMapSurfaceError::TileDimensions);
    }
    Ok(image.to_rgb8())
}

#[allow(clippy::too_many_arguments)]
fn copy_tile_core(
    tile: &RgbImage,
    gutter: u32,
    core: u32,
    tile_x: u32,
    tile_y: u32,
    map_width: u32,
    map_height: u32,
    destination: &mut [u32],
) -> Result<(), ApprovedMapSurfaceError> {
    let origin_x = tile_x
        .checked_mul(core)
        .ok_or(ApprovedMapSurfaceError::TileGeometry)?;
    let origin_y = tile_y
        .checked_mul(core)
        .ok_or(ApprovedMapSurfaceError::TileGeometry)?;
    let copy_width = core.min(map_width.saturating_sub(origin_x));
    let copy_height = core.min(map_height.saturating_sub(origin_y));
    if copy_width == 0 || copy_height == 0 {
        return Err(ApprovedMapSurfaceError::TileGeometry);
    }

    for local_y in 0..copy_height {
        let destination_y = origin_y
            .checked_add(local_y)
            .ok_or(ApprovedMapSurfaceError::TileGeometry)?;
        for local_x in 0..copy_width {
            let destination_x = origin_x
                .checked_add(local_x)
                .ok_or(ApprovedMapSurfaceError::TileGeometry)?;
            let source_x = gutter
                .checked_add(local_x)
                .ok_or(ApprovedMapSurfaceError::TileGeometry)?;
            let source_y = gutter
                .checked_add(local_y)
                .ok_or(ApprovedMapSurfaceError::TileGeometry)?;
            let source = tile
                .get_pixel_checked(source_x, source_y)
                .ok_or(ApprovedMapSurfaceError::TileDimensions)?;
            let index = usize::try_from(destination_y)
                .ok()
                .and_then(|y| {
                    usize::try_from(map_width)
                        .ok()
                        .and_then(|width| y.checked_mul(width))
                })
                .and_then(|row| {
                    usize::try_from(destination_x)
                        .ok()
                        .and_then(|x| row.checked_add(x))
                })
                .ok_or(ApprovedMapSurfaceError::TileGeometry)?;
            let output = destination
                .get_mut(index)
                .ok_or(ApprovedMapSurfaceError::TileGeometry)?;
            *output =
                (u32::from(source[0]) << 16) | (u32::from(source[1]) << 8) | u32::from(source[2]);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{Rgb, RgbImage};

    use crate::actual_map_preview::{MapPoi, MapPoiKind, MapRaster, WorldToImageTransform};

    use super::{
        ApprovedMapSurfaceError, PoiTerminologyIndex, PreparedApprovedMapPack,
        PreparedApprovedMapRegion, WANTED_PORTRAIT_FILES, copy_tile_core, map_category_icon_spec,
        public_supplemental_layer, required_supplemental_layer_icon, same_public_poi_identity,
    };

    struct TemporaryIconRoot(PathBuf);

    impl TemporaryIconRoot {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "pal-overlay-map-icon-{label}-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir_all(root.join("icons/layers")).expect("create temporary icon root");
            Self(root)
        }
    }

    impl Drop for TemporaryIconRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture_terminology() -> std::collections::HashMap<String, super::PoiTerminologyTerm> {
        let index: PoiTerminologyIndex = serde_json::from_str(include_str!(
            "../../../assets/palbeacon/game/map/poi-terminology.ko.v1.json"
        ))
        .expect("shared POI terminology parses");
        index
            .terms
            .into_iter()
            .map(|term| (term.id.clone(), term))
            .collect()
    }

    fn fixture_region(
        map_id: &str,
        region_id: &str,
        priority: i32,
        bounds: [f64; 4],
    ) -> PreparedApprovedMapRegion {
        PreparedApprovedMapRegion {
            map_id: map_id.to_owned(),
            region_id: region_id.to_owned(),
            priority,
            transform_sha256: [0; 32],
            raster: std::sync::Arc::new(MapRaster::new(1, 1, vec![0]).expect("fixture raster")),
            world_to_image: WorldToImageTransform::from_world_bounds(
                bounds[0], bounds[1], bounds[2], bounds[3],
            )
            .expect("fixture transform"),
            pois: Vec::new(),
            search_entries: Vec::new(),
        }
    }

    #[test]
    fn tile_stitching_discards_gutters_and_clips_edge_tiles() {
        let mut tile = RgbImage::from_pixel(8, 8, Rgb([250, 1, 2]));
        for y in 1..=6 {
            for x in 1..=6 {
                tile.put_pixel(x, y, Rgb([x as u8, y as u8, 7]));
            }
        }
        let mut destination = vec![0_u32; 8 * 7];

        copy_tile_core(&tile, 1, 6, 1, 1, 8, 7, &mut destination).expect("edge tile copies");

        assert_eq!(destination[6 + 6 * 8], 0x0001_0107);
        assert_eq!(destination[7 + 6 * 8], 0x0002_0107);
        assert!(!destination.contains(&0x00fa_0102));
    }

    #[test]
    fn tile_stitching_rejects_coordinates_outside_the_map_grid() {
        let tile = RgbImage::from_pixel(8, 8, Rgb([1, 2, 3]));
        let mut destination = vec![0_u32; 8 * 7];

        assert!(matches!(
            copy_tile_core(&tile, 1, 6, 2, 0, 8, 7, &mut destination),
            Err(ApprovedMapSurfaceError::TileGeometry)
        ));
    }

    #[test]
    fn overlapping_world_tree_region_wins_by_priority_and_outside_fails_closed() {
        let pack = PreparedApprovedMapPack {
            build_id: "24181527".to_owned(),
            canonical_pack_sha256: [0; 32],
            regions: vec![
                fixture_region("MainMap", "FirstRegion", 0, [0.0, 0.0, 100.0, 100.0]),
                fixture_region("Tree", "DummyRegion", 10, [40.0, 40.0, 60.0, 60.0]),
            ],
        };

        assert_eq!(pack.select_region_index(20.0, 20.0), Some(0));
        assert_eq!(pack.select_region_index(50.0, 50.0), Some(1));
        assert_eq!(pack.select_region_index(120.0, 120.0), None);
        assert_eq!(pack.select_region_index(f64::NAN, 50.0), None);
    }

    #[test]
    fn every_wanted_target_resolves_to_an_extracted_game_portrait() {
        let display_names = WANTED_PORTRAIT_FILES
            .iter()
            .map(|(display_name, _)| *display_name)
            .collect::<HashSet<_>>();
        let portrait_files = WANTED_PORTRAIT_FILES
            .iter()
            .map(|(_, file_name)| *file_name)
            .collect::<HashSet<_>>();
        assert_eq!(display_names.len(), 33, "wanted display names stay unique");
        assert_eq!(
            portrait_files.len(),
            32,
            "two wanted targets intentionally share the same in-game NPC portrait"
        );

        let asset_root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/palbeacon/game/wanted");
        for file_name in portrait_files {
            let path = asset_root.join(file_name);
            let metadata = std::fs::metadata(&path).unwrap_or_else(|error| {
                panic!("missing game portrait {}: {error}", path.display())
            });
            assert!(metadata.is_file());
            assert!(metadata.len() > 500);
        }
    }

    #[test]
    fn generic_boss_marker_never_uses_the_reviewed_character_portrait() {
        assert_eq!(map_category_icon_spec(MapPoiKind::Boss), None);
        assert!(map_category_icon_spec(MapPoiKind::FastTravel).is_some());
        assert!(map_category_icon_spec(MapPoiKind::Dungeon).is_some());
        assert!(map_category_icon_spec(MapPoiKind::Wanted).is_some());
    }

    #[test]
    fn required_layer_icon_reports_the_exact_missing_or_corrupt_layer() {
        let root = TemporaryIconRoot::new("required");
        let mut cache = HashMap::new();

        for contents in [None, Some(b"not an image".as_slice())] {
            if let Some(contents) = contents {
                std::fs::write(root.0.join("icons/layers/tower.png"), contents)
                    .expect("write corrupt fixture icon");
            }
            let Err(error) = required_supplemental_layer_icon(&root.0, "tower", &mut cache) else {
                panic!("missing or corrupt required icon must fail closed");
            };
            assert!(matches!(
                error,
                ApprovedMapSurfaceError::MapIconAsset { ref layer_id } if layer_id == "tower"
            ));
        }
    }

    #[test]
    fn supplemental_aliases_match_the_public_filter_contract() {
        let terms = fixture_terminology();
        assert_eq!(public_supplemental_layer("respawn", &terms), None);
        assert_eq!(
            public_supplemental_layer("sealed-realm", &terms),
            Some((MapPoiKind::Boss, "boss", "필드 보스"))
        );
        assert_eq!(
            public_supplemental_layer("biome-boss", &terms),
            Some((MapPoiKind::Boss, "boss", "필드 보스"))
        );
        assert_eq!(
            public_supplemental_layer("world-tree-fruit", &terms),
            Some((MapPoiKind::Supplemental, "skill-fruit", "기술 열매 나무"))
        );
        assert_eq!(
            public_supplemental_layer("egg", &terms),
            Some((MapPoiKind::Supplemental, "egg", "팰의 알"))
        );
    }

    #[test]
    fn zero_count_source_layer_may_remain_as_a_public_filter_alias() {
        let index: super::SupplementalLayerIndex = serde_json::from_str(include_str!(
            "../../../assets/palbeacon/game/map/layers.v1.json"
        ))
        .expect("shared supplemental layer index parses");
        let layer = index
            .layers
            .iter()
            .find(|layer| layer.id == "biome-boss")
            .expect("biome boss taxonomy row remains declared");
        assert_eq!(layer.point_count, 0);

        let terms = fixture_terminology();
        assert_eq!(
            public_supplemental_layer(&layer.id, &terms),
            Some((MapPoiKind::Boss, "boss", "필드 보스"))
        );
    }

    #[test]
    fn unified_search_uses_the_canonical_korean_layer_terms() {
        let terms = fixture_terminology();
        assert_eq!(
            public_supplemental_layer("chromite", &terms).map(|value| value.2),
            Some("크로마이트")
        );
        assert_eq!(
            public_supplemental_layer("salvage-rank-2", &terms).map(|value| value.2),
            Some("인양 2단계")
        );
        assert_eq!(
            public_supplemental_layer("ore-quartz", &terms).map(|value| value.2),
            Some("순수한 석영")
        );
        assert_eq!(
            public_supplemental_layer("npc", &terms).map(|value| value.2),
            Some("등장인물")
        );
        assert_eq!(public_supplemental_layer("future-layer", &terms), None);
    }

    #[test]
    fn canonical_poi_identity_removes_only_same_kind_and_coordinate() {
        let boss = MapPoi::new(120.123_44, 840.987_44, MapPoiKind::Boss).expect("boss");
        let same_public_boss =
            MapPoi::new(120.123_49, 840.987_49, MapPoiKind::Boss).expect("boss alias");
        let nearby_dungeon =
            MapPoi::new(120.123_49, 840.987_49, MapPoiKind::Dungeon).expect("dungeon");

        assert!(same_public_poi_identity(&boss, &same_public_boss));
        assert!(!same_public_poi_identity(&boss, &nearby_dungeon));
    }
}
