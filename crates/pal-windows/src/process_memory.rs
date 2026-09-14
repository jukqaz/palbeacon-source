use std::{
    ffi::OsString,
    fmt, mem,
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
};

use windows_sys::Win32::{
    Foundation::{
        ERROR_BAD_LENGTH, ERROR_NO_MORE_FILES, FILETIME, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE, STILL_ACTIVE,
    },
    System::{
        Diagnostics::{
            Debug::ReadProcessMemory,
            ToolHelp::{
                CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
                TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
            },
        },
        Memory::{
            MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
            PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
            PAGE_WRITECOPY, VirtualQueryEx,
        },
        Threading::{
            GetExitCodeProcess, GetProcessId, GetProcessTimes, OpenProcess,
            PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
            QueryFullProcessImageNameW,
        },
    },
};

use crate::TrackedWindow;

/// A deliberately small ceiling for one exact process-memory read.
pub const MAX_EXACT_READ_BYTES: usize = 1024 * 1024;

const PROCESS_ACCESS: u32 = PROCESS_QUERY_INFORMATION | PROCESS_VM_READ;
const QUERY_ONLY_PROCESS_ACCESS: u32 = PROCESS_QUERY_LIMITED_INFORMATION;
const SNAPSHOT_FLAGS: u32 = TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32;
const SNAPSHOT_RETRY_LIMIT: usize = 3;
const PROCESS_IMAGE_BUFFER_LEN: usize = 32_768;
const BASIC_PROTECTION_MASK: u32 = 0xff;

/// A read-only handle to the exact process identity selected by the window tracker.
///
/// This boundary does not discover offsets, interpret game data, or expose raw identity details in
/// diagnostics. It is feature-gated so release/default builds do not make process-memory reads.
pub struct ReadOnlyProcess {
    process: OwnedHandle,
    process_id: u32,
    expected_image_path: PathBuf,
    creation_time_100ns: u64,
    main_module: ReadOnlyProcessModule,
}

/// Holds the exact tracked process identity without granting process-memory access.
///
/// Keep this guard alive while validating a large executable image, then use it to open the
/// process-memory boundary. The creation time comparison prevents a recycled PID from being
/// accepted after the image hash completes.
pub struct QueryOnlyProcessGuard {
    process: OwnedHandle,
    process_id: u32,
    expected_image_path: PathBuf,
    creation_time_100ns: u64,
}

/// A short-lived read session bracketed by exact process-identity validation.
///
/// The session keeps repeated pointer-chain reads from repeating expensive image and creation-time
/// queries for every individual word. It is not a frozen memory snapshot: callers that follow
/// mutable pointers must still perform their own before/after consistency checks.
pub struct ReadOnlyProcessSession<'process> {
    process: &'process ReadOnlyProcess,
}

impl fmt::Debug for ReadOnlyProcessSession<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED READ-ONLY PROCESS SESSION]")
    }
}

impl fmt::Debug for ReadOnlyProcess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED READ-ONLY PROCESS]")
    }
}

impl fmt::Debug for QueryOnlyProcessGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED QUERY-ONLY PROCESS GUARD]")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ReadOnlyProcessModule {
    base_address: usize,
    size: usize,
}

impl ReadOnlyProcessModule {
    pub const fn base_address(self) -> usize {
        self.base_address
    }

    pub const fn size(self) -> usize {
        self.size
    }
}

impl fmt::Debug for ReadOnlyProcessModule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED READ-ONLY PROCESS MODULE]")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOnlyProcessError {
    InvalidTarget,
    ProcessUnavailable,
    IdentityMismatch,
    ProcessExited,
    ModuleUnavailable,
    InvalidModule,
    InvalidRead,
    UnreadableMemory,
    ReadFailed,
}

impl fmt::Display for ReadOnlyProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidTarget => "the selected process target is invalid",
            Self::ProcessUnavailable => "the selected process is unavailable",
            Self::IdentityMismatch => "the selected process identity changed",
            Self::ProcessExited => "the selected process exited",
            Self::ModuleUnavailable => "the selected process module is unavailable",
            Self::InvalidModule => "the selected process module is invalid",
            Self::InvalidRead => "the requested process-memory read is invalid",
            Self::UnreadableMemory => "the requested process-memory range is not readable",
            Self::ReadFailed => "the exact process-memory read failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ReadOnlyProcessError {}

