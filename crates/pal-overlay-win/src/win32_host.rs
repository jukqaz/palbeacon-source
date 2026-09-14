use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::fmt;
use std::marker::PhantomData;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::Arc;
#[cfg(any(
    test,
    feature = "approved-map-pack-runtime",
    feature = "test-harness",
    feature = "development-live-agent",
    feature = "development-local-readonly-position"
))]
use std::time::{Duration, Instant};

use pal_domain::{DisplayMode, Freshness, HotkeyAction, InputMode, PoiFilters};
#[cfg(any(
    feature = "approved-map-pack-runtime",
    feature = "test-harness",
    feature = "development-live-agent",
    feature = "development-local-readonly-position"
))]
use pal_domain::{
    EGG_LAYER_IDS, RESOURCE_LAYER_IDS, SALVAGE_LAYER_IDS, TOWER_LAYER_IDS, layer_group_selected,
};
use pal_fullscreen_bridge::{ConsumerAction, FrameSearchEntry, FrameWriter};
#[cfg(any(
    feature = "approved-map-pack-runtime",
    feature = "test-harness",
    feature = "development-live-agent",
    feature = "development-local-readonly-position"
))]
use pal_fullscreen_bridge::{
    FrameControls, FrameDisplayMode, FrameInputMode, FrameMetadata, FramePoiFilters,
};
use pal_render::{GateBadge, RenderSnapshot, build_chrome_plan};
use pal_windows::{TrackedWindow, action_from_registration_id, enable_per_monitor_v2};
use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, SetLastError, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    BLACK_BRUSH, BeginPaint, EndPaint, GetStockObject, InvalidateRect, PAINTSTRUCT, SetWindowRgn,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWL_EXSTYLE,
    GWLP_USERDATA, GetWindowLongPtrW, HTCLIENT, HTTRANSPARENT, HWND_TOPMOST, IsWindow, LWA_ALPHA,
    LWA_COLORKEY, MA_NOACTIVATE, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
    WM_CLOSE, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND, WM_HOTKEY, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST,
    WM_PAINT, WM_QUIT, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

#[cfg(any(
    feature = "approved-map-pack-runtime",
    feature = "development-local-readonly-position"
))]
use crate::actual_map_preview::MapPoi;
#[cfg(any(
    feature = "approved-map-pack-runtime",
    feature = "test-harness",
    feature = "development-live-agent",
    feature = "development-local-readonly-position"
))]
use crate::actual_map_preview::{
    ActualMapSurface, ActualMapSurfacePerformanceCounters, MapRaster, MapView,
};
use crate::gdi_correctness_renderer::{GdiCorrectnessRenderer, estimated_dpi};
#[cfg(any(
    feature = "approved-map-pack-runtime",
    feature = "test-harness",
    feature = "development-live-agent",
    feature = "development-local-readonly-position"
))]
use crate::renderer::OverlayRenderer;
use crate::renderer::RendererKind;
#[cfg(feature = "test-harness")]
use crate::synthetic_preview::SyntheticSurface;
use crate::{
    ControlIntent, ControlIntentSink, HitShape, HitTest, HostPlan, HostUpdate, InputController,
    LatestMailbox, OverlayHostController, OverlayLayout, OverlayShellBackend, OwnedRoundedRegion,
    PREVIEW_COLOR_KEY, PhysicalPoint, PhysicalRect, PhysicalSize, SuspendReason,
    TopLeftOverlayLayouts,
};

const CLASS_NAME: &[u16] = &[
    b'P' as u16,
    b'a' as u16,
    b'l' as u16,
    b'C' as u16,
    b'o' as u16,
    b'm' as u16,
    b'p' as u16,
    b'a' as u16,
    b'n' as u16,
    b'i' as u16,
    b'o' as u16,
    b'n' as u16,
    b'O' as u16,
    b'v' as u16,
    b'e' as u16,
    b'r' as u16,
    b'l' as u16,
    b'a' as u16,
    b'y' as u16,
    0,
];
const WINDOW_TITLE: &[u16] = &[
    b'P' as u16,
    b'a' as u16,
    b'l' as u16,
    b' ' as u16,
    b'C' as u16,
    b'o' as u16,
    b'm' as u16,
    b'p' as u16,
    b'a' as u16,
    b'n' as u16,
    b'i' as u16,
    b'o' as u16,
    b'n' as u16,
    b' ' as u16,
    b'P' as u16,
    b'r' as u16,
    b'e' as u16,
    b'v' as u16,
    b'i' as u16,
    b'e' as u16,
    b'w' as u16,
    0,
];
const BASE_EX_STYLE: u32 = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED;

#[cfg(any(
    feature = "approved-map-pack-runtime",
    feature = "test-harness",
    feature = "development-live-agent",
    feature = "development-local-readonly-position"
))]
#[derive(Clone, Copy)]
enum ActualMapSurfaceMode {
    #[cfg(feature = "approved-map-pack-runtime")]
    Approved,
    #[cfg(feature = "test-harness")]
    StaticTest,
    #[cfg(feature = "test-harness")]
    DevelopmentReplay,
    #[cfg(feature = "development-live-agent")]
    DevelopmentLive,
    #[cfg(feature = "development-local-readonly-position")]
    DevelopmentLocalRead,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActualMapPaintPerformanceCounters {
    pub present_requests: u64,
    pub paint_calls: u64,
    pub upload_calls: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayHostOperation {
    DpiAwareness,
    ModuleHandle,
    RegisterClass,
    CreateWindow,
    ConfigureLayered,
    SetStyle,
    Position,
    Region,
    Invalidate,
    Renderer,
    MessagePump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayHostError {
    operation: OverlayHostOperation,
    code: u32,
}

impl OverlayHostError {
    const fn new(operation: OverlayHostOperation, code: u32) -> Self {
        Self { operation, code }
    }

    pub const fn operation(self) -> OverlayHostOperation {
        self.operation
    }

    pub const fn code(self) -> u32 {
        self.code
    }
}

impl fmt::Display for OverlayHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "overlay host {:?} failed with code {}",
            self.operation, self.code
        )
    }
}

impl std::error::Error for OverlayHostError {}

