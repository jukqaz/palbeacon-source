use pal_windows::{
    DpiAwarenessError, HotkeyBackend, Win32Backend, Win32HotkeyBackend, WindowBackend,
    enable_per_monitor_v2,
};

#[test]
fn exposes_real_win32_backends_without_performing_registration() {
    let _dpi_entrypoint: fn() -> Result<(), DpiAwarenessError> = enable_per_monitor_v2;

    let mut windows = Win32Backend::new();
    let _enumerate: fn(
        &mut Win32Backend,
    ) -> Result<
        Vec<pal_windows::WindowObservation>,
        pal_windows::WindowBackendError,
    > = WindowBackend::enumerate_top_level;
    let _ = &mut windows;

    let mut hotkeys = Win32HotkeyBackend::new();
    let _register: fn(
        &mut Win32HotkeyBackend,
        i32,
        u32,
        u32,
    ) -> Result<(), pal_windows::HotkeyBackendError> = HotkeyBackend::register;
    let _ = &mut hotkeys;
}