impl ReadOnlyProcess {
    pub fn open_tracked(tracked: &TrackedWindow) -> Result<Self, ReadOnlyProcessError> {
        let process_id = tracked.snapshot().process_id();
        let expected_image_path = Path::new(tracked.image_path());
        Self::open_identity(process_id, expected_image_path)
    }

    pub const fn main_module(&self) -> ReadOnlyProcessModule {
        self.main_module
    }

    pub fn revalidate(&self) -> Result<(), ReadOnlyProcessError> {
        let handle = self.handle();
        // SAFETY: handle is owned by self and supports process queries.
        let observed_process_id = unsafe { GetProcessId(handle) };
        if observed_process_id == 0 {
            return Err(ReadOnlyProcessError::ProcessUnavailable);
        }
        if observed_process_id != self.process_id {
            return Err(ReadOnlyProcessError::IdentityMismatch);
        }
        ensure_process_alive(handle)?;
        if process_creation_time(handle)? != self.creation_time_100ns {
            return Err(ReadOnlyProcessError::IdentityMismatch);
        }
        if !paths_equal_case_insensitive(&process_image_path(handle)?, &self.expected_image_path) {
            return Err(ReadOnlyProcessError::IdentityMismatch);
        }
        Ok(())
    }

    pub fn read_exact_at(
        &self,
        address: usize,
        output: &mut [u8],
    ) -> Result<(), ReadOnlyProcessError> {
        self.with_read_session(|session| session.read_exact_at(address, output))
    }

    pub fn with_read_session<T>(
        &self,
        operation: impl FnOnce(&ReadOnlyProcessSession<'_>) -> Result<T, ReadOnlyProcessError>,
    ) -> Result<T, ReadOnlyProcessError> {
        self.revalidate()?;
        let outcome = operation(&ReadOnlyProcessSession { process: self });
        let identity_outcome = self.revalidate();
        match (outcome, identity_outcome) {
            (Ok(value), Ok(())) => Ok(value),
            (_, Err(identity_error)) => Err(identity_error),
            (Err(operation_error), Ok(())) => Err(operation_error),
        }
    }

    pub fn read_module_exact(
        &self,
        relative_address: usize,
        output: &mut [u8],
    ) -> Result<(), ReadOnlyProcessError> {
        if output.is_empty() || output.len() > MAX_EXACT_READ_BYTES {
            return Err(ReadOnlyProcessError::InvalidRead);
        }
        let relative_end = relative_address
            .checked_add(output.len())
            .ok_or(ReadOnlyProcessError::InvalidRead)?;
        if relative_end > self.main_module.size {
            return Err(ReadOnlyProcessError::InvalidRead);
        }
        let address = self
            .main_module
            .base_address
            .checked_add(relative_address)
            .ok_or(ReadOnlyProcessError::InvalidRead)?;
        self.read_exact_at(address, output)
    }

    fn open_identity(
        process_id: u32,
        expected_image_path: &Path,
    ) -> Result<Self, ReadOnlyProcessError> {
        if process_id == 0 || !expected_image_path.is_absolute() {
            return Err(ReadOnlyProcessError::InvalidTarget);
        }

        // SAFETY: the PID is inert and the access mask grants only query/read rights. Handle
        // inheritance is explicitly disabled.
        let raw_process = unsafe { OpenProcess(PROCESS_ACCESS, 0, process_id) };
        if raw_process.is_null() {
            return Err(ReadOnlyProcessError::ProcessUnavailable);
        }
        // SAFETY: OpenProcess returned a newly owned, non-null kernel handle.
        let process = unsafe { OwnedHandle::from_raw_handle(raw_process) };
        let creation_time_100ns = process_creation_time(raw_process)?;
        ensure_process_alive(raw_process)?;
        let observed_image_path = process_image_path(raw_process)?;
        if !paths_equal_case_insensitive(&observed_image_path, expected_image_path) {
            return Err(ReadOnlyProcessError::IdentityMismatch);
        }
        let main_module = exact_main_module(process_id, expected_image_path)?;

        let identity = Self {
            process,
            process_id,
            expected_image_path: expected_image_path.to_path_buf(),
            creation_time_100ns,
            main_module,
        };
        identity.revalidate()?;
        Ok(identity)
    }

    fn handle(&self) -> HANDLE {
        self.process.as_raw_handle()
    }

    #[cfg(test)]
    fn raw_handle_for_test(&self) -> HANDLE {
        self.handle()
    }

    #[cfg(test)]
    const fn process_id_for_test(&self) -> u32 {
        self.process_id
    }
}

impl QueryOnlyProcessGuard {
    pub fn open_tracked(tracked: &TrackedWindow) -> Result<Self, ReadOnlyProcessError> {
        Self::open_identity(
            tracked.snapshot().process_id(),
            Path::new(tracked.image_path()),
        )
    }