struct WindowContext {
    input: InputController,
    display_mode: DisplayMode,
    window_rect: PhysicalRect,
    reattach_requested: Cell<bool>,
    shutdown_requested: Cell<bool>,
    received_clicks: Cell<u64>,
    effective_settings_version: Cell<u64>,
    poi_filters: RefCell<PoiFilters>,
    control_intents: RefCell<VecDeque<ControlIntent>>,
    hotkey_actions: RefCell<VecDeque<HotkeyAction>>,
    actual_map_paint_performance_counters: Cell<ActualMapPaintPerformanceCounters>,
    #[cfg(any(
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    actual_map_surface: Option<Box<ActualMapSurface>>,
    #[cfg(any(
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    actual_map_surface_mode: Option<ActualMapSurfaceMode>,
    #[cfg(feature = "test-harness")]
    synthetic_surface: Option<Arc<SyntheticSurface>>,
}

impl WindowContext {
    fn new(layout: OverlayLayout) -> Self {
        Self {
            input: InputController::new(InputMode::Locked, layout),
            display_mode: DisplayMode::MiniMap,
            window_rect: layout.surface_shape.bounds(),
            reattach_requested: Cell::new(false),
            shutdown_requested: Cell::new(false),
            received_clicks: Cell::new(0),
            effective_settings_version: Cell::new(0),
            poi_filters: RefCell::new(PoiFilters::default()),
            control_intents: RefCell::new(VecDeque::new()),
            hotkey_actions: RefCell::new(VecDeque::new()),
            actual_map_paint_performance_counters: Cell::new(
                ActualMapPaintPerformanceCounters::default(),
            ),
            #[cfg(any(
                feature = "approved-map-pack-runtime",
                feature = "test-harness",
                feature = "development-live-agent",
                feature = "development-local-readonly-position"
            ))]
            actual_map_surface: None,
            #[cfg(any(
                feature = "approved-map-pack-runtime",
                feature = "test-harness",
                feature = "development-live-agent",
                feature = "development-local-readonly-position"
            ))]
            actual_map_surface_mode: None,
            #[cfg(feature = "test-harness")]
            synthetic_surface: None,
        }
    }

    #[cfg(any(
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    fn record_actual_map_present_request(&self) {
        let mut counters = self.actual_map_paint_performance_counters.get();
        counters.present_requests = counters.present_requests.saturating_add(1);
        self.actual_map_paint_performance_counters.set(counters);
    }

    #[cfg(any(
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    fn record_actual_map_paint(&self) {
        let mut counters = self.actual_map_paint_performance_counters.get();
        counters.paint_calls = counters.paint_calls.saturating_add(1);
        self.actual_map_paint_performance_counters.set(counters);
    }

    #[cfg(any(
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    fn record_actual_map_upload(&self) {
        let mut counters = self.actual_map_paint_performance_counters.get();
        counters.upload_calls = counters.upload_calls.saturating_add(1);
        self.actual_map_paint_performance_counters.set(counters);
    }

    #[cfg(any(
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    fn resize_and_rasterize_actual_map(
        &mut self,
        map: &MapRaster,
        view: MapView,
    ) -> Result<bool, OverlayHostError> {
        let target_size = self.input.layout().viewport_size;
        let Some(surface) = self.actual_map_surface.as_mut() else {
            return Ok(false);
        };
        surface
            .resize(target_size)
            .map_err(|_| OverlayHostError::new(OverlayHostOperation::Renderer, 0))?;
        surface.rasterize(map, view);
        Ok(true)
    }

    #[cfg(any(
        feature = "approved-map-pack-runtime",
        feature = "development-local-readonly-position"
    ))]
    fn resize_and_rasterize_actual_map_with_pois(
        &mut self,
        map: &MapRaster,
        view: MapView,
        pois: &[MapPoi],
        filters: &pal_domain::PoiFilters,
    ) -> Result<bool, OverlayHostError> {
        let target_size = self.input.layout().viewport_size;
        let Some(surface) = self.actual_map_surface.as_mut() else {
            return Ok(false);
        };
        surface
            .resize(target_size)
            .map_err(|_| OverlayHostError::new(OverlayHostOperation::Renderer, 0))?;
        surface.rasterize_with_pois(map, view, pois, filters);
        Ok(true)
    }
}

struct WindowIntentSink<'a>(&'a WindowContext);

impl ControlIntentSink for WindowIntentSink<'_> {
    type Error = std::convert::Infallible;

    fn submit(&self, intent: ControlIntent) -> Result<(), Self::Error> {
        self.0.control_intents.borrow_mut().push_back(intent);
        Ok(())
    }
}

struct Win32ShellBackend {
    hwnd: HWND,
    owner_thread_id: u32,
    context: Box<WindowContext>,
    external_renderer_suppressed: bool,
    _thread_bound: PhantomData<Rc<()>>,
}

impl Win32ShellBackend {
    fn create(layout: OverlayLayout) -> Result<Self, OverlayHostError> {
        enable_per_monitor_v2().map_err(|error| {
            OverlayHostError::new(OverlayHostOperation::DpiAwareness, error.code())
        })?;

        // SAFETY: a null module name asks for the current executable module.
        let module = unsafe { GetModuleHandleW(null()) };
        if module.is_null() {
            return Err(last_error(OverlayHostOperation::ModuleHandle));
        }

        let mut context = Box::new(WindowContext::new(layout));
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module,
            hbrBackground: unsafe { GetStockObject(BLACK_BRUSH) },
            lpszClassName: CLASS_NAME.as_ptr(),
            ..unsafe { std::mem::zeroed() }
        };
        // SAFETY: all pointers in the class descriptor remain valid for process lifetime.
        if unsafe { RegisterClassW(&class) } == 0 {
            // GetLastError must be captured before another Win32 call.
            let code = unsafe { GetLastError() };
            if code != ERROR_CLASS_ALREADY_EXISTS {
                return Err(OverlayHostError::new(
                    OverlayHostOperation::RegisterClass,
                    code,
                ));
            }
        }

        // SAFETY: the registered class and context pointer are valid. WM_NCCREATE stores but does
        // not take ownership of the context.
        let hwnd = unsafe {
            CreateWindowExW(
                BASE_EX_STYLE | WS_EX_TRANSPARENT,
                CLASS_NAME.as_ptr(),
                WINDOW_TITLE.as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                module,
                (&mut *context as *mut WindowContext).cast::<c_void>(),
            )
        };
        if hwnd.is_null() {
            return Err(last_error(OverlayHostOperation::CreateWindow));
        }

