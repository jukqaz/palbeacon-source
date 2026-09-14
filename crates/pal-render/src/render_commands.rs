use pal_map_pack_store::{MapPoint, MapRect, PoiHandle, PoiKind, TileKey};

use crate::{ActiveViewport, RenderSnapshot, ScreenPoint};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapTransformCommand {
    viewport: ActiveViewport,
    map_rotation_degrees: f32,
}

impl MapTransformCommand {
    pub const fn viewport(self) -> ActiveViewport {
        self.viewport
    }

    pub const fn map_rotation_degrees(self) -> f32 {
        self.map_rotation_degrees
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapTileCommand {
    identity: RegionTileIdentity,
    map_rect: MapRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RegionTileIdentity {
    pack_hash: [u8; 32],
    region_index: usize,
    key: TileKey,
}

impl RegionTileIdentity {
    pub const fn pack_hash(self) -> [u8; 32] {
        self.pack_hash
    }

    pub const fn region_index(self) -> usize {
        self.region_index
    }

    pub const fn key(self) -> TileKey {
        self.key
    }
}

impl MapTileCommand {
    pub const fn key(self) -> TileKey {
        self.identity.key
    }

    pub const fn identity(self) -> RegionTileIdentity {
        self.identity
    }

    pub const fn map_rect(self) -> MapRect {
        self.map_rect
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoiCommand {
    identity: RegionPoiIdentity,
    kind: PoiKind,
    map_position: MapPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RegionPoiIdentity {
    pack_hash: [u8; 32],
    region_index: usize,
    handle: PoiHandle,
}

impl RegionPoiIdentity {
    pub const fn pack_hash(self) -> [u8; 32] {
        self.pack_hash
    }

    pub const fn region_index(self) -> usize {
        self.region_index
    }

    pub const fn handle(self) -> PoiHandle {
        self.handle
    }
}

impl PoiCommand {
    pub const fn handle(self) -> PoiHandle {
        self.identity.handle
    }

    pub const fn identity(self) -> RegionPoiIdentity {
        self.identity
    }

    pub const fn kind(self) -> PoiKind {
        self.kind
    }

    pub const fn map_position(self) -> MapPoint {
        self.map_position
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerArrowCommand {
    screen_position: ScreenPoint,
    rotation_degrees: f32,
}

impl PlayerArrowCommand {
    pub const fn screen_position(self) -> ScreenPoint {
        self.screen_position
    }

    pub const fn rotation_degrees(self) -> f32 {
        self.rotation_degrees
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderCommand {
    MapTransform(MapTransformCommand),
    MapTile(MapTileCommand),
    Poi(PoiCommand),
    PlayerArrow(PlayerArrowCommand),
}

#[derive(Debug)]
pub struct RenderCommandBuffer {
    commands: Vec<RenderCommand>,
}

impl RenderCommandBuffer {
    pub fn required_capacity(snapshot: &RenderSnapshot) -> usize {
        if !snapshot.visible() || snapshot.region_pack().is_none() {
            return 0;
        }
        let player_arrow_count = usize::from(snapshot.player_pose().is_some());
        1_usize
            .saturating_add(snapshot.visible_tiles().len())
            .saturating_add(snapshot.visible_pois().len())
            .saturating_add(player_arrow_count)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            commands: Vec::with_capacity(capacity),
        }
    }

    pub fn build_from(&mut self, snapshot: &RenderSnapshot) {
        self.commands.clear();
        let Some(_) = snapshot.region_pack() else {
            return;
        };
        if !snapshot.visible() {
            return;
        }
        self.commands
            .push(RenderCommand::MapTransform(MapTransformCommand {
                viewport: snapshot.active_viewport(),
                map_rotation_degrees: snapshot.viewport_pose().map_rotation_degrees(),
            }));
        let region_index = snapshot
            .active_region_index()
            .expect("renderable snapshot has an active region");
        let pack_hash = snapshot.canonical_pack_hash();
        self.commands
            .extend(snapshot.visible_tiles().iter().copied().map(|key| {
                let map_rect = snapshot
                    .region_pack()
                    .expect("visible snapshot remains bound to its selected region")
                    .tile_descriptor(key)
                    .expect("snapshot tile handles remain bound to their immutable pack")
                    .map_rect();
                RenderCommand::MapTile(MapTileCommand {
                    identity: RegionTileIdentity {
                        pack_hash,
                        region_index,
                        key,
                    },
                    map_rect,
                })
            }));
        self.commands
            .extend(snapshot.visible_pois().iter().copied().map(|handle| {
                let poi = snapshot
                    .region_pack()
                    .expect("visible snapshot remains bound to its selected region")
                    .poi_index()
                    .get(handle)
                    .expect("snapshot POI handles remain bound to their immutable pack");
                RenderCommand::Poi(PoiCommand {
                    identity: RegionPoiIdentity {
                        pack_hash,
                        region_index,
                        handle,
                    },
                    kind: poi.kind(),
                    map_position: poi.map(),
                })
            }));

        if snapshot.player_pose().is_some() {
            let metrics = snapshot.active_viewport().metrics();
            let screen_position = ScreenPoint::new(
                f64::from(metrics.output_width_px()) * 0.5,
                f64::from(metrics.output_height_px()) * 0.5,
            )
            .expect("validated viewport dimensions have a finite center");
            self.commands
                .push(RenderCommand::PlayerArrow(PlayerArrowCommand {
                    screen_position,
                    rotation_degrees: snapshot.viewport_pose().player_rotation_degrees(),
                }));
        }
    }

    pub fn commands(&self) -> &[RenderCommand] {
        &self.commands
    }
}
