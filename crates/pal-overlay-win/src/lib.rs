use pal_domain::{InputMode, OverlayAction};
use pal_render::{FilterAction, RailAction};
use std::ffi::c_void;
use std::ptr::NonNull;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, HRGN, PtInRegion};

pub mod actual_map_preview;
#[cfg(feature = "map-runtime")]
pub mod actual_map_runtime;
#[cfg(feature = "approved-map-pack-runtime")]
pub mod approved_map_surface;
#[cfg(any(
    feature = "development-live-agent",
    feature = "local-readonly-position-runtime"
))]
pub mod client_build_binding;
pub mod control_client;
pub mod control_intent;
pub mod gdi_correctness_renderer;
mod host_model;
#[cfg(feature = "development-live-agent")]
pub mod live_agent_runtime;
#[cfg(feature = "development-live-agent")]
mod live_source_config;
#[cfg(feature = "development-local-readonly-position")]
pub mod local_alignment_diagnostic;
#[cfg(feature = "development-local-alignment-diagnostic")]
pub mod local_alignment_runtime;
#[cfg(feature = "local-readonly-position-runtime")]
pub mod local_overlay_control;
#[cfg(all(windows, feature = "local-readonly-position-runtime"))]
pub mod local_position_runtime;
#[cfg(feature = "local-readonly-position-runtime")]
pub mod local_position_source;
#[cfg(all(windows, feature = "local-readonly-position-runtime"))]
pub mod local_position_win32;
mod mailbox;
mod placement;
pub mod renderer;
pub mod runtime_signals;
#[cfg(feature = "test-harness")]
pub mod synthetic_preview;
mod win32_host;

