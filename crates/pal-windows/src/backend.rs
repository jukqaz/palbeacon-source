use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(u64);

impl WindowId {
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn as_raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MonitorId(u64);

impl MonitorId {
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn as_raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalClientRect {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}

impl PhysicalClientRect {
    pub const fn new(left: i32, top: i32, width: u32, height: u32) -> Self {
        Self {
            left,
            top,
            width,
            height,
        }
    }

    pub const fn left(self) -> i32 {
        self.left
    }

    pub const fn top(self) -> i32 {
        self.top
    }

    pub const fn width(self) -> u32 {
        self.width
    }

    pub const fn height(self) -> u32 {
        self.height
    }

    pub const fn area(self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct WindowObservationParts {
    pub id: WindowId,
    pub monitor_id: MonitorId,
    pub process_id: u32,
    pub image_path: String,
    pub window_title: String,
    pub client_rect: PhysicalClientRect,
    pub dpi: u32,
    pub visible: bool,
    pub top_level: bool,
    pub foreground: bool,
    pub minimized: bool,
    pub inspectable: bool,
}

impl fmt::Debug for WindowObservationParts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowObservationParts")
            .field("id", &self.id)
            .field("monitor_id", &self.monitor_id)
            .field("process_id", &self.process_id)
            .field("image_path", &"<redacted>")
            .field("window_title", &self.window_title)
            .field("client_rect", &self.client_rect)
            .field("dpi", &self.dpi)
            .field("visible", &self.visible)
            .field("top_level", &self.top_level)
            .field("foreground", &self.foreground)
            .field("minimized", &self.minimized)
            .field("inspectable", &self.inspectable)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct WindowObservation {
    id: WindowId,
    monitor_id: MonitorId,
    process_id: u32,
    image_path: String,
    window_title: String,
    client_rect: PhysicalClientRect,
    dpi: u32,
    visible: bool,
    top_level: bool,
    foreground: bool,
    minimized: bool,
    inspectable: bool,
}

impl fmt::Debug for WindowObservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowObservation")
            .field("id", &self.id)
            .field("monitor_id", &self.monitor_id)
            .field("process_id", &self.process_id)
            .field("image_path", &"<redacted>")
            .field("window_title", &self.window_title)
            .field("client_rect", &self.client_rect)
            .field("dpi", &self.dpi)
            .field("visible", &self.visible)
            .field("top_level", &self.top_level)
            .field("foreground", &self.foreground)
            .field("minimized", &self.minimized)
            .field("inspectable", &self.inspectable)
            .finish()
    }
}

impl WindowObservation {
    pub fn for_test(parts: WindowObservationParts) -> Self {
        Self::from_backend(parts)
    }

    pub(crate) fn from_backend(parts: WindowObservationParts) -> Self {
        Self {
            id: parts.id,
            monitor_id: parts.monitor_id,
            process_id: parts.process_id,
            image_path: parts.image_path,
            window_title: parts.window_title,
            client_rect: parts.client_rect,
            dpi: parts.dpi,
            visible: parts.visible,
            top_level: parts.top_level,
            foreground: parts.foreground,
            minimized: parts.minimized,
            inspectable: parts.inspectable,
        }
    }

    pub const fn id(&self) -> WindowId {
        self.id
    }

    pub const fn monitor_id(&self) -> MonitorId {
        self.monitor_id
    }

    pub const fn process_id(&self) -> u32 {
        self.process_id
    }

    pub fn image_path(&self) -> &str {
        &self.image_path
    }

    pub fn image_basename(&self) -> &str {
        self.image_path
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or_default()
    }

    pub const fn client_rect(&self) -> PhysicalClientRect {
        self.client_rect
    }

    pub const fn dpi(&self) -> u32 {
        self.dpi
    }

    pub const fn visible(&self) -> bool {
        self.visible
    }

    pub const fn top_level(&self) -> bool {
        self.top_level
    }

    pub const fn foreground(&self) -> bool {
        self.foreground
    }

    pub const fn minimized(&self) -> bool {
        self.minimized
    }

    pub const fn inspectable(&self) -> bool {
        self.inspectable
    }

    pub fn with_monitor_for_test(mut self, monitor_id: MonitorId) -> Self {
        self.monitor_id = monitor_id;
        self
    }

    pub fn with_image_path_for_test(mut self, image_path: String) -> Self {
        self.image_path = image_path;
        self
    }

