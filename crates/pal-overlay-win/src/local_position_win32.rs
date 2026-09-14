//! Windows binding for the exact-build, read-only local position source.
//!
//! This module owns no renderer or visibility policy. It only binds an already verified Steam
//! build and tracked window to the read-only process-memory boundary.

use std::{
    fmt,
    fs::{File, OpenOptions},
    io::Read,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use pal_windows::{QueryOnlyProcessGuard, ReadOnlyProcess, TrackedWindow};
use sha2::{Digest, Sha256};
use thiserror::Error;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, GetFileInformationByHandle,
};

use crate::{
    client_build_binding::VerifiedSteamClientBuild,
    local_position_source::{
        LocalMemoryReadError, LocalPositionMemoryReader, LocalPositionReadSession,
        LocalProcessIdentity, WINDOWS_LIVE_POSITION_PROFILE,
    },
};

const HASH_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum Win32LocalPositionReaderError {
    #[error("the tracked window and verified Steam build do not match")]
    BuildBindingMismatch,
    #[error("the selected process is unavailable")]
    ProcessUnavailable,
    #[error("the exact local position image is unavailable")]
    ImageUnavailable,
    #[error("the exact local position image does not match the supported build")]
    ExactImageMismatch,
    #[error("the exact local position reader was cancelled")]
    Cancelled,
    #[error("the loaded module does not match the supported build")]
    ModuleMismatch,
}

/// Read-only Win32 implementation of [`LocalPositionMemoryReader`].
///
/// The executable handle remains open for the reader lifetime. The cached digest is accepted only
/// after a no-follow streaming read and process identity validation on both sides of that read.
pub struct Win32LocalPositionMemoryReader {
    process: ReadOnlyProcess,
    image: File,
    image_stamp: ImageStamp,
    identity: LocalProcessIdentity,
}

impl fmt::Debug for Win32LocalPositionMemoryReader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED WIN32 LOCAL POSITION MEMORY READER]")
    }
}

/// Binds one tracked Palworld window to the immutable current-build memory profile.
///
/// This function performs no offset discovery and enables no overlay or map visibility. The
/// caller must first obtain `verified_build` from the Steam client-build verifier.
pub fn open_windows_live_position_reader(
    window: &TrackedWindow,
    verified_build: &VerifiedSteamClientBuild,
    cancelled: &AtomicBool,
) -> Result<Win32LocalPositionMemoryReader, Win32LocalPositionReaderError> {
    check_cancelled(cancelled)?;
    let profile = WINDOWS_LIVE_POSITION_PROFILE;
    if verified_build.build_id() != profile.build_id() || !verified_build.matches(window) {
        return Err(Win32LocalPositionReaderError::BuildBindingMismatch);
    }

    let process_guard = QueryOnlyProcessGuard::open_tracked(window)
        .map_err(|_| Win32LocalPositionReaderError::ProcessUnavailable)?;
    process_guard
        .revalidate()
        .map_err(|_| Win32LocalPositionReaderError::ProcessUnavailable)?;

    let verified_image = open_and_hash_exact_image(
        Path::new(window.image_path()),
        profile.executable_file_size(),
        cancelled,
    )?;
    require_exact_sha256(verified_image.sha256, profile.executable_sha256())?;
    check_cancelled(cancelled)?;

    let process = process_guard
        .open_read_only_tracked(window)
        .map_err(|_| Win32LocalPositionReaderError::ProcessUnavailable)?;
    process
        .revalidate()
        .map_err(|_| Win32LocalPositionReaderError::ProcessUnavailable)?;
    if !verified_build.matches(window) {
        return Err(Win32LocalPositionReaderError::BuildBindingMismatch);
    }
    let module = process.main_module();
    if module.size() != profile.loaded_module_size()
        || module.base_address() == 0
        || module.base_address().checked_add(module.size()).is_none()
    {
        return Err(Win32LocalPositionReaderError::ModuleMismatch);
    }

    let identity = LocalProcessIdentity::new(
        profile.build_id(),
        verified_image.sha256,
        verified_image.stamp.length,
        module.base_address(),
        module.size(),
    );
    Ok(Win32LocalPositionMemoryReader {
        process,
        image: verified_image.file,
        image_stamp: verified_image.stamp,
        identity,
    })
}

/// Compatibility entry point for callers compiled against the first exact-build reader API.
pub fn open_build_24181527_local_reader(
    window: &TrackedWindow,
    verified_build: &VerifiedSteamClientBuild,
    cancelled: &AtomicBool,
) -> Result<Win32LocalPositionMemoryReader, Win32LocalPositionReaderError> {
    open_windows_live_position_reader(window, verified_build, cancelled)
}

