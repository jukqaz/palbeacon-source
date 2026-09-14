use std::ffi::c_void;
use std::fmt;
use std::ptr::null_mut;

use pal_render::{
    ChromePlan, FilterAction, GateBadge, OVERLAY_VISUAL_V1, RailAction, Rgba8, StatusTone,
    filter_button_tone, rail_button_tone,
};
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreatePen, CreateSolidBrush, DIB_RGB_COLORS,
    DeleteObject, FillRect, HDC, LineTo, MoveToEx, NULL_BRUSH, NULL_PEN, PS_SOLID, RoundRect,
    SRCCOPY, SelectObject, SetBkMode, SetTextColor, StretchDIBits, TRANSPARENT, TextOutW,
};

use crate::actual_map_preview::CpuMapSurface;
use crate::renderer::{OverlayRenderer, RendererKind};
use crate::{HitShape, OverlayLayout, PhysicalPoint, PhysicalRect, PhysicalSize};

const GATE_BADGE: &[u16] = &[0xC88C, 0xD45C, 0x0020, 0xAC80, 0xC99D, 0x0020, 0xC911];
const CARDINAL_N: &[u16] = &widen_ascii(*b"N");
const CARDINAL_E: &[u16] = &widen_ascii(*b"E");
const CARDINAL_S: &[u16] = &widen_ascii(*b"S");
const CARDINAL_W: &[u16] = &widen_ascii(*b"W");
const FILTER_FAST_TRAVEL: &[u16] = &[0xC774, 0xB3D9];
const FILTER_BOSS: &[u16] = &[0xBCF4, 0xC2A4];
const FILTER_WANTED: &[u16] = &[0xD604, 0xC0C1];
const FILTER_DUNGEON: &[u16] = &[0xB358, 0xC804];
const FILTER_TOWER: &[u16] = &[0xD0D1];
const FILTER_EGG: &[u16] = &[0xC54C];
const FILTER_RESOURCES: &[u16] = &[0xC790, 0xC6D0];
const FILTER_SALVAGE: &[u16] = &[0xC778, 0xC591];
const FILTER_PANEL_TITLE: &[u16] = &[0xC9C0, 0xB3C4, 0x0020, 0xC815, 0xBCF4];

const fn widen_ascii<const N: usize>(input: [u8; N]) -> [u16; N] {
    let mut output = [0; N];
    let mut index = 0;
    while index < N {
        output[index] = input[index] as u16;
        index += 1;
    }
    output
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GdiPaintPlan {
    layout: OverlayLayout,
    chrome: ChromePlan,
}

impl GdiPaintPlan {
    pub const fn from_layout(layout: OverlayLayout, chrome: ChromePlan) -> Self {
        Self { layout, chrome }
    }

    pub const fn renderer_kind(self) -> RendererKind {
        RendererKind::GdiCorrectnessPreview
    }

    pub const fn status_tone(self) -> StatusTone {
        self.chrome.status_tone()
    }

    pub const fn map_viewport_rect(self) -> PhysicalRect {
        self.layout.map_viewport_rect
    }

    pub const fn has_rounded_border(self) -> bool {
        matches!(self.layout.surface_shape, HitShape::RoundedRectangle { .. })
    }

    pub const fn has_cardinals(self) -> bool {
        self.chrome.shows_cardinals()
    }

    pub const fn has_scale_bar(self) -> bool {
        self.chrome.shows_scale_bar()
    }

    pub const fn has_layered_minimap_bezel(self) -> bool {
        is_circular_minimap_layout(self.layout)
    }

    pub const fn minimap_bezel_layer_count(self) -> usize {
        if self.has_layered_minimap_bezel() {
            5
        } else {
            0
        }
    }

    pub const fn expanded_frame_layer_count(self) -> usize {
        if self.has_layered_minimap_bezel() {
            0
        } else {
            3
        }
    }

    pub const fn has_compact_gate_badge(self) -> bool {
        matches!(self.chrome.gate_badge(), GateBadge::NotApproved)
            && self.layout.control_rail_rect.is_some()
    }

    pub const fn has_control_rail(self) -> bool {
        self.layout.control_rail_rect.is_some()
    }

    pub const fn rail_glyph_count(self) -> usize {
        self.chrome.rail_actions().len()
    }

    pub fn rail_glyph_center(self, action: RailAction) -> Option<PhysicalPoint> {
        self.chrome
            .rail_actions()
            .contains(&action)
            .then(|| rail_action_rect(self.layout, action))
            .flatten()
            .and_then(rect_center)
    }

    pub fn rail_glyph_tone(self, action: RailAction) -> Option<StatusTone> {
        self.chrome
            .rail_actions()
            .contains(&action)
            .then(|| rail_button_tone(self.chrome.rail_button_state(action)))
    }

    pub const fn has_filter_strip(self) -> bool {
        self.chrome.has_filter_strip()
    }

    pub const fn filter_button_count(self) -> usize {
        self.chrome.filter_actions().len()
    }

    pub fn filter_panel_rect(self) -> Option<PhysicalRect> {
        self.has_filter_strip()
            .then(|| filter_panel_rect(self.layout))
            .flatten()
    }

    pub fn filter_button_rect(self, action: FilterAction) -> Option<PhysicalRect> {
        self.chrome
            .filter_actions()
            .contains(&action)
            .then(|| filter_button_rect(self.layout, action))
            .flatten()
    }

    pub fn filter_button_tone(self, action: FilterAction) -> Option<StatusTone> {
        self.chrome
            .filter_actions()
            .contains(&action)
            .then(|| filter_button_tone(self.chrome.filter_button_state(action)))
    }

    pub const fn has_center_crosshair(self) -> bool {
        false
    }

    pub const fn has_debug_bands(self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GdiRendererError {
    InvalidGeometry,
    PixelCountMismatch,
    SurfaceSizeMismatch {
        expected: PhysicalSize,
        actual: PhysicalSize,
    },
    NativeResource,
}

impl fmt::Display for GdiRendererError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidGeometry => "GDI correctness preview geometry is invalid",
            Self::PixelCountMismatch => "GDI correctness preview pixel count does not match",
            Self::SurfaceSizeMismatch { .. } => {
                "CPU map surface does not match the active map viewport"
            }
            Self::NativeResource => "GDI correctness preview could not allocate a drawing object",
        })
    }
}

