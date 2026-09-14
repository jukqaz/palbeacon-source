use std::{
    io,
    thread::{self, JoinHandle},
};

use pal_protocol::v2::LocalEnvelope;
use pal_windows_ipc::{
    CORE_PIPE_NAME,
    handshake::SessionPolicy,
    server::PipeListener,
    session::{CancellationToken, LocalSession, PipeError, SessionFailure, run_session},
};
use thiserror::Error;

use crate::control_session::{ControlSession, SharedSettingsStore};

const MAX_CORE_SESSIONS: usize = 4;

#[derive(Debug, Error)]
pub enum IpcRuntimeError {
    #[error("core named-pipe listener could not be created")]
    Bind(#[source] io::Error),
    #[error("core IPC supervisor thread could not be created")]
    Spawn(#[source] io::Error),
    #[error("core IPC accept failed")]
    Accept(#[source] io::Error),
    #[error("core IPC supervisor thread panicked")]
    SupervisorPanicked,
    #[error("core IPC supervisor stopped unexpectedly")]
    SupervisorStopped,
    #[error("core IPC worker thread panicked")]
    WorkerPanicked,
}

pub struct CoreIpcRuntime {
    cancel: CancellationToken,
    supervisor: Option<JoinHandle<Result<(), IpcRuntimeError>>>,
}

impl CoreIpcRuntime {
    pub fn start(settings: SharedSettingsStore) -> Result<Self, IpcRuntimeError> {
        Self::start_at(CORE_PIPE_NAME, settings)
    }

    pub fn start_at(
        endpoint: &str,
        settings: SharedSettingsStore,
    ) -> Result<Self, IpcRuntimeError> {
        let listener = PipeListener::bind(
            endpoint,
            SessionPolicy::any_local_client(),
            MAX_CORE_SESSIONS,
        )
        .map_err(IpcRuntimeError::Bind)?;
        let cancel = CancellationToken::new();
        let supervisor_cancel = cancel.clone();
        let supervisor = thread::Builder::new()
            .name("pal-core-ipc-supervisor".to_owned())
            .spawn(move || supervise(listener, settings, supervisor_cancel))
            .map_err(IpcRuntimeError::Spawn)?;
        Ok(Self {
            cancel,
            supervisor: Some(supervisor),
        })
    }

    pub fn shutdown(mut self) -> Result<(), IpcRuntimeError> {
        self.shutdown_inner()
    }

    pub fn check_health(&mut self) -> Result<(), IpcRuntimeError> {
        let finished = self
            .supervisor
            .as_ref()
            .is_some_and(JoinHandle::is_finished);
        if !finished {
            return Ok(());
        }
        let supervisor = self
            .supervisor
            .take()
            .ok_or(IpcRuntimeError::SupervisorStopped)?;
        match supervisor.join() {
            Ok(Ok(())) => Err(IpcRuntimeError::SupervisorStopped),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(IpcRuntimeError::SupervisorPanicked),
        }
    }

    fn shutdown_inner(&mut self) -> Result<(), IpcRuntimeError> {
        self.cancel.cancel();
        let Some(supervisor) = self.supervisor.take() else {
            return Ok(());
        };
        supervisor
            .join()
            .map_err(|_| IpcRuntimeError::SupervisorPanicked)?
    }
}

impl Drop for CoreIpcRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown_inner();
    }
}

struct ControlSessionAdapter {
    inner: ControlSession,
}

impl ControlSessionAdapter {
    fn new(settings: SharedSettingsStore) -> Self {
        Self {
            inner: ControlSession::from_shared(settings),
        }
    }
}

impl LocalSession for ControlSessionAdapter {
    fn handle(&mut self, envelope: LocalEnvelope) -> Result<Option<Vec<u8>>, SessionFailure> {
        self.inner
            .handle_envelope(envelope)
            .map_err(SessionFailure::other)
    }

    fn disconnected(&mut self) {
        let _ = self.inner.disconnect();
    }
}

fn supervise(
    listener: PipeListener,
    settings: SharedSettingsStore,
    cancel: CancellationToken,
) -> Result<(), IpcRuntimeError> {
    let mut workers: Vec<JoinHandle<Result<(), PipeError>>> =
        Vec::with_capacity(listener.max_sessions());
    let accept_result = (|| {
        while !cancel.is_cancelled() {
            reap_completed(&mut workers)?;
            let connection = match listener.accept(&cancel) {
                Ok(connection) => connection,
                Err(error)
                    if cancel.is_cancelled() || error.kind() == io::ErrorKind::Interrupted =>
                {
                    break;
                }
                Err(error) => return Err(IpcRuntimeError::Accept(error)),
            };
            reap_completed(&mut workers)?;
            if workers.len() == listener.max_sessions() {
                drop(connection);
                continue;
            }
            let worker_settings = settings.clone();
            let worker_cancel = cancel.clone();
            let worker = thread::Builder::new()
                .name("pal-core-ipc-session".to_owned())
                .spawn(move || {
                    run_session(
                        connection,
                        ControlSessionAdapter::new(worker_settings),
                        worker_cancel,
                    )
                })
                .map_err(IpcRuntimeError::Spawn)?;
            workers.push(worker);
        }
        Ok(())
    })();

    cancel.cancel();
    let join_result = join_workers(workers);
    accept_result?;
    join_result
}

fn join_workers(workers: Vec<JoinHandle<Result<(), PipeError>>>) -> Result<(), IpcRuntimeError> {
    let mut worker_panicked = false;
    for worker in workers {
        match worker.join() {
            Ok(result) => drop(result),
            Err(_) => worker_panicked = true,
        }
    }
    if worker_panicked {
        Err(IpcRuntimeError::WorkerPanicked)
    } else {
        Ok(())
    }
}

fn reap_completed(
    workers: &mut Vec<JoinHandle<Result<(), PipeError>>>,
) -> Result<(), IpcRuntimeError> {
    let mut index = 0;
    while index < workers.len() {
        if workers[index].is_finished() {
            let worker = workers.swap_remove(index);
            drop(worker.join().map_err(|_| IpcRuntimeError::WorkerPanicked)?);
        } else {
            index += 1;
        }
    }
    Ok(())
}