        // SAFETY: hwnd is a newly-created layered window. Black is the preview transparency key.
        if unsafe { SetLayeredWindowAttributes(hwnd, PREVIEW_COLOR_KEY, 255, LWA_COLORKEY) } == 0 {
            let error = last_error(OverlayHostOperation::ConfigureLayered);
            // SAFETY: hwnd is owned by this function on the current thread.
            unsafe { DestroyWindow(hwnd) };
            return Err(error);
        }

        Ok(Self {
            hwnd,
            // SAFETY: this has no preconditions.
            owner_thread_id: unsafe { GetCurrentThreadId() },
            context,
            external_renderer_suppressed: false,
            _thread_bound: PhantomData,
        })
    }

    fn ensure_owner_thread(&self) -> Result<(), OverlayHostError> {
        // SAFETY: this has no preconditions.
        if unsafe { GetCurrentThreadId() } == self.owner_thread_id {
            Ok(())
        } else {
            Err(OverlayHostError::new(OverlayHostOperation::MessagePump, 0))
        }
    }

    fn set_extended_style(&self, transparent: bool) -> Result<(), OverlayHostError> {
        let style = BASE_EX_STYLE | if transparent { WS_EX_TRANSPARENT } else { 0 };
        // SetWindowLongPtrW can validly return zero, so last error must be cleared first.
        unsafe { SetLastError(0) };
        // SAFETY: hwnd belongs to this backend and GWL_EXSTYLE accepts the style bit mask.
        let previous = unsafe { SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, style as isize) };
        if previous == 0 {
            // Capture immediately after the ambiguous return.
            let code = unsafe { GetLastError() };
            if code != 0 {
                return Err(OverlayHostError::new(OverlayHostOperation::SetStyle, code));
            }
        }
        // SAFETY: this only refreshes the frame/style and explicitly forbids activation.
        if unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            )
        } == 0
        {
            return Err(last_error(OverlayHostOperation::SetStyle));
        }
        Ok(())
    }

    fn set_opacity(&self, opacity: f32) -> Result<(), OverlayHostError> {
        let alpha = (opacity.clamp(0.20, 1.0) * 255.0).round() as u8;
        // SAFETY: hwnd is a live layered window and both color-key and constant alpha are valid.
        if unsafe {
            SetLayeredWindowAttributes(
                self.hwnd,
                PREVIEW_COLOR_KEY,
                alpha,
                LWA_COLORKEY | LWA_ALPHA,
            )
        } == 0
        {
            return Err(last_error(OverlayHostOperation::ConfigureLayered));
        }
        Ok(())
    }

    fn position(&self, plan: HostPlan, visible: bool) -> Result<(), OverlayHostError> {
        let rect = plan.window_rect();
        let width = i32::try_from(rect.width)
            .map_err(|_| OverlayHostError::new(OverlayHostOperation::Position, 0))?;
        let height = i32::try_from(rect.height)
            .map_err(|_| OverlayHostError::new(OverlayHostOperation::Position, 0))?;
        let mut flags = SWP_NOACTIVATE;
        if visible {
            flags |= SWP_SHOWWINDOW;
        }
        // SAFETY: hwnd is valid, dimensions were range-checked, and activation is forbidden.
        if unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                rect.left,
                rect.top,
                width,
                height,
                flags,
            )
        } == 0
        {
            return Err(last_error(OverlayHostOperation::Position));
        }
        Ok(())
    }

    fn set_external_renderer_suppressed(
        &mut self,
        suppressed: bool,
        logical_visible: bool,
    ) -> Result<(), OverlayHostError> {
        self.ensure_owner_thread()?;
        if self.external_renderer_suppressed == suppressed {
            return Ok(());
        }
        self.external_renderer_suppressed = suppressed;
        if logical_visible && !suppressed {
            unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
            self.refresh_topmost()?;
        } else {
            unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
        Ok(())
    }

    fn refresh_topmost(&self) -> Result<(), OverlayHostError> {
        self.ensure_owner_thread()?;
        // A borderless/fullscreen swap-chain transition can rebuild the game's z-order without
        // changing its client rectangle. Reassert the overlay's topmost band without showing,
        // activating, moving, or resizing it.
        if unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        } == 0
        {
            Err(last_error(OverlayHostOperation::Position))
        } else {
            Ok(())
        }
    }

    fn set_region(&self, plan: HostPlan) -> Result<(), OverlayHostError> {
        let local = local_rect(plan.layout().surface_shape, plan.window_rect())?;
        let HitShape::RoundedRectangle {
            corner_radius_px, ..
        } = plan.layout().surface_shape;
        let region = OwnedRoundedRegion::create(local, corner_radius_px)
            .map_err(|error| OverlayHostError::new(OverlayHostOperation::Region, error.code()))?;
        // SAFETY: hwnd and region are valid. On success Windows takes ownership of the handle.
        if unsafe { SetWindowRgn(self.hwnd, region.handle(), 1) } == 0 {
            return Err(last_error(OverlayHostOperation::Region));
        }
        std::mem::forget(region);
        Ok(())
    }

    fn invalidate(&self) -> Result<(), OverlayHostError> {
        // SAFETY: hwnd is valid; null invalidates the entire client area without erasing.
        if unsafe { InvalidateRect(self.hwnd, null(), 0) } == 0 {
            Err(last_error(OverlayHostOperation::Invalidate))
        } else {
            Ok(())
        }
    }
}

impl OverlayShellBackend for Win32ShellBackend {
    type Error = OverlayHostError;

