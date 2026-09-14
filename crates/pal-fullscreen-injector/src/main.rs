use std::{
    collections::BTreeSet,
    env,
    error::Error,
    ffi::{OsString, c_void},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use pal_fullscreen_rhi_protocol::{
    DetectedRhi, PROBE_DLL, PROBE_MAPPING_BYTES, ProbeControl, ProbeState, probe_mapping_id,
};
use pal_windows::{
    GameWindowTracker, QueryOnlyProcessGuard, ReadOnlyProcessError, TrackedWindow, Win32Backend,
    WindowEvent,
};
use shared_memory::{Shmem, ShmemConf};
use std::os::windows::ffi::OsStrExt as _;
use thiserror::Error;
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_FAILED,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    System::{
        Diagnostics::{
            Debug::WriteProcessMemory,
            ToolHelp::{
                CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
                TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
            },
        },
        LibraryLoader::{GetModuleHandleW, GetProcAddress},
        Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
        },
        Threading::{
            CreateMutexW, CreateRemoteThread, GetExitCodeThread, OpenProcess,
            PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_SYNCHRONIZE,
            PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE, WaitForSingleObject,
        },
    },
};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const ATTACHED_MODULE_RECHECK_INTERVAL: Duration = Duration::from_secs(5);
const PROBE_RESULT_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_UNLOAD_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const DX11_DLL: &str = "pal-fullscreen-overlay-dx11.dll";
const DX12_DLL: &str = "pal-fullscreen-overlay-dx12.dll";
const INSTANCE_MUTEX_NAME: &str = r"Local\PalBeacon.FullscreenInjector.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GraphicsBackend {
    DirectX11,
    DirectX12,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectionEvidence {
    EnvironmentOverride,
    SwapChainProbe,
}

impl SelectionEvidence {
    const fn label(self) -> &'static str {
        match self {
            Self::EnvironmentOverride => "PAL_COMPANION_RENDERER override",
            Self::SwapChainProbe => "DXGI swap-chain device probe",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BackendSelection {
    backend: GraphicsBackend,
    evidence: SelectionEvidence,
}

impl GraphicsBackend {
    const fn dll_name(self) -> &'static str {
        match self {
            Self::DirectX11 => DX11_DLL,
            Self::DirectX12 => DX12_DLL,
        }
    }
}

#[derive(Debug, Error)]
enum InjectorError {
    #[error("injector executable has no parent directory")]
    MissingExecutableParent,
    #[error("required fullscreen DLL is missing: {0}")]
    MissingDll(PathBuf),
    #[error("fullscreen DLL path could not be resolved: {0}")]
    DllPathUnavailable(PathBuf),
    #[error("module enumeration failed with Windows error {0}")]
    ModuleEnumeration(u32),
    #[error("RHI probe shared memory failed: {0}")]
    ProbeSharedMemory(String),
    #[error("RHI probe failed in process {pid} with code {code}")]
    ProbeFailed { pid: u32, code: u32 },
    #[error("RHI probe timed out in process {0}")]
    ProbeTimedOut(u32),
    #[error("RHI probe did not unload from process {0}")]
    ProbeDidNotUnload(u32),
    #[error("a fullscreen renderer is already injected into process {0}")]
    RendererAlreadyInjected(u32),
    #[error("process {pid} could not be opened for injection with Windows error {code}")]
    InjectionProcessUnavailable { pid: u32, code: u32 },
    #[error("tracked Palworld process identity changed before injection: {0}")]
    TargetIdentity(#[from] ReadOnlyProcessError),
    #[error(
        "remote DLL injection into process {pid} failed at {operation} with Windows error {code}"
    )]
    RemoteInjection {
        pid: u32,
        operation: &'static str,
        code: u32,
    },
    #[error("remote LoadLibraryW returned null in process {0}")]
    RemoteLoadLibraryFailed(u32),
    #[error("fullscreen injector single-instance guard failed with Windows error {0}")]
    SingleInstanceUnavailable(u32),
    #[error("parent process {pid} could not be opened with Windows error {code}")]
    ParentProcessUnavailable { pid: u32, code: u32 },
    #[error("parent process wait failed with Windows error {0}")]
    ParentProcessWait(u32),
    #[error("missing value after --parent-pid")]
    MissingParentPid,
    #[error("invalid --parent-pid value: {0}")]
    InvalidParentPid(String),
    #[error("missing value after --detect-pid")]
    MissingDetectPid,
    #[error("invalid --detect-pid value: {0}")]
    InvalidDetectPid(String),
    #[error("unknown fullscreen injector argument: {0}")]
    UnknownArgument(String),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RuntimeArguments {
    health_check: bool,
    parent_pid: Option<u32>,
    detect_pid: Option<u32>,
}

#[derive(Debug)]
enum InstanceAcquire {
    Acquired(InstanceGuard),
    AlreadyRunning,
}

#[derive(Debug)]
struct InstanceGuard(HANDLE);

impl InstanceGuard {
    fn acquire() -> Result<InstanceAcquire, InjectorError> {
        Self::acquire_named(INSTANCE_MUTEX_NAME)
    }

