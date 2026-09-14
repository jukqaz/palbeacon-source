use std::fmt;

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    GetDpiAwarenessContextForProcess, SetProcessDpiAwarenessContext,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DpiAwarenessError {
    code: u32,
}

impl DpiAwarenessError {
    pub const fn code(self) -> u32 {
        self.code
    }
}

impl fmt::Display for DpiAwarenessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "setting Per-Monitor-V2 DPI awareness failed with code {}",
            self.code
        )
    }
}

impl std::error::Error for DpiAwarenessError {}

pub fn enable_per_monitor_v2() -> Result<(), DpiAwarenessError> {
    // SAFETY: the constant is a system-defined, process-wide awareness context.
    if unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) } != 0 {
        return Ok(());
    }

    // GetLastError must be captured before making any other Win32 call.
    // SAFETY: GetLastError has no preconditions.
    let code = unsafe { GetLastError() };

    // A process manifest or an earlier call may already have established the exact context.
    // SAFETY: GetCurrentProcess returns a pseudo handle valid in this process, and both DPI
    // functions accept it without taking ownership.
    let current = unsafe { GetDpiAwarenessContextForProcess(GetCurrentProcess()) };
    // SAFETY: both values are system DPI-awareness context tokens.
    if unsafe { AreDpiAwarenessContextsEqual(current, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
        != 0
    {
        Ok(())
    } else {
        Err(DpiAwarenessError { code })
    }
}
