use std::error::Error;
use std::fmt;
use std::sync::Arc;

use pal_domain::{CoreState, DisplayMode, Freshness, PoiFilters};
use pal_map_pack_store::{
    MapPackStore, MapPoint, MapRegionPack, PoiFilterMask, PoiHandle, PoiKind, TileKey,
    TransformError, WorldPoint,
};

use crate::{
    ActiveViewport, ExpandedMapViewport, HeadingStatus, InterpolationPolicy, MiniMapViewport,
    PlayerPose, ViewportLayout, ViewportPose, ViewportValidationError, compute_viewport_pose,
    select_visible_pois_into, select_visible_tiles_into,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceCursor {
    generation: u64,
    boot_id: Arc<[u8]>,
    sequence: u64,
}

impl SourceCursor {
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn boot_id(&self) -> &[u8] {
        &self.boot_id
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    Transform(TransformError),
    Viewport(ViewportValidationError),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transform(error) => write!(formatter, "snapshot transform failed: {error}"),
            Self::Viewport(error) => write!(formatter, "snapshot viewport failed: {error}"),
        }
    }
}

impl Error for SnapshotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transform(error) => Some(error),
            Self::Viewport(error) => Some(error),
        }
    }
}

impl From<TransformError> for SnapshotError {
    fn from(error: TransformError) -> Self {
        Self::Transform(error)
    }
}

impl From<ViewportValidationError> for SnapshotError {
    fn from(error: ViewportValidationError) -> Self {
        Self::Viewport(error)
    }
}

#[derive(Debug)]
pub struct RenderSnapshot {
    pack: Arc<MapPackStore>,
    build_id: Arc<str>,
    canonical_pack_hash: [u8; 32],
    settings_version: u64,
    source_cursor: Option<SourceCursor>,
    visible: bool,
    display_mode: DisplayMode,
    freshness: Freshness,
    heading_status: HeadingStatus,
    interpolation_policy: InterpolationPolicy,
    player_pose: Option<PlayerPose>,
    active_region_index: Option<usize>,
    active_viewport: ActiveViewport,
    viewport_pose: ViewportPose,
    poi_filters: PoiFilterMask,
    visible_tiles: Arc<[TileKey]>,
    visible_pois: Arc<[PoiHandle]>,
}

impl RenderSnapshot {
    pub const fn pack(&self) -> &Arc<MapPackStore> {
        &self.pack
    }

    pub fn build_id(&self) -> &str {
        &self.build_id
    }

    pub const fn canonical_pack_hash(&self) -> [u8; 32] {
        self.canonical_pack_hash
    }

    pub const fn settings_version(&self) -> u64 {
        self.settings_version
    }

    pub const fn source_cursor(&self) -> Option<&SourceCursor> {
        self.source_cursor.as_ref()
    }

    pub const fn visible(&self) -> bool {
        self.visible
    }

    pub const fn display_mode(&self) -> DisplayMode {
        self.display_mode
    }

    pub const fn freshness(&self) -> Freshness {
        self.freshness
    }

    pub const fn heading_status(&self) -> HeadingStatus {
        self.heading_status
    }

    pub const fn interpolation_policy(&self) -> InterpolationPolicy {
        self.interpolation_policy
    }

    pub const fn player_pose(&self) -> Option<PlayerPose> {
        self.player_pose
    }

    pub fn region_pack(&self) -> Option<&MapRegionPack> {
        self.active_region_index
            .and_then(|index| self.pack.region_pack(index))
    }

    pub const fn active_region_index(&self) -> Option<usize> {
        self.active_region_index
    }

    pub const fn active_viewport(&self) -> ActiveViewport {
        self.active_viewport
    }

    pub const fn viewport_pose(&self) -> ViewportPose {
        self.viewport_pose
    }

    pub const fn poi_filters(&self) -> PoiFilterMask {
        self.poi_filters
    }

    pub fn visible_tiles(&self) -> &[TileKey] {
        &self.visible_tiles
    }

    pub fn visible_pois(&self) -> &[PoiHandle] {
        &self.visible_pois
    }
}

pub struct SnapshotBuilder {
    pack: Arc<MapPackStore>,
    tile_scratch: Vec<TileKey>,
    poi_scratch: Vec<PoiHandle>,
}

struct SnapshotSourceData {
    cursor: Option<SourceCursor>,
    player_pose: Option<PlayerPose>,
    region_index: Option<usize>,
}

impl SnapshotBuilder {
    pub fn new(pack: Arc<MapPackStore>) -> Self {
        Self {
            pack,
            tile_scratch: Vec::new(),
            poi_scratch: Vec::new(),
        }
    }

    pub fn replace_pack(&mut self, pack: Arc<MapPackStore>) -> Arc<MapPackStore> {
        std::mem::replace(&mut self.pack, pack)
    }