pub use control_intent::{ControlIntent, ControlIntentSink, control_intent_for_hotkey};
pub use host_model::{
    HostPlan, HostUpdate, OverlayHostController, OverlayLifecycle, OverlayShellBackend,
    SuspendReason,
};
#[cfg(feature = "development-live-agent")]
pub use live_source_config::{
    DEV_DIAGNOSTIC_COORDINATE_PROFILE_SHA256, DEV_DIAGNOSTIC_GAME_BUILD_ID,
    DEV_DIAGNOSTIC_MAP_BMP_SHA256, LiveSourceConfig, LiveSourceConfigError, LiveSourceMode,
    VerifiedMapBmp, load_live_source_config, parse_live_source_config,
};
pub use mailbox::LatestMailbox;
pub use placement::{
    DEFAULT_MINIMAP_SIZE_DIP, MAX_SUPPORTED_MINIMAP_SIZE_DIP, MIN_SUPPORTED_MINIMAP_SIZE_DIP,
    TopLeftLayoutError, TopLeftOverlayLayouts, top_left_overlay_layouts,
};
pub use win32_host::{
    ActualMapPaintPerformanceCounters, FullscreenMapNavigation, OverlayHostError,
    OverlayHostOperation, OverlayWindowHost, PumpOutcome,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalPoint {
    pub x: i32,
    pub y: i32,
}

impl PhysicalPoint {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalSize {
    pub width: u32,
    pub height: u32,
}

impl PhysicalSize {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

impl From<u32> for PhysicalSize {
    fn from(side: u32) -> Self {
        Self::new(side, side)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalRect {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

impl PhysicalRect {
    pub const fn new(left: i32, top: i32, width: u32, height: u32) -> Self {
        Self {
            left,
            top,
            width,
            height,
        }
    }

    fn contains(self, point: PhysicalPoint) -> bool {
        let x = i64::from(point.x);
        let y = i64::from(point.y);
        let left = i64::from(self.left);
        let top = i64::from(self.top);
        x >= left
            && x < left + i64::from(self.width)
            && y >= top
            && y < top + i64::from(self.height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitShape {
    RoundedRectangle {
        bounds: PhysicalRect,
        corner_radius_px: u32,
    },
}

impl HitShape {
    pub const fn bounds(self) -> PhysicalRect {
        match self {
            Self::RoundedRectangle { bounds, .. } => bounds,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RoundedRegionError {
    InvalidGeometry,
    NativeCreateFailed(u32),
}

impl RoundedRegionError {
    pub(crate) const fn code(self) -> u32 {
        match self {
            Self::InvalidGeometry => 0,
            Self::NativeCreateFailed(code) => code,
        }
    }
}

pub(crate) struct OwnedRoundedRegion {
    handle: NonNull<c_void>,
}

impl OwnedRoundedRegion {
    pub(crate) fn create(
        bounds: PhysicalRect,
        corner_radius_px: u32,
    ) -> Result<Self, RoundedRegionError> {
        let width = i32::try_from(bounds.width).map_err(|_| RoundedRegionError::InvalidGeometry)?;
        let height =
            i32::try_from(bounds.height).map_err(|_| RoundedRegionError::InvalidGeometry)?;
        let right = bounds
            .left
            .checked_add(width)
            .and_then(|value| value.checked_add(1))
            .ok_or(RoundedRegionError::InvalidGeometry)?;
        let bottom = bounds
            .top
            .checked_add(height)
            .and_then(|value| value.checked_add(1))
            .ok_or(RoundedRegionError::InvalidGeometry)?;
        let effective_corner_radius_px = corner_radius_px
            .min(bounds.width / 2)
            .min(bounds.height / 2);
        let ellipse = effective_corner_radius_px
            .checked_mul(2)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or(RoundedRegionError::InvalidGeometry)?;

        // CreateRoundRectRgn rasterizes its lower/right edge one pixel short of the documented
        // half-open rectangle. Extending only those native boundaries restores the intended final
        // in-client row and column. The caller still enforces the original half-open HWND bounds.
        // SAFETY: every coordinate and dimension was checked above.
        let handle =
            unsafe { CreateRoundRectRgn(bounds.left, bounds.top, right, bottom, ellipse, ellipse) };
        let handle = NonNull::new(handle).ok_or_else(|| {
            // SAFETY: GetLastError has no preconditions and is captured directly after failure.
            RoundedRegionError::NativeCreateFailed(unsafe { GetLastError() })
        })?;
        Ok(Self { handle })
    }

    pub(crate) fn contains(&self, point: PhysicalPoint) -> bool {
        // SAFETY: the region is owned by self and remains alive for this call.
        unsafe { PtInRegion(self.handle.as_ptr(), point.x, point.y) != 0 }
    }

    pub(crate) fn handle(&self) -> HRGN {
        self.handle.as_ptr()
    }
}

impl Drop for OwnedRoundedRegion {
    fn drop(&mut self) {
        // SAFETY: this object exclusively owns the region unless ownership is explicitly
        // transferred to SetWindowRgn by forgetting the wrapper after a successful call.
        unsafe {
            DeleteObject(self.handle.as_ptr());
        }
    }
}

struct SurfaceHitRegion {
    screen_bounds: PhysicalRect,
    local_region: Option<OwnedRoundedRegion>,
}

impl SurfaceHitRegion {
    fn new(shape: HitShape) -> Self {
        let HitShape::RoundedRectangle {
            bounds,
            corner_radius_px,
        } = shape;
        let local_bounds = PhysicalRect::new(0, 0, bounds.width, bounds.height);
        Self {
            screen_bounds: bounds,
            // Invalid or unrepresentable geometry fails closed instead of becoming a rectangular
            // input fallback.
            local_region: OwnedRoundedRegion::create(local_bounds, corner_radius_px).ok(),
        }
    }

    fn contains(&self, screen_point: PhysicalPoint) -> bool {
        if !self.screen_bounds.contains(screen_point) {
            return false;
        }
        let Some(region) = self.local_region.as_ref() else {
            return false;
        };
        let local_x = i64::from(screen_point.x).checked_sub(i64::from(self.screen_bounds.left));
        let local_y = i64::from(screen_point.y).checked_sub(i64::from(self.screen_bounds.top));
        let (Some(Ok(local_x)), Some(Ok(local_y))) =
            (local_x.map(i32::try_from), local_y.map(i32::try_from))
        else {
            return false;
        };
        region.contains(PhysicalPoint::new(local_x, local_y))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitTest {
    Transparent,
    Client,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeHandle {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutHitTarget {
    None,
    Map,
    Move,
    Resize(ResizeHandle),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionTarget {
    None,
    Map,
    FilterStrip,
    ControlRail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayLayout {
    pub host_rect: PhysicalRect,
    pub surface_shape: HitShape,
    pub map_viewport_rect: PhysicalRect,
    pub control_rail_rect: Option<PhysicalRect>,
    pub viewport_size: PhysicalSize,
}

impl OverlayLayout {
    pub const fn new(
        host_rect: PhysicalRect,
        surface_shape: HitShape,
        map_viewport_rect: PhysicalRect,
        control_rail_rect: Option<PhysicalRect>,
        viewport_size: PhysicalSize,
    ) -> Self {
        Self {
            host_rect,
            surface_shape,
            map_viewport_rect,
            control_rail_rect,
            viewport_size,
        }
    }
}

pub const PREVIEW_COLOR_KEY: u32 = 0x0000_0000;
pub const PREVIEW_MAP_FILL_COLOR: u32 = 0x0080_8080;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewPixelRole {
    ExteriorColorKey,
    OpaqueMapInterior,
}

impl PreviewPixelRole {
    pub const fn base_color_ref(self) -> u32 {
        match self {
            Self::ExteriorColorKey => PREVIEW_COLOR_KEY,
            Self::OpaqueMapInterior => PREVIEW_MAP_FILL_COLOR,
        }
    }
}

pub fn preview_pixel_role(layout: OverlayLayout, screen_point: PhysicalPoint) -> PreviewPixelRole {
    if SurfaceHitRegion::new(layout.surface_shape).contains(screen_point) {
        PreviewPixelRole::OpaqueMapInterior
    } else {
        PreviewPixelRole::ExteriorColorKey
    }
}

pub struct InputController {
    mode: InputMode,
    layout: OverlayLayout,
    surface_region: SurfaceHitRegion,
    effective_settings_version: u64,
}

impl InputController {
    pub fn new(mode: InputMode, layout: OverlayLayout) -> Self {
        Self {
            mode,
            layout,
            surface_region: SurfaceHitRegion::new(layout.surface_shape),
            effective_settings_version: 0,
        }
    }

    pub const fn mode(&self) -> InputMode {
        self.mode
    }

    pub const fn layout(&self) -> OverlayLayout {
        self.layout
    }

    pub const fn effective_settings_version(&self) -> u64 {
        self.effective_settings_version
    }

    pub fn set_effective_settings_version(&mut self, version: u64) {
        self.effective_settings_version = version;
    }

    pub fn wm_nchittest(&self, screen_point: PhysicalPoint) -> HitTest {
        if self.mode == InputMode::Locked || !self.surface_region.contains(screen_point) {
            HitTest::Transparent
        } else {
            HitTest::Client
        }
    }

    pub fn interaction_target(&self, screen_point: PhysicalPoint) -> InteractionTarget {
        if self.mode == InputMode::Locked || !self.surface_region.contains(screen_point) {
            return InteractionTarget::None;
        }
        if self
            .layout
            .control_rail_rect
            .is_some_and(|rail| rail.contains(screen_point))
            || self.rail_action_at(screen_point).is_some()
        {
            InteractionTarget::ControlRail
        } else if self.filter_action_at(screen_point).is_some() {
            InteractionTarget::FilterStrip
        } else if self.layout.map_viewport_rect.contains(screen_point) {
            InteractionTarget::Map
        } else {
            InteractionTarget::None
        }
    }

    pub fn layout_hit_target(&self, screen_point: PhysicalPoint) -> LayoutHitTarget {
        if self.mode == InputMode::Locked || !self.surface_region.contains(screen_point) {
            return LayoutHitTarget::None;
        }
        if self.mode != InputMode::LayoutEdit {
            return match self.interaction_target(screen_point) {
                InteractionTarget::Map
                | InteractionTarget::FilterStrip
                | InteractionTarget::ControlRail => LayoutHitTarget::Map,
                InteractionTarget::None => LayoutHitTarget::None,
            };
        }

        resize_handle(self.layout.surface_shape.bounds(), screen_point)
            .map(LayoutHitTarget::Resize)
            .unwrap_or(LayoutHitTarget::Move)
    }

    pub fn handle_left_click<S: ControlIntentSink>(
        &self,
        screen_point: PhysicalPoint,
        sink: &S,
    ) -> Result<bool, S::Error> {
        if self.mode == InputMode::Locked {
            return Ok(false);
        }
        let action = self
            .rail_action_at(screen_point)
            .map(rail_overlay_action)
            .or_else(|| {
                self.filter_action_at(screen_point)
                    .map(filter_overlay_action)
            });
        let Some(action) = action else {
            return Ok(false);
        };
        sink.submit(ControlIntent::Action {
            expected_settings_version: self.effective_settings_version,
            action,
        })?;
        Ok(true)
    }

    pub fn handle_wheel<S: ControlIntentSink>(
        &self,
        screen_point: PhysicalPoint,
        delta: i16,
        sink: &S,
    ) -> Result<bool, S::Error> {
        if self.mode == InputMode::Locked
            || delta == 0
            || self.interaction_target(screen_point) != InteractionTarget::Map
        {
            return Ok(false);
        }
        let action = if delta > 0 {
            OverlayAction::ZoomIn
        } else {
            OverlayAction::ZoomOut
        };
        sink.submit(ControlIntent::Action {
            expected_settings_version: self.effective_settings_version,
            action,
        })?;
        Ok(true)
    }

    fn rail_action_at(&self, screen_point: PhysicalPoint) -> Option<RailAction> {
        if self.mode == InputMode::Locked || self.layout.control_rail_rect.is_none() {
            return None;
        }
        [
            RailAction::Collapse,
            RailAction::ZoomIn,
            RailAction::ZoomOut,
            RailAction::ToggleRotation,
            RailAction::Lock,
        ]
        .into_iter()
        .find(|action| {
            crate::gdi_correctness_renderer::rail_action_rect(self.layout, *action)
                .is_some_and(|bounds| bounds.contains(screen_point))
        })
    }

    fn filter_action_at(&self, screen_point: PhysicalPoint) -> Option<FilterAction> {
        if self.mode == InputMode::Locked || self.layout.control_rail_rect.is_none() {
            return None;
        }
        [
            FilterAction::FastTravel,
            FilterAction::Boss,
            FilterAction::Wanted,
            FilterAction::Dungeon,
            FilterAction::Tower,
            FilterAction::Egg,
            FilterAction::Resources,
            FilterAction::Salvage,
        ]
        .into_iter()
        .find(|action| {
            crate::gdi_correctness_renderer::filter_button_rect(self.layout, *action)
                .is_some_and(|bounds| bounds.contains(screen_point))
        })
    }
}

const fn rail_overlay_action(action: RailAction) -> OverlayAction {
    match action {
        RailAction::Collapse => OverlayAction::CloseExpanded,
        RailAction::ZoomIn => OverlayAction::ZoomIn,
        RailAction::ZoomOut => OverlayAction::ZoomOut,
        RailAction::ToggleRotation => OverlayAction::ToggleRotation,
        RailAction::Lock => OverlayAction::Lock,
    }
}

const fn filter_overlay_action(action: FilterAction) -> OverlayAction {
    match action {
        FilterAction::FastTravel => OverlayAction::ToggleFastTravel,
        FilterAction::Boss => OverlayAction::ToggleBoss,
        FilterAction::Wanted => OverlayAction::ToggleWanted,
        FilterAction::Dungeon => OverlayAction::ToggleDungeon,
        FilterAction::Tower => OverlayAction::ToggleTower,
        FilterAction::Egg => OverlayAction::ToggleEgg,
        FilterAction::Resources => OverlayAction::ToggleResources,
        FilterAction::Salvage => OverlayAction::ToggleSalvage,
    }
}

fn resize_handle(bounds: PhysicalRect, point: PhysicalPoint) -> Option<ResizeHandle> {
    const GRIP: i64 = 12;
    let x = i64::from(point.x);
    let y = i64::from(point.y);
    let left = i64::from(bounds.left);
    let top = i64::from(bounds.top);
    let right = left + i64::from(bounds.width);
    let bottom = top + i64::from(bounds.height);
    let west = x - left < GRIP;
    let east = right - x <= GRIP;
    let north = y - top < GRIP;
    let south = bottom - y <= GRIP;

    match (north, east, south, west) {
        (true, true, _, _) => Some(ResizeHandle::NorthEast),
        (true, _, _, true) => Some(ResizeHandle::NorthWest),
        (_, true, true, _) => Some(ResizeHandle::SouthEast),
        (_, _, true, true) => Some(ResizeHandle::SouthWest),
        (true, _, _, _) => Some(ResizeHandle::North),
        (_, true, _, _) => Some(ResizeHandle::East),
        (_, _, true, _) => Some(ResizeHandle::South),
        (_, _, _, true) => Some(ResizeHandle::West),
        _ => None,
    }
}