impl LocalPositionMemoryReader for Win32LocalPositionMemoryReader {
    fn identity(&mut self) -> Result<LocalProcessIdentity, LocalMemoryReadError> {
        let current_stamp =
            image_stamp(&self.image).map_err(|_| LocalMemoryReadError::Unavailable)?;
        if current_stamp != self.image_stamp
            || self.process.main_module().base_address() != self.identity.module_base()
            || self.process.main_module().size() != self.identity.module_size()
        {
            return Err(LocalMemoryReadError::Unavailable);
        }
        Ok(self.identity)
    }

    fn with_read_session<T>(
        &mut self,
        operation: impl FnOnce(&mut dyn LocalPositionReadSession) -> T,
    ) -> Result<T, LocalMemoryReadError> {
        self.process
            .with_read_session(|session| {
                let mut adapter = Win32LocalPositionReadSession { session };
                Ok(operation(&mut adapter))
            })
            .map_err(|_| LocalMemoryReadError::Unavailable)
    }
}

struct Win32LocalPositionReadSession<'session, 'process> {
    session: &'session pal_windows::ReadOnlyProcessSession<'process>,
}

impl LocalPositionReadSession for Win32LocalPositionReadSession<'_, '_> {
    fn read_exact(
        &mut self,
        address: usize,
        output: &mut [u8],
    ) -> Result<(), LocalMemoryReadError> {
        self.session
            .read_exact_at(address, output)
            .map_err(|_| LocalMemoryReadError::Unavailable)
    }
}

struct VerifiedImage {
    file: File,
    stamp: ImageStamp,
    sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ImageStamp {
    length: u64,
    attributes: u32,
    creation_time: u64,
    last_write_time: u64,
    volume_serial_number: u32,
    file_index: u64,
}

#[cfg(test)]
fn stream_exact_image_sha256(
    path: &Path,
    expected_size: u64,
    cancelled: &AtomicBool,
) -> Result<[u8; 32], Win32LocalPositionReaderError> {
    open_and_hash_exact_image(path, expected_size, cancelled).map(|verified| verified.sha256)
}

fn require_exact_sha256(
    actual: [u8; 32],
    expected: [u8; 32],
) -> Result<(), Win32LocalPositionReaderError> {
    if actual == expected {
        Ok(())
    } else {
        Err(Win32LocalPositionReaderError::ExactImageMismatch)
    }
}

fn open_and_hash_exact_image(
    path: &Path,
    expected_size: u64,
    cancelled: &AtomicBool,
) -> Result<VerifiedImage, Win32LocalPositionReaderError> {
    check_cancelled(cancelled)?;
    if expected_size == 0 || !path.is_absolute() {
        return Err(Win32LocalPositionReaderError::ImageUnavailable);
    }
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| Win32LocalPositionReaderError::ImageUnavailable)?;
    let before = image_stamp(&file)?;
    if before.length != expected_size {
        return Err(Win32LocalPositionReaderError::ExactImageMismatch);
    }

    let mut reader = file
        .try_clone()
        .map_err(|_| Win32LocalPositionReaderError::ImageUnavailable)?;
    let sha256 = stream_exact_image_reader_sha256(&mut reader, expected_size, cancelled)?;
    if image_stamp(&file)? != before {
        return Err(Win32LocalPositionReaderError::ExactImageMismatch);
    }

    Ok(VerifiedImage {
        file,
        stamp: before,
        sha256,
    })
}

fn stream_exact_image_reader_sha256<R: Read>(
    reader: &mut R,
    expected_size: u64,
    cancelled: &AtomicBool,
) -> Result<[u8; 32], Win32LocalPositionReaderError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; HASH_BUFFER_BYTES];
    let mut remaining = expected_size;
    while remaining != 0 {
        check_cancelled(cancelled)?;
        let requested = usize::try_from(remaining.min(HASH_BUFFER_BYTES as u64))
            .map_err(|_| Win32LocalPositionReaderError::ImageUnavailable)?;
        let count = reader
            .read(&mut buffer[..requested])
            .map_err(|_| Win32LocalPositionReaderError::ImageUnavailable)?;
        if count == 0 {
            buffer.fill(0);
            return Err(Win32LocalPositionReaderError::ExactImageMismatch);
        }
        hasher.update(&buffer[..count]);
        remaining -=
            u64::try_from(count).map_err(|_| Win32LocalPositionReaderError::ImageUnavailable)?;
    }
    check_cancelled(cancelled)?;
    let mut trailing = [0_u8; 1];
    let trailing_count = reader
        .read(&mut trailing)
        .map_err(|_| Win32LocalPositionReaderError::ImageUnavailable)?;
    buffer.fill(0);
    trailing.fill(0);
    if trailing_count != 0 {
        return Err(Win32LocalPositionReaderError::ExactImageMismatch);
    }
    Ok(hasher.finalize().into())
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), Win32LocalPositionReaderError> {
    if cancelled.load(Ordering::Acquire) {
        Err(Win32LocalPositionReaderError::Cancelled)
    } else {
        Ok(())
    }
}

