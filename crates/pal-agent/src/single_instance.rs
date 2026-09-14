use sha2::{Digest, Sha256};
use thiserror::Error;

pub struct SingleInstanceGuard {
    _guard: platform::Guard,
}

impl SingleInstanceGuard {
    pub fn acquire(world_alias: &str, subject_id: &[u8]) -> Result<Self, SingleInstanceError> {
        if world_alias.is_empty()
            || world_alias.len() > 64
            || !world_alias.is_ascii()
            || subject_id.len() != 32
        {
            return Err(SingleInstanceError::InvalidIdentity);
        }
        let name = lock_name(world_alias, subject_id);
        Ok(Self {
            _guard: platform::acquire(&name)?,
        })
    }
}

fn lock_name(world_alias: &str, subject_id: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"pal-companion/agent-single-instance/v2");
    hasher.update((world_alias.len() as u64).to_le_bytes());
    hasher.update(world_alias.as_bytes());
    hasher.update(subject_id);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(windows)]
mod platform {
    use std::{
        mem,
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        ptr,
    };

    use windows_sys::Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER, GetLastError, LocalFree},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SDDL_REVISION_1,
            },
            GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        System::Threading::{CreateMutexW, GetCurrentProcess, OpenProcessToken},
    };

    use super::SingleInstanceError;

    pub(super) struct Guard {
        _mutex: OwnedHandle,
    }

    pub(super) fn acquire(name: &str) -> Result<Guard, SingleInstanceError> {
        let current_sid = current_user_sid_string()?;
        let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{current_sid})");
        let mut sddl_wide = std::ffi::OsStr::new(&sddl)
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut descriptor = ptr::null_mut();
        // SAFETY: sddl_wide is terminated and descriptor is a valid out-pointer.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_wide.as_mut_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
            || descriptor.is_null()
        {
            return Err(SingleInstanceError::Unavailable);
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let full_name = format!("Global\\PalCompanion.Agent.{name}");
        let wide_name = std::ffi::OsStr::new(&full_name)
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: attributes and its descriptor remain live, and wide_name is terminated.
        let raw_mutex = unsafe { CreateMutexW(&attributes, 0, wide_name.as_ptr()) };
        // SAFETY: GetLastError must be sampled immediately after CreateMutexW.
        let create_error = unsafe { GetLastError() };
        // SAFETY: descriptor was allocated by the SDDL conversion and is freed once.
        unsafe {
            LocalFree(descriptor);
        }
        if raw_mutex.is_null() {
            return Err(SingleInstanceError::Unavailable);
        }
        // SAFETY: CreateMutexW returned a newly owned, non-null kernel handle.
        let mutex = unsafe { OwnedHandle::from_raw_handle(raw_mutex) };
        if create_error == ERROR_ALREADY_EXISTS {
            return Err(SingleInstanceError::AlreadyRunning);
        }
        Ok(Guard { _mutex: mutex })
    }

    fn current_user_sid_string() -> Result<String, SingleInstanceError> {
        let mut raw_token = ptr::null_mut();
        // SAFETY: GetCurrentProcess is a valid pseudo-handle and raw_token is writable.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) } == 0
            || raw_token.is_null()
        {
            return Err(SingleInstanceError::Unavailable);
        }
        // SAFETY: OpenProcessToken returned a newly owned, non-null handle.
        let token = unsafe { OwnedHandle::from_raw_handle(raw_token) };
        let mut required = 0_u32;
        // SAFETY: null buffer is the documented size query.
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                ptr::null_mut(),
                0,
                &mut required,
            );
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
            || required < mem::size_of::<TOKEN_USER>() as u32
        {
            return Err(SingleInstanceError::Unavailable);
        }
        let words = usize::try_from(required)
            .map_err(|_| SingleInstanceError::Unavailable)?
            .div_ceil(mem::size_of::<usize>());
        let mut token_user = vec![0_usize; words];
        // SAFETY: token_user has at least required bytes and token is live.
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                token_user.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(SingleInstanceError::Unavailable);
        }
        // SAFETY: successful TokenUser query initialized a TOKEN_USER at buffer start.
        let sid = unsafe { (*(token_user.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        let mut string_sid = ptr::null_mut();
        // SAFETY: sid points into the live token_user buffer and string_sid is writable.
        if unsafe { ConvertSidToStringSidW(sid, &mut string_sid) } == 0 || string_sid.is_null() {
            return Err(SingleInstanceError::Unavailable);
        }
        let mut length = 0;
        // SAFETY: ConvertSidToStringSidW returns a terminated UTF-16 string.
        unsafe {
            while *string_sid.add(length) != 0 {
                length += 1;
            }
        }
        // SAFETY: length was found by scanning the terminated allocation.
        let sid_slice = unsafe { std::slice::from_raw_parts(string_sid, length) };
        let result = String::from_utf16(sid_slice).map_err(|_| SingleInstanceError::Unavailable);
        // SAFETY: string_sid was allocated by ConvertSidToStringSidW and is freed once.
        unsafe {
            LocalFree(string_sid.cast());
        }
        result
    }
}

#[cfg(not(windows))]
mod platform {
    use std::{
        fs::{File, OpenOptions},
        path::PathBuf,
    };

    use fs2::FileExt;

    use super::SingleInstanceError;

    pub(super) struct Guard {
        file: File,
        _path: PathBuf,
    }

    pub(super) fn acquire(name: &str) -> Result<Guard, SingleInstanceError> {
        let path = std::env::temp_dir().join(format!("pal-companion-agent-{name}.lock"));
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|_| SingleInstanceError::Unavailable)?;
        FileExt::try_lock_exclusive(&file).map_err(|_| SingleInstanceError::AlreadyRunning)?;
        Ok(Guard { file, _path: path })
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = FileExt::unlock(&self.file);
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SingleInstanceError {
    #[error("single-instance identity is invalid")]
    InvalidIdentity,
    #[error("another agent instance already holds this world binding")]
    AlreadyRunning,
    #[error("single-instance mutex is unavailable")]
    Unavailable,
}
