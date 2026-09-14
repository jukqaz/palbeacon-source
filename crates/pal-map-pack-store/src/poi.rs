use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::{MapPackError, MapPoint, MapRect, MapRegion, POI_SCHEMA_VERSION, WorldPoint};

const MAX_POIS: usize = 100_000;
const MAX_POI_ID_BYTES: usize = 128;
const MAX_DISPLAY_NAME_BYTES: usize = 256;
const MAX_ENTITY_ID_BYTES: usize = 128;
const PARITY_TOLERANCE_PX: f64 = 0.01;
const BUCKET_SIZE_PX: f64 = 256.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PoiKind {
    FastTravel = 0,
    Boss = 1,
    Wanted = 2,
    Dungeon = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PoiHandle(u32);

impl PoiHandle {
    pub const fn index(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PoiFilterMask(u8);

impl PoiFilterMask {
    pub const fn none() -> Self {
        Self(0)
    }

    pub const fn all() -> Self {
        Self(0b1111)
    }

    pub fn from_kinds(kinds: impl IntoIterator<Item = PoiKind>) -> Self {
        let mut mask = 0_u8;
        for kind in kinds {
            mask |= 1 << kind as u8;
        }
        Self(mask)
    }

    pub const fn contains(self, kind: PoiKind) -> bool {
        self.0 & (1 << kind as u8) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CullingQuery {
    rect: MapRect,
}

impl CullingQuery {
    pub const fn new(rect: MapRect) -> Self {
        Self { rect }
    }

    pub const fn rect(self) -> MapRect {
        self.rect
    }
}

#[derive(Clone, Debug)]
pub struct Poi {
    id: String,
    map_id: String,
    region_id: String,
    kind: PoiKind,
    display_name: String,
    entity_id: Option<String>,
    world: WorldPoint,
    map: MapPoint,
    source_build_id: String,
    verified: bool,
}

impl Poi {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn map_id(&self) -> &str {
        &self.map_id
    }

    pub fn region_id(&self) -> &str {
        &self.region_id
    }

    pub const fn kind(&self) -> PoiKind {
        self.kind
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn entity_id(&self) -> Option<&str> {
        self.entity_id.as_deref()
    }

    pub const fn world(&self) -> WorldPoint {
        self.world
    }

    pub const fn map(&self) -> MapPoint {
        self.map
    }

    pub fn source_build_id(&self) -> &str {
        &self.source_build_id
    }

    pub const fn verified(&self) -> bool {
        self.verified
    }
}

#[derive(Clone, Debug)]
pub struct PoiIndex {
    records: Vec<Poi>,
    buckets: BTreeMap<(u32, u32), Vec<PoiHandle>>,
    map_width_px: u32,
    map_height_px: u32,
}

impl PoiIndex {
    pub fn all(&self) -> &[Poi] {
        &self.records
    }

    pub fn get(&self, handle: PoiHandle) -> Option<&Poi> {
        self.records.get(handle.0 as usize)
    }

    pub fn visible_into(
        &self,
        query: &CullingQuery,
        filters: PoiFilterMask,
        output: &mut Vec<PoiHandle>,
    ) {
        output.clear();
        if filters == PoiFilterMask::none() {
            return;
        }
        let rect = query.rect;
        let min_x = rect.min_x().max(0.0);
        let min_y = rect.min_y().max(0.0);
        let max_x = rect.max_x().min(f64::from(self.map_width_px));
        let max_y = rect.max_y().min(f64::from(self.map_height_px));
        if min_x > max_x || min_y > max_y {
            return;
        }
        let min_bucket_x = bucket(min_x);
        let min_bucket_y = bucket(min_y);
        let max_bucket_x = bucket(max_x);
        let max_bucket_y = bucket(max_y);
        for y in min_bucket_y..=max_bucket_y {
            for x in min_bucket_x..=max_bucket_x {
                if let Some(handles) = self.buckets.get(&(y, x)) {
                    for handle in handles {
                        let poi = &self.records[handle.0 as usize];
                        if filters.contains(poi.kind) && rect.contains(poi.map) {
                            output.push(*handle);
                        }
                    }
                }
            }
        }
        output.sort_unstable();
        output.dedup();
    }

    pub(crate) fn parse_by_region(
        bytes: &[u8],
        build: &str,
        regions: &[MapRegion],
    ) -> Result<Vec<Self>, MapPackError> {
        let raw: RawPoiDocument = serde_json::from_slice(bytes)
            .map_err(|_| MapPackError::ParseFailed { component: "pois" })?;
        if raw.schema_version != POI_SCHEMA_VERSION {
            return invalid("schema_version");
        }
        if raw.game_build_id != build {
            return invalid("game_build_id");
        }
        if raw.poi_count as usize != raw.pois.len()
            || raw.pois.len() > MAX_POIS
            || raw.pois.len() < 3
        {
            return invalid("poi_count");
        }

        let mut ids = BTreeSet::new();
        let mut kind_coverage = [false; 4];
        let mut records_by_region: Vec<Vec<Poi>> = regions.iter().map(|_| Vec::new()).collect();
        for raw_poi in raw.pois {
            if raw_poi.id.is_empty()
                || raw_poi.id.len() > MAX_POI_ID_BYTES
                || !raw_poi.id.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
                || !ids.insert(raw_poi.id.clone())
            {
                return invalid("pois.id");
            }
            if raw_poi.display_name.is_empty()
                || raw_poi.display_name.len() > MAX_DISPLAY_NAME_BYTES
            {
                return invalid("pois.display_name");
            }
            if raw_poi.entity_id.as_ref().is_some_and(|entity_id| {
                entity_id.is_empty()
                    || entity_id.len() > MAX_ENTITY_ID_BYTES
                    || !entity_id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            }) {
                return invalid("pois.entity_id");
            }
            let kind = parse_kind(&raw_poi.kind)?;
            kind_coverage[kind as usize] = true;
            let region_index = regions
                .iter()
                .position(|region| {
                    region.map_id() == raw_poi.map_id && region.region_id() == raw_poi.region_id
                })
                .ok_or(MapPackError::InvalidField {
                    component: "pois",
                    field: "pois.region_identity",
                })?;
            let region = &regions[region_index];
            if raw_poi.source_build_id != build {
                return invalid("pois.source_build_id");
            }
            if !raw_poi.verified {
                return invalid("pois.verified");
            }
            if ![
                raw_poi.world_x,
                raw_poi.world_y,
                raw_poi.map_x,
                raw_poi.map_y,
            ]
            .into_iter()
            .all(f64::is_finite)
            {
                return invalid("pois.coordinates");
            }
            let selected_region =
                crate::manifest::select_map_region(regions, raw_poi.world_x, raw_poi.world_y)?;
            if selected_region.map_id() != raw_poi.map_id
                || selected_region.region_id() != raw_poi.region_id
            {
                return invalid("pois.region_selection");
            }
            let world = WorldPoint::new(raw_poi.world_x, raw_poi.world_y)?;
            let map = MapPoint::new(raw_poi.map_x, raw_poi.map_y)?;
            if !region.contains(raw_poi.world_x, raw_poi.world_y)
                || raw_poi.map_x < 0.0
                || raw_poi.map_x > f64::from(region.map_width_px())
                || raw_poi.map_y < 0.0
                || raw_poi.map_y > f64::from(region.map_height_px())
            {
                return invalid("pois.map_coordinates");
            }
            let projected = region.world_to_map_transform().project(world)?;
            let parity_error = (projected.x() - map.x()).hypot(projected.y() - map.y());
            if !parity_error.is_finite() || parity_error > PARITY_TOLERANCE_PX {
                return invalid("pois.coordinate_parity");
            }
            records_by_region[region_index].push(Poi {
                id: raw_poi.id,
                map_id: raw_poi.map_id,
                region_id: raw_poi.region_id,
                kind,
                display_name: raw_poi.display_name,
                entity_id: raw_poi.entity_id,
                world,
                map,
                source_build_id: raw_poi.source_build_id,
                verified: raw_poi.verified,
            });
        }
        if !kind_coverage[PoiKind::FastTravel as usize]
            || !kind_coverage[PoiKind::Boss as usize]
            || !kind_coverage[PoiKind::Dungeon as usize]
        {
            return invalid("poi_kind_coverage");
        }
        Ok(records_by_region
            .into_iter()
            .zip(regions)
            .map(|(records, region)| Self::from_records(records, region))
            .collect())
    }

    fn from_records(mut records: Vec<Poi>, region: &MapRegion) -> Self {
        records.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.id.as_bytes().cmp(right.id.as_bytes()))
        });
        let mut buckets: BTreeMap<(u32, u32), Vec<PoiHandle>> = BTreeMap::new();
        for (index, poi) in records.iter().enumerate() {
            let handle = PoiHandle(u32::try_from(index).expect("POI cap fits u32"));
            buckets
                .entry((bucket(poi.map.y()), bucket(poi.map.x())))
                .or_default()
                .push(handle);
        }
        Self {
            records,
            buckets,
            map_width_px: region.map_width_px(),
            map_height_px: region.map_height_px(),
        }
    }
}

fn bucket(value: f64) -> u32 {
    (value / BUCKET_SIZE_PX).floor().max(0.0) as u32
}

fn parse_kind(value: &str) -> Result<PoiKind, MapPackError> {
    match value {
        "fast_travel" => Ok(PoiKind::FastTravel),
        "boss" => Ok(PoiKind::Boss),
        "wanted" => Ok(PoiKind::Wanted),
        "dungeon" => Ok(PoiKind::Dungeon),
        _ => invalid("pois.kind"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPoiDocument {
    schema_version: u32,
    game_build_id: String,
    poi_count: u32,
    pois: Vec<RawPoi>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPoi {
    id: String,
    map_id: String,
    region_id: String,
    kind: String,
    display_name: String,
    entity_id: Option<String>,
    world_x: f64,
    world_y: f64,
    map_x: f64,
    map_y: f64,
    source_build_id: String,
    verified: bool,
}

fn invalid<T>(field: &'static str) -> Result<T, MapPackError> {
    Err(MapPackError::InvalidField {
        component: "pois",
        field,
    })
}