    fn acquire_named(name: &str) -> Result<InstanceAcquire, InjectorError> {
        let wide_name = wide_z(name);
        // SAFETY: the default security descriptor is valid and wide_name is NUL-terminated.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide_name.as_ptr()) };
        // SAFETY: GetLastError is sampled immediately after CreateMutexW.
        let create_error = unsafe { GetLastError() };
        if handle.is_null() {
            return Err(InjectorError::SingleInstanceUnavailable(create_error));
        }
        if create_error == ERROR_ALREADY_EXISTS {
            // SAFETY: CreateMutexW returned one owned handle which is closed on this path.
            unsafe {
                CloseHandle(handle);
            }
            return Ok(InstanceAcquire::AlreadyRunning);
        }
        Ok(InstanceAcquire::Acquired(Self(handle)))
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: the acquired variant owns this non-null kernel handle.
        unsafe {
            CloseHandle(self.0);
        }
        self.0 = std::ptr::null_mut();
    }
}

#[derive(Debug)]
struct ParentProcessGuard(HANDLE);

impl ParentProcessGuard {
    fn open(pid: u32) -> Result<Self, InjectorError> {
        // SAFETY: pid is a scalar supplied by the supervising Core. No inherited handle is needed.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            // SAFETY: GetLastError is sampled immediately after OpenProcess failed.
            let code = unsafe { GetLastError() };
            return Err(InjectorError::ParentProcessUnavailable { pid, code });
        }
        Ok(Self(handle))
    }

    fn has_exited(&self) -> Result<bool, InjectorError> {
        // SAFETY: this guard owns a live process handle with synchronization access.
        match unsafe { WaitForSingleObject(self.0, 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            WAIT_FAILED => {
                // SAFETY: GetLastError is sampled immediately after the failed wait.
                Err(InjectorError::ParentProcessWait(unsafe { GetLastError() }))
            }
            _ => Ok(false),
        }
    }
}

impl Drop for ParentProcessGuard {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: this guard owns one non-null process handle.
        unsafe {
            CloseHandle(self.0);
        }
        self.0 = std::ptr::null_mut();
    }
}

struct ProbeSession {
    mapping: Shmem,
}

impl ProbeSession {
    fn create(pid: u32) -> Result<Self, InjectorError> {
        let mapping = ShmemConf::new()
            .os_id(probe_mapping_id(pid))
            .size(PROBE_MAPPING_BYTES)
            .create()
            .map_err(|error| InjectorError::ProbeSharedMemory(error.to_string()))?;
        if mapping.len() < PROBE_MAPPING_BYTES {
            return Err(InjectorError::ProbeSharedMemory(
                "mapping is smaller than the probe protocol".to_owned(),
            ));
        }
        unsafe {
            mapping
                .as_ptr()
                .cast::<ProbeControl>()
                .write(ProbeControl::new(pid));
        }
        Ok(Self { mapping })
    }

