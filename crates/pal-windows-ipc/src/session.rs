use std::{fmt, io, time::Duration};

use pal_protocol::{
    FrameError, LocalValidationError, MAX_PAYLOAD_LEN, v2::LocalEnvelope, validate_local_envelope,
};
use thiserror::Error;
pub use tokio_util::sync::CancellationToken;

use crate::{
    framed_stream::{FramedStream, StreamError},
    server::PipeConnection,
};

const SESSION_WRITE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub struct SessionFailure {
    message: String,
}

impl SessionFailure {
    pub fn other(error: impl fmt::Debug) -> Self {
        Self {
            message: format!("{error:?}"),
        }
    }
}

impl fmt::Display for SessionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SessionFailure {}

#[derive(Debug, Error)]
pub enum PipeError {
    #[error("named-pipe I/O failed")]
    Io(#[source] io::Error),
    #[error("framed stream failed")]
    Stream(#[from] StreamError),
    #[error("frame encoding or decoding failed")]
    Codec(#[from] FrameError),
    #[error("local envelope validation failed")]
    InvalidEnvelope(#[from] LocalValidationError),
    #[error("local session failed")]
    Session(#[from] SessionFailure),
    #[error("response frame is too large: {0} bytes")]
    FrameTooLarge(usize),
    #[error("named-pipe write timed out")]
    WriteTimeout,
    #[error("named-pipe read timed out")]
    ReadTimeout,
    #[error("named-pipe operation was cancelled")]
    Cancelled,
    #[error("response does not correlate to the active connection and request")]
    ResponseMismatch,
}

pub trait LocalSession: Send + 'static {
    fn handle(&mut self, envelope: LocalEnvelope) -> Result<Option<Vec<u8>>, SessionFailure>;

    fn disconnected(&mut self);
}

pub fn run_session<S: LocalSession>(
    mut connection: PipeConnection,
    mut session: S,
    cancel: CancellationToken,
) -> Result<(), PipeError> {
    let result = run_session_inner(&mut connection, &mut session, &cancel);
    session.disconnected();
    result
}

fn run_session_inner<S: LocalSession>(
    connection: &mut PipeConnection,
    session: &mut S,
    cancel: &CancellationToken,
) -> Result<(), PipeError> {
    let mut decoder = FramedStream::new();
    while !cancel.is_cancelled() {
        let count = connection
            .read_chunk(&mut decoder, cancel, None)
            .map_err(map_read_error)?;
        if count == 0 {
            break;
        }
        while let Some(envelope) = decoder.next_message::<LocalEnvelope>()? {
            validate_local_envelope(&envelope)?;
            if let Some(frame) = session.handle(envelope)? {
                if frame.len() > MAX_PAYLOAD_LEN + 4 {
                    return Err(PipeError::FrameTooLarge(frame.len()));
                }
                connection
                    .write_all_bounded(&frame, cancel, SESSION_WRITE_TIMEOUT)
                    .map_err(map_write_error)?;
            }
        }
    }
    if cancel.is_cancelled() {
        return Err(PipeError::Cancelled);
    }
    decoder.finish()?;
    Ok(())
}

pub(crate) fn map_read_error(error: io::Error) -> PipeError {
    match error.kind() {
        io::ErrorKind::Interrupted => PipeError::Cancelled,
        io::ErrorKind::TimedOut => PipeError::ReadTimeout,
        _ => PipeError::Io(error),
    }
}

pub(crate) fn map_write_error(error: io::Error) -> PipeError {
    match error.kind() {
        io::ErrorKind::Interrupted => PipeError::Cancelled,
        io::ErrorKind::TimedOut => PipeError::WriteTimeout,
        _ => PipeError::Io(error),
    }
}