    pub fn revalidate(&self) -> Result<(), ReadOnlyProcessError> {
        let handle = self.handle();
        // SAFETY: handle is owned by self and supports process queries.
        let observed_process_id = unsafe { GetProcessId(handle) };
        if observed_process_id == 0 {
            return Err(ReadOnlyProcessError::ProcessUnavailable);
        }
        if observed_process_id != self.process_id {
            return Err(ReadOnlyProcessError::IdentityMismatch);
        }
        ensure_process_alive(handle)?;
        if process_creation_time(handle)? != self.creation_time_100ns {
            return Err(ReadOnlyProcessError::IdentityMismatch);
        }
        if !paths_equal_case_insensitive(&process_image_path(handle)?, &self.expected_image_path) {
            return Err(ReadOnlyProcessError::IdentityMismatch);
        }
        Ok(())
    }

    pub fn open_read_only_tracked(
        &self,
        tracked: &TrackedWindow,
    ) -> Result<ReadOnlyProcess, ReadOnlyProcessError> {
        if tracked.snapshot().process_id() != self.process_id
            || !paths_equal_case_insensitive(
                Path::new(tracked.image_path()),
                &self.expected_image_path,
            )
        {
            return Err(ReadOnlyProcessError::IdentityMismatch);
        }
        self.open_read_only_identity()
    }

    fn open_identity(
        process_id: u32,
        expected_image_path: &Path,
    ) -> Result<Self, ReadOnlyProcessError> {
        if process_id == 0 || !expected_image_path.is_absolute() {
            return Err(ReadOnlyProcessError::InvalidTarget);
        }

        // SAFETY: the PID is inert and the access mask grants only limited process queries. Handle
        // inheritance is explicitly disabled.
        let raw_process = unsafe { OpenProcess(QUERY_ONLY_PROCESS_ACCESS, 0, process_id) };
        if raw_process.is_null() {
            return Err(ReadOnlyProcessError::ProcessUnavailable);
        }
        // SAFETY: OpenProcess returned a newly owned, non-null kernel handle.
        let process = unsafe { OwnedHandle::from_raw_handle(raw_process) };
        let creation_time_100ns = process_creation_time(raw_process)?;
        ensure_process_alive(raw_process)?;
        let observed_image_path = process_image_path(raw_process)?;
        if !paths_equal_case_insensitive(&observed_image_path, expected_image_path) {
            return Err(ReadOnlyProcessError::IdentityMismatch);
        }

        let identity = Self {
            process,
            process_id,
            expected_image_path: expected_image_path.to_path_buf(),
            creation_time_100ns,
        };
        identity.revalidate()?;
        Ok(identity)
    }

    fn open_read_only_identity(&self) -> Result<ReadOnlyProcess, ReadOnlyProcessError> {
        self.revalidate()?;
        let process = ReadOnlyProcess::open_identity(self.process_id, &self.expected_image_path)?;
        if process.creation_time_100ns != self.creation_time_100ns {
            return Err(ReadOnlyProcessError::IdentityMismatch);
        }
        self.revalidate()?;
        process.revalidate()?;
        Ok(process)
    }

    fn handle(&self) -> HANDLE {
        self.process.as_raw_handle()
    }
}

impl ReadOnlyProcessSession<'_> {
    pub fn read_exact_at(
        &self,
        address: usize,
        output: &mut [u8],
    ) -> Result<(), ReadOnlyProcessError> {
        validate_read_request(address, output.len())?;
        validate_readable_range(self.process.handle(), address, output.len())?;

        let mut bytes_read = 0_usize;
        // SAFETY: the full remote range was checked with VirtualQueryEx, output is writable for its
        // exact length, and the process handle has only query/read access.
        let succeeded = unsafe {
            ReadProcessMemory(
                self.process.handle(),
                address as *const _,
                output.as_mut_ptr().cast(),
                output.len(),
                &mut bytes_read,
            )
        };
        if succeeded == 0 || bytes_read != output.len() {
            return Err(ReadOnlyProcessError::ReadFailed);
        }
        Ok(())
    }
}

