use reqwest::Url;

use crate::AccessAuthError;

pub trait BrowserLauncher: Send + Sync {
    /// Opens one already-validated HTTPS authorization URL. Implementations
    /// must never log, format into an error, persist or otherwise expose it.
    fn open(&self, authorization_url: &Url) -> Result<(), AccessAuthError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemBrowser;

#[cfg(windows)]
impl BrowserLauncher for SystemBrowser {
    fn open(&self, authorization_url: &Url) -> Result<(), AccessAuthError> {
        use std::{ffi::OsStr, os::windows::ffi::OsStrExt as _};

        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

        if authorization_url.scheme() != "https"
            || authorization_url.host_str().is_none()
            || !authorization_url.username().is_empty()
            || authorization_url.password().is_some()
            || authorization_url.fragment().is_some()
        {
            return Err(AccessAuthError::BrowserLaunchFailed);
        }
        let operation = OsStr::new("open")
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let target = OsStr::new(authorization_url.as_str())
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: all string pointers are NUL-terminated, immutable for the
        // duration of the call, and no returned handle is owned by this code.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if result as usize <= 32 {
            Err(AccessAuthError::BrowserLaunchFailed)
        } else {
            Ok(())
        }
    }
}

#[cfg(not(windows))]
impl BrowserLauncher for SystemBrowser {
    fn open(&self, _authorization_url: &Url) -> Result<(), AccessAuthError> {
        Err(AccessAuthError::BrowserLaunchFailed)
    }
}