    fn control(&self) -> &ProbeControl {
        unsafe { &*self.mapping.as_ptr().cast::<ProbeControl>() }
    }
}

struct InjectionProcess {
    pid: u32,
    handle: HANDLE,
}

impl InjectionProcess {
    fn open(pid: u32) -> Result<Self, InjectorError> {
        let access = PROCESS_CREATE_THREAD
            | PROCESS_QUERY_INFORMATION
            | PROCESS_VM_OPERATION
            | PROCESS_VM_READ
            | PROCESS_VM_WRITE;
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            return Err(InjectorError::InjectionProcessUnavailable {
                pid,
                code: unsafe { GetLastError() },
            });
        }
        Ok(Self { pid, handle })
    }

    fn inject(&self, dll_path: &Path) -> Result<(), InjectorError> {
        let canonical = dll_path
            .canonicalize()
            .map_err(|_| InjectorError::DllPathUnavailable(dll_path.to_path_buf()))?;
        let wide_path = canonical
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let allocation_bytes = wide_path.len() * size_of::<u16>();
        let remote_path = unsafe {
            VirtualAllocEx(
                self.handle,
                std::ptr::null(),
                allocation_bytes,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_READWRITE,
            )
        };
        if remote_path.is_null() {
            return Err(self.remote_error("VirtualAllocEx"));
        }

        let result = self.inject_allocated(remote_path, &wide_path);
        let free_succeeded =
            unsafe { VirtualFreeEx(self.handle, remote_path, 0, MEM_RELEASE) } != 0;
        if result.is_ok() && !free_succeeded {
            return Err(self.remote_error("VirtualFreeEx"));
        }
        result
    }

    fn inject_allocated(
        &self,
        remote_path: *mut c_void,
        wide_path: &[u16],
    ) -> Result<(), InjectorError> {
        let mut bytes_written = 0usize;
        let bytes = std::mem::size_of_val(wide_path);
        if unsafe {
            WriteProcessMemory(
                self.handle,
                remote_path,
                wide_path.as_ptr().cast::<c_void>(),
                bytes,
                &mut bytes_written,
            )
        } == 0
            || bytes_written != bytes
        {
            return Err(self.remote_error("WriteProcessMemory"));
        }

        let kernel32 = unsafe { GetModuleHandleW(wide_z("kernel32.dll").as_ptr()) };
        if kernel32.is_null() {
            return Err(self.remote_error("GetModuleHandleW"));
        }
        let load_library = unsafe { GetProcAddress(kernel32, c"LoadLibraryW".as_ptr().cast()) };
        let Some(load_library) = load_library else {
            return Err(self.remote_error("GetProcAddress"));
        };
        let start_routine = Some(unsafe {
            std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(*mut c_void) -> u32,
            >(load_library)
        });
        let remote_thread = unsafe {
            CreateRemoteThread(
                self.handle,
                std::ptr::null(),
                0,
                start_routine,
                remote_path,
                0,
                std::ptr::null_mut(),
            )
        };
        if remote_thread.is_null() {
            return Err(self.remote_error("CreateRemoteThread"));
        }

        let wait_result = unsafe { WaitForSingleObject(remote_thread, u32::MAX) };
        if wait_result != WAIT_OBJECT_0 {
            unsafe { CloseHandle(remote_thread) };
            return Err(self.remote_error("WaitForSingleObject"));
        }
        let mut exit_code = 0u32;
        let exit_succeeded = unsafe { GetExitCodeThread(remote_thread, &mut exit_code) } != 0;
        unsafe { CloseHandle(remote_thread) };
        if !exit_succeeded {
            return Err(self.remote_error("GetExitCodeThread"));
        }
        if exit_code == 0 {
            return Err(InjectorError::RemoteLoadLibraryFailed(self.pid));
        }
        Ok(())
    }

    fn remote_error(&self, operation: &'static str) -> InjectorError {
        InjectorError::RemoteInjection {
            pid: self.pid,
            operation,
            code: unsafe { GetLastError() },
        }
    }
}