fn validate_read_request(address: usize, length: usize) -> Result<(), ReadOnlyProcessError> {
    if address == 0 || length == 0 || length > MAX_EXACT_READ_BYTES {
        return Err(ReadOnlyProcessError::InvalidRead);
    }
    address
        .checked_add(length)
        .ok_or(ReadOnlyProcessError::InvalidRead)?;
    Ok(())
}

fn ensure_process_alive(process: HANDLE) -> Result<(), ReadOnlyProcessError> {
    let mut exit_code = 0_u32;
    // SAFETY: process is an owned query handle and exit_code is writable.
    if unsafe { GetExitCodeProcess(process, &mut exit_code) } == 0 {
        return Err(ReadOnlyProcessError::ProcessUnavailable);
    }
    if exit_code != STILL_ACTIVE as u32 {
        return Err(ReadOnlyProcessError::ProcessExited);
    }
    Ok(())
}

fn process_creation_time(process: HANDLE) -> Result<u64, ReadOnlyProcessError> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: process is an owned query handle and every FILETIME pointer is writable.
    if unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(ReadOnlyProcessError::ProcessUnavailable);
    }
    let value = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    if value == 0 {
        return Err(ReadOnlyProcessError::ProcessUnavailable);
    }
    Ok(value)
}

fn process_image_path(process: HANDLE) -> Result<PathBuf, ReadOnlyProcessError> {
    let mut buffer = vec![0_u16; PROCESS_IMAGE_BUFFER_LEN];
    let mut length =
        u32::try_from(buffer.len()).map_err(|_| ReadOnlyProcessError::ProcessUnavailable)?;
    // SAFETY: process is an owned query handle and buffer has the declared UTF-16 capacity.
    if unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) } == 0 {
        return Err(ReadOnlyProcessError::ProcessUnavailable);
    }
    let length = usize::try_from(length).map_err(|_| ReadOnlyProcessError::ProcessUnavailable)?;
    if length == 0 || length > buffer.len() {
        return Err(ReadOnlyProcessError::ProcessUnavailable);
    }
    buffer.truncate(length);
    Ok(PathBuf::from(OsString::from_wide(&buffer)))
}

fn exact_main_module(
    process_id: u32,
    expected_image_path: &Path,
) -> Result<ReadOnlyProcessModule, ReadOnlyProcessError> {
    let snapshot = module_snapshot(process_id)?;
    let snapshot_handle = snapshot.as_raw_handle();
    let mut entry = MODULEENTRY32W {
        dwSize: u32::try_from(mem::size_of::<MODULEENTRY32W>())
            .map_err(|_| ReadOnlyProcessError::ModuleUnavailable)?,
        ..MODULEENTRY32W::default()
    };

    // SAFETY: snapshot is a live module snapshot and entry has the required size and storage.
    if unsafe { Module32FirstW(snapshot_handle, &mut entry) } == 0 {
        return Err(ReadOnlyProcessError::ModuleUnavailable);
    }

    loop {
        if entry.th32ProcessID == process_id
            && paths_equal_case_insensitive(&wide_nul_path(&entry.szExePath)?, expected_image_path)
        {
            let base_address = entry.modBaseAddr as usize;
            let size = usize::try_from(entry.modBaseSize)
                .map_err(|_| ReadOnlyProcessError::InvalidModule)?;
            if base_address == 0 || size == 0 || base_address.checked_add(size).is_none() {
                return Err(ReadOnlyProcessError::InvalidModule);
            }
            return Ok(ReadOnlyProcessModule { base_address, size });
        }

        // SAFETY: snapshot remains live and entry remains correctly sized and writable.
        if unsafe { Module32NextW(snapshot_handle, &mut entry) } == 0 {
            // SAFETY: GetLastError has no preconditions.
            let error = unsafe { GetLastError() };
            if error == ERROR_NO_MORE_FILES {
                return Err(ReadOnlyProcessError::ModuleUnavailable);
            }
            return Err(ReadOnlyProcessError::ModuleUnavailable);
        }
    }
}