impl std::error::Error for GdiRendererError {}

pub struct GdiCorrectnessRenderer {
    dc: HDC,
    layout: OverlayLayout,
    dpi: u32,
}

impl GdiCorrectnessRenderer {
    /// Creates a renderer for a DC that remains valid for the renderer's lifetime.
    ///
    /// # Safety
    ///
    /// `dc` must be a valid paint DC owned by the calling thread.
    pub unsafe fn for_paint(dc: HDC, layout: OverlayLayout, dpi: u32) -> Self {
        Self {
            dc,
            layout,
            dpi: dpi.max(1),
        }
    }

    pub fn present_pixels(
        &mut self,
        size: PhysicalSize,
        pixels: &[u32],
        chrome: ChromePlan,
    ) -> Result<(), GdiRendererError> {
        let expected = usize::try_from(size.width)
            .ok()
            .and_then(|width| {
                usize::try_from(size.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or(GdiRendererError::InvalidGeometry)?;
        if pixels.len() != expected {
            return Err(GdiRendererError::PixelCountMismatch);
        }
        self.paint(Some((size, pixels)), chrome)
    }

    pub fn present_without_map(&mut self, chrome: ChromePlan) -> Result<(), GdiRendererError> {
        self.paint(None, chrome)
    }

    fn paint(
        &mut self,
        map: Option<(PhysicalSize, &[u32])>,
        chrome: ChromePlan,
    ) -> Result<(), GdiRendererError> {
        let plan = GdiPaintPlan::from_layout(self.layout, chrome);
        let surface = local_surface_rect(self.layout);
        let map_rect = localize(self.layout.map_viewport_rect, self.layout)?;
        fill(self.dc, surface, OVERLAY_VISUAL_V1.palette.neutral_backdrop)?;
        if let Some((size, pixels)) = map {
            stretch_pixels(self.dc, map_rect, size, pixels)?;
        }
        if let Some(rail) = self.layout.control_rail_rect {
            let rail = localize(rail, self.layout)?;
            fill(self.dc, rail, OVERLAY_VISUAL_V1.palette.neutral_rail)?;
            draw_divider(
                self.dc,
                rail,
                self.dip(1),
                OVERLAY_VISUAL_V1.palette.neutral_border,
            )?;
            draw_rail_glyphs(self.dc, plan)?;
        }
        if plan.has_filter_strip() {
            draw_filter_strip(self.dc, plan)?;
        }
        if plan.has_layered_minimap_bezel() {
            draw_minimap_bezel(
                self.dc,
                surface,
                corner_radius(self.layout),
                self.dip(OVERLAY_VISUAL_V1.minimap_bezel_body_dip),
                self.dip(OVERLAY_VISUAL_V1.minimap_bezel_inner_inset_dip),
                self.dip(OVERLAY_VISUAL_V1.minimap_tick_length_dip),
            )?;
        } else {
            draw_expanded_frame(
                self.dc,
                surface,
                corner_radius(self.layout),
                self.dip(OVERLAY_VISUAL_V1.border_dip),
            )?;
        }
        if plan.has_cardinals() {
            draw_cardinals(
                self.dc,
                map_rect,
                self.dip(8),
                OVERLAY_VISUAL_V1.palette.neutral_text,
                OVERLAY_VISUAL_V1.palette.cyan_live,
            );
        }
        if plan.has_scale_bar() {
            draw_scale_bar(
                self.dc,
                map_rect,
                self.dip(48),
                self.dip(12),
                OVERLAY_VISUAL_V1.palette.neutral_text,
            )?;
        }
        draw_status_dot(
            self.dc,
            map_rect,
            self.dip(OVERLAY_VISUAL_V1.status_dot_dip),
            tone_color(chrome.status_tone()),
            plan.has_layered_minimap_bezel(),
        )?;
        if plan.has_compact_gate_badge() {
            draw_gate_badge(
                self.dc,
                map_rect,
                self.dip(6),
                OVERLAY_VISUAL_V1.palette.amber_warning,
            )?;
        }
        Ok(())
    }

    fn dip(&self, value: u32) -> i32 {
        let physical = u64::from(value)
            .saturating_mul(u64::from(self.dpi))
            .saturating_add(48)
            / 96;
        i32::try_from(physical.max(1)).unwrap_or(i32::MAX)
    }
}

pub fn filter_button_rect(layout: OverlayLayout, action: FilterAction) -> Option<PhysicalRect> {
    let panel = filter_panel_rect(layout)?;
    let dpi = estimated_dpi(layout);
    let width = scale_dip(OVERLAY_VISUAL_V1.filter_button_width_dip, dpi);
    let height = scale_dip(OVERLAY_VISUAL_V1.filter_button_height_dip, dpi);
    let gap = scale_dip(OVERLAY_VISUAL_V1.filter_button_gap_dip, dpi);
    let inset = scale_dip(OVERLAY_VISUAL_V1.expanded_panel_inset_dip, dpi);
    let index = match action {
        FilterAction::FastTravel => 0_i32,
        FilterAction::Boss => 1,
        FilterAction::Wanted => 2,
        FilterAction::Dungeon => 3,
        FilterAction::Tower => 4,
        FilterAction::Egg => 5,
        FilterAction::Resources => 6,
        FilterAction::Salvage => 7,
    };
    let column = index % 2;
    let row = index / 2;
    let left = panel
        .left
        .checked_add(inset)?
        .checked_add(column.checked_mul(width.checked_add(gap)?)?)?;
    let top = panel
        .top
        .checked_add(scale_dip(58, dpi))?
        .checked_add(row.checked_mul(height.checked_add(gap)?)?)?;
    let rect = PhysicalRect::new(
        left,
        top,
        u32::try_from(width).ok()?,
        u32::try_from(height).ok()?,
    );
    layout
        .map_viewport_rect
        .contains(PhysicalPoint::new(
            rect.left.checked_add(width.checked_sub(1)?)?,
            rect.top.checked_add(height.checked_sub(1)?)?,
        ))
        .then_some(rect)
}

pub fn rail_action_rect(layout: OverlayLayout, action: RailAction) -> Option<PhysicalRect> {
    let rail = layout.control_rail_rect?;
    let dpi = estimated_dpi(layout);
    let inset = scale_dip(OVERLAY_VISUAL_V1.rail_inset_dip, dpi);
    let button = scale_dip(OVERLAY_VISUAL_V1.rail_button_dip, dpi);
    let gap = scale_dip(OVERLAY_VISUAL_V1.rail_inset_dip, dpi);
    let (left, top) = if action == RailAction::Collapse {
        (
            rail.left
                .checked_add(i32::try_from(rail.width).ok()?.checked_sub(button)? / 2)?,
            rail.top.checked_add(inset)?,
        )
    } else {
        let index = match action {
            RailAction::ZoomOut => 0_i32,
            RailAction::ZoomIn => 1,
            RailAction::ToggleRotation => 2,
            RailAction::Lock => 3,
            RailAction::Collapse => return None,
        };
        let bottom = layout
            .map_viewport_rect
            .top
            .checked_add(i32::try_from(layout.map_viewport_rect.height).ok()?)?;
        (
            layout
                .map_viewport_rect
                .left
                .checked_add(inset)?
                .checked_add(index.checked_mul(button.checked_add(gap)?)?)?,
            bottom.checked_sub(inset)?.checked_sub(button)?,
        )
    };
    Some(PhysicalRect::new(
        left,
        top,
        u32::try_from(button).ok()?,
        u32::try_from(button).ok()?,
    ))
}

pub fn action_dock_rect(layout: OverlayLayout) -> Option<PhysicalRect> {
    layout.control_rail_rect?;
    let dpi = estimated_dpi(layout);
    let inset = scale_dip(OVERLAY_VISUAL_V1.rail_inset_dip, dpi);
    let button = scale_dip(OVERLAY_VISUAL_V1.rail_button_dip, dpi);
    let gap = scale_dip(OVERLAY_VISUAL_V1.rail_inset_dip, dpi);
    let width = button.checked_mul(4)?.checked_add(gap.checked_mul(3)?)?;
    let bottom = layout
        .map_viewport_rect
        .top
        .checked_add(i32::try_from(layout.map_viewport_rect.height).ok()?)?;
    Some(PhysicalRect::new(
        layout.map_viewport_rect.left.checked_add(inset)?,
        bottom.checked_sub(inset)?.checked_sub(button)?,
        u32::try_from(width).ok()?,
        u32::try_from(button).ok()?,
    ))
}

fn rect_center(rect: PhysicalRect) -> Option<PhysicalPoint> {
    Some(PhysicalPoint::new(
        rect.left.checked_add(i32::try_from(rect.width).ok()? / 2)?,
        rect.top.checked_add(i32::try_from(rect.height).ok()? / 2)?,
    ))
}

pub fn filter_panel_rect(layout: OverlayLayout) -> Option<PhysicalRect> {
    layout.control_rail_rect?;
    let dpi = estimated_dpi(layout);
    let inset = scale_dip(OVERLAY_VISUAL_V1.expanded_panel_inset_dip, dpi);
    let width = scale_dip(OVERLAY_VISUAL_V1.expanded_panel_width_dip, dpi);
    let button_height = scale_dip(OVERLAY_VISUAL_V1.filter_button_height_dip, dpi);
    let gap = scale_dip(OVERLAY_VISUAL_V1.filter_button_gap_dip, dpi);
    let height = scale_dip(70, dpi)
        .checked_add(button_height.checked_mul(4)?)?
        .checked_add(gap.checked_mul(3)?)?;
    let top = layout.map_viewport_rect.top.checked_add(inset)?;
    let rect = PhysicalRect::new(
        layout.map_viewport_rect.left.checked_add(inset)?,
        top,
        u32::try_from(width).ok()?,
        u32::try_from(height).ok()?,
    );
    layout
        .map_viewport_rect
        .contains(PhysicalPoint::new(
            rect.left.checked_add(width.checked_sub(1)?)?,
            rect.top.checked_add(height.checked_sub(1)?)?,
        ))
        .then_some(rect)
}

impl OverlayRenderer for GdiCorrectnessRenderer {
    type Error = GdiRendererError;

    fn kind(&self) -> RendererKind {
        RendererKind::GdiCorrectnessPreview
    }

    fn resize(&mut self, layout: OverlayLayout, dpi: u32) -> Result<(), Self::Error> {
        if layout.viewport_size.width == 0 || layout.viewport_size.height == 0 || dpi == 0 {
            return Err(GdiRendererError::InvalidGeometry);
        }
        self.layout = layout;
        self.dpi = dpi;
        Ok(())
    }

    fn present(&mut self, map: &CpuMapSurface, chrome: ChromePlan) -> Result<(), Self::Error> {
        if map.size() != self.layout.viewport_size {
            return Err(GdiRendererError::SurfaceSizeMismatch {
                expected: self.layout.viewport_size,
                actual: map.size(),
            });
        }
        self.present_pixels(map.size(), map.pixels(), chrome)
    }
}

pub fn estimated_dpi(layout: OverlayLayout) -> u32 {
    if is_circular_minimap_layout(layout) {
        let width = layout.surface_shape.bounds().width;
        return width
            .saturating_mul(96)
            .saturating_add(OVERLAY_VISUAL_V1.mini_width_dip / 2)
            .checked_div(OVERLAY_VISUAL_V1.mini_width_dip)
            .unwrap_or(96)
            .max(1);
    }
    let radius = corner_radius(layout);
    if radius == 0 {
        return 96;
    }
    let reference_radius = OVERLAY_VISUAL_V1.corner_radius_dip.max(1);
    radius
        .saturating_mul(96)
        .saturating_add(reference_radius / 2)
        / reference_radius
}

const fn is_circular_minimap_layout(layout: OverlayLayout) -> bool {
    layout.control_rail_rect.is_none()
        && layout.map_viewport_rect.width == layout.map_viewport_rect.height
        && match layout.surface_shape {
            HitShape::RoundedRectangle {
                bounds,
                corner_radius_px,
            } => {
                bounds.width == bounds.height && corner_radius_px.saturating_mul(2) >= bounds.width
            }
        }
}

fn scale_dip(value: u32, dpi: u32) -> i32 {
    let physical = u64::from(value)
        .saturating_mul(u64::from(dpi))
        .saturating_add(48)
        / 96;
    i32::try_from(physical.max(1)).unwrap_or(i32::MAX)
}

fn local_surface_rect(layout: OverlayLayout) -> PhysicalRect {
    let bounds = layout.surface_shape.bounds();
    PhysicalRect::new(0, 0, bounds.width, bounds.height)
}

fn localize(rect: PhysicalRect, layout: OverlayLayout) -> Result<PhysicalRect, GdiRendererError> {
    let bounds = layout.surface_shape.bounds();
    let left = i64::from(rect.left) - i64::from(bounds.left);
    let top = i64::from(rect.top) - i64::from(bounds.top);
    Ok(PhysicalRect::new(
        i32::try_from(left).map_err(|_| GdiRendererError::InvalidGeometry)?,
        i32::try_from(top).map_err(|_| GdiRendererError::InvalidGeometry)?,
        rect.width,
        rect.height,
    ))
}

const fn corner_radius(layout: OverlayLayout) -> u32 {
    match layout.surface_shape {
        HitShape::RoundedRectangle {
            corner_radius_px, ..
        } => corner_radius_px,
    }
}

fn native_rect(rect: PhysicalRect) -> Result<RECT, GdiRendererError> {
    let right = rect
        .left
        .checked_add(i32::try_from(rect.width).map_err(|_| GdiRendererError::InvalidGeometry)?)
        .ok_or(GdiRendererError::InvalidGeometry)?;
    let bottom = rect
        .top
        .checked_add(i32::try_from(rect.height).map_err(|_| GdiRendererError::InvalidGeometry)?)
        .ok_or(GdiRendererError::InvalidGeometry)?;
    Ok(RECT {
        left: rect.left,
        top: rect.top,
        right,
        bottom,
    })
}

fn color_ref(color: Rgba8) -> u32 {
    let [red, green, blue] = color.rgb();
    u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16)
}

fn fill(dc: HDC, rect: PhysicalRect, color: Rgba8) -> Result<(), GdiRendererError> {
    let rect = native_rect(rect)?;
    // SAFETY: the brush is checked, used only for this call, and deleted afterward.
    let brush = unsafe { CreateSolidBrush(color_ref(color)) };
    if brush.is_null() {
        return Err(GdiRendererError::NativeResource);
    }
    // SAFETY: dc, rect, and brush are valid for this synchronous drawing call.
    unsafe {
        FillRect(dc, &rect, brush);
        DeleteObject(brush);
    }
    Ok(())
}

fn fill_round_rect(
    dc: HDC,
    rect: PhysicalRect,
    radius: u32,
    color: Rgba8,
) -> Result<(), GdiRendererError> {
    let rect = native_rect(rect)?;
    let brush = unsafe { CreateSolidBrush(color_ref(color)) };
    if brush.is_null() {
        return Err(GdiRendererError::NativeResource);
    }
    unsafe {
        let prior_pen = SelectObject(
            dc,
            windows_sys::Win32::Graphics::Gdi::GetStockObject(NULL_PEN),
        );
        let prior_brush = SelectObject(dc, brush);
        let ellipse = i32::try_from(radius.saturating_mul(2)).unwrap_or(i32::MAX);
        RoundRect(
            dc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            ellipse,
            ellipse,
        );
        SelectObject(dc, prior_brush);
        SelectObject(dc, prior_pen);
        DeleteObject(brush);
    }
    Ok(())
}

fn stretch_pixels(
    dc: HDC,
    destination: PhysicalRect,
    size: PhysicalSize,
    pixels: &[u32],
) -> Result<(), GdiRendererError> {
    let source_width = i32::try_from(size.width).map_err(|_| GdiRendererError::InvalidGeometry)?;
    let source_height =
        i32::try_from(size.height).map_err(|_| GdiRendererError::InvalidGeometry)?;
    let destination_width =
        i32::try_from(destination.width).map_err(|_| GdiRendererError::InvalidGeometry)?;
    let destination_height =
        i32::try_from(destination.height).map_err(|_| GdiRendererError::InvalidGeometry)?;
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>())
                .expect("BITMAPINFOHEADER size fits u32"),
            biWidth: source_width,
            biHeight: -source_height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..unsafe { std::mem::zeroed() }
        },
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: the top-down 32-bit DIB metadata exactly matches the immutable pixel slice.
    unsafe {
        StretchDIBits(
            dc,
            destination.left,
            destination.top,
            destination_width,
            destination_height,
            0,
            0,
            source_width,
            source_height,
            pixels.as_ptr().cast::<c_void>(),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
    Ok(())
}

fn draw_border(
    dc: HDC,
    rect: PhysicalRect,
    radius: u32,
    width: i32,
    color: Rgba8,
) -> Result<(), GdiRendererError> {
    let rect = native_rect(rect)?;
    // SAFETY: created pen is checked and all prior GDI objects are restored.
    let pen = unsafe { CreatePen(PS_SOLID, width, color_ref(color)) };
    if pen.is_null() {
        return Err(GdiRendererError::NativeResource);
    }
    unsafe {
        let prior_pen = SelectObject(dc, pen);
        let prior_brush = SelectObject(
            dc,
            windows_sys::Win32::Graphics::Gdi::GetStockObject(NULL_BRUSH),
        );
        let ellipse = i32::try_from(radius.saturating_mul(2)).unwrap_or(i32::MAX);
        RoundRect(
            dc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            ellipse,
            ellipse,
        );
        SelectObject(dc, prior_brush);
        SelectObject(dc, prior_pen);
        DeleteObject(pen);
    }
    Ok(())
}

fn draw_minimap_bezel(
    dc: HDC,
    rect: PhysicalRect,
    radius: u32,
    body_width: i32,
    inner_inset: i32,
    tick_length: i32,
) -> Result<(), GdiRendererError> {
    let body_width = body_width.max(4);
    let shadow_rect = inset_rect(rect, 1)?;
    draw_border(
        dc,
        shadow_rect,
        inset_radius(radius, 1),
        4,
        OVERLAY_VISUAL_V1.palette.minimap_bezel_shadow,
    )?;

    let body_center_inset = (body_width / 2).max(3);
    let body_rect = inset_rect(rect, body_center_inset)?;
    draw_border(
        dc,
        body_rect,
        inset_radius(radius, body_center_inset),
        body_width,
        OVERLAY_VISUAL_V1.palette.minimap_bezel_body,
    )?;

    let outer_inset = 2;
    let outer_rect = inset_rect(rect, outer_inset)?;
    draw_border(
        dc,
        outer_rect,
        inset_radius(radius, outer_inset),
        1,
        OVERLAY_VISUAL_V1.palette.minimap_bezel_highlight,
    )?;

    let inner_inset = inner_inset.max(body_width.saturating_add(3));
    let inner_shadow_inset = inner_inset.saturating_sub(1);
    let inner_shadow_rect = inset_rect(rect, inner_shadow_inset)?;
    draw_border(
        dc,
        inner_shadow_rect,
        inset_radius(radius, inner_shadow_inset),
        3,
        OVERLAY_VISUAL_V1.palette.minimap_bezel_outer,
    )?;

    let inner_rect = inset_rect(rect, inner_inset)?;
    draw_border(
        dc,
        inner_rect,
        inset_radius(radius, inner_inset),
        1,
        OVERLAY_VISUAL_V1.palette.minimap_bezel_inner,
    )?;

    draw_minimap_ticks(dc, rect, inner_inset.saturating_add(2), tick_length.max(4))
}

fn draw_expanded_frame(
    dc: HDC,
    rect: PhysicalRect,
    radius: u32,
    width: i32,
) -> Result<(), GdiRendererError> {
    let shadow_rect = inset_rect(rect, 1)?;
    draw_border(
        dc,
        shadow_rect,
        inset_radius(radius, 1),
        width.max(3),
        OVERLAY_VISUAL_V1.palette.minimap_bezel_shadow,
    )?;

    let body_rect = inset_rect(rect, 2)?;
    draw_border(
        dc,
        body_rect,
        inset_radius(radius, 2),
        width.max(3),
        OVERLAY_VISUAL_V1.palette.minimap_bezel_outer,
    )?;

    let inner_rect = inset_rect(rect, width.saturating_add(2))?;
    draw_border(
        dc,
        inner_rect,
        inset_radius(radius, width.saturating_add(2)),
        1,
        OVERLAY_VISUAL_V1.palette.neutral_border,
    )
}

fn inset_rect(rect: PhysicalRect, inset: i32) -> Result<PhysicalRect, GdiRendererError> {
    let inset = u32::try_from(inset).map_err(|_| GdiRendererError::InvalidGeometry)?;
    let doubled = inset
        .checked_mul(2)
        .ok_or(GdiRendererError::InvalidGeometry)?;
    let width = rect
        .width
        .checked_sub(doubled)
        .ok_or(GdiRendererError::InvalidGeometry)?;
    let height = rect
        .height
        .checked_sub(doubled)
        .ok_or(GdiRendererError::InvalidGeometry)?;
    if width == 0 || height == 0 {
        return Err(GdiRendererError::InvalidGeometry);
    }
    Ok(PhysicalRect::new(
        rect.left
            .checked_add(i32::try_from(inset).map_err(|_| GdiRendererError::InvalidGeometry)?)
            .ok_or(GdiRendererError::InvalidGeometry)?,
        rect.top
            .checked_add(i32::try_from(inset).map_err(|_| GdiRendererError::InvalidGeometry)?)
            .ok_or(GdiRendererError::InvalidGeometry)?,
        width,
        height,
    ))
}

fn inset_radius(radius: u32, inset: i32) -> u32 {
    radius.saturating_sub(u32::try_from(inset).unwrap_or(u32::MAX))
}

fn draw_minimap_ticks(
    dc: HDC,
    rect: PhysicalRect,
    inset: i32,
    length: i32,
) -> Result<(), GdiRendererError> {
    let width = i32::try_from(rect.width).map_err(|_| GdiRendererError::InvalidGeometry)?;
    let height = i32::try_from(rect.height).map_err(|_| GdiRendererError::InvalidGeometry)?;
    let center_x = rect.left.saturating_add(width / 2);
    let center_y = rect.top.saturating_add(height / 2);
    let radius = (width.min(height) / 2).saturating_sub(inset.max(0));
    if radius <= length {
        return Err(GdiRendererError::InvalidGeometry);
    }
    let muted = OVERLAY_VISUAL_V1.palette.minimap_tick_muted;

    const MINOR_TICKS: [(i32, i32); 8] = [
        (500, -866),
        (866, -500),
        (866, 500),
        (500, 866),
        (-500, 866),
        (-866, 500),
        (-866, -500),
        (-500, -866),
    ];
    for (x, y) in MINOR_TICKS {
        draw_radial_tick(
            dc,
            center_x,
            center_y,
            radius,
            x,
            y,
            (length / 2).max(3),
            1,
            muted,
        )?;
    }

    for (x, y) in [(1000, 0), (0, 1000), (-1000, 0)] {
        draw_radial_tick(
            dc,
            center_x,
            center_y,
            radius,
            x,
            y,
            length.saturating_sub(1),
            2,
            muted,
        )?;
    }
    draw_radial_tick(
        dc,
        center_x,
        center_y,
        radius,
        0,
        -1000,
        length.saturating_add(2),
        2,
        OVERLAY_VISUAL_V1.palette.cyan_live,
    )?;

    let north_y = center_y.saturating_sub(radius);
    draw_line(
        dc,
        center_x.saturating_sub(4),
        north_y.saturating_add(4),
        center_x,
        north_y,
        2,
        OVERLAY_VISUAL_V1.palette.cyan_live,
    )?;
    draw_line(
        dc,
        center_x,
        north_y,
        center_x.saturating_add(4),
        north_y.saturating_add(4),
        2,
        OVERLAY_VISUAL_V1.palette.cyan_live,
    )
}

#[allow(clippy::too_many_arguments)]
fn draw_radial_tick(
    dc: HDC,
    center_x: i32,
    center_y: i32,
    radius: i32,
    direction_x_milli: i32,
    direction_y_milli: i32,
    length: i32,
    width: i32,
    color: Rgba8,
) -> Result<(), GdiRendererError> {
    let inner_radius = radius.saturating_sub(length.max(1));
    let outer_x = center_x.saturating_add(direction_x_milli.saturating_mul(radius) / 1000);
    let outer_y = center_y.saturating_add(direction_y_milli.saturating_mul(radius) / 1000);
    let inner_x = center_x.saturating_add(direction_x_milli.saturating_mul(inner_radius) / 1000);
    let inner_y = center_y.saturating_add(direction_y_milli.saturating_mul(inner_radius) / 1000);
    draw_line(dc, inner_x, inner_y, outer_x, outer_y, width, color)
}

fn draw_divider(
    dc: HDC,
    rail: PhysicalRect,
    width: i32,
    color: Rgba8,
) -> Result<(), GdiRendererError> {
    draw_line(
        dc,
        rail.left,
        rail.top,
        rail.left,
        rail.top
            .saturating_add(i32::try_from(rail.height).unwrap_or(i32::MAX)),
        width,
        color,
    )
}

fn draw_cardinals(dc: HDC, map: PhysicalRect, inset: i32, color: Rgba8, north_color: Rgba8) {
    let width = i32::try_from(map.width).unwrap_or(i32::MAX);
    let height = i32::try_from(map.height).unwrap_or(i32::MAX);
    let center_x = map.left.saturating_add(width / 2);
    let center_y = map.top.saturating_add(height / 2);
    // SAFETY: the DC is valid and the static UTF-16 slices remain alive for each call.
    unsafe {
        SetBkMode(dc, TRANSPARENT as i32);
        draw_text_with_shadow(
            dc,
            center_x.saturating_sub(4),
            map.top.saturating_add(inset),
            CARDINAL_N,
            north_color,
        );
        draw_text_with_shadow(
            dc,
            map.left.saturating_add(width).saturating_sub(inset + 8),
            center_y.saturating_sub(7),
            CARDINAL_E,
            color,
        );
        draw_text_with_shadow(
            dc,
            center_x.saturating_sub(4),
            map.top.saturating_add(height).saturating_sub(inset + 14),
            CARDINAL_S,
            color,
        );
        draw_text_with_shadow(
            dc,
            map.left.saturating_add(inset),
            center_y.saturating_sub(7),
            CARDINAL_W,
            color,
        );
    }
}

unsafe fn draw_text_with_shadow(dc: HDC, x: i32, y: i32, value: &[u16], color: Rgba8) {
    unsafe {
        SetTextColor(dc, color_ref(Rgba8::new(0x04, 0x0a, 0x10, 0xff)));
        text(dc, x.saturating_add(1), y.saturating_add(1), value);
        SetTextColor(dc, color_ref(color));
        text(dc, x, y, value);
    }
}

unsafe fn text(dc: HDC, x: i32, y: i32, value: &[u16]) {
    unsafe {
        TextOutW(
            dc,
            x,
            y,
            value.as_ptr(),
            i32::try_from(value.len()).unwrap_or(i32::MAX),
        );
    }
}

fn draw_scale_bar(
    dc: HDC,
    map: PhysicalRect,
    width: i32,
    inset: i32,
    color: Rgba8,
) -> Result<(), GdiRendererError> {
    let map_height = i32::try_from(map.height).map_err(|_| GdiRendererError::InvalidGeometry)?;
    let y = map.top.saturating_add(map_height).saturating_sub(inset);
    let left = map.left.saturating_add(inset);
    let right = left.saturating_add(width);
    draw_line(dc, left, y, right, y, 1, color)?;
    draw_line(
        dc,
        left,
        y.saturating_sub(4),
        left,
        y.saturating_add(1),
        1,
        color,
    )?;
    draw_line(
        dc,
        right,
        y.saturating_sub(4),
        right,
        y.saturating_add(1),
        1,
        color,
    )
}

fn draw_status_dot(
    dc: HDC,
    map: PhysicalRect,
    diameter: i32,
    color: Rgba8,
    circular: bool,
) -> Result<(), GdiRendererError> {
    let map_width = i32::try_from(map.width).map_err(|_| GdiRendererError::InvalidGeometry)?;
    let map_height = i32::try_from(map.height).map_err(|_| GdiRendererError::InvalidGeometry)?;
    let (left, top) = if circular {
        let radius = map_width.min(map_height) / 2;
        let offset = radius
            .saturating_sub(diameter.saturating_add(10))
            .saturating_mul(707)
            / 1000;
        (
            map.left
                .saturating_add(map_width / 2)
                .saturating_add(offset)
                .saturating_sub(diameter / 2),
            map.top
                .saturating_add(map_height / 2)
                .saturating_sub(offset)
                .saturating_sub(diameter / 2),
        )
    } else {
        (
            map.left
                .saturating_add(map_width)
                .saturating_sub(diameter + 12),
            map.top.saturating_add(12),
        )
    };
    let halo = unsafe { CreateSolidBrush(color_ref(Rgba8::new(0x04, 0x0a, 0x10, 0xff))) };
    if halo.is_null() {
        return Err(GdiRendererError::NativeResource);
    }
    let brush = unsafe { CreateSolidBrush(color_ref(color)) };
    if brush.is_null() {
        unsafe { DeleteObject(halo) };
        return Err(GdiRendererError::NativeResource);
    }
    unsafe {
        let prior_brush = SelectObject(dc, halo);
        let prior_pen = SelectObject(
            dc,
            windows_sys::Win32::Graphics::Gdi::GetStockObject(NULL_PEN),
        );
        RoundRect(
            dc,
            left.saturating_sub(2),
            top.saturating_sub(2),
            left.saturating_add(diameter + 2),
            top.saturating_add(diameter + 2),
            diameter + 4,
            diameter + 4,
        );
        SelectObject(dc, brush);
        RoundRect(
            dc,
            left,
            top,
            left.saturating_add(diameter),
            top.saturating_add(diameter),
            diameter,
            diameter,
        );
        SelectObject(dc, prior_pen);
        SelectObject(dc, prior_brush);
        DeleteObject(halo);
        DeleteObject(brush);
    }
    Ok(())
}

fn draw_gate_badge(
    dc: HDC,
    map: PhysicalRect,
    inset: i32,
    color: Rgba8,
) -> Result<(), GdiRendererError> {
    let left = map.left.saturating_add(inset);
    let top = map.top.saturating_add(inset);
    let text_width = 142;
    draw_line(
        dc,
        left,
        top.saturating_add(17),
        left.saturating_add(text_width),
        top.saturating_add(17),
        1,
        color,
    )?;
    unsafe {
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color_ref(color));
        text(dc, left.saturating_add(3), top, GATE_BADGE);
    }
    Ok(())
}

fn draw_rail_glyphs(dc: HDC, plan: GdiPaintPlan) -> Result<(), GdiRendererError> {
    let surface = plan.layout.surface_shape.bounds();
    if let Some(screen_dock) = action_dock_rect(plan.layout) {
        let dock = PhysicalRect::new(
            screen_dock
                .left
                .checked_sub(surface.left)
                .ok_or(GdiRendererError::InvalidGeometry)?,
            screen_dock
                .top
                .checked_sub(surface.top)
                .ok_or(GdiRendererError::InvalidGeometry)?,
            screen_dock.width,
            screen_dock.height,
        );
        let radius = u32::try_from(scale_dip(8, estimated_dpi(plan.layout)))
            .map_err(|_| GdiRendererError::InvalidGeometry)?;
        fill_round_rect(dc, dock, radius, OVERLAY_VISUAL_V1.palette.neutral_backdrop)?;
        draw_border(
            dc,
            dock,
            radius,
            1,
            OVERLAY_VISUAL_V1.palette.neutral_border,
        )?;
    }
    for action in plan.chrome.rail_actions() {
        let center = plan
            .rail_glyph_center(*action)
            .ok_or(GdiRendererError::InvalidGeometry)?;
        let center_x = center.x.saturating_sub(surface.left);
        let center_y = center.y.saturating_sub(surface.top);
        let color = match plan
            .rail_glyph_tone(*action)
            .ok_or(GdiRendererError::InvalidGeometry)?
        {
            StatusTone::Neutral => OVERLAY_VISUAL_V1.palette.neutral_muted,
            StatusTone::Amber => OVERLAY_VISUAL_V1.palette.amber_warning,
            StatusTone::Cyan => return Err(GdiRendererError::InvalidGeometry),
        };
        draw_glyph(dc, center_x, center_y, *action, color)?;
    }
    Ok(())
}

fn draw_filter_strip(dc: HDC, plan: GdiPaintPlan) -> Result<(), GdiRendererError> {
    let surface = plan.layout.surface_shape.bounds();
    let radius = scale_dip(8, estimated_dpi(plan.layout));
    let panel = plan
        .filter_panel_rect()
        .ok_or(GdiRendererError::InvalidGeometry)?;
    let panel = PhysicalRect::new(
        panel
            .left
            .checked_sub(surface.left)
            .ok_or(GdiRendererError::InvalidGeometry)?,
        panel
            .top
            .checked_sub(surface.top)
            .ok_or(GdiRendererError::InvalidGeometry)?,
        panel.width,
        panel.height,
    );
    fill_round_rect(
        dc,
        panel,
        u32::try_from(radius).unwrap_or(u32::MAX),
        OVERLAY_VISUAL_V1.palette.neutral_backdrop,
    )?;
    draw_border(
        dc,
        panel,
        u32::try_from(radius).unwrap_or(u32::MAX),
        1,
        OVERLAY_VISUAL_V1.palette.neutral_border,
    )?;
    unsafe {
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color_ref(OVERLAY_VISUAL_V1.palette.neutral_text));
        text(
            dc,
            panel
                .left
                .saturating_add(scale_dip(12, estimated_dpi(plan.layout))),
            panel
                .top
                .saturating_add(scale_dip(16, estimated_dpi(plan.layout))),
            FILTER_PANEL_TITLE,
        );
    }
    for action in plan.chrome.filter_actions() {
        let screen_rect = plan
            .filter_button_rect(*action)
            .ok_or(GdiRendererError::InvalidGeometry)?;
        let rect = PhysicalRect::new(
            screen_rect
                .left
                .checked_sub(surface.left)
                .ok_or(GdiRendererError::InvalidGeometry)?,
            screen_rect
                .top
                .checked_sub(surface.top)
                .ok_or(GdiRendererError::InvalidGeometry)?,
            screen_rect.width,
            screen_rect.height,
        );
        let tone = plan
            .filter_button_tone(*action)
            .ok_or(GdiRendererError::InvalidGeometry)?;
        let selected = tone == StatusTone::Cyan;
        fill_round_rect(
            dc,
            rect,
            u32::try_from(radius).unwrap_or(u32::MAX),
            if selected {
                OVERLAY_VISUAL_V1.palette.neutral_rail
            } else {
                OVERLAY_VISUAL_V1.palette.neutral_backdrop
            },
        )?;
        draw_border(
            dc,
            rect,
            u32::try_from(radius).unwrap_or(u32::MAX),
            1,
            if selected {
                OVERLAY_VISUAL_V1.palette.cyan_live
            } else {
                OVERLAY_VISUAL_V1.palette.neutral_border
            },
        )?;
        let label = match action {
            FilterAction::FastTravel => FILTER_FAST_TRAVEL,
            FilterAction::Boss => FILTER_BOSS,
            FilterAction::Wanted => FILTER_WANTED,
            FilterAction::Dungeon => FILTER_DUNGEON,
            FilterAction::Tower => FILTER_TOWER,
            FilterAction::Egg => FILTER_EGG,
            FilterAction::Resources => FILTER_RESOURCES,
            FilterAction::Salvage => FILTER_SALVAGE,
        };
        let width = i32::try_from(rect.width).map_err(|_| GdiRendererError::InvalidGeometry)?;
        let height = i32::try_from(rect.height).map_err(|_| GdiRendererError::InvalidGeometry)?;
        let label_x = rect.left.saturating_add(width / 2).saturating_sub(12);
        let label_y = rect.top.saturating_add(height / 2).saturating_sub(8);
        unsafe {
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(
                dc,
                color_ref(if selected {
                    OVERLAY_VISUAL_V1.palette.neutral_text
                } else {
                    OVERLAY_VISUAL_V1.palette.neutral_muted
                }),
            );
            text(dc, label_x, label_y, label);
        }
    }
    Ok(())
}