    fn apply_plan(&mut self, plan: HostPlan) -> Result<(), Self::Error> {
        self.ensure_owner_thread()?;
        let visible = plan.visible() && !self.external_renderer_suppressed;
        self.set_extended_style(plan.transparent())?;
        self.position(plan, visible)?;
        self.set_region(plan)?;
        self.context.input = InputController::new(plan.input_mode(), plan.layout());
        self.context
            .input
            .set_effective_settings_version(self.context.effective_settings_version.get());
        self.context.display_mode = plan.display_mode();
        self.context.window_rect = plan.window_rect();
        if visible {
            // SAFETY: ShowWindow with SW_SHOWNOACTIVATE explicitly avoids activation.
            unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
        } else {
            // SAFETY: hiding an owned HWND has no activation side effect.
            unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
        self.invalidate()?;
        Ok(())
    }

    fn hide_fail_closed(&mut self) -> Result<(), Self::Error> {
        self.ensure_owner_thread()?;
        let style_result = self.set_extended_style(true);
        self.context.input = InputController::new(InputMode::Locked, self.context.input.layout());
        // SAFETY: hiding an owned HWND is always the fail-closed final action.
        unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        style_result
    }
}

impl Drop for Win32ShellBackend {
    fn drop(&mut self) {
        // The !Send/!Sync marker ensures normal Rust code drops on the owner thread.
        // SAFETY: hwnd is owned by this backend. WM_NCDESTROY clears GWLP_USERDATA.
        if unsafe { IsWindow(self.hwnd) } != 0 {
            unsafe { DestroyWindow(self.hwnd) };
        }
    }
}

pub struct OverlayWindowHost {
    controller: OverlayHostController<Win32ShellBackend>,
    mailbox: LatestMailbox<RenderSnapshot>,
    fullscreen_writer: Option<FrameWriter>,
    fullscreen_action_sequence: u32,
    fullscreen_search_sequence: u32,
    fullscreen_recenter_sequence: u32,
    fullscreen_publish_diagnostics: FullscreenPublishDiagnostics,
    opacity: f32,
    _thread_bound: PhantomData<Rc<()>>,
}

#[cfg(any(
    test,
    feature = "approved-map-pack-runtime",
    feature = "test-harness",
    feature = "development-live-agent",
    feature = "development-local-readonly-position"
))]
const FULLSCREEN_PUBLISH_REPORT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
struct FullscreenPublishDiagnostics {
    consecutive_failures: u64,
    last_error: Option<String>,
    #[cfg(any(
        test,
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    last_reported_at: Option<Instant>,
}

