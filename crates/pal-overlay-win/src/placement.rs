use crate::{HitShape, OverlayLayout, PhysicalRect, PhysicalSize};
use pal_fullscreen_bridge::MAX_FRAME_PIXELS;
use pal_render::OVERLAY_VISUAL_V1;
use std::fmt;

pub const DEFAULT_MINIMAP_SIZE_DIP: u32 = OVERLAY_VISUAL_V1.mini_width_dip;
pub const MIN_SUPPORTED_MINIMAP_SIZE_DIP: u32 = 180;
pub const MAX_SUPPORTED_MINIMAP_SIZE_DIP: u32 = 640;
const REFERENCE_CLIENT_WIDTH_PX: f64 = 1_600.0;
const REFERENCE_CLIENT_HEIGHT_PX: f64 = 900.0;
const MIN_RESPONSIVE_UI_SCALE: f64 = 0.50;
const MINI_MAX_CLIENT_WIDTH_FRACTION: f64 = 0.28;
const MINI_MAX_CLIENT_HEIGHT_FRACTION: f64 = 0.38;
const EXPANDED_CLIENT_WIDTH_FRACTION: f64 = 0.88;
const EXPANDED_CLIENT_HEIGHT_FRACTION: f64 = 0.88;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopLeftOverlayLayouts {
    mini: OverlayLayout,
    expanded: OverlayLayout,
}

impl TopLeftOverlayLayouts {
    pub const fn new(mini: OverlayLayout, expanded: OverlayLayout) -> Self {
        Self { mini, expanded }
    }

    pub const fn mini(self) -> OverlayLayout {
        self.mini
    }

    pub const fn expanded(self) -> OverlayLayout {
        self.expanded
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopLeftLayoutError {
    ZeroDpi,
    MiniSizeOutOfRange,
    ScaledGeometryOverflow,
    InsufficientClientSpace,
}

impl fmt::Display for TopLeftLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDpi => write!(formatter, "tracked client DPI must be nonzero"),
            Self::MiniSizeOutOfRange => write!(
                formatter,
                "minimap size must be within {MIN_SUPPORTED_MINIMAP_SIZE_DIP}..={MAX_SUPPORTED_MINIMAP_SIZE_DIP} DIP"
            ),
            Self::ScaledGeometryOverflow => {
                write!(
                    formatter,
                    "scaled overlay geometry exceeds supported bounds"
                )
            }
            Self::InsufficientClientSpace => {
                write!(
                    formatter,
                    "tracked client cannot fit the exact overlay surface and two insets"
                )
            }
        }
    }
}

impl std::error::Error for TopLeftLayoutError {}

pub fn top_left_overlay_layouts(
    client: PhysicalRect,
    dpi: u32,
    requested_mini_size_px: u32,
) -> Result<TopLeftOverlayLayouts, TopLeftLayoutError> {
    if dpi == 0 {
        return Err(TopLeftLayoutError::ZeroDpi);
    }
    if !(MIN_SUPPORTED_MINIMAP_SIZE_DIP..=MAX_SUPPORTED_MINIMAP_SIZE_DIP)
        .contains(&requested_mini_size_px)
    {
        return Err(TopLeftLayoutError::MiniSizeOutOfRange);
    }

    // `diameter_px` is an explicit physical-pixel preference. Applying the monitor DPI to it a
    // second time made a 420 px map become 630 px at 150% scaling. Chrome still follows DPI, but
    // is capped by the current Palworld client size so switching to a smaller window produces a
    // proportionally smaller surface instead of covering most of the game.
    let ui_scale = responsive_ui_scale(client, dpi);
    let inset = scale_visual(OVERLAY_VISUAL_V1.client_inset_dip, ui_scale)?;
    let expanded_width = fraction_of(client.width, EXPANDED_CLIENT_WIDTH_FRACTION)?;
    let expanded_height = fraction_of(client.height, EXPANDED_CLIENT_HEIGHT_FRACTION)?;
    let minimum_expanded_width = scale_visual(
        OVERLAY_VISUAL_V1.expanded_width_dip,
        MIN_RESPONSIVE_UI_SCALE,
    )?;
    let minimum_expanded_height = scale_visual(
        OVERLAY_VISUAL_V1.expanded_height_dip,
        MIN_RESPONSIVE_UI_SCALE,
    )?;
    if expanded_width < minimum_expanded_width || expanded_height < minimum_expanded_height {
        return Err(TopLeftLayoutError::InsufficientClientSpace);
    }
    let expanded_scale = (f64::from(expanded_width)
        / f64::from(OVERLAY_VISUAL_V1.expanded_width_dip))
    .min(f64::from(expanded_height) / f64::from(OVERLAY_VISUAL_V1.expanded_height_dip));
    let rail_width = scale_visual(OVERLAY_VISUAL_V1.expanded_rail_width_dip, expanded_scale)?;
    let corner_radius = scale_visual(OVERLAY_VISUAL_V1.corner_radius_dip, expanded_scale)?;
    let available_width = client.width.saturating_sub(inset.saturating_mul(2));
    let available_height = client.height.saturating_sub(inset.saturating_mul(2));
    let mini_width_cap = fraction_of(client.width, MINI_MAX_CLIENT_WIDTH_FRACTION)?;
    let mini_height_cap = fraction_of(client.height, MINI_MAX_CLIENT_HEIGHT_FRACTION)?;
    let mini_size = requested_mini_size_px
        .min(mini_width_cap)
        .min(mini_height_cap)
        .min(available_width)
        .min(available_height);
    if mini_size == 0 {
        return Err(TopLeftLayoutError::InsufficientClientSpace);
    }
    let map_width = expanded_width
        .checked_sub(rail_width)
        .ok_or(TopLeftLayoutError::ScaledGeometryOverflow)?;
    let two_insets = inset
        .checked_mul(2)
        .ok_or(TopLeftLayoutError::ScaledGeometryOverflow)?;
    let required_width = mini_size
        .checked_add(two_insets)
        .ok_or(TopLeftLayoutError::ScaledGeometryOverflow)?;
    let required_height = mini_size
        .checked_add(two_insets)
        .ok_or(TopLeftLayoutError::ScaledGeometryOverflow)?;
    if client.width < required_width
        || client.height < required_height
        || expanded_width > client.width
        || expanded_height > client.height
    {
        return Err(TopLeftLayoutError::InsufficientClientSpace);
    }

    let mini_left = checked_screen_add(client.left, inset)?;
    let mini_top = checked_screen_add(client.top, inset)?;
    let expanded_left = checked_screen_add(client.left, (client.width - expanded_width) / 2)?;
    let expanded_top = checked_screen_add(client.top, (client.height - expanded_height) / 2)?;
    let mini_bounds = PhysicalRect::new(mini_left, mini_top, mini_size, mini_size);
    let expanded_bounds =
        PhysicalRect::new(expanded_left, expanded_top, expanded_width, expanded_height);
    let rail_rect = PhysicalRect::new(expanded_left, expanded_top, rail_width, expanded_height);
    let map_left = checked_screen_add(expanded_left, rail_width)?;
    let map_viewport_rect = PhysicalRect::new(map_left, expanded_top, map_width, expanded_height);
    let transfer_size = bounded_transfer_size(map_width, expanded_height)?;

    Ok(TopLeftOverlayLayouts::new(
        OverlayLayout::new(
            client,
            HitShape::RoundedRectangle {
                bounds: mini_bounds,
                // Compact mode uses the familiar circular navigation silhouette. Expanded mode
                // keeps the rectangular map plus its filter rail.
                corner_radius_px: mini_size / 2,
            },
            mini_bounds,
            None,
            PhysicalSize::new(mini_size, mini_size),
        ),
        OverlayLayout::new(
            client,
            HitShape::RoundedRectangle {
                bounds: expanded_bounds,
                corner_radius_px: corner_radius,
            },
            map_viewport_rect,
            Some(rail_rect),
            transfer_size,
        ),
    ))
}

