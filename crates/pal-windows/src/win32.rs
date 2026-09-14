use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::null_mut;
use std::rc::Rc;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_HOTKEY_ALREADY_REGISTERED, ERROR_NO_UNICODE_TRANSLATION,
    ERROR_NOT_ENOUGH_MEMORY, ERROR_SUCCESS, GetLastError, HANDLE, HWND, LPARAM, POINT, RECT,
    SetLastError,
};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, MONITOR_DEFAULTTONEAREST, MonitorFromWindow,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GetAncestor, GetClientRect, GetForegroundWindow,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible,
};

use crate::backend::{
    MonitorId, PhysicalClientRect, WindowBackend, WindowBackendError, WindowBackendOperation,
    WindowId, WindowObservation, WindowObservationParts,
};
use crate::hotkeys::{
    HotkeyBackend, HotkeyBackendError, HotkeyBackendErrorCategory, HotkeyBackendOperation,
};

const PROCESS_IMAGE_BUFFER_LEN: usize = 32_768;
const CALLBACK_PANIC_CODE: u32 = 0xE000_0001;

#[derive(Debug, Default)]
pub struct Win32Backend;

impl Win32Backend {
    pub const fn new() -> Self {
        Self
    }

    fn inspect_hwnd(
        &mut self,
        hwnd: HWND,
    ) -> Result<Option<WindowObservation>, WindowBackendError> {
        if hwnd.is_null() {
            return Ok(None);
        }

        let mut process_id = 0;
        // SAFETY: hwnd is an opaque value supplied by Win32 enumeration or the caller; the output
        // pointer is valid for the duration of the call.
        if unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) } == 0 || process_id == 0 {
            // GetLastError must be captured immediately after failure.
            let code = unsafe { GetLastError() };
            return if code == ERROR_SUCCESS {
                Ok(None)
            } else {
                Err(window_error(WindowBackendOperation::Inspect, code))
            };
        }

        // SAFETY: GetAncestor does not dereference memory owned by Rust.
        let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
        if root.is_null() {
            let code = unsafe { GetLastError() };
            return if code == ERROR_SUCCESS {
                Ok(None)
            } else {
                Err(window_error(WindowBackendOperation::Inspect, code))
            };
        }
        if root != hwnd {
            return Ok(None);
        }

        let image_path = query_process_image_path(process_id)?;
        let mut client = RECT::default();
        // SAFETY: client points to initialized writable storage.
        if unsafe { GetClientRect(hwnd, &mut client) } == 0 {
            let code = unsafe { GetLastError() };
            return Err(window_error(WindowBackendOperation::Inspect, code));
        }
        let mut origin = POINT {
            x: client.left,
            y: client.top,
        };
        // SAFETY: origin points to writable storage and hwnd is only inspected.
        if unsafe { ClientToScreen(hwnd, &mut origin) } == 0 {
            let code = unsafe { GetLastError() };
            return Err(window_error(WindowBackendOperation::Inspect, code));
        }

        let width = client.right.checked_sub(client.left).unwrap_or_default();
        let height = client.bottom.checked_sub(client.top).unwrap_or_default();
        let width = u32::try_from(width).unwrap_or_default();
        let height = u32::try_from(height).unwrap_or_default();

        // SAFETY: GetDpiForWindow only reads window metadata.
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 {
            let code = unsafe { GetLastError() };
            return Err(window_error(WindowBackendOperation::Inspect, code));
        }

        // SAFETY: the requested fallback always returns the nearest monitor for a valid window.
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        if monitor.is_null() {
            let code = unsafe { GetLastError() };
            return Err(window_error(WindowBackendOperation::Inspect, code));
        }

        // SAFETY: these calls only query window state.
        let visible = unsafe { IsWindowVisible(hwnd) } != 0;
        let minimized = unsafe { IsIconic(hwnd) } != 0;
        let foreground = foreground_process_id() == process_id;

        Ok(Some(WindowObservation::from_backend(
            WindowObservationParts {
                id: WindowId::from_raw(hwnd as usize as u64),
                monitor_id: MonitorId::from_raw(monitor as usize as u64),
                process_id,
                image_path,
                window_title: String::new(),
                client_rect: PhysicalClientRect::new(origin.x, origin.y, width, height),
                dpi,
                visible,
                top_level: true,
                foreground,
                minimized,
                inspectable: true,
            },
        )))
    }
}

fn foreground_process_id() -> u32 {
    // Unreal can hand foreground ownership to another top-level helper HWND from the same game
    // process while changing display mode. Treat the process, rather than one exact HWND, as the
    // active game so a fullscreen transition does not incorrectly suspend the overlay.
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return 0;
    }
    let mut process_id = 0;
    // SAFETY: foreground is an opaque HWND supplied by User32 and the output pointer is valid.
    let _ = unsafe { GetWindowThreadProcessId(foreground, &mut process_id) };
    process_id
}

impl WindowBackend for Win32Backend {
    fn inspect(&mut self, id: WindowId) -> Result<Option<WindowObservation>, WindowBackendError> {
        self.inspect_hwnd(id.as_raw() as usize as HWND)
    }

