#[cfg(windows)]
mod windows_impl {
    use std::{
        ffi::{OsStr, c_void},
        io, mem,
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        ptr,
    };

    use windows_sys::Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, LocalFree},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SDDL_REVISION_1,
            },
            GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
            TOKEN_USER, TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    pub struct PipeSecurity {
        sid: String,
        sddl: String,
        descriptor: PSECURITY_DESCRIPTOR,
    }

    impl PipeSecurity {
        pub fn current_user() -> io::Result<Self> {
            let sid = current_user_sid_string()?;
            let sddl = format!("D:P(A;;GA;;;{sid})");
            let wide = OsStr::new(&sddl)
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let mut descriptor = ptr::null_mut();
            // SAFETY: wide is terminated and descriptor is a valid out-pointer.
            let converted = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    ptr::null_mut(),
                )
            };
            if descriptor.is_null() {
                return Err(io::Error::last_os_error());
            }
            if converted == 0 {
                let error = io::Error::last_os_error();
                // SAFETY: even on failure, a non-null returned local allocation is owned here.
                unsafe {
                    LocalFree(descriptor.cast());
                }
                return Err(error);
            }
            Ok(Self {
                sid,
                sddl,
                descriptor,
            })
        }

        pub fn sid(&self) -> &str {
            &self.sid
        }

        pub fn sddl(&self) -> &str {
            &self.sddl
        }

        pub(crate) fn attributes(&self) -> SECURITY_ATTRIBUTES {
            SECURITY_ATTRIBUTES {
                nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.descriptor,
                bInheritHandle: 0,
            }
        }
    }

    impl Drop for PipeSecurity {
        fn drop(&mut self) {
            // SAFETY: descriptor is a non-null LocalAlloc allocation owned by self.
            unsafe {
                LocalFree(self.descriptor.cast());
            }
        }
    }

    pub fn current_user_sid_string() -> io::Result<String> {
        let mut raw_token = ptr::null_mut();
        // SAFETY: the process pseudo-handle is valid and raw_token is writable.
        let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) };
        if raw_token.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: OpenProcessToken returned a newly owned kernel handle.
        let token = unsafe { OwnedHandle::from_raw_handle(raw_token) };
        if opened == 0 {
            return Err(io::Error::last_os_error());
        }

        let mut required = 0;
        // SAFETY: a null buffer is the documented size query.
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                ptr::null_mut(),
                0,
                &mut required,
            );
        }
        if io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
            || required < mem::size_of::<TOKEN_USER>() as u32
        {
            return Err(io::Error::last_os_error());
        }

        let words = usize::try_from(required)
            .map_err(|_| io::Error::other("token information size is invalid"))?
            .div_ceil(mem::size_of::<usize>());
        let mut token_user = vec![0_usize; words];
        // SAFETY: token_user provides at least required writable bytes and token is live.
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                token_user.as_mut_ptr().cast::<c_void>(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful TokenUser query initialized TOKEN_USER at the buffer start.
        let sid = unsafe { (*(token_user.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        let mut string_sid = ptr::null_mut();
        // SAFETY: sid points into the live token buffer and string_sid is writable.
        let converted = unsafe { ConvertSidToStringSidW(sid, &mut string_sid) };
        if string_sid.is_null() {
            return Err(io::Error::last_os_error());
        }
        let string_sid = LocalWideString(string_sid);
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        // SAFETY: the API returned a terminated UTF-16 string.
        unsafe {
            while *string_sid.0.add(length) != 0 {
                length += 1;
            }
        }
        // SAFETY: length was found inside the terminated allocation.
        let value = unsafe { std::slice::from_raw_parts(string_sid.0, length) };
        String::from_utf16(value).map_err(|_| io::Error::other("current user SID is invalid"))
    }

    pub fn current_user_only_sddl() -> io::Result<String> {
        Ok(PipeSecurity::current_user()?.sddl().to_owned())
    }

    struct LocalWideString(*mut u16);

    impl Drop for LocalWideString {
        fn drop(&mut self) {
            // SAFETY: pointer was allocated by ConvertSidToStringSidW and is owned here.
            unsafe {
                LocalFree(self.0.cast());
            }
        }
    }
}

#[cfg(windows)]
pub use windows_impl::{PipeSecurity, current_user_only_sddl, current_user_sid_string};

#[cfg(not(windows))]
mod unsupported {
    use std::io;

    pub struct PipeSecurity;

    impl PipeSecurity {
        pub fn current_user() -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Windows pipe security is unavailable",
            ))
        }
    }

    pub fn current_user_sid_string() -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows token APIs are unavailable",
        ))
    }

    pub fn current_user_only_sddl() -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows security descriptors are unavailable",
        ))
    }
}

#[cfg(not(windows))]
pub use unsupported::{PipeSecurity, current_user_only_sddl, current_user_sid_string};