impl FullscreenPublishDiagnostics {
    #[cfg(any(
        test,
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.last_error = None;
        self.last_reported_at = None;
    }

    #[cfg(any(
        test,
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    fn record_failure(&mut self, error: &pal_fullscreen_bridge::BridgeError, now: Instant) -> bool {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.last_error = Some(error.to_string());
        let should_report = self.last_reported_at.is_none_or(|last_reported_at| {
            now.saturating_duration_since(last_reported_at) >= FULLSCREEN_PUBLISH_REPORT_INTERVAL
        });
        if should_report {
            self.last_reported_at = Some(now);
        }
        should_report
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum PumpOutcome {
    Continue,
    ShutdownRequested,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FullscreenMapNavigation {
    Pan {
        raster_delta_x: f32,
        raster_delta_y: f32,
    },
    Recenter,
}

impl OverlayWindowHost {
    pub fn create() -> Result<Self, OverlayHostError> {
        let bounds = PhysicalRect::new(0, 0, 1, 1);
        let layout = OverlayLayout::new(
            PhysicalRect::new(0, 0, 1, 1),
            HitShape::RoundedRectangle {
                bounds,
                corner_radius_px: 0,
            },
            bounds,
            None,
            PhysicalSize::new(1, 1),
        );
        let backend = Win32ShellBackend::create(layout)?;
        Ok(Self {
            controller: OverlayHostController::new(
                backend,
                TopLeftOverlayLayouts::new(layout, layout),
            ),
            mailbox: LatestMailbox::new(),
            fullscreen_writer: FrameWriter::create_or_open_default().ok(),
            fullscreen_action_sequence: 0,
            fullscreen_search_sequence: 0,
            fullscreen_recenter_sequence: 0,
            fullscreen_publish_diagnostics: FullscreenPublishDiagnostics::default(),
            opacity: 1.0,
            _thread_bound: PhantomData,
        })
    }

    pub fn attach(&mut self, game: &TrackedWindow) -> Result<HostUpdate, OverlayHostError> {
        let update = self.controller.attach(game)?;
        self.publish_fullscreen_visibility();
        Ok(update)
    }

    pub fn apply_layouts(
        &mut self,
        layouts: TopLeftOverlayLayouts,
    ) -> Result<HostUpdate, OverlayHostError> {
        self.controller.apply_layouts(layouts)
    }

    pub fn apply_snapshot(
        &mut self,
        snapshot: Arc<RenderSnapshot>,
    ) -> Result<u64, OverlayHostError> {
        let generation = self.mailbox.publish(snapshot);
        self.controller.backend().invalidate()?;
        Ok(generation)
    }

    #[cfg(feature = "test-harness")]
    pub fn apply_synthetic_surface(
        &mut self,
        surface: Arc<SyntheticSurface>,
    ) -> Result<(), OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        self.controller.backend_mut().context.actual_map_surface = None;
        self.controller
            .backend_mut()
            .context
            .actual_map_surface_mode = None;
        self.controller.backend_mut().context.synthetic_surface = Some(surface);
        self.controller.backend().invalidate()
    }

    #[cfg(feature = "test-harness")]
    pub fn apply_actual_map_surface(
        &mut self,
        surface: ActualMapSurface,
    ) -> Result<(), OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        if surface.size()
            != self
                .controller
                .backend()
                .context
                .input
                .layout()
                .viewport_size
        {
            return Err(OverlayHostError::new(OverlayHostOperation::Renderer, 0));
        }
        self.controller.backend_mut().context.synthetic_surface = None;
        self.controller.backend_mut().context.actual_map_surface = Some(Box::new(surface));
        self.controller
            .backend_mut()
            .context
            .actual_map_surface_mode = Some(ActualMapSurfaceMode::StaticTest);
        self.controller.backend().invalidate()?;
        self.publish_fullscreen_frame();
        Ok(())
    }

    #[cfg(feature = "test-harness")]
    pub fn update_actual_map_surface(
        &mut self,
        map: &MapRaster,
        view: MapView,
    ) -> Result<bool, OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        let updated = self
            .controller
            .backend_mut()
            .context
            .resize_and_rasterize_actual_map(map, view)?;
        if updated {
            self.controller
                .backend_mut()
                .context
                .actual_map_surface_mode = Some(ActualMapSurfaceMode::DevelopmentReplay);
            self.controller.backend().invalidate()?;
            self.controller
                .backend()
                .context
                .record_actual_map_present_request();
            self.publish_fullscreen_frame();
        }
        Ok(updated)
    }

    #[cfg(feature = "development-live-agent")]
    pub fn apply_live_actual_map_surface(
        &mut self,
        surface: ActualMapSurface,
    ) -> Result<(), OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        self.controller.backend_mut().context.actual_map_surface = Some(Box::new(surface));
        self.controller
            .backend_mut()
            .context
            .actual_map_surface_mode = Some(ActualMapSurfaceMode::DevelopmentLive);
        self.controller.backend().invalidate()?;
        self.publish_fullscreen_frame();
        Ok(())
    }

    #[cfg(feature = "development-live-agent")]
    pub fn update_live_actual_map_surface(
        &mut self,
        map: &MapRaster,
        view: MapView,
    ) -> Result<bool, OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        let updated = self
            .controller
            .backend_mut()
            .context
            .resize_and_rasterize_actual_map(map, view)?;
        if updated {
            self.controller.backend().invalidate()?;
            self.controller
                .backend()
                .context
                .record_actual_map_present_request();
            self.publish_fullscreen_frame();
        }
        Ok(updated)
    }

    #[cfg(feature = "approved-map-pack-runtime")]
    pub fn apply_approved_map_surface(
        &mut self,
        mut surface: ActualMapSurface,
    ) -> Result<(), OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        // The persisted display mode can already be ExpandedMap when the approved runtime starts.
        // Startup used to construct a square surface from `diameter_px`, then reject it here before
        // the normal update path could resize it to the rectangular expanded viewport. Normalize
        // the surface at the ownership boundary so both startup modes follow the same invariant.
        let target_size = self
            .controller
            .backend()
            .context
            .input
            .layout()
            .viewport_size;
        surface
            .resize(target_size)
            .map_err(|_| OverlayHostError::new(OverlayHostOperation::Renderer, 0))?;
        self.controller.backend_mut().context.actual_map_surface = Some(Box::new(surface));
        self.controller
            .backend_mut()
            .context
            .actual_map_surface_mode = Some(ActualMapSurfaceMode::Approved);
        self.controller.backend().invalidate()?;
        self.publish_fullscreen_frame();
        Ok(())
    }

    #[cfg(feature = "approved-map-pack-runtime")]
    pub fn update_approved_map_surface_with_pois(
        &mut self,
        map: &MapRaster,
        view: MapView,
        pois: &[MapPoi],
        filters: &pal_domain::PoiFilters,
    ) -> Result<bool, OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        if !matches!(
            self.controller.backend().context.actual_map_surface_mode,
            Some(ActualMapSurfaceMode::Approved)
        ) {
            return Ok(false);
        }
        let updated = self
            .controller
            .backend_mut()
            .context
            .resize_and_rasterize_actual_map_with_pois(map, view, pois, filters)?;
        if updated {
            self.controller.backend().invalidate()?;
            self.controller
                .backend()
                .context
                .record_actual_map_present_request();
            self.publish_fullscreen_frame();
        }
        Ok(updated)
    }

    #[cfg(feature = "development-local-readonly-position")]
    pub fn apply_local_unapproved_map_surface(
        &mut self,
        surface: ActualMapSurface,
    ) -> Result<(), OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        self.controller.backend_mut().context.actual_map_surface = Some(Box::new(surface));
        self.controller
            .backend_mut()
            .context
            .actual_map_surface_mode = Some(ActualMapSurfaceMode::DevelopmentLocalRead);
        self.controller.backend().invalidate()?;
        self.publish_fullscreen_frame();
        Ok(())
    }

    #[cfg(feature = "development-local-readonly-position")]
    pub fn update_local_unapproved_map_surface(
        &mut self,
        map: &MapRaster,
        view: MapView,
    ) -> Result<bool, OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        let updated = self
            .controller
            .backend_mut()
            .context
            .resize_and_rasterize_actual_map(map, view)?;
        if updated {
            self.controller.backend().invalidate()?;
            self.controller
                .backend()
                .context
                .record_actual_map_present_request();
            self.publish_fullscreen_frame();
        }
        Ok(updated)
    }

    #[cfg(feature = "development-local-readonly-position")]
    pub fn update_local_unapproved_map_surface_with_pois(
        &mut self,
        map: &MapRaster,
        view: MapView,
        pois: &[MapPoi],
        filters: &pal_domain::PoiFilters,
    ) -> Result<bool, OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        let updated = self
            .controller
            .backend_mut()
            .context
            .resize_and_rasterize_actual_map_with_pois(map, view, pois, filters)?;
        if updated {
            self.controller.backend().invalidate()?;
            self.controller
                .backend()
                .context
                .record_actual_map_present_request();
            self.publish_fullscreen_frame();
        }
        Ok(updated)
    }

    pub fn set_display_mode(&mut self, mode: DisplayMode) -> Result<HostUpdate, OverlayHostError> {
        let update = self.controller.set_display_mode(mode)?;
        self.publish_fullscreen_frame();
        Ok(update)
    }

    pub fn set_input_mode(&mut self, mode: InputMode) -> Result<HostUpdate, OverlayHostError> {
        let update = self.controller.set_input_mode(mode)?;
        self.publish_fullscreen_frame();
        Ok(update)
    }

    pub fn set_effective_settings_version(&mut self, version: u64) {
        let context = &mut self.controller.backend_mut().context;
        context.effective_settings_version.set(version);
        context.input.set_effective_settings_version(version);
    }

    pub fn set_poi_filters(&mut self, filters: PoiFilters) -> Result<(), OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        let context = &mut self.controller.backend_mut().context;
        if *context.poi_filters.borrow() == filters {
            return Ok(());
        }
        *context.poi_filters.borrow_mut() = filters;
        self.controller.backend().invalidate()?;
        self.publish_fullscreen_frame();
        Ok(())
    }

    pub fn take_control_intents(&mut self) -> Vec<ControlIntent> {
        let mut intents: Vec<_> = self
            .controller
            .backend_mut()
            .context
            .control_intents
            .borrow_mut()
            .drain(..)
            .collect();
        if let Some((sequence, action)) = self
            .fullscreen_writer
            .as_ref()
            .and_then(|writer| writer.take_consumer_action(self.fullscreen_action_sequence))
        {
            self.fullscreen_action_sequence = sequence;
            intents.push(ControlIntent::Action {
                expected_settings_version: self
                    .controller
                    .backend()
                    .context
                    .effective_settings_version
                    .get(),
                action: fullscreen_overlay_action(action),
            });
        }
        intents
    }

    pub fn take_hotkey_actions(&mut self) -> Vec<HotkeyAction> {
        self.controller
            .backend_mut()
            .context
            .hotkey_actions
            .borrow_mut()
            .drain(..)
            .collect()
    }

    pub fn set_fullscreen_search_catalog(
        &self,
        entries: &[FrameSearchEntry],
    ) -> Result<(), pal_fullscreen_bridge::BridgeError> {
        if let Some(writer) = self.fullscreen_writer.as_ref() {
            writer.publish_search_catalog(entries)?;
        }
        Ok(())
    }

    pub fn take_fullscreen_search_selection(&mut self) -> Option<usize> {
        let (sequence, index) = self
            .fullscreen_writer
            .as_ref()?
            .take_consumer_search_selection(self.fullscreen_search_sequence)?;
        self.fullscreen_search_sequence = sequence;
        Some(index)
    }

    /// Drains map navigation separately from settings actions.
    ///
    /// Recenter wins when both commands arrive in the same producer tick and clears the stale
    /// accumulated pan, matching the user's final input without changing the settings protocol.
    pub fn take_fullscreen_map_navigation(&mut self) -> Option<FullscreenMapNavigation> {
        let writer = self.fullscreen_writer.as_ref()?;
        if let Some(sequence) = writer.take_consumer_recenter(self.fullscreen_recenter_sequence) {
            self.fullscreen_recenter_sequence = sequence;
            let _ = writer.take_consumer_pan_delta();
            return Some(FullscreenMapNavigation::Recenter);
        }
        writer
            .take_consumer_pan_delta()
            .map(|delta| FullscreenMapNavigation::Pan {
                raster_delta_x: delta.raster_delta_x,
                raster_delta_y: delta.raster_delta_y,
            })
    }

    pub fn set_requested_visible(&mut self, visible: bool) -> Result<HostUpdate, OverlayHostError> {
        let update = self.controller.set_requested_visible(visible)?;
        self.publish_fullscreen_visibility();
        Ok(update)
    }

    pub fn set_opacity(&mut self, opacity: f32) -> Result<(), OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        self.controller.backend().set_opacity(opacity)?;
        self.opacity = opacity.clamp(0.2, 1.0);
        self.publish_fullscreen_frame();
        Ok(())
    }

    pub fn sync_fullscreen_consumer(&mut self) -> Result<bool, OverlayHostError> {
        const MAXIMUM_HEARTBEAT_AGE_MS: u64 = 1_500;
        let active = self
            .fullscreen_writer
            .as_ref()
            .and_then(|writer| writer.consumer_status(MAXIMUM_HEARTBEAT_AGE_MS))
            .is_some();
        let logical_visible = self.controller.is_visible();
        self.controller
            .backend_mut()
            .set_external_renderer_suppressed(active, logical_visible)?;
        Ok(active)
    }

    pub fn refresh_topmost(&self) -> Result<(), OverlayHostError> {
        self.controller.backend().refresh_topmost()
    }

    pub fn suspend(&mut self, reason: SuspendReason) -> Result<(), OverlayHostError> {
        self.controller.suspend(reason)?;
        self.publish_fullscreen_visibility();
        Ok(())
    }

    pub fn detach(&mut self) -> Result<(), OverlayHostError> {
        self.controller.detach()?;
        self.publish_fullscreen_visibility();
        Ok(())
    }

    pub fn pump_messages(&mut self) -> Result<PumpOutcome, OverlayHostError> {
        self.controller.backend().ensure_owner_thread()?;
        if self.controller.backend().context.shutdown_requested.get()
            || unsafe { IsWindow(self.raw_hwnd()) } == 0
        {
            return Ok(PumpOutcome::ShutdownRequested);
        }
        let mut message: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: message is valid for writes and the pump is restricted to this thread.
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            if message.message == WM_QUIT {
                self.controller
                    .backend()
                    .context
                    .shutdown_requested
                    .set(true);
                return Ok(PumpOutcome::ShutdownRequested);
            }
            if message.message == WM_HOTKEY {
                if let Ok(id) = i32::try_from(message.wParam)
                    && let Some(action) = action_from_registration_id(id)
                {
                    self.controller
                        .backend()
                        .context
                        .hotkey_actions
                        .borrow_mut()
                        .push_back(action);
                }
                continue;
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if self.controller.backend().context.shutdown_requested.get() {
                return Ok(PumpOutcome::ShutdownRequested);
            }
        }
        Ok(PumpOutcome::Continue)
    }

    pub fn reattach_requested(&self) -> bool {
        self.controller
            .backend()
            .context
            .reattach_requested
            .replace(false)
    }

    pub const fn raw_hwnd(&self) -> HWND {
        self.controller.backend().hwnd
    }

    pub fn received_clicks(&self) -> u64 {
        self.controller.backend().context.received_clicks.get()
    }

    pub fn actual_map_paint_performance_counters(&self) -> ActualMapPaintPerformanceCounters {
        self.controller
            .backend()
            .context
            .actual_map_paint_performance_counters
            .get()
    }

    pub const fn renderer_kind(&self) -> RendererKind {
        RendererKind::GdiCorrectnessPreview
    }

    #[cfg(any(
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    pub fn actual_map_surface_performance_counters(
        &self,
    ) -> Option<ActualMapSurfacePerformanceCounters> {
        self.controller
            .backend()
            .context
            .actual_map_surface
            .as_ref()
            .map(|surface| surface.performance_counters())
    }

    #[cfg(any(
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    pub fn actual_map_surface_size(&self) -> Option<PhysicalSize> {
        self.controller
            .backend()
            .context
            .actual_map_surface
            .as_ref()
            .map(|surface| surface.size())
    }

    #[cfg(any(
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    pub fn actual_map_surface_pixel(&self, x: u32, y: u32) -> Option<u32> {
        self.controller
            .backend()
            .context
            .actual_map_surface
            .as_ref()
            .and_then(|surface| surface.pixel(x, y))
    }

    pub fn is_visible(&self) -> bool {
        self.controller.is_visible()
    }

    /// Returns the current fullscreen frame publication failure, if publication has not recovered.
    /// The count is consecutive so callers can surface a stable degraded-state indicator without
    /// parsing stderr diagnostics.
    pub fn fullscreen_publish_failure(&self) -> Option<(u64, &str)> {
        self.fullscreen_publish_diagnostics
            .last_error
            .as_deref()
            .map(|error| {
                (
                    self.fullscreen_publish_diagnostics.consecutive_failures,
                    error,
                )
            })
    }

    fn publish_fullscreen_visibility(&self) {
        if let Some(writer) = self.fullscreen_writer.as_ref() {
            writer.set_logical_visible(self.controller.is_visible());
        }
    }

    #[cfg(any(
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    fn publish_fullscreen_frame(&mut self) {
        let Some(writer) = self.fullscreen_writer.as_mut() else {
            return;
        };
        let Some(surface) = self
            .controller
            .backend()
            .context
            .actual_map_surface
            .as_ref()
        else {
            writer.set_logical_visible(self.controller.is_visible());
            return;
        };
        let size = surface.size();
        let display_mode = match self.controller.display_mode() {
            DisplayMode::MiniMap => FrameDisplayMode::MiniMap,
            DisplayMode::ExpandedMap => FrameDisplayMode::ExpandedMap,
        };
        let filters = self.controller.backend().context.poi_filters.borrow();
        let controls =
            fullscreen_controls(self.controller.backend().context.input.mode(), &filters);
        let logical_viewport = self
            .controller
            .backend()
            .context
            .input
            .layout()
            .map_viewport_rect;
        let result = writer.publish_rgb_u32_with_controls(
            size.width,
            size.height,
            FrameMetadata {
                display_mode,
                controls,
                opacity: self.opacity,
                logical_visible: self.controller.is_visible(),
                display_width: logical_viewport.width,
                display_height: logical_viewport.height,
            },
            surface.pixels(),
        );
        drop(filters);
        match result {
            Ok(_) => self.fullscreen_publish_diagnostics.record_success(),
            Err(error) => {
                let should_report = self
                    .fullscreen_publish_diagnostics
                    .record_failure(&error, Instant::now());
                if should_report {
                    eprintln!(
                        "fullscreen overlay frame publication failed ({} consecutive; raster={}x{}, display={}x{}): {error}",
                        self.fullscreen_publish_diagnostics.consecutive_failures,
                        size.width,
                        size.height,
                        logical_viewport.width,
                        logical_viewport.height,
                    );
                }
            }
        }
    }

    #[cfg(not(any(
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    )))]
    fn publish_fullscreen_frame(&self) {
        self.publish_fullscreen_visibility();
    }
}