impl Drop for InjectionProcess {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { CloseHandle(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    match run() {
        Ok(()) => Ok(()),
        Err(error) => {
            let message = format!("fatal error: {error}");
            log_event(&message);
            eprintln!("pal-fullscreen-injector: {message}");
            Err(error)
        }
    }
}

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let arguments = parse_runtime_arguments(env::args_os().skip(1))?;
    let dll_root = env::current_exe()?
        .parent()
        .ok_or(InjectorError::MissingExecutableParent)?
        .to_path_buf();
    if arguments.health_check {
        validate_runtime_files(&dll_root)?;
        println!("pal-fullscreen-injector: health check passed");
        return Ok(());
    }
    if let Some(pid) = arguments.detect_pid {
        if contains_renderer_dll(&module_names(pid)?) {
            return Err(InjectorError::RendererAlreadyInjected(pid).into());
        }
        let selection =
            detect_backend(pid, &dll_root, env::var_os("PAL_COMPANION_RENDERER"), None)?;
        println!(
            "pal-fullscreen-injector: pid {pid} renderer={:?} evidence={}",
            selection.backend,
            selection.evidence.label()
        );
        return Ok(());
    }
    let _instance_guard = match InstanceGuard::acquire()? {
        InstanceAcquire::Acquired(guard) => guard,
        InstanceAcquire::AlreadyRunning => {
            log_event("duplicate instance ignored");
            return Ok(());
        }
    };
    let parent_process = arguments
        .parent_pid
        .map(ParentProcessGuard::open)
        .transpose()?;
    log_event("started");
    let mut tracker = GameWindowTracker::new(Win32Backend::new());
    let mut target_window = None;
    let mut attached_pid = None;
    let mut attached_module_recheck_at = None;
    let mut conflict_reported_for = None;
    let mut probe_retry_at = None;
    let mut probe_failure_reported_for = None;

