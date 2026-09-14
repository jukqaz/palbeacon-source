//! Pure, platform-independent render contracts.

mod culling;
mod overlay_visual;
mod render_commands;
mod snapshot;
mod viewport;

pub use culling::{select_lod_level, select_visible_pois_into, select_visible_tiles_into};
pub use overlay_visual::{
    ChromePlan, FilterAction, GateBadge, OVERLAY_VISUAL_V1, OverlayPalette, OverlayVisualSpec,
    RailAction, RailButtonState, Rgba8, StatusTone, build_chrome_plan, filter_button_tone,
    rail_button_tone,
};
pub use render_commands::{
    MapTileCommand, MapTransformCommand, PlayerArrowCommand, PoiCommand, RegionPoiIdentity,
    RegionTileIdentity, RenderCommand, RenderCommandBuffer,
};
pub use snapshot::{RenderSnapshot, SnapshotBuilder, SnapshotError, SourceCursor};
pub use viewport::{
    ActiveViewport, ExpandedMapViewport, HeadingStatus, InterpolationPolicy, MiniMapViewport,
    PlayerPose, ScreenPoint, ViewportLayout, ViewportMetrics, ViewportPose,
    ViewportValidationError, compute_viewport_pose, interpolate_player_pose,
};