const fn fullscreen_overlay_action(action: ConsumerAction) -> pal_domain::OverlayAction {
    match action {
        ConsumerAction::ToggleFastTravel => pal_domain::OverlayAction::ToggleFastTravel,
        ConsumerAction::ToggleBoss => pal_domain::OverlayAction::ToggleBoss,
        ConsumerAction::ToggleWanted => pal_domain::OverlayAction::ToggleWanted,
        ConsumerAction::ToggleDungeon => pal_domain::OverlayAction::ToggleDungeon,
        ConsumerAction::ToggleTower => pal_domain::OverlayAction::ToggleTower,
        ConsumerAction::ToggleEgg => pal_domain::OverlayAction::ToggleEgg,
        ConsumerAction::ToggleResources => pal_domain::OverlayAction::ToggleResources,
        ConsumerAction::ToggleSalvage => pal_domain::OverlayAction::ToggleSalvage,
        ConsumerAction::CloseExpanded => pal_domain::OverlayAction::CloseExpanded,
        ConsumerAction::ZoomIn => pal_domain::OverlayAction::ZoomIn,
        ConsumerAction::ZoomOut => pal_domain::OverlayAction::ZoomOut,
        ConsumerAction::ToggleRotation => pal_domain::OverlayAction::ToggleRotation,
        ConsumerAction::Lock => pal_domain::OverlayAction::Lock,
        ConsumerAction::EnterInteractive => pal_domain::OverlayAction::EnterInteractive,
    }
}

