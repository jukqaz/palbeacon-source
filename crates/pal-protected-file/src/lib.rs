use std::{
    fs::File,
    io::{Read, Take},
    path::Path,
};

use thiserror::Error;

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ProtectedFileError {
    #[error("protected file is unavailable")]
    Unavailable,
    #[error("protected file has an invalid type")]
    Invalid,
    #[error("protected file permissions are not owner-only")]
    InsecurePermissions,
    #[error("protected file exceeds its size limit")]
    TooLarge,
}

pub fn read_protected_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, ProtectedFileError> {
    read_protected_file_with_hook(path, max_bytes, || {})
}

/// Reads a bounded regular file without following a final link or reparse point.
///
/// Unlike [`read_protected_file`], this does not require owner-only permissions and is
/// suitable for public, integrity-bound assets such as a map BMP. On Windows every parent
/// directory is also opened without following a reparse point and held during the read.
pub fn read_regular_file_no_follow(
    path: &Path,
    max_bytes: usize,
) -> Result<Vec<u8>, ProtectedFileError> {
    read_regular_file_with_hook(path, max_bytes, || {})
}

fn read_protected_file_with_hook(
    path: &Path,
    max_bytes: usize,
    before_read: impl FnOnce(),
) -> Result<Vec<u8>, ProtectedFileError> {
    if max_bytes == 0 {
        return Err(ProtectedFileError::Invalid);
    }
    platform::read(path, max_bytes, true, before_read)
}

fn read_regular_file_with_hook(
    path: &Path,
    max_bytes: usize,
    before_read: impl FnOnce(),
) -> Result<Vec<u8>, ProtectedFileError> {
    if max_bytes == 0 {
        return Err(ProtectedFileError::Invalid);
    }
    platform::read(path, max_bytes, false, before_read)
}

fn read_bounded(
    file: File,
    max_bytes: usize,
    before_read: impl FnOnce(),
) -> Result<Vec<u8>, ProtectedFileError> {
    before_read();
    let limit = u64::try_from(max_bytes)
        .map_err(|_| ProtectedFileError::TooLarge)?
        .saturating_add(1);
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    let mut reader: Take<File> = file.take(limit);
    reader
        .read_to_end(&mut bytes)
        .map_err(|_| ProtectedFileError::Unavailable)?;
    if bytes.len() > max_bytes {
        return Err(ProtectedFileError::TooLarge);
    }
    Ok(bytes)
}

#[cfg(windows)]
mod platform {
    use std::{
        fs::{File, OpenOptions},
        io::ErrorKind,
        mem,
        os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        path::Path,
        ptr,
    };