    pub fn build(
        &mut self,
        state: &CoreState,
        layout: ViewportLayout,
    ) -> Result<Arc<RenderSnapshot>, SnapshotError> {
        let SnapshotSourceData {
            cursor: source_cursor,
            player_pose,
            region_index: active_region_index,
        } = self.source_data(state)?;
        let active_viewport = active_viewport(state, layout, player_pose)?;
        let heading = player_pose.and_then(PlayerPose::heading_degrees);
        let viewport_pose = compute_viewport_pose(state.settings().rotation_mode, heading);
        let poi_filters = poi_filter_mask(&state.settings().poi_filters);
        let manifest = self.pack.manifest();

        if let Some(region_pack) =
            active_region_index.and_then(|index| self.pack.region_pack(index))
        {
            let region = region_pack.region();
            select_visible_tiles_into(
                region_pack.tile_index(),
                active_viewport,
                viewport_pose,
                region.map_width_px(),
                region.map_height_px(),
                &mut self.tile_scratch,
            );
            select_visible_pois_into(
                region_pack.poi_index(),
                active_viewport,
                viewport_pose,
                region.map_width_px(),
                region.map_height_px(),
                poi_filters,
                &mut self.poi_scratch,
            );
        } else {
            self.tile_scratch.clear();
            self.poi_scratch.clear();
        }

        let interpolation_policy = match state.freshness() {
            Freshness::Live | Freshness::Delayed => InterpolationPolicy::Interpolate,
            Freshness::Stale | Freshness::Offline => InterpolationPolicy::Freeze,
        };
        Ok(Arc::new(RenderSnapshot {
            pack: Arc::clone(&self.pack),
            build_id: Arc::from(manifest.game_build_id()),
            canonical_pack_hash: self.pack.canonical_pack_hash(),
            settings_version: state.settings_version(),
            source_cursor,
            visible: state.visible() && active_region_index.is_some(),
            display_mode: state.settings().display_mode,
            freshness: state.freshness(),
            heading_status: viewport_pose.heading_status(),
            interpolation_policy,
            player_pose,
            active_region_index,
            active_viewport,
            viewport_pose,
            poi_filters,
            visible_tiles: Arc::from(self.tile_scratch.as_slice()),
            visible_pois: Arc::from(self.poi_scratch.as_slice()),
        }))
    }

    fn source_data(&self, state: &CoreState) -> Result<SnapshotSourceData, SnapshotError> {
        let Some(sample) = state.position_sample() else {
            return Ok(SnapshotSourceData {
                cursor: None,
                player_pose: None,
                region_index: None,
            });
        };
        let world = WorldPoint::new(sample.x(), sample.y())?;
        let cursor = SourceCursor {
            generation: sample.source_connection_generation(),
            boot_id: Arc::from(sample.agent_boot_id()),
            sequence: sample.sequence(),
        };
        let Ok(region_index) = self.pack.select_region_pack_index(world.x(), world.y()) else {
            return Ok(cleared_source_data(cursor));
        };
        let Some(region_pack) = self.pack.region_pack(region_index) else {
            return Ok(cleared_source_data(cursor));
        };
        let Ok(map) = region_pack.transform().project(world) else {
            return Ok(cleared_source_data(cursor));
        };
        let heading = state
            .heading_available()
            .then(|| sample.heading_degrees())
            .flatten();
        let pose = PlayerPose::new(world, map, sample.z(), heading)?;
        Ok(SnapshotSourceData {
            cursor: Some(cursor),
            player_pose: Some(pose),
            region_index: Some(region_index),
        })
    }
}

fn cleared_source_data(cursor: SourceCursor) -> SnapshotSourceData {
    SnapshotSourceData {
        cursor: Some(cursor),
        player_pose: None,
        region_index: None,
    }
}

fn active_viewport(
    state: &CoreState,
    layout: ViewportLayout,
    player_pose: Option<PlayerPose>,
) -> Result<ActiveViewport, SnapshotError> {
    match state.settings().display_mode {
        DisplayMode::MiniMap => {
            let view = state.mini_map_view();
            let center = match player_pose {
                Some(player) => player.map(),
                None => MapPoint::new(view.center_x(), view.center_y())?,
            };
            Ok(ActiveViewport::Mini(MiniMapViewport::new(
                center,
                view.zoom(),
                layout.mini_metrics(),
            )?))
        }
        DisplayMode::ExpandedMap => {
            let view = state.expanded_map_view();
            let center = MapPoint::new(view.center_x(), view.center_y())?;
            Ok(ActiveViewport::Expanded(ExpandedMapViewport::new(
                center,
                view.zoom(),
                layout.expanded_metrics(),
            )?))
        }
    }
}

fn poi_filter_mask(filters: &PoiFilters) -> PoiFilterMask {
    let mut kinds = [None; 4];
    if filters.fast_travel {
        kinds[0] = Some(PoiKind::FastTravel);
    }
    if filters.boss {
        kinds[1] = Some(PoiKind::Boss);
    }
    if filters.wanted {
        kinds[2] = Some(PoiKind::Wanted);
    }
    if filters.dungeon {
        kinds[3] = Some(PoiKind::Dungeon);
    }
    PoiFilterMask::from_kinds(kinds.into_iter().flatten())
}