#[cfg(any(
    feature = "approved-map-pack-runtime",
    feature = "test-harness",
    feature = "development-live-agent",
    feature = "development-local-readonly-position"
))]
fn fullscreen_controls(input_mode: InputMode, filters: &PoiFilters) -> FrameControls {
    FrameControls {
        input_mode: match input_mode {
            InputMode::Locked => FrameInputMode::Locked,
            InputMode::TemporaryInteractive
            | InputMode::PinnedInteractive
            | InputMode::LayoutEdit => FrameInputMode::Interactive,
        },
        poi_filters: FramePoiFilters {
            fast_travel: filters.fast_travel,
            boss: filters.boss,
            wanted: filters.wanted,
            dungeon: filters.dungeon,
            tower: layer_group_selected(&filters.enabled_layer_ids, &TOWER_LAYER_IDS),
            egg: layer_group_selected(&filters.enabled_layer_ids, &EGG_LAYER_IDS),
            resources: layer_group_selected(&filters.enabled_layer_ids, &RESOURCE_LAYER_IDS),
            salvage: layer_group_selected(&filters.enabled_layer_ids, &SALVAGE_LAYER_IDS),
        },
    }
}

fn local_rect(shape: HitShape, host: PhysicalRect) -> Result<PhysicalRect, OverlayHostError> {
    let rect = shape.bounds();
    let left = i64::from(rect.left) - i64::from(host.left);
    let top = i64::from(rect.top) - i64::from(host.top);
    Ok(PhysicalRect::new(
        i32::try_from(left).map_err(|_| OverlayHostError::new(OverlayHostOperation::Region, 0))?,
        i32::try_from(top).map_err(|_| OverlayHostError::new(OverlayHostOperation::Region, 0))?,
        rect.width,
        rect.height,
    ))
}

