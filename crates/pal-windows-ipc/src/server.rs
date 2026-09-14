#[cfg(windows)]
mod windows_impl {
    use std::{
        ffi::OsStr,
        fmt, io,
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        ptr,
        sync::{Mutex, MutexGuard},
        time::{Duration, Instant},
    };

    use pal_protocol::{MAX_FRAME_LEN, MAX_PAYLOAD_LEN};
    use tokio_util::sync::CancellationToken;
    use windows_sys::Win32::{
        Foundation::{
            ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_OPERATION_ABORTED,
            ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        Storage::FileSystem::{
            FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile,
            WriteFile,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
                PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
            },
            Threading::{CreateEventW, WaitForSingleObject},
        },
    };

    use crate::{
        CORE_PIPE_NAME,
        framed_stream::FramedStream,
        handshake::{HelloGate, SessionPolicy},
        security::PipeSecurity,
    };

    const OPERATION_POLL_INTERVAL: Duration = Duration::from_millis(20);
    const READ_CHUNK_LEN: usize = 64 * 1024;

    pub struct PipeListener {
        endpoint: String,
        endpoint_wide: Vec<u16>,
        policy: SessionPolicy,
        max_sessions: usize,
        pending: Mutex<Option<OwnedHandle>>,
    }

    impl PipeListener {
        pub fn bind(
            endpoint: &str,
            policy: SessionPolicy,
            max_sessions: usize,
        ) -> io::Result<Self> {
            if !(1..=254).contains(&max_sessions) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "max_sessions must be between one and 254",
                ));
            }
            let endpoint_wide = endpoint_wide(endpoint)?;
            let pending = create_pipe_instance(&endpoint_wide, max_sessions + 1, true)?;
            Ok(Self {
                endpoint: endpoint.to_owned(),
                endpoint_wide,
                policy,
                max_sessions,
                pending: Mutex::new(Some(pending)),
            })
        }

        pub fn accept(&self, cancel: &CancellationToken) -> io::Result<PipeConnection> {
            loop {
                if cancel.is_cancelled() {
                    return Err(cancelled());
                }
                let connected = self
                    .pending_lock()?
                    .take()
                    .ok_or_else(|| io::Error::other("listener has no pending pipe instance"))?;
                if let Err(error) = connect_named_pipe(&connected, cancel) {
                    drop(connected);
                    if cancel.is_cancelled() || error.kind() == io::ErrorKind::Interrupted {
                        return Err(error);
                    }
                    self.install_replacement()?;
                    if is_pipe_closed(&error) {
                        continue;
                    }
                    return Err(error);
                }

                let replacement =
                    match create_pipe_instance(&self.endpoint_wide, self.max_sessions + 1, false) {
                        Ok(value) => value,
                        Err(error) => {
                            drop(connected);
                            return Err(error);
                        }
                    };
                let mut pending = self.pending_lock()?;
                if pending.is_some() {
                    drop(connected);
                    return Err(io::Error::other(
                        "listener pending pipe instance was replaced concurrently",
                    ));
                }
                *pending = Some(replacement);
                return Ok(PipeConnection::from_server(connected, self.policy));
            }
        }

        pub fn endpoint(&self) -> &str {
            &self.endpoint
        }

        pub const fn max_sessions(&self) -> usize {
            self.max_sessions
        }

        fn pending_lock(&self) -> io::Result<MutexGuard<'_, Option<OwnedHandle>>> {
            self.pending
                .lock()
                .map_err(|_| io::Error::other("listener state lock was poisoned"))
        }

        fn install_replacement(&self) -> io::Result<()> {
            let replacement =
                create_pipe_instance(&self.endpoint_wide, self.max_sessions + 1, false)?;
            let mut pending = self.pending_lock()?;
            if pending.is_some() {
                return Err(io::Error::other(
                    "listener pending pipe instance was replaced concurrently",
                ));
            }
            *pending = Some(replacement);
            Ok(())
        }
    }

    pub struct PipeConnection {
        handle: OwnedHandle,
        policy: SessionPolicy,
        server_end: bool,
    }

    impl fmt::Debug for PipeConnection {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("PipeConnection")
                .field("handle", &"<redacted>")
                .field("policy", &self.policy)
                .field("server_end", &self.server_end)
                .finish()
        }
    }

    impl PipeConnection {
        fn from_server(handle: OwnedHandle, policy: SessionPolicy) -> Self {
            Self {
                handle,
                policy,
                server_end: true,
            }
        }

        pub(crate) fn from_client(handle: OwnedHandle, policy: SessionPolicy) -> Self {
            Self {
                handle,
                policy,
                server_end: false,
            }
        }

        pub const fn hello_gate(&self) -> HelloGate {
            HelloGate::new(self.policy)
        }

        pub(crate) fn read_chunk(
            &mut self,
            decoder: &mut FramedStream,
            cancel: &CancellationToken,
            deadline: Option<Instant>,
        ) -> io::Result<usize> {
            let mut chunk = [0_u8; READ_CHUNK_LEN];
            let count = self.read_overlapped(&mut chunk, cancel, deadline)?;
            if count != 0 {
                decoder
                    .push(&chunk[..count])
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            }
            Ok(count)
        }

        pub(crate) fn write_all_bounded(
            &mut self,
            mut frame: &[u8],
            cancel: &CancellationToken,
            timeout: Duration,
        ) -> io::Result<()> {
            if frame.len() > MAX_PAYLOAD_LEN + 4 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "frame exceeds the bounded payload limit",
                ));
            }
            let deadline = Instant::now()
                .checked_add(timeout)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "timeout overflow"))?;
            while !frame.is_empty() {
                let written = self.write_overlapped(frame, cancel, Some(deadline))?;
                if written == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "named-pipe write completed without progress",
                    ));
                }
                frame = &frame[written..];
            }
            Ok(())
        }

        fn read_overlapped(
            &self,
            buffer: &mut [u8],
            cancel: &CancellationToken,
            deadline: Option<Instant>,
        ) -> io::Result<usize> {
            let event = create_event()?;
            let mut overlapped = OVERLAPPED {
                hEvent: raw_handle(&event),
                ..OVERLAPPED::default()
            };
            let mut transferred = 0_u32;
            let requested = u32::try_from(buffer.len())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "read is too large"))?;
            // SAFETY: the pipe, buffer, transfer count, and OVERLAPPED remain live until completion.
            let started = unsafe {
                ReadFile(
                    self.raw_handle(),
                    buffer.as_mut_ptr(),
                    requested,
                    &mut transferred,
                    &mut overlapped,
                )
            };
            if started != 0 {
                return Ok(transferred as usize);
            }
            let error = io::Error::last_os_error();
            match error.raw_os_error().map(|value| value as u32) {
                Some(ERROR_IO_PENDING) => {
                    match wait_for_operation(
                        self.raw_handle(),
                        &overlapped,
                        &event,
                        cancel,
                        deadline,
                    ) {
                        Ok(value) => Ok(value as usize),
                        Err(error) if is_pipe_closed(&error) => Ok(0),
                        Err(error) => Err(error),
                    }
                }
                Some(ERROR_BROKEN_PIPE) | Some(ERROR_NO_DATA) => Ok(0),
                _ => Err(error),
            }
        }

        fn write_overlapped(
            &self,
            buffer: &[u8],
            cancel: &CancellationToken,
            deadline: Option<Instant>,
        ) -> io::Result<usize> {
            let event = create_event()?;
            let mut overlapped = OVERLAPPED {
                hEvent: raw_handle(&event),
                ..OVERLAPPED::default()
            };
            let mut transferred = 0_u32;
            let requested = u32::try_from(buffer.len())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "write is too large"))?;
            // SAFETY: the pipe, buffer, transfer count, and OVERLAPPED remain live until completion.
            let started = unsafe {
                WriteFile(
                    self.raw_handle(),
                    buffer.as_ptr(),
                    requested,
                    &mut transferred,
                    &mut overlapped,
                )
            };
            if started != 0 {
                return Ok(transferred as usize);
            }
            let error = io::Error::last_os_error();
            match error.raw_os_error().map(|value| value as u32) {
                Some(ERROR_IO_PENDING) => {
                    wait_for_operation(self.raw_handle(), &overlapped, &event, cancel, deadline)
                        .map(|value| value as usize)
                }
                Some(ERROR_BROKEN_PIPE) | Some(ERROR_NO_DATA) => Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "named-pipe peer disconnected",
                )),
                _ => Err(error),
            }
        }

        fn raw_handle(&self) -> HANDLE {
            self.handle.as_raw_handle()
        }
    }

    impl Drop for PipeConnection {
        fn drop(&mut self) {
            if self.server_end {
                // SAFETY: handle is a live server-side named-pipe instance owned by self.
                unsafe {
                    DisconnectNamedPipe(self.raw_handle());
                }
            }
        }
    }

    pub struct CorePipeServer {
        listener: PipeListener,
    }

    impl CorePipeServer {
        pub fn bind() -> io::Result<Self> {
            PipeListener::bind(CORE_PIPE_NAME, SessionPolicy::management_ui(), 4)
                .map(|listener| Self { listener })
        }

        pub const fn endpoint(&self) -> &'static str {
            CORE_PIPE_NAME
        }

        pub fn listener(&self) -> &PipeListener {
            &self.listener
        }
    }

    fn endpoint_wide(endpoint: &str) -> io::Result<Vec<u16>> {
        if !endpoint.starts_with(r"\\.\pipe\") || endpoint.encode_utf16().any(|value| value == 0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "endpoint must be a local named-pipe path without embedded nulls",
            ));
        }
        Ok(OsStr::new(endpoint).encode_wide().chain(Some(0)).collect())
    }

    fn create_pipe_instance(
        endpoint: &[u16],
        max_sessions: usize,
        first: bool,
    ) -> io::Result<OwnedHandle> {
        let security = PipeSecurity::current_user()?;
        let attributes = security.attributes();
        let mut open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
        if first {
            open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let pipe_mode =
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
        // SAFETY: endpoint is terminated and attributes references a live descriptor.
        let raw_pipe = unsafe {
            CreateNamedPipeW(
                endpoint.as_ptr(),
                open_mode,
                pipe_mode,
                max_sessions as u32,
                MAX_FRAME_LEN as u32,
                MAX_FRAME_LEN as u32,
                0,
                &attributes,
            )
        };
        if raw_pipe == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateNamedPipeW returned a newly owned valid kernel handle.
        Ok(unsafe { OwnedHandle::from_raw_handle(raw_pipe) })
    }

    fn connect_named_pipe(pipe: &OwnedHandle, cancel: &CancellationToken) -> io::Result<()> {
        let event = create_event()?;
        let mut overlapped = OVERLAPPED {
            hEvent: raw_handle(&event),
            ..OVERLAPPED::default()
        };
        // SAFETY: pipe and OVERLAPPED remain live until the connect completes or is cancelled.
        let connected = unsafe { ConnectNamedPipe(pipe.as_raw_handle(), &mut overlapped) };
        if connected != 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error().map(|value| value as u32) {
            Some(ERROR_PIPE_CONNECTED) => Ok(()),
            Some(ERROR_IO_PENDING) => {
                wait_for_operation(pipe.as_raw_handle(), &overlapped, &event, cancel, None)
                    .map(|_| ())
            }
            _ => Err(error),
        }
    }

    fn create_event() -> io::Result<OwnedHandle> {
        // SAFETY: default security, unnamed event, and valid Boolean arguments are used.
        let raw_event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if raw_event.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateEventW returned a newly owned valid kernel handle.
        Ok(unsafe { OwnedHandle::from_raw_handle(raw_event) })
    }

    fn wait_for_operation(
        pipe: HANDLE,
        overlapped: &OVERLAPPED,
        event: &OwnedHandle,
        cancel: &CancellationToken,
        deadline: Option<Instant>,
    ) -> io::Result<u32> {
        loop {
            if cancel.is_cancelled() {
                cancel_operation(pipe, overlapped);
                return Err(cancelled());
            }
            let wait_for = match deadline {
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        cancel_operation(pipe, overlapped);
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "named-pipe operation timed out",
                        ));
                    }
                    remaining.min(OPERATION_POLL_INTERVAL)
                }
                None => OPERATION_POLL_INTERVAL,
            };
            let wait_ms = u32::try_from(wait_for.as_millis().max(1)).unwrap_or(u32::MAX);
            // SAFETY: event is a live kernel event handle.
            match unsafe { WaitForSingleObject(raw_handle(event), wait_ms) } {
                WAIT_OBJECT_0 => {
                    let mut transferred = 0_u32;
                    // SAFETY: the operation completed and all referenced storage remains live.
                    if unsafe { GetOverlappedResult(pipe, overlapped, &mut transferred, 0) } != 0 {
                        return Ok(transferred);
                    }
                    let error = io::Error::last_os_error();
                    if error.raw_os_error().map(|value| value as u32)
                        == Some(ERROR_OPERATION_ABORTED)
                    {
                        return Err(cancelled());
                    }
                    return Err(error);
                }
                WAIT_TIMEOUT => {}
                _ => {
                    cancel_operation(pipe, overlapped);
                    return Err(io::Error::last_os_error());
                }
            }
        }
    }

    fn cancel_operation(pipe: HANDLE, overlapped: &OVERLAPPED) {
        // SAFETY: pipe and OVERLAPPED identify the live operation being cancelled.
        unsafe {
            CancelIoEx(pipe, overlapped);
            let mut ignored = 0_u32;
            GetOverlappedResult(pipe, overlapped, &mut ignored, 1);
        }
    }

    fn raw_handle(handle: &OwnedHandle) -> HANDLE {
        handle.as_raw_handle()
    }

    fn cancelled() -> io::Error {
        io::Error::new(io::ErrorKind::Interrupted, "named-pipe operation cancelled")
    }

    fn is_pipe_closed(error: &io::Error) -> bool {
        matches!(
            error.raw_os_error().map(|value| value as u32),
            Some(ERROR_BROKEN_PIPE) | Some(ERROR_NO_DATA)
        )
    }
}