fn draw_glyph(
    dc: HDC,
    x: i32,
    y: i32,
    action: RailAction,
    color: Rgba8,
) -> Result<(), GdiRendererError> {
    match action {
        RailAction::Collapse => {
            draw_line(dc, x - 5, y - 6, x + 2, y, 2, color)?;
            draw_line(dc, x + 2, y, x - 5, y + 6, 2, color)
        }
        RailAction::ZoomIn => {
            draw_line(dc, x - 6, y, x + 6, y, 2, color)?;
            draw_line(dc, x, y - 6, x, y + 6, 2, color)
        }
        RailAction::ZoomOut => draw_line(dc, x - 6, y, x + 6, y, 2, color),
        RailAction::ToggleRotation => {
            draw_line(dc, x - 6, y + 4, x - 6, y - 5, 2, color)?;
            draw_line(dc, x - 6, y - 5, x + 5, y - 5, 2, color)?;
            draw_line(dc, x + 5, y - 5, x + 2, y - 8, 2, color)
        }
        RailAction::Lock => {
            draw_line(dc, x - 6, y - 1, x + 6, y - 1, 2, color)?;
            draw_line(dc, x - 6, y - 1, x - 6, y + 7, 2, color)?;
            draw_line(dc, x + 6, y - 1, x + 6, y + 7, 2, color)?;
            draw_line(dc, x - 6, y + 7, x + 6, y + 7, 2, color)?;
            draw_line(dc, x - 4, y - 1, x - 4, y - 7, 2, color)?;
            draw_line(dc, x + 4, y - 1, x + 4, y - 7, 2, color)?;
            draw_line(dc, x - 4, y - 7, x + 4, y - 7, 2, color)
        }
    }
}

fn draw_line(
    dc: HDC,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    width: i32,
    color: Rgba8,
) -> Result<(), GdiRendererError> {
    let pen = unsafe { CreatePen(PS_SOLID, width, color_ref(color)) };
    if pen.is_null() {
        return Err(GdiRendererError::NativeResource);
    }
    unsafe {
        let prior = SelectObject(dc, pen);
        MoveToEx(dc, x1, y1, null_mut());
        LineTo(dc, x2, y2);
        SelectObject(dc, prior);
        DeleteObject(pen);
    }
    Ok(())
}

const fn tone_color(tone: StatusTone) -> Rgba8 {
    match tone {
        StatusTone::Neutral => OVERLAY_VISUAL_V1.palette.neutral_muted,
        StatusTone::Cyan => OVERLAY_VISUAL_V1.palette.cyan_live,
        StatusTone::Amber => OVERLAY_VISUAL_V1.palette.amber_warning,
    }
}