    loop {
        if let Some(parent) = parent_process.as_ref()
            && parent.has_exited()?
        {
            log_event("supervising Core exited; injector stopped");
            return Ok(());
        }
        if let Some(event) = tracker.poll()? {
            match event {
                WindowEvent::Attached(window) => {
                    target_window = Some(window);
                    attached_pid = None;
                    attached_module_recheck_at = None;
                    conflict_reported_for = None;
                    probe_retry_at = None;
                    probe_failure_reported_for = None;
                }
                WindowEvent::Changed { current, .. } => {
                    let pid = current.snapshot().process_id();
                    if target_window
                        .as_ref()
                        .is_none_or(|window: &TrackedWindow| window.snapshot().process_id() != pid)
                    {
                        attached_pid = None;
                        attached_module_recheck_at = None;
                        conflict_reported_for = None;
                        probe_retry_at = None;
                        probe_failure_reported_for = None;
                    }
                    target_window = Some(current);
                }
                WindowEvent::Detached { .. } => {
                    target_window = None;
                    attached_pid = None;
                    attached_module_recheck_at = None;
                    conflict_reported_for = None;
                    probe_retry_at = None;
                    probe_failure_reported_for = None;
                }
            }
        }

        let Some(target) = target_window.as_ref() else {
            thread::sleep(POLL_INTERVAL);
            continue;
        };
        let pid = target.snapshot().process_id();
        let now = Instant::now();
        if attached_pid == Some(pid) {
            if attached_module_recheck_at.is_some_and(|deadline| now < deadline) {
                thread::sleep(POLL_INTERVAL);
                continue;
            }
            attached_pid = None;
            attached_module_recheck_at = None;
        }
        let modules = module_names(pid)?;
        if has_competing_overlay(&modules) {
            if conflict_reported_for != Some(pid) {
                eprintln!(
                    "pal-fullscreen-injector: another injected overlay is active; \
                     close it before enabling Pal Companion fullscreen mode"
                );
                log_event(&format!(
                    "injection deferred for pid {pid}: competing overlay detected"
                ));
                conflict_reported_for = Some(pid);
            }
            thread::sleep(POLL_INTERVAL);
            continue;
        }
        if contains_renderer_dll(&modules) {
            attached_pid = Some(pid);
            attached_module_recheck_at = Some(now + ATTACHED_MODULE_RECHECK_INTERVAL);
            thread::sleep(POLL_INTERVAL);
            continue;
        }
        if modules.contains(PROBE_DLL) {
            thread::sleep(POLL_INTERVAL);
            continue;
        }
        if probe_retry_at.is_some_and(|deadline| now < deadline) {
            thread::sleep(POLL_INTERVAL);
            continue;
        }

        let selection = match detect_backend(
            pid,
            &dll_root,
            env::var_os("PAL_COMPANION_RENDERER"),
            Some(target),
        ) {
            Ok(selection) => {
                probe_retry_at = None;
                probe_failure_reported_for = None;
                selection
            }
            Err(error) => {
                if probe_failure_reported_for != Some(pid) {
                    let message = format!(
                        "renderer detection deferred for pid {pid}: {error}; \
                         external overlay fallback remains active"
                    );
                    eprintln!("pal-fullscreen-injector: {message}");
                    log_event(&message);
                    probe_failure_reported_for = Some(pid);
                }
                probe_retry_at = Some(now + PROBE_RETRY_INTERVAL);
                thread::sleep(POLL_INTERVAL);
                continue;
            }
        };
        let backend = selection.backend;
        let dll_path = dll_root.join(backend.dll_name());
        if !dll_path.is_file() {
            return Err(InjectorError::MissingDll(dll_path).into());
        }
        inject_exact_tracked_process(target, &dll_path)?;
        let message = format!(
            "attached {backend:?} renderer to tracked Palworld process {pid} \
             (evidence: {})",
            selection.evidence.label()
        );
        println!("pal-fullscreen-injector: {message}");
        log_event(&message);
        attached_pid = Some(pid);
        attached_module_recheck_at = Some(now + ATTACHED_MODULE_RECHECK_INTERVAL);
        thread::sleep(POLL_INTERVAL);
    }
}

fn parse_runtime_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<RuntimeArguments, InjectorError> {
    let mut parsed = RuntimeArguments::default();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--health-check" {
            parsed.health_check = true;
            continue;
        }
        if argument == "--parent-pid" {
            let raw = arguments.next().ok_or(InjectorError::MissingParentPid)?;
            let rendered = raw.to_string_lossy().into_owned();
            let pid = rendered
                .parse::<u32>()
                .ok()
                .filter(|pid| *pid != 0)
                .ok_or_else(|| InjectorError::InvalidParentPid(rendered.clone()))?;
            parsed.parent_pid = Some(pid);
            continue;
        }
        if argument == "--detect-pid" {
            let raw = arguments.next().ok_or(InjectorError::MissingDetectPid)?;
            let rendered = raw.to_string_lossy().into_owned();
            let pid = rendered
                .parse::<u32>()
                .ok()
                .filter(|pid| *pid != 0)
                .ok_or_else(|| InjectorError::InvalidDetectPid(rendered.clone()))?;
            parsed.detect_pid = Some(pid);
            continue;
        }
        return Err(InjectorError::UnknownArgument(
            argument.to_string_lossy().into_owned(),
        ));
    }
    Ok(parsed)
}

fn validate_runtime_files(dll_root: &Path) -> Result<(), InjectorError> {
    for dll_name in [PROBE_DLL, DX11_DLL, DX12_DLL] {
        let dll_path = dll_root.join(dll_name);
        if !dll_path.is_file() {
            return Err(InjectorError::MissingDll(dll_path));
        }
    }
    Ok(())
}