    fn enumerate_top_level(&mut self) -> Result<Vec<WindowObservation>, WindowBackendError> {
        let mut context = EnumContext::default();
        // SAFETY: last-error state is thread-local.
        unsafe { SetLastError(ERROR_SUCCESS) };
        // SAFETY: the callback uses the context only during this synchronous call.
        let succeeded = unsafe {
            EnumWindows(
                Some(enum_window_callback),
                (&mut context as *mut EnumContext) as LPARAM,
            )
        };
        // GetLastError must be captured before any other Win32 call.
        let code = if succeeded == 0 {
            // SAFETY: GetLastError has no preconditions.
            unsafe { GetLastError() }
        } else {
            ERROR_SUCCESS
        };

        if context.stop_code != ERROR_SUCCESS {
            return Err(window_error(
                WindowBackendOperation::EnumerateTopLevel,
                context.stop_code,
            ));
        }
        if succeeded == 0 {
            return Err(window_error(
                WindowBackendOperation::EnumerateTopLevel,
                code,
            ));
        }

        let mut observations = Vec::new();
        for id in context.ids {
            if let Ok(Some(observation)) = self.inspect(id) {
                observations.push(observation);
            }
        }
        Ok(observations)
    }
}

#[derive(Default)]
struct EnumContext {
    ids: Vec<WindowId>,
    stop_code: u32,
}

unsafe extern "system" fn enum_window_callback(hwnd: HWND, lparam: LPARAM) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let context_ptr = lparam as *mut EnumContext;
        if context_ptr.is_null() {
            return false;
        }
        // SAFETY: EnumWindows invokes this callback synchronously with the live context pointer.
        let context = unsafe { &mut *context_ptr };
        if context.ids.try_reserve(1).is_err() {
            context.stop_code = ERROR_NOT_ENOUGH_MEMORY;
            return false;
        }
        context.ids.push(WindowId::from_raw(hwnd as usize as u64));
        true
    }));

    match result {
        Ok(true) => 1,
        Ok(false) => 0,
        Err(_) => {
            let context_ptr = lparam as *mut EnumContext;
            if !context_ptr.is_null() {
                // SAFETY: the pointer remains valid for this synchronous callback.
                unsafe { (*context_ptr).stop_code = CALLBACK_PANIC_CODE };
            }
            0
        }
    }
}

fn query_process_image_path(process_id: u32) -> Result<String, WindowBackendError> {
    // SAFETY: OpenProcess is called with a PID returned by User32; ownership is transferred into
    // OwnedHandle on success.
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if raw.is_null() {
        let code = unsafe { GetLastError() };
        return Err(window_error(WindowBackendOperation::Inspect, code));
    }
    let process = OwnedHandle(raw);
    let mut buffer = Box::new([0u16; PROCESS_IMAGE_BUFFER_LEN]);
    let mut length = u32::try_from(buffer.len()).expect("fixed process image buffer fits u32");
    // SAFETY: buffer is writable for `length` UTF-16 code units and process owns a valid HANDLE.
    if unsafe { QueryFullProcessImageNameW(process.0, 0, buffer.as_mut_ptr(), &mut length) } == 0 {
        let code = unsafe { GetLastError() };
        return Err(window_error(WindowBackendOperation::Inspect, code));
    }
    let length = usize::try_from(length)
        .unwrap_or_default()
        .min(buffer.len());
    String::from_utf16(&buffer[..length]).map_err(|_| {
        window_error(
            WindowBackendOperation::Inspect,
            ERROR_NO_UNICODE_TRANSLATION,
        )
    })
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this wrapper uniquely owns the non-null handle.
            unsafe { CloseHandle(self.0) };
            self.0 = null_mut();
        }
    }
}

fn window_error(operation: WindowBackendOperation, code: u32) -> WindowBackendError {
    WindowBackendError::new(operation, code)
}

/// A null-HWND global-hotkey backend confined to its creating message-pump thread.
///
/// ```compile_fail
/// fn assert_send<T: Send>() {}
/// assert_send::<pal_windows::Win32HotkeyBackend>();
/// ```
///
/// ```compile_fail
/// fn assert_sync<T: Sync>() {}
/// assert_sync::<pal_windows::Win32HotkeyBackend>();
/// ```
#[derive(Debug, Default)]
pub struct Win32HotkeyBackend {
    owner_thread: PhantomData<Rc<()>>,
}

impl Win32HotkeyBackend {
    pub const fn new() -> Self {
        Self {
            owner_thread: PhantomData,
        }
    }
}

impl HotkeyBackend for Win32HotkeyBackend {
    fn register(
        &mut self,
        id: i32,
        modifiers: u32,
        virtual_key: u32,
    ) -> Result<(), HotkeyBackendError> {
        // SAFETY: a null HWND creates a registration owned by the calling thread.
        if unsafe { RegisterHotKey(null_mut(), id, modifiers, virtual_key) } != 0 {
            return Ok(());
        }
        let code = unsafe { GetLastError() };
        let category = if code == ERROR_HOTKEY_ALREADY_REGISTERED {
            HotkeyBackendErrorCategory::Collision
        } else {
            HotkeyBackendErrorCategory::System
        };
        Err(HotkeyBackendError::new(
            HotkeyBackendOperation::Register,
            category,
            code,
        ))
    }

    fn unregister(&mut self, id: i32) -> Result<(), HotkeyBackendError> {
        // SAFETY: registration IDs are scoped to this calling thread by construction.
        if unsafe { UnregisterHotKey(null_mut(), id) } != 0 {
            return Ok(());
        }
        let code = unsafe { GetLastError() };
        Err(HotkeyBackendError::new(
            HotkeyBackendOperation::Unregister,
            HotkeyBackendErrorCategory::System,
            code,
        ))
    }
}
