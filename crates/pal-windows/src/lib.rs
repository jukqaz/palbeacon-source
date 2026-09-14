mod backend;
mod dpi;
mod hotkeys;
#[cfg(all(windows, feature = "development-readonly-process-memory"))]
mod process_memory;
mod tracker;
mod win32;

pub use backend::{
    FakeWindowBackend, FakeWindowCalls, MonitorId, PhysicalClientRect, WindowBackend,
    WindowBackendError, WindowBackendOperation, WindowId, WindowObservation,
    WindowObservationParts,
};
pub use dpi::{DpiAwarenessError, enable_per_monitor_v2};
pub use hotkeys::{
    DisabledHotkey, FakeHotkeyBackend, HotkeyBackend, HotkeyBackendCall, HotkeyBackendError,
    HotkeyBackendErrorCategory, HotkeyBackendOperation, HotkeyDisableReason, HotkeyRegistry,
    HotkeyRegistryError, HotkeyReport, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    TRANSIENT_HOTKEY_ID_END, TRANSIENT_HOTKEY_ID_START, action_from_registration_id,
    action_registration_id,
};
#[cfg(all(windows, feature = "development-readonly-process-memory"))]
pub use process_memory::{
    MAX_EXACT_READ_BYTES, QueryOnlyProcessGuard, ReadOnlyProcess, ReadOnlyProcessError,
    ReadOnlyProcessModule, ReadOnlyProcessSession,
};
pub use tracker::{
    GameWindowTracker, PALWORLD_IMAGE_NAME, TrackedWindow, TrackerError, TrackerErrorCategory,
    WindowEvent,
};
pub use win32::{Win32Backend, Win32HotkeyBackend};
