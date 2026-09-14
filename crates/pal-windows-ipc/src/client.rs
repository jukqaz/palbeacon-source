#[cfg(windows)]
mod windows_impl {
    use std::{
        ffi::OsStr,
        io,
        os::windows::{
            ffi::OsStrExt,
            io::{FromRawHandle, OwnedHandle},
        },
        ptr, thread,
        time::{Duration, Instant},
    };

    use pal_protocol::{
        FrameCodec, PROTOCOL_VERSION,
        v2::{ClientRole, LocalEnvelope, local_envelope::Payload},
        validate_local_envelope,
    };
    use tokio_util::sync::CancellationToken;
    use windows_sys::Win32::{
        Foundation::{
            ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_SEM_TIMEOUT, GENERIC_READ, GENERIC_WRITE,
            INVALID_HANDLE_VALUE,
        },
        Storage::FileSystem::{CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING},
        System::Pipes::WaitNamedPipeW,
    };

    use crate::{
        framed_stream::FramedStream,
        handshake::SessionPolicy,
        server::PipeConnection,
        session::{PipeError, map_read_error, map_write_error},
    };

    pub struct PipeClient {
        connection: PipeConnection,
        decoder: FramedStream,
        connection_id: [u8; 16],
    }

    impl PipeClient {
        pub fn connect(
            endpoint: &str,
            hello: LocalEnvelope,
            timeout: Duration,
        ) -> Result<Self, PipeError> {
            validate_local_envelope(&hello)?;
            let Some(Payload::ClientHello(client_hello)) = hello.payload.as_ref() else {
                return Err(invalid_input("initial client payload must be ClientHello"));
            };
            let role = ClientRole::try_from(client_hello.role)
                .map_err(|_| invalid_input("client hello role is invalid"))?;
            let policy = match role {
                ClientRole::ManagementUi => SessionPolicy::management_ui(),
                ClientRole::Overlay => SessionPolicy::overlay(),
                ClientRole::Unspecified => {
                    return Err(invalid_input("client hello role is unspecified"));
                }
            };
            let connection_id: [u8; 16] = hello
                .connection_id
                .as_slice()
                .try_into()
                .map_err(|_| invalid_input("connection ID must contain sixteen bytes"))?;
            let endpoint_wide = endpoint_wide(endpoint)?;
            let deadline = Instant::now()
                .checked_add(timeout)
                .ok_or_else(|| invalid_input("connect timeout overflow"))?;
            let raw_pipe = loop {
                let wait = remaining_connect(deadline)?;
                // SAFETY: endpoint_wide is terminated and remains live for the call.
                if unsafe { WaitNamedPipeW(endpoint_wide.as_ptr(), duration_ms(wait)) } == 0 {
                    let error = io::Error::last_os_error();
                    if is_retryable_connect_error(&error) {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    return Err(map_connect_error(error));
                }
                // SAFETY: endpoint is terminated, no inheritable security attributes are supplied,
                // and the returned handle is checked before ownership is assumed.
                let candidate = unsafe {
                    CreateFileW(
                        endpoint_wide.as_ptr(),
                        GENERIC_READ | GENERIC_WRITE,
                        0,
                        ptr::null(),
                        OPEN_EXISTING,
                        FILE_FLAG_OVERLAPPED,
                        ptr::null_mut(),
                    )
                };
                if candidate != INVALID_HANDLE_VALUE {
                    break candidate;
                }
                let error = io::Error::last_os_error();
                if !is_retryable_connect_error(&error) {
                    return Err(map_connect_error(error));
                }
                thread::sleep(Duration::from_millis(5));
            };
            // SAFETY: CreateFileW returned a newly owned valid kernel handle.
            let handle = unsafe { OwnedHandle::from_raw_handle(raw_pipe) };
            let mut connection = PipeConnection::from_client(handle, policy);
            let cancel = CancellationToken::new();
            let frame = FrameCodec::encode(&hello)?;
            connection
                .write_all_bounded(&frame, &cancel, timeout)
                .map_err(map_write_error)?;
            Ok(Self {
                connection,
                decoder: FramedStream::new(),
                connection_id,
            })
        }

        pub fn request(
            &mut self,
            request: LocalEnvelope,
            timeout: Duration,
        ) -> Result<LocalEnvelope, PipeError> {
            validate_local_envelope(&request)?;
            if request.protocol_version != PROTOCOL_VERSION
                || request.connection_id.as_slice() != self.connection_id
            {
                return Err(PipeError::ResponseMismatch);
            }
            let request_id = request.message_id;
            let deadline = Instant::now()
                .checked_add(timeout)
                .ok_or_else(|| invalid_input("request timeout overflow"))?;
            let cancel = CancellationToken::new();
            let frame = FrameCodec::encode(&request)?;
            self.connection
                .write_all_bounded(&frame, &cancel, remaining(deadline)?)
                .map_err(map_write_error)?;

            loop {
                if let Some(response) = self.decoder.next_message::<LocalEnvelope>()? {
                    validate_local_envelope(&response)?;
                    if response.protocol_version != PROTOCOL_VERSION
                        || response.connection_id.as_slice() != self.connection_id
                        || response.reply_to_message_id != Some(request_id)
                    {
                        return Err(PipeError::ResponseMismatch);
                    }
                    return Ok(response);
                }
                let count = self
                    .connection
                    .read_chunk(&mut self.decoder, &cancel, Some(deadline))
                    .map_err(map_read_error)?;
                if count == 0 {
                    return Err(PipeError::Io(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "named-pipe server closed before responding",
                    )));
                }
            }
        }
    }