    pub fn with_window_title_for_test(mut self, window_title: String) -> Self {
        self.window_title = window_title;
        self
    }

    pub fn with_client_rect_for_test(mut self, client_rect: PhysicalClientRect) -> Self {
        self.client_rect = client_rect;
        self
    }

    pub fn with_dpi_for_test(mut self, dpi: u32) -> Self {
        self.dpi = dpi;
        self
    }

    pub fn with_visible_for_test(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn with_top_level_for_test(mut self, top_level: bool) -> Self {
        self.top_level = top_level;
        self
    }

    pub fn with_foreground_for_test(mut self, foreground: bool) -> Self {
        self.foreground = foreground;
        self
    }

    pub fn with_minimized_for_test(mut self, minimized: bool) -> Self {
        self.minimized = minimized;
        self
    }

    pub fn with_inspectable_for_test(mut self, inspectable: bool) -> Self {
        self.inspectable = inspectable;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowBackendOperation {
    Inspect,
    EnumerateTopLevel,
}

impl WindowBackendOperation {
    const fn label(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::EnumerateTopLevel => "enumerate_top_level",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowBackendError {
    operation: WindowBackendOperation,
    code: u32,
}

impl WindowBackendError {
    pub const fn new(operation: WindowBackendOperation, code: u32) -> Self {
        Self { operation, code }
    }

    pub const fn operation(self) -> WindowBackendOperation {
        self.operation
    }

    pub const fn code(self) -> u32 {
        self.code
    }
}

impl fmt::Display for WindowBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "window backend {} failed with code {}",
            self.operation.label(),
            self.code
        )
    }
}

impl std::error::Error for WindowBackendError {}

pub trait WindowBackend {
    fn inspect(&mut self, id: WindowId) -> Result<Option<WindowObservation>, WindowBackendError>;

    fn enumerate_top_level(&mut self) -> Result<Vec<WindowObservation>, WindowBackendError>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FakeWindowCalls {
    pub inspected: Vec<WindowId>,
    pub enumerate_top_level: usize,
}

#[derive(Debug, Default)]
struct FakeWindowState {
    windows: Vec<WindowObservation>,
    calls: FakeWindowCalls,
    inspection_errors: Vec<(WindowId, WindowBackendError)>,
    enumeration_error: Option<WindowBackendError>,
}

#[derive(Clone, Debug, Default)]
pub struct FakeWindowBackend {
    state: Arc<Mutex<FakeWindowState>>,
}

impl FakeWindowBackend {
    pub fn with_windows(windows: impl IntoIterator<Item = WindowObservation>) -> FakeWindowBackend {
        Self {
            state: Arc::new(Mutex::new(FakeWindowState {
                windows: windows.into_iter().collect(),
                ..FakeWindowState::default()
            })),
        }
    }

    pub fn replace_windows(&self, windows: impl IntoIterator<Item = WindowObservation>) {
        self.state().windows = windows.into_iter().collect();
    }

    pub fn fail_enumeration(&self, error: WindowBackendError) {
        self.state().enumeration_error = Some(error);
    }

    pub fn fail_inspection(&self, id: WindowId, error: WindowBackendError) {
        let mut state = self.state();
        state
            .inspection_errors
            .retain(|(existing, _)| *existing != id);
        state.inspection_errors.push((id, error));
    }

    pub fn calls(&self) -> FakeWindowCalls {
        self.state().calls.clone()
    }

    pub fn clear_calls(&self) {
        self.state().calls = FakeWindowCalls::default();
    }

    fn state(&self) -> MutexGuard<'_, FakeWindowState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl WindowBackend for FakeWindowBackend {
    fn inspect(&mut self, id: WindowId) -> Result<Option<WindowObservation>, WindowBackendError> {
        let mut state = self.state();
        state.calls.inspected.push(id);
        if let Some(error) = state
            .inspection_errors
            .iter()
            .find_map(|(failed_id, error)| (*failed_id == id).then_some(*error))
        {
            return Err(error);
        }
        Ok(state.windows.iter().find(|window| window.id == id).cloned())
    }

    fn enumerate_top_level(&mut self) -> Result<Vec<WindowObservation>, WindowBackendError> {
        let mut state = self.state();
        state.calls.enumerate_top_level += 1;
        if let Some(error) = state.enumeration_error {
            return Err(error);
        }
        Ok(state.windows.clone())
    }
}