fn module_snapshot(process_id: u32) -> Result<OwnedHandle, ReadOnlyProcessError> {
    for _ in 0..SNAPSHOT_RETRY_LIMIT {
        // SAFETY: only an exact-PID module snapshot is requested.
        let raw_snapshot = unsafe { CreateToolhelp32Snapshot(SNAPSHOT_FLAGS, process_id) };
        if raw_snapshot != INVALID_HANDLE_VALUE {
            // SAFETY: the snapshot call returned a newly owned, valid handle.
            return Ok(unsafe { OwnedHandle::from_raw_handle(raw_snapshot) });
        }
        // SAFETY: GetLastError has no preconditions.
        if unsafe { GetLastError() } != ERROR_BAD_LENGTH {
            break;
        }
    }
    Err(ReadOnlyProcessError::ModuleUnavailable)
}

fn wide_nul_path(buffer: &[u16]) -> Result<PathBuf, ReadOnlyProcessError> {
    let length = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    if length == 0 {
        return Err(ReadOnlyProcessError::ModuleUnavailable);
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

fn paths_equal_case_insensitive(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn validate_readable_range(
    process: HANDLE,
    address: usize,
    length: usize,
) -> Result<(), ReadOnlyProcessError> {
    let end = address
        .checked_add(length)
        .ok_or(ReadOnlyProcessError::InvalidRead)?;
    let mut cursor = address;
    while cursor < end {
        let mut information = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: process is an owned query handle and information is writable for its size.
        let queried = unsafe {
            VirtualQueryEx(
                process,
                cursor as *const _,
                &mut information,
                mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if queried != mem::size_of::<MEMORY_BASIC_INFORMATION>() {
            return Err(ReadOnlyProcessError::UnreadableMemory);
        }

        let region_start = information.BaseAddress as usize;
        let region_end = region_start
            .checked_add(information.RegionSize)
            .ok_or(ReadOnlyProcessError::UnreadableMemory)?;
        if information.State != MEM_COMMIT
            || !is_readable_protection(information.Protect)
            || cursor < region_start
            || cursor >= region_end
        {
            return Err(ReadOnlyProcessError::UnreadableMemory);
        }
        let next = region_end.min(end);
        if next <= cursor {
            return Err(ReadOnlyProcessError::UnreadableMemory);
        }
        cursor = next;
    }
    Ok(())
}

fn is_readable_protection(protection: u32) -> bool {
    if protection & (PAGE_GUARD | PAGE_NOACCESS) != 0 {
        return false;
    }
    matches!(
        protection & BASIC_PROTECTION_MASK,
        PAGE_READONLY
            | PAGE_READWRITE
            | PAGE_WRITECOPY
            | PAGE_EXECUTE_READ
            | PAGE_EXECUTE_READWRITE
            | PAGE_EXECUTE_WRITECOPY
    )
}

#[cfg(test)]
mod tests {
    use std::{env, ptr};

    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_HANDLE, GetHandleInformation, GetLastError},
        System::Threading::GetCurrentProcessId,
    };

    use super::*;

    static TEST_VALUE: u64 = 0x0f1e_2d3c_4b5a_6978;

    fn open_current_process() -> ReadOnlyProcess {
        // SAFETY: GetCurrentProcessId has no preconditions.
        let process_id = unsafe { GetCurrentProcessId() };
        let image_path = env::current_exe().expect("current executable path");
        ReadOnlyProcess::open_identity(process_id, &image_path).expect("open current process")
    }

    fn guard_current_process() -> QueryOnlyProcessGuard {
        // SAFETY: GetCurrentProcessId has no preconditions.
        let process_id = unsafe { GetCurrentProcessId() };
        let image_path = env::current_exe().expect("current executable path");
        QueryOnlyProcessGuard::open_identity(process_id, &image_path)
            .expect("guard current process")
    }

    #[test]
    fn requested_access_is_exactly_query_and_read() {
        assert_eq!(PROCESS_ACCESS, PROCESS_QUERY_INFORMATION | PROCESS_VM_READ);
        assert_eq!(PROCESS_ACCESS, 0x410);
    }

    #[test]
    fn query_guard_uses_no_process_memory_read_capability() {
        assert_eq!(QUERY_ONLY_PROCESS_ACCESS, PROCESS_QUERY_LIMITED_INFORMATION);
        assert_eq!(QUERY_ONLY_PROCESS_ACCESS, 0x1000);
    }

    #[test]
    fn query_guard_binds_the_later_read_handle_to_the_same_process_instance() {
        let guard = guard_current_process();
        let process = guard
            .open_read_only_identity()
            .expect("open guarded read-only process");

        guard.revalidate().expect("guard remains live");
        process.revalidate().expect("read process remains live");
        assert_eq!(guard.creation_time_100ns, process.creation_time_100ns);
    }

    #[test]
    fn query_guard_diagnostics_do_not_render_identity() {
        let guard = guard_current_process();
        let rendered = format!("{guard:?}");
        let process_id = guard.process_id.to_string();
        let image_path = guard.expected_image_path.display().to_string();

        assert!(!rendered.contains(&process_id));
        assert!(!rendered.contains(&image_path));
    }

    #[test]
    fn windows_identity_paths_are_compared_case_insensitively() {
        assert!(paths_equal_case_insensitive(
            Path::new(r"C:\\Games\\Palworld-Win64-Shipping.exe"),
            Path::new(r"c:\\games\\PALWORLD-WIN64-SHIPPING.EXE"),
        ));
        assert!(!paths_equal_case_insensitive(
            Path::new(r"C:\\Games\\Palworld-Win64-Shipping.exe"),
            Path::new(r"C:\\Other\\Palworld-Win64-Shipping.exe"),
        ));
    }

    #[test]
    fn current_process_module_and_exact_read_are_valid() {
        let process = open_current_process();
        let module = process.main_module();
        assert_ne!(module.base_address(), 0);
        assert_ne!(module.size(), 0);

        let address = ptr::addr_of!(TEST_VALUE) as usize;
        let mut output = [0_u8; mem::size_of::<u64>()];
        process
            .read_exact_at(address, &mut output)
            .expect("read static value");
        assert_eq!(u64::from_ne_bytes(output), TEST_VALUE);
    }

    #[test]
    fn one_identity_bracket_can_service_multiple_bounded_reads() {
        let process = open_current_process();
        let address = ptr::addr_of!(TEST_VALUE) as usize;

        let (first, second) = process
            .with_read_session(|session| {
                let mut first = [0_u8; mem::size_of::<u64>()];
                let mut second = [0_u8; mem::size_of::<u64>()];
                session.read_exact_at(address, &mut first)?;
                session.read_exact_at(address, &mut second)?;
                Ok((first, second))
            })
            .expect("read two values in one identity bracket");

        assert_eq!(u64::from_ne_bytes(first), TEST_VALUE);
        assert_eq!(u64::from_ne_bytes(second), TEST_VALUE);
    }

    #[test]
    fn invalid_reads_fail_before_crossing_the_boundary() {
        let process = open_current_process();
        let mut byte = [0_u8; 1];
        assert_eq!(
            process.read_exact_at(0, &mut byte),
            Err(ReadOnlyProcessError::InvalidRead)
        );
        assert_eq!(
            process.read_exact_at(usize::MAX, &mut byte),
            Err(ReadOnlyProcessError::InvalidRead)
        );
        let mut oversized = vec![0_u8; MAX_EXACT_READ_BYTES + 1];
        assert_eq!(
            process.read_exact_at(ptr::addr_of!(TEST_VALUE) as usize, &mut oversized),
            Err(ReadOnlyProcessError::InvalidRead)
        );
    }

    #[test]
    fn diagnostics_do_not_render_identity_or_addresses() {
        let process = open_current_process();
        let rendered = format!("{process:?} {:?}", process.main_module());
        let process_id = process.process_id_for_test().to_string();
        let image_path = env::current_exe()
            .expect("current executable path")
            .display()
            .to_string();
        let module_address = format!("{:x}", process.main_module().base_address());

        assert!(!rendered.contains(&process_id));
        assert!(!rendered.contains(&image_path));
        assert!(!rendered.contains(&module_address));
        assert_eq!(
            format!("{:?}", ReadOnlyProcessError::ReadFailed),
            "ReadFailed"
        );
    }

    #[test]
    fn owned_process_handle_is_closed_on_drop() {
        let process = open_current_process();
        let raw_handle = process.raw_handle_for_test();
        drop(process);

        let mut flags = 0_u32;
        // SAFETY: probing a stale handle value is supported; the API reports invalid handles.
        assert_eq!(unsafe { GetHandleInformation(raw_handle, &mut flags) }, 0);
        // SAFETY: GetLastError has no preconditions.
        assert_eq!(unsafe { GetLastError() }, ERROR_INVALID_HANDLE);
    }
}
