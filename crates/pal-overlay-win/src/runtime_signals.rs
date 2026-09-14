use std::{ffi::OsStr, io, os::windows::ffi::OsStrExt as _};

use pal_windows_ipc::{OVERLAY_POSITION_LIVE_EVENT, OVERLAY_RUNNING_EVENT};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::Threading::{CreateEventW, ResetEvent, SetEvent},
};

/// Current-user session signals used only for local product health reporting.
///
/// They carry no coordinates or identity data. The running signal remains set for the
/// lifetime of the overlay process; the live-position signal follows the fail-closed map
/// lifetime of the overlay process; the live-position signal follows a trusted, fresh position
/// stream after an approved map surface has been applied. Map browsing can remain visible when
/// that signal is reset.
pub struct OverlayRuntimeSignals {
    running: HANDLE,
    position_live: HANDLE,
}

impl OverlayRuntimeSignals {
    pub fn start() -> io::Result<Self> {
        Self::start_named(OVERLAY_RUNNING_EVENT, OVERLAY_POSITION_LIVE_EVENT)
    }

    fn start_named(running_name: &str, position_live_name: &str) -> io::Result<Self> {
        let running = create_named_event(running_name, true)?;
        let position_live = match create_named_event(position_live_name, false) {
            Ok(event) => event,
            Err(error) => {
                // SAFETY: running is uniquely owned on this error path.
                unsafe { CloseHandle(running) };
                return Err(error);
            }
        };
        // CreateEventW does not apply the initial state when a stale named object already
        // exists in this logon session, so explicitly restore the product invariants.
        // SAFETY: both handles were returned by CreateEventW with EVENT_ALL_ACCESS.
        unsafe {
            SetEvent(running);
            ResetEvent(position_live);
        }
        Ok(Self {
            running,
            position_live,
        })
    }

    pub fn set_position_live(&self, live: bool) -> io::Result<()> {
        // SAFETY: position_live is owned and valid for the lifetime of self.
        let succeeded = unsafe {
            if live {
                SetEvent(self.position_live)
            } else {
                ResetEvent(self.position_live)
            }
        };
        if succeeded == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for OverlayRuntimeSignals {
    fn drop(&mut self) {
        // Do not reset the shared running event here. During a supervised restart the old
        // and replacement overlay can overlap briefly; resetting from the old process would
        // make the healthy replacement look stopped for the rest of its lifetime. Once the
        // final handle closes, Windows removes the named event and observers correctly see
        // it as unavailable.
        //
        // The position event is refreshed by the active overlay every frame and remains
        // fail-closed on shutdown.
        // SAFETY: the handles are uniquely owned and closed exactly once here.
        unsafe {
            ResetEvent(self.position_live);
            CloseHandle(self.position_live);
            CloseHandle(self.running);
        }
    }
}

fn create_named_event(name: &str, initial_state: bool) -> io::Result<HANDLE> {
    let wide = OsStr::new(name)
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: wide is NUL-terminated and lives for the call. No security descriptor is borrowed.
    let event =
        unsafe { CreateEventW(std::ptr::null(), 1, i32::from(initial_state), wide.as_ptr()) };
    if event.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use std::process;

    use windows_sys::Win32::{
        Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::WaitForSingleObject,
    };

    use super::OverlayRuntimeSignals;

    #[test]
    fn dropping_an_overlapped_instance_does_not_clear_the_running_signal() {
        let suffix = format!("{}.{}", process::id(), line!());
        let running = format!(r"Local\PalBeacon.Tests.Overlay.Running.{suffix}");
        let position = format!(r"Local\PalBeacon.Tests.Overlay.Position.{suffix}");
        let first = OverlayRuntimeSignals::start_named(&running, &position).unwrap();
        let second = OverlayRuntimeSignals::start_named(&running, &position).unwrap();

        // SAFETY: first.running is live until first is dropped below.
        assert_eq!(
            unsafe { WaitForSingleObject(first.running, 0) },
            WAIT_OBJECT_0
        );
        drop(second);
        // SAFETY: first.running remains live and must still represent the replacement.
        assert_eq!(
            unsafe { WaitForSingleObject(first.running, 0) },
            WAIT_OBJECT_0
        );
        assert_ne!(
            unsafe { WaitForSingleObject(first.running, 0) },
            WAIT_TIMEOUT
        );
    }
}