    use windows_sys::Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, LocalFree, NO_ERROR},
        Security::{
            ACCESS_ALLOWED_ACE,
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
            DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetTokenInformation, IsWellKnownSid,
            OWNER_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER, TokenUser,
            WinBuiltinAdministratorsSid, WinLocalSystemSid,
        },
        Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        },
        System::{
            SystemServices::{
                ACCESS_ALLOWED_ACE_TYPE, ACCESS_ALLOWED_CALLBACK_ACE_TYPE,
                ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_ALLOWED_COMPOUND_ACE_TYPE,
                ACCESS_ALLOWED_OBJECT_ACE_TYPE,
            },
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };

    use super::{ProtectedFileError, read_bounded};

    pub(super) fn read(
        path: &Path,
        max_bytes: usize,
        require_owner_only: bool,
        before_read: impl FnOnce(),
    ) -> Result<Vec<u8>, ProtectedFileError> {
        let absolute = std::path::absolute(path).map_err(|_| ProtectedFileError::Unavailable)?;
        let _parents = hold_parent_chain(&absolute)?;
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&absolute)
            .map_err(|_| ProtectedFileError::Unavailable)?;
        let metadata = file
            .metadata()
            .map_err(|_| ProtectedFileError::Unavailable)?;
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(ProtectedFileError::Invalid);
        }
        if metadata.len() > max_bytes as u64 {
            return Err(ProtectedFileError::TooLarge);
        }
        if require_owner_only {
            validate_owner_and_dacl(&file)?;
        }
        read_bounded(file, max_bytes, before_read)
    }

    fn hold_parent_chain(path: &Path) -> Result<Vec<File>, ProtectedFileError> {
        let parent = path.parent().ok_or(ProtectedFileError::Invalid)?;
        let mut ancestors = parent.ancestors().collect::<Vec<_>>();
        ancestors.reverse();
        let mut held = Vec::with_capacity(ancestors.len());
        for ancestor in ancestors {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            let path_metadata =
                std::fs::symlink_metadata(ancestor).map_err(|_| ProtectedFileError::Unavailable)?;
            if !path_metadata.is_dir()
                || path_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(ProtectedFileError::Invalid);
            }
            let directory = match OpenOptions::new()
                // A directory handle used only for identity/type pinning needs no data access.
                // Requesting GENERIC_READ makes otherwise valid Steam libraries fail under common
                // inherited ACLs.
                .access_mode(0)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(ancestor)
            {
                Ok(directory) => directory,
                // Windows profile roots and some managed Steam-library parents can deny a
                // directory handle even though their descendants are readable. The no-follow
                // metadata check above still rejects junctions; retain handles wherever the ACL
                // permits instead of making every asset below such a root unusable.
                Err(error) if error.kind() == ErrorKind::PermissionDenied => continue,
                Err(_) => return Err(ProtectedFileError::Unavailable),
            };
            let metadata = directory
                .metadata()
                .map_err(|_| ProtectedFileError::Unavailable)?;
            if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(ProtectedFileError::Invalid);
            }
            held.push(directory);
        }
        Ok(held)
    }

    fn validate_owner_and_dacl(file: &File) -> Result<(), ProtectedFileError> {
        let token_user = current_token_user()?;
        // SAFETY: current_token_user returns an aligned TOKEN_USER buffer.
        let current_sid = unsafe { (*(token_user.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        let mut owner = ptr::null_mut();
        let mut dacl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        // SAFETY: the file handle is live and all requested out-pointers are valid.
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != NO_ERROR || descriptor.is_null() {
            if !descriptor.is_null() {
                // SAFETY: a non-null descriptor returned by GetSecurityInfo is locally allocated.
                unsafe {
                    LocalFree(descriptor.cast());
                }
            }
            return Err(ProtectedFileError::Unavailable);
        }
        let result = (|| {
            if owner.is_null() {
                return Err(ProtectedFileError::Unavailable);
            }
            if dacl.is_null() || unsafe { EqualSid(owner, current_sid) } == 0 {
                return Err(ProtectedFileError::InsecurePermissions);
            }
            // SAFETY: dacl is non-null and points inside the live security descriptor.
            let count = unsafe { (*dacl).AceCount };
            let mut current_user_allowed = false;
            for index in 0..u32::from(count) {
                let mut raw_ace = ptr::null_mut();
                // SAFETY: index is bounded by AceCount and raw_ace is a valid out-pointer.
                if unsafe { GetAce(dacl, index, &mut raw_ace) } == 0 || raw_ace.is_null() {
                    return Err(ProtectedFileError::Unavailable);
                }
                // SAFETY: every ACE begins with the header embedded in ACCESS_ALLOWED_ACE.
                let ace = unsafe { &*raw_ace.cast::<ACCESS_ALLOWED_ACE>() };
                let ace_type = u32::from(ace.Header.AceType);
                if [
                    ACCESS_ALLOWED_CALLBACK_ACE_TYPE,
                    ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE,
                    ACCESS_ALLOWED_COMPOUND_ACE_TYPE,
                    ACCESS_ALLOWED_OBJECT_ACE_TYPE,
                ]
                .contains(&ace_type)
                {
                    return Err(ProtectedFileError::InsecurePermissions);
                }
                if ace_type != ACCESS_ALLOWED_ACE_TYPE {
                    continue;
                }
                let sid = ptr::addr_of!(ace.SidStart).cast_mut().cast();
                // SAFETY: a simple ACCESS_ALLOWED_ACE stores its SID at SidStart.
                let is_current_user = unsafe { EqualSid(sid, current_sid) } != 0;
                // SAFETY: sid points to the SID embedded in the live ACE.
                let is_system = unsafe { IsWellKnownSid(sid, WinLocalSystemSid) } != 0;
                // SAFETY: sid points to the SID embedded in the live ACE.
                let is_admin = unsafe { IsWellKnownSid(sid, WinBuiltinAdministratorsSid) } != 0;
                if !(is_current_user || is_system || is_admin) {
                    return Err(ProtectedFileError::InsecurePermissions);
                }
                current_user_allowed |= is_current_user;
            }
            if !current_user_allowed {
                return Err(ProtectedFileError::InsecurePermissions);
            }
            Ok(())
        })();
        // SAFETY: descriptor was allocated by GetSecurityInfo and is freed exactly once.
        unsafe {
            LocalFree(descriptor.cast());
        }
        result
    }

    fn current_token_user() -> Result<Vec<usize>, ProtectedFileError> {
        let mut raw_token = ptr::null_mut();
        // SAFETY: GetCurrentProcess returns a valid pseudo-handle and raw_token is writable.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) } == 0
            || raw_token.is_null()
        {
            return Err(ProtectedFileError::Unavailable);
        }
        // SAFETY: OpenProcessToken returned a newly owned, non-null kernel handle.
        let token = unsafe { OwnedHandle::from_raw_handle(raw_token) };
        let mut required = 0_u32;
        // SAFETY: a null information buffer is the documented size query.
        let loaded = unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                ptr::null_mut(),
                0,
                &mut required,
            )
        };
        if loaded != 0
            || std::io::Error::last_os_error().raw_os_error()
                != Some(ERROR_INSUFFICIENT_BUFFER as i32)
            || required < mem::size_of::<TOKEN_USER>() as u32
        {
            return Err(ProtectedFileError::Unavailable);
        }
        let words = usize::try_from(required)
            .map_err(|_| ProtectedFileError::Unavailable)?
            .div_ceil(mem::size_of::<usize>());
        let mut buffer = vec![0_usize; words];
        // SAFETY: buffer has at least required writable bytes and the token is live.
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                buffer.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(ProtectedFileError::Unavailable);
        }
        Ok(buffer)
    }

    #[cfg(test)]
    mod tests {
        use std::{cell::Cell, fs, process::Command};

        use tempfile::TempDir;

        use super::super::{
            ProtectedFileError, read_protected_file, read_protected_file_with_hook,
            read_regular_file_no_follow, read_regular_file_with_hook,
        };

        #[test]
        fn replacement_is_blocked_while_verified_handle_is_read() {
            let temp = TempDir::new().unwrap();
            let path = temp.path().join("protected.txt");
            fs::write(&path, b"verified").unwrap();
            let identity = Command::new("whoami").output().unwrap();
            let identity = String::from_utf8(identity.stdout).unwrap();
            let owner_status = Command::new("icacls")
                .arg(&path)
                .args(["/setowner", identity.trim()])
                .status()
                .unwrap();
            assert!(owner_status.success());
            let grant = format!("{}:(F)", identity.trim());
            let status = Command::new("icacls")
                .arg(&path)
                .args(["/inheritance:r", "/grant:r"])
                .arg(grant)
                .status()
                .unwrap();
            assert!(status.success());

            let removal_blocked = Cell::new(false);
            let bytes = read_protected_file_with_hook(&path, 1024, || {
                removal_blocked.set(fs::remove_file(&path).is_err());
            })
            .unwrap();

            assert!(removal_blocked.get());
            assert_eq!(bytes, b"verified");
        }

        #[test]
        fn regular_reader_does_not_require_private_acl() {
            let temp = TempDir::new().unwrap();
            let path = temp.path().join("public-map.bmp");
            fs::write(&path, b"map").unwrap();

            let bytes = read_regular_file_with_hook(&path, 1024, || {}).unwrap();

            assert_eq!(bytes, b"map");
        }

        #[test]
        fn regular_reader_rejects_a_final_reparse_point() {
            let temp = TempDir::new().unwrap();
            let target = temp.path().join("map-target.bmp");
            let linked = temp.path().join("map-link.bmp");
            fs::write(&target, b"map").unwrap();
            if std::os::windows::fs::symlink_file(&target, &linked).is_err() {
                eprintln!(
                    "SKIP final-file reparse assertion: Windows symbolic-link privilege is unavailable"
                );
                return;
            }

            assert_eq!(
                read_regular_file_with_hook(&linked, 1024, || {}).unwrap_err(),
                super::super::ProtectedFileError::Invalid
            );
        }

        #[test]
        fn directory_and_oversized_inputs_are_refused() {
            let temp = TempDir::new().unwrap();
            assert!(matches!(
                read_regular_file_no_follow(temp.path(), 1024),
                Err(ProtectedFileError::Invalid | ProtectedFileError::Unavailable)
            ));

            let oversized = temp.path().join("oversized.bin");
            fs::write(&oversized, vec![0_u8; 1025]).unwrap();
            assert_eq!(
                read_regular_file_no_follow(&oversized, 1024).unwrap_err(),
                ProtectedFileError::TooLarge
            );
        }

        #[test]
        fn a_parent_directory_reparse_point_is_refused() {
            let temp = TempDir::new().unwrap();
            let real_directory = temp.path().join("real-directory");
            fs::create_dir(&real_directory).unwrap();
            fs::write(real_directory.join("inside.txt"), b"secret").unwrap();
            let directory_link = temp.path().join("linked-directory");
            let status = Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&directory_link)
                .arg(&real_directory)
                .status()
                .unwrap();
            assert!(status.success(), "junction fixture must be available");

            assert!(matches!(
                read_regular_file_no_follow(&directory_link.join("inside.txt"), 1024),
                Err(ProtectedFileError::Invalid | ProtectedFileError::Unavailable)
            ));
        }

        #[test]
        fn an_everyone_allow_ace_is_not_owner_only() {
            let temp = TempDir::new().unwrap();
            let path = temp.path().join("shared.txt");
            fs::write(&path, b"secret").unwrap();
            let status = Command::new("icacls")
                .arg(&path)
                .args(["/grant", "*S-1-1-0:(R)"])
                .status()
                .unwrap();
            assert!(status.success());

            assert_eq!(
                read_protected_file(&path, 1024).unwrap_err(),
                ProtectedFileError::InsecurePermissions
            );
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::{
        fs::OpenOptions,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
        path::Path,
    };

    use super::{ProtectedFileError, read_bounded};

    pub(super) fn read(
        path: &Path,
        max_bytes: usize,
        require_owner_only: bool,
        before_read: impl FnOnce(),
    ) -> Result<Vec<u8>, ProtectedFileError> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|_| ProtectedFileError::Unavailable)?;
        let metadata = file
            .metadata()
            .map_err(|_| ProtectedFileError::Unavailable)?;
        if !metadata.is_file() {
            return Err(ProtectedFileError::Invalid);
        }
        if metadata.len() > max_bytes as u64 {
            return Err(ProtectedFileError::TooLarge);
        }
        if require_owner_only && metadata.mode() & 0o077 != 0 {
            return Err(ProtectedFileError::InsecurePermissions);
        }
        read_bounded(file, max_bytes, before_read)
    }
}