fn image_stamp(file: &File) -> Result<ImageStamp, Win32LocalPositionReaderError> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: file owns a live read-only handle and information is writable for its full size.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
        return Err(Win32LocalPositionReaderError::ImageUnavailable);
    }
    let attributes = information.dwFileAttributes;
    if attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
        return Err(Win32LocalPositionReaderError::ImageUnavailable);
    }
    Ok(ImageStamp {
        length: (u64::from(information.nFileSizeHigh) << 32) | u64::from(information.nFileSizeLow),
        attributes,
        creation_time: (u64::from(information.ftCreationTime.dwHighDateTime) << 32)
            | u64::from(information.ftCreationTime.dwLowDateTime),
        last_write_time: (u64::from(information.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(information.ftLastWriteTime.dwLowDateTime),
        volume_serial_number: information.dwVolumeSerialNumber,
        file_index: (u64::from(information.nFileIndexHigh) << 32)
            | u64::from(information.nFileIndexLow),
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::atomic::AtomicBool};

    use tempfile::tempdir_in;

    use super::{
        Win32LocalPositionReaderError, require_exact_sha256, stream_exact_image_reader_sha256,
        stream_exact_image_sha256,
    };

    #[test]
    fn exact_fixture_is_streamed_and_hashed_without_a_whole_file_buffer() {
        let temp = tempdir_in(".").unwrap();
        let fixture = temp.path().join("fixture.exe");
        fs::write(&fixture, b"palworld exact image fixture").unwrap();
        let cancelled = AtomicBool::new(false);

        assert_eq!(
            stream_exact_image_sha256(&fixture, 28, &cancelled).unwrap(),
            [
                0x3a, 0xaa, 0xa1, 0x77, 0x11, 0xde, 0x6b, 0x11, 0x12, 0xb8, 0x22, 0x39, 0xf1, 0xc9,
                0x36, 0xc6, 0xb9, 0xc1, 0xe9, 0x0d, 0xd8, 0x10, 0x03, 0x63, 0x44, 0xdd, 0x32, 0x20,
                0x47, 0xa1, 0x10, 0x63,
            ]
        );
    }

    #[test]
    fn size_mismatch_fails_closed_before_accepting_a_digest() {
        let temp = tempdir_in(".").unwrap();
        let fixture = temp.path().join("fixture.exe");
        fs::write(&fixture, b"short").unwrap();
        let cancelled = AtomicBool::new(false);

        assert_eq!(
            stream_exact_image_sha256(&fixture, 6, &cancelled),
            Err(Win32LocalPositionReaderError::ExactImageMismatch)
        );
    }

    #[test]
    fn cancelled_exact_image_hash_stops_before_reading_the_next_64kib_chunk() {
        let cancelled = AtomicBool::new(true);
        let mut reader = CancellingReader::new(&cancelled);

        assert_eq!(
            stream_exact_image_reader_sha256(
                &mut reader,
                (super::HASH_BUFFER_BYTES * 2) as u64,
                &cancelled,
            ),
            Err(Win32LocalPositionReaderError::Cancelled)
        );
        assert_eq!(reader.reads, 1);
    }

    struct CancellingReader<'a> {
        cancelled: &'a AtomicBool,
        reads: usize,
    }

    impl<'a> CancellingReader<'a> {
        fn new(cancelled: &'a AtomicBool) -> Self {
            cancelled.store(false, std::sync::atomic::Ordering::Release);
            Self {
                cancelled,
                reads: 0,
            }
        }
    }

    impl std::io::Read for CancellingReader<'_> {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            self.reads += 1;
            output.fill(0x5a);
            self.cancelled
                .store(true, std::sync::atomic::Ordering::Release);
            Ok(output.len())
        }
    }

    #[test]
    fn digest_mismatch_is_rejected_by_the_exact_profile_check() {
        let actual = [0x11; 32];
        let expected = [0x22; 32];
        assert_eq!(
            require_exact_sha256(actual, expected),
            Err(Win32LocalPositionReaderError::ExactImageMismatch)
        );
        assert_eq!(require_exact_sha256(expected, expected), Ok(()));
    }
}