fn last_error(operation: OverlayHostOperation) -> OverlayHostError {
    // SAFETY: GetLastError has no preconditions and is called immediately after failure.
    OverlayHostError::new(operation, unsafe { GetLastError() })
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    std::panic::catch_unwind(|| {
        if message == WM_NCCREATE {
            // SAFETY: lparam points to CREATESTRUCTW for WM_NCCREATE.
            let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
            let context = create.lpCreateParams as *mut WindowContext;
            if context.is_null() {
                return 0;
            }
            unsafe { SetLastError(0) };
            // SAFETY: the context remains boxed for the entire HWND lifetime.
            let previous = unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, context.expose_provenance() as isize)
            };
            if previous == 0 && unsafe { GetLastError() } != 0 {
                return 0;
            }
            return 1;
        }

        // SAFETY: reading window user data is valid for any existing HWND.
        let context = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowContext };
        match message {
            WM_NCHITTEST if !context.is_null() => {
                let point = decode_screen_point(lparam);
                // SAFETY: context is cleared only in WM_NCDESTROY.
                match unsafe { &*context }.input.wm_nchittest(point) {
                    HitTest::Transparent => HTTRANSPARENT as isize,
                    HitTest::Client => HTCLIENT as isize,
                }
            }
            WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
            WM_LBUTTONDOWN if !context.is_null() => {
                // SAFETY: context remains valid while the HWND exists.
                let clicks = unsafe { &*context }.received_clicks.get();
                // SAFETY: context remains valid while the HWND exists.
                unsafe { &*context }
                    .received_clicks
                    .set(clicks.saturating_add(1));
                0
            }
            WM_LBUTTONUP if !context.is_null() => {
                // SAFETY: context remains valid while the HWND exists.
                let context = unsafe { &*context };
                let point = decode_client_point(lparam, context.window_rect);
                let _ = context
                    .input
                    .handle_left_click(point, &WindowIntentSink(context));
                0
            }
            WM_MOUSEWHEEL if !context.is_null() => {
                // SAFETY: context remains valid while the HWND exists.
                let context = unsafe { &*context };
                let point = decode_screen_point(lparam);
                let delta = ((wparam >> 16) & 0xffff) as u16 as i16;
                let _ = context
                    .input
                    .handle_wheel(point, delta, &WindowIntentSink(context));
                0
            }
            WM_ERASEBKGND => 1,
            WM_PAINT if !context.is_null() => {
                // SAFETY: context remains valid while the HWND exists.
                unsafe { paint_preview(hwnd, &*context) };
                0
            }
            WM_DPICHANGED | WM_DISPLAYCHANGE if !context.is_null() => {
                // SAFETY: context remains valid while the HWND exists.
                unsafe { &*context }.reattach_requested.set(true);
                0
            }
            WM_CLOSE => {
                if !context.is_null() {
                    // SAFETY: context remains valid until WM_NCDESTROY clears window user data.
                    unsafe { &*context }.shutdown_requested.set(true);
                }
                // SAFETY: hwnd is the window receiving WM_CLOSE.
                unsafe { DestroyWindow(hwnd) };
                0
            }
            WM_NCDESTROY => {
                if !context.is_null() {
                    // SAFETY: context remains valid until this callback clears window user data.
                    unsafe { &*context }.shutdown_requested.set(true);
                }
                unsafe { SetLastError(0) };
                // SAFETY: clearing user data prevents later callback use.
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
                // SAFETY: default processing is required for non-client teardown.
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            _ => {
                // SAFETY: unhandled messages are delegated to the system default procedure.
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
    })
    .unwrap_or_else(|_| {
        // SAFETY: hiding the affected window is the fail-closed response to a callback panic.
        unsafe { ShowWindow(hwnd, SW_HIDE) };
        0
    })
}

fn decode_screen_point(lparam: LPARAM) -> PhysicalPoint {
    let packed = lparam as u32;
    PhysicalPoint::new(
        (packed as u16) as i16 as i32,
        (packed >> 16) as u16 as i16 as i32,
    )
}

fn decode_client_point(lparam: LPARAM, window_rect: PhysicalRect) -> PhysicalPoint {
    let local = decode_screen_point(lparam);
    PhysicalPoint::new(
        window_rect.left.saturating_add(local.x),
        window_rect.top.saturating_add(local.y),
    )
}

unsafe fn paint_preview(hwnd: HWND, context: &WindowContext) {
    let mut paint: PAINTSTRUCT = unsafe { std::mem::zeroed() };
    // SAFETY: BeginPaint/EndPaint are paired for this WM_PAINT.
    let dc = unsafe { BeginPaint(hwnd, &mut paint) };
    if dc.is_null() {
        return;
    }
    let layout = context.input.layout();
    let development_surface = {
        #[cfg(any(
            feature = "approved-map-pack-runtime",
            feature = "test-harness",
            feature = "development-live-agent",
            feature = "development-local-readonly-position"
        ))]
        {
            context.actual_map_surface_mode.is_some_and(|_mode| {
                #[cfg(feature = "approved-map-pack-runtime")]
                if matches!(_mode, ActualMapSurfaceMode::Approved) {
                    return false;
                }
                true
            })
        }
        #[cfg(not(any(
            feature = "approved-map-pack-runtime",
            feature = "test-harness",
            feature = "development-live-agent",
            feature = "development-local-readonly-position"
        )))]
        {
            false
        }
    };
    let gate_badge = if development_surface || {
        #[cfg(feature = "test-harness")]
        {
            context.synthetic_surface.is_some()
        }
        #[cfg(not(feature = "test-harness"))]
        {
            false
        }
    } {
        GateBadge::NotApproved
    } else {
        GateBadge::Approved
    };
    let chrome = build_chrome_plan(
        context.display_mode,
        context.input.mode(),
        Freshness::Live,
        gate_badge,
    )
    .with_filter_selection(&context.poi_filters.borrow());
    // SAFETY: dc is the active BeginPaint DC and remains valid until EndPaint below.
    let mut renderer =
        unsafe { GdiCorrectnessRenderer::for_paint(dc, layout, estimated_dpi(layout)) };

    #[cfg(any(
        feature = "approved-map-pack-runtime",
        feature = "test-harness",
        feature = "development-live-agent",
        feature = "development-local-readonly-position"
    ))]
    if let Some(surface) = context.actual_map_surface.as_ref() {
        context.record_actual_map_paint();
        if renderer.present(surface, chrome).is_ok() {
            context.record_actual_map_upload();
        }
        unsafe { EndPaint(hwnd, &paint) };
        return;
    }

    #[cfg(feature = "test-harness")]
    if let Some(surface) = context.synthetic_surface.as_ref() {
        let size = PhysicalSize::new(surface.diameter(), surface.diameter());
        let _ = renderer.present_pixels(size, surface.pixels(), chrome);
        unsafe { EndPaint(hwnd, &paint) };
        return;
    }

    let _ = renderer.present_without_map(chrome);
    unsafe { EndPaint(hwnd, &paint) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_publish_failures_are_stateful_and_rate_limited() {
        let mut diagnostics = FullscreenPublishDiagnostics::default();
        let started_at = Instant::now();
        let error = pal_fullscreen_bridge::BridgeError::InvalidDimensions;

        assert!(diagnostics.record_failure(&error, started_at));
        assert!(!diagnostics.record_failure(
            &error,
            started_at + FULLSCREEN_PUBLISH_REPORT_INTERVAL - Duration::from_millis(1)
        ));
        assert!(
            diagnostics.record_failure(&error, started_at + FULLSCREEN_PUBLISH_REPORT_INTERVAL)
        );
        assert_eq!(diagnostics.consecutive_failures, 3);
        assert_eq!(
            diagnostics.last_error.as_deref(),
            Some("shared frame dimensions are invalid")
        );

        diagnostics.record_success();
        assert_eq!(diagnostics.consecutive_failures, 0);
        assert_eq!(diagnostics.last_error, None);
        assert_eq!(diagnostics.last_reported_at, None);
    }
}