    fn endpoint_wide(endpoint: &str) -> Result<Vec<u16>, PipeError> {
        if !endpoint.starts_with(r"\\.\pipe\") || endpoint.encode_utf16().any(|value| value == 0) {
            return Err(invalid_input(
                "endpoint must be a local named-pipe path without embedded nulls",
            ));
        }
        Ok(OsStr::new(endpoint).encode_wide().chain(Some(0)).collect())
    }

    fn duration_ms(duration: Duration) -> u32 {
        u32::try_from(duration.as_millis().max(1)).unwrap_or(u32::MAX)
    }

    fn remaining(deadline: Instant) -> Result<Duration, PipeError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(PipeError::WriteTimeout)
        } else {
            Ok(remaining)
        }
    }

    fn remaining_connect(deadline: Instant) -> Result<Duration, PipeError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(PipeError::ReadTimeout)
        } else {
            Ok(remaining)
        }
    }

    fn is_retryable_connect_error(error: &io::Error) -> bool {
        matches!(
            error.raw_os_error().map(|value| value as u32),
            Some(ERROR_PIPE_BUSY) | Some(ERROR_FILE_NOT_FOUND)
        )
    }

    fn map_connect_error(error: io::Error) -> PipeError {
        if error.kind() == io::ErrorKind::TimedOut
            || error.raw_os_error().map(|value| value as u32) == Some(ERROR_SEM_TIMEOUT)
        {
            PipeError::ReadTimeout
        } else {
            PipeError::Io(error)
        }
    }

    fn invalid_input(message: &'static str) -> PipeError {
        PipeError::Io(io::Error::new(io::ErrorKind::InvalidInput, message))
    }
}

#[cfg(windows)]
pub use windows_impl::PipeClient;

#[cfg(not(windows))]
pub struct PipeClient;

#[cfg(not(windows))]
impl PipeClient {
    pub fn connect(
        _endpoint: &str,
        _hello: pal_protocol::v2::LocalEnvelope,
        _timeout: std::time::Duration,
    ) -> Result<Self, crate::session::PipeError> {
        Err(crate::session::PipeError::Io(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Windows named pipes are unavailable",
        )))
    }

    pub fn request(
        &mut self,
        _request: pal_protocol::v2::LocalEnvelope,
        _timeout: std::time::Duration,
    ) -> Result<pal_protocol::v2::LocalEnvelope, crate::session::PipeError> {
        Err(crate::session::PipeError::Io(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Windows named pipes are unavailable",
        )))
    }
}
