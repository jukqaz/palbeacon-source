use pal_map_pack_store::{
    CullingQuery, MapRect, PoiFilterMask, PoiHandle, PoiIndex, TileIndex, TileKey, TileLevel,
};

use crate::{ActiveViewport, ViewportPose};

/// Selects the greatest available level whose scale is no greater than the effective map scale.
///
/// Scales below the first available level use that first level; scales above the last available
/// level clamp to the last level.
pub fn select_lod_level(levels: &[TileLevel], effective_mpp: f64) -> Option<u8> {
    if effective_mpp.is_nan() || effective_mpp <= 0.0 {
        return None;
    }
    let first_level = levels.iter().map(|level| level.level()).min()?;
    Some(
        levels
            .iter()
            .map(|level| level.level())
            .filter(|level| 2.0_f64.powi(i32::from(*level)) <= effective_mpp)
            .max()
            .unwrap_or(first_level),
    )
}

/// Reuses `output` to publish the filtered, precisely culled POI handles in canonical order.
pub fn select_visible_pois_into(
    index: &PoiIndex,
    viewport: ActiveViewport,
    pose: ViewportPose,
    map_width_px: u32,
    map_height_px: u32,
    filters: PoiFilterMask,
    output: &mut Vec<PoiHandle>,
) {
    output.clear();
    let Some(bounds) = conservative_bounds(
        viewport,
        pose.map_rotation_degrees(),
        map_width_px,
        map_height_px,
    ) else {
        return;
    };

    index.visible_into(&CullingQuery::new(bounds), filters, output);
    output.retain(|handle| {
        index
            .get(*handle)
            .is_some_and(|poi| viewport.contains_map_point(poi.map(), pose.map_rotation_degrees()))
    });
    output.sort_unstable();
    output.dedup();
}

/// Reuses `output` to select current-LOD tiles by the conservative viewport AABB.
pub fn select_visible_tiles_into(
    index: &TileIndex,
    viewport: ActiveViewport,
    pose: ViewportPose,
    map_width_px: u32,
    map_height_px: u32,
    output: &mut Vec<TileKey>,
) {
    output.clear();
    let Some(level) = select_lod_level(
        index.levels(),
        viewport.effective_map_pixels_per_screen_pixel(),
    ) else {
        return;
    };
    let Some(bounds) = conservative_bounds(
        viewport,
        pose.map_rotation_degrees(),
        map_width_px,
        map_height_px,
    ) else {
        return;
    };

    output.extend(
        index
            .all()
            .iter()
            .filter(|tile| {
                tile.key().level == level && rectangles_intersect(bounds, tile.map_rect())
            })
            .map(|tile| tile.key()),
    );
    output.sort_unstable();
    output.dedup();
}

fn conservative_bounds(
    viewport: ActiveViewport,
    map_rotation_degrees: f32,
    map_width_px: u32,
    map_height_px: u32,
) -> Option<MapRect> {
    let bounds = viewport.broad_phase_bounds(map_width_px, map_height_px, map_rotation_degrees)?;
    let metrics = viewport.metrics();
    let extent = 0.5
        * f64::from(metrics.output_width_px().max(metrics.output_height_px()))
        * viewport.effective_map_pixels_per_screen_pixel();
    let epsilon = 64.0
        * f64::EPSILON
        * viewport
            .center()
            .x()
            .abs()
            .max(viewport.center().y().abs())
            .max(extent.abs())
            .max(1.0);
    MapRect::new(
        (bounds.min_x() - epsilon).max(0.0),
        (bounds.min_y() - epsilon).max(0.0),
        (bounds.max_x() + epsilon).min(f64::from(map_width_px)),
        (bounds.max_y() + epsilon).min(f64::from(map_height_px)),
    )
    .ok()
}

fn rectangles_intersect(left: MapRect, right: MapRect) -> bool {
    left.min_x() <= right.max_x()
        && left.max_x() >= right.min_x()
        && left.min_y() <= right.max_y()
        && left.max_y() >= right.min_y()
}