fn bounded_transfer_size(
    logical_width: u32,
    logical_height: u32,
) -> Result<PhysicalSize, TopLeftLayoutError> {
    if logical_width == 0 || logical_height == 0 {
        return Err(TopLeftLayoutError::ScaledGeometryOverflow);
    }
    let logical_pixels = u64::from(logical_width)
        .checked_mul(u64::from(logical_height))
        .ok_or(TopLeftLayoutError::ScaledGeometryOverflow)?;
    let maximum_pixels = MAX_FRAME_PIXELS as u64;
    if logical_pixels <= maximum_pixels {
        return Ok(PhysicalSize::new(logical_width, logical_height));
    }

    // Preserve the logical viewport aspect ratio while fitting the fixed bridge allocation. The
    // final integer correction is intentionally conservative so rounding can never cross the
    // shared-memory boundary.
    let scale = (maximum_pixels as f64 / logical_pixels as f64).sqrt();
    let mut width = (f64::from(logical_width) * scale).floor().max(1.0) as u32;
    let mut height = (f64::from(logical_height) * scale).floor().max(1.0) as u32;
    while u64::from(width) * u64::from(height) > maximum_pixels {
        if width >= height {
            width = width.saturating_sub(1).max(1);
        } else {
            height = height.saturating_sub(1).max(1);
        }
    }
    Ok(PhysicalSize::new(width, height))
}

fn responsive_ui_scale(client: PhysicalRect, dpi: u32) -> f64 {
    let dpi_scale = f64::from(dpi) / 96.0;
    let client_scale = (f64::from(client.width) / REFERENCE_CLIENT_WIDTH_PX)
        .min(f64::from(client.height) / REFERENCE_CLIENT_HEIGHT_PX)
        .max(MIN_RESPONSIVE_UI_SCALE);
    dpi_scale.min(client_scale)
}

fn scale_visual(value: u32, scale: f64) -> Result<u32, TopLeftLayoutError> {
    let scaled = (f64::from(value) * scale).round();
    if !scaled.is_finite() || scaled < 1.0 || scaled > f64::from(u32::MAX) {
        return Err(TopLeftLayoutError::ScaledGeometryOverflow);
    }
    Ok(scaled as u32)
}

fn fraction_of(value: u32, fraction: f64) -> Result<u32, TopLeftLayoutError> {
    let scaled = (f64::from(value) * fraction).round();
    if !scaled.is_finite() || scaled < 1.0 || scaled > f64::from(u32::MAX) {
        return Err(TopLeftLayoutError::ScaledGeometryOverflow);
    }
    Ok(scaled as u32)
}

fn checked_screen_add(base: i32, offset: u32) -> Result<i32, TopLeftLayoutError> {
    i32::try_from(i64::from(base) + i64::from(offset))
        .map_err(|_| TopLeftLayoutError::ScaledGeometryOverflow)
}