fn inject_exact_tracked_process(
    target: &TrackedWindow,
    dll_path: &Path,
) -> Result<(), InjectorError> {
    let identity = QueryOnlyProcessGuard::open_tracked(target)?;
    let process = InjectionProcess::open(target.snapshot().process_id())?;
    identity.revalidate()?;
    process.inject(dll_path)
}

fn detect_backend(
    pid: u32,
    dll_root: &Path,
    override_value: Option<OsString>,
    tracked: Option<&TrackedWindow>,
) -> Result<BackendSelection, InjectorError> {
    if let Some(backend) = override_value
        .as_deref()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        let backend = match backend {
            "dx11" => Some(GraphicsBackend::DirectX11),
            "dx12" => Some(GraphicsBackend::DirectX12),
            _ => None,
        };
        if let Some(backend) = backend {
            return Ok(BackendSelection {
                backend,
                evidence: SelectionEvidence::EnvironmentOverride,
            });
        }
    }

    let probe_path = dll_root.join(PROBE_DLL);
    if !probe_path.is_file() {
        return Err(InjectorError::MissingDll(probe_path));
    }
    let detected = detect_backend_with_probe(pid, &probe_path, tracked)?;
    Ok(BackendSelection {
        backend: match detected {
            DetectedRhi::DirectX11 => GraphicsBackend::DirectX11,
            DetectedRhi::DirectX12 => GraphicsBackend::DirectX12,
        },
        evidence: SelectionEvidence::SwapChainProbe,
    })
}

fn detect_backend_with_probe(
    pid: u32,
    probe_path: &Path,
    tracked: Option<&TrackedWindow>,
) -> Result<DetectedRhi, InjectorError> {
    let session = ProbeSession::create(pid)?;
    if let Some(target) = tracked {
        inject_exact_tracked_process(target, probe_path)?;
    } else {
        InjectionProcess::open(pid)?.inject(probe_path)?;
    }
    let deadline = Instant::now() + PROBE_RESULT_TIMEOUT;
    let detection = loop {
        match session.control().state() {
            Some(ProbeState::Detected) => {
                break session
                    .control()
                    .detected_rhi()
                    .ok_or(InjectorError::ProbeFailed { pid, code: 0 });
            }
            Some(ProbeState::Failed) => {
                break Err(InjectorError::ProbeFailed {
                    pid,
                    code: session.control().error_code(),
                });
            }
            _ if Instant::now() >= deadline => {
                break Err(InjectorError::ProbeTimedOut(pid));
            }
            _ => thread::sleep(Duration::from_millis(10)),
        }
    };
    session.control().request_stop();

    let unload_deadline = Instant::now() + PROBE_UNLOAD_TIMEOUT;
    loop {
        if !module_names(pid)?.contains(PROBE_DLL) {
            break;
        }
        if Instant::now() >= unload_deadline {
            return Err(InjectorError::ProbeDidNotUnload(pid));
        }
        thread::sleep(Duration::from_millis(10));
    }
    detection
}

fn contains_renderer_dll(modules: &BTreeSet<String>) -> bool {
    modules.contains(DX11_DLL) || modules.contains(DX12_DLL)
}

fn has_competing_overlay(modules: &BTreeSet<String>) -> bool {
    modules
        .iter()
        .any(|module| module == "n_overlay.x64.dll" || module == "n_overlay.dll")
}