#[cfg(windows)]
pub use windows_impl::{CorePipeServer, PipeConnection, PipeListener};

#[cfg(not(windows))]
mod unsupported {
    use std::{io, time::Duration};

    use pal_protocol::MAX_PAYLOAD_LEN;
    use tokio_util::sync::CancellationToken;

    use crate::{framed_stream::FramedStream, handshake::SessionPolicy};

    pub struct PipeListener;
    #[derive(Debug)]
    pub struct PipeConnection;
    pub struct CorePipeServer;

    impl PipeListener {
        pub fn bind(
            _endpoint: &str,
            _policy: SessionPolicy,
            _max_sessions: usize,
        ) -> io::Result<Self> {
            Err(unsupported())
        }

        pub fn accept(&self, _cancel: &CancellationToken) -> io::Result<PipeConnection> {
            Err(unsupported())
        }

        pub fn endpoint(&self) -> &str {
            crate::CORE_PIPE_NAME
        }

        pub const fn max_sessions(&self) -> usize {
            0
        }
    }

    impl PipeConnection {
        pub(crate) fn read_chunk(
            &mut self,
            _decoder: &mut FramedStream,
            _cancel: &CancellationToken,
            _deadline: Option<std::time::Instant>,
        ) -> io::Result<usize> {
            Err(unsupported())
        }

        pub(crate) fn write_all_bounded(
            &mut self,
            frame: &[u8],
            _cancel: &CancellationToken,
            _timeout: Duration,
        ) -> io::Result<()> {
            if frame.len() > MAX_PAYLOAD_LEN + 4 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "frame exceeds the bounded payload limit",
                ));
            }
            Err(unsupported())
        }
    }

    impl CorePipeServer {
        pub fn bind() -> io::Result<Self> {
            Err(unsupported())
        }

        pub const fn endpoint(&self) -> &'static str {
            crate::CORE_PIPE_NAME
        }
    }

    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows named pipes are unavailable",
        )
    }
}

#[cfg(not(windows))]
pub use unsupported::{CorePipeServer, PipeConnection, PipeListener};