fn module_names(pid: u32) -> Result<BTreeSet<String>, InjectorError> {
    let snapshot =
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(InjectorError::ModuleEnumeration(
            std::io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or_default() as u32,
        ));
    }
    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };
    let mut modules = BTreeSet::new();
    let mut success = unsafe { Module32FirstW(snapshot, &mut entry) } != 0;
    while success {
        modules.insert(wide_name(&entry.szModule).to_ascii_lowercase());
        success = unsafe { Module32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe {
        CloseHandle(snapshot);
    }
    Ok(modules)
}

fn wide_name(buffer: &[u16]) -> String {
    let length = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}

fn wide_z(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn log_event(event: &str) {
    let Some(root) = env::var_os("LOCALAPPDATA") else {
        return;
    };
    let log_directory = PathBuf::from(root).join("PalBeacon").join("logs");
    if fs::create_dir_all(&log_directory).is_err() {
        return;
    }
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_directory.join("fullscreen-injector.log"))
    else {
        return;
    };
    let unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = writeln!(file, "{unix_seconds} {event}");
}

const fn size_of<T>() -> usize {
    std::mem::size_of::<T>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modules(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn explicit_renderer_override_skips_the_probe() {
        assert_eq!(
            detect_backend(
                42,
                Path::new("missing-probe-directory"),
                Some(OsString::from("dx11")),
                None,
            )
            .expect("forced renderer"),
            BackendSelection {
                backend: GraphicsBackend::DirectX11,
                evidence: SelectionEvidence::EnvironmentOverride,
            }
        );
    }

    #[test]
    fn probe_session_exchanges_atomic_state() {
        let pid = std::process::id();
        let session = ProbeSession::create(pid).expect("probe mapping");
        assert!(session.control().is_compatible(pid));
        session.control().publish_detection(DetectedRhi::DirectX12);
        assert_eq!(
            session.control().detected_rhi(),
            Some(DetectedRhi::DirectX12)
        );
    }

    #[test]
    fn detects_only_known_competing_and_owned_overlay_modules() {
        assert!(has_competing_overlay(&modules(&["n_overlay.x64.dll"])));
        assert!(!has_competing_overlay(&modules(&["discordhook64.dll"])));
        assert!(contains_renderer_dll(&modules(&[DX12_DLL])));
        assert!(!contains_renderer_dll(&modules(&[PROBE_DLL])));
    }

    #[test]
    fn runtime_validation_requires_both_renderer_dlls() {
        let root = std::env::temp_dir().join(format!(
            "pal-fullscreen-injector-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test directory");
        fs::write(root.join(PROBE_DLL), b"probe").expect("probe");
        fs::write(root.join(DX11_DLL), b"dx11").expect("dx11");
        assert!(matches!(
            validate_runtime_files(&root),
            Err(InjectorError::MissingDll(path)) if path.ends_with(DX12_DLL)
        ));
        fs::write(root.join(DX12_DLL), b"dx12").expect("dx12");
        assert!(validate_runtime_files(&root).is_ok());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn parses_health_check_and_parent_process_arguments() {
        assert_eq!(
            parse_runtime_arguments([
                OsString::from("--parent-pid"),
                OsString::from("42"),
                OsString::from("--health-check"),
            ])
            .expect("arguments"),
            RuntimeArguments {
                health_check: true,
                parent_pid: Some(42),
                detect_pid: None,
            }
        );
        assert!(matches!(
            parse_runtime_arguments([OsString::from("--parent-pid"), OsString::from("0")]),
            Err(InjectorError::InvalidParentPid(_))
        ));
    }

    #[test]
    fn parses_renderer_detection_pid() {
        assert_eq!(
            parse_runtime_arguments([OsString::from("--detect-pid"), OsString::from("73"),])
                .expect("arguments"),
            RuntimeArguments {
                detect_pid: Some(73),
                ..RuntimeArguments::default()
            }
        );
        assert!(matches!(
            parse_runtime_arguments([OsString::from("--detect-pid")]),
            Err(InjectorError::MissingDetectPid)
        ));
    }

    #[test]
    fn named_mutex_rejects_a_second_runtime_instance() {
        let name = format!(
            r"Local\PalBeacon.FullscreenInjector.Test.{}.{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let first = InstanceGuard::acquire_named(&name).expect("first guard");
        assert!(matches!(first, InstanceAcquire::Acquired(_)));
        assert!(matches!(
            InstanceGuard::acquire_named(&name).expect("second guard"),
            InstanceAcquire::AlreadyRunning
        ));
        drop(first);
        assert!(matches!(
            InstanceGuard::acquire_named(&name).expect("reacquired guard"),
            InstanceAcquire::Acquired(_)
        ));
    }
}
