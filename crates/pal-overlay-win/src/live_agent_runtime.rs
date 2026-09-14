use std::{ffi::OsString, fmt, path::PathBuf, sync::mpsc, thread, time::Duration};

use pal_state::{ServerAgentPositionSource, server_agent_channel};
use pal_telemetry::{MonotonicEpoch, NetworkConsumer, NetworkConsumerConfig, NetworkConsumerError};
use thiserror::Error;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum LiveAgentRuntimeError {
    #[error("live Agent identity is invalid")]
    InvalidIdentity,
    #[error("live Agent network consumer is invalid")]
    InvalidConsumer,
    #[error("live Agent runtime initialization failed")]
    RuntimeInitialization,
    #[error("live Agent network thread could not start")]
    ThreadStart,
    #[error("live Agent network thread failed")]
    ThreadFailure,
    #[error("live Agent network shutdown exceeded its bounded wait")]
    ShutdownTimeout,
}

const SHUTDOWN_WAIT: Duration = Duration::from_millis(400);

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum LiveAgentCommandError {
    #[error("development live Agent command is invalid")]
    Invalid,
}

pub struct LiveAgentCommand {
    config_path: PathBuf,
}

impl fmt::Debug for LiveAgentCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED LIVE AGENT COMMAND]")
    }
}

impl LiveAgentCommand {
    /// Accepts only a protected configuration file path. Endpoints, identities, and mTLS
    /// credentials intentionally have no command-line representation.
    pub fn parse<I, S>(arguments: I) -> Result<Self, LiveAgentCommandError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut arguments = arguments.into_iter().map(Into::into);
        if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--live-agent-config")) {
            return Err(LiveAgentCommandError::Invalid);
        }
        let config_path = PathBuf::from(arguments.next().ok_or(LiveAgentCommandError::Invalid)?);
        if config_path.as_os_str().is_empty()
            || !config_path.is_absolute()
            || arguments.next().is_some()
        {
            return Err(LiveAgentCommandError::Invalid);
        }
        Ok(Self { config_path })
    }

    pub fn config_path(&self) -> &std::path::Path {
        &self.config_path
    }
}

/// Owns the cancellation and join boundary for the development live Agent connection.
///
/// The network loop runs on one dedicated current-thread Tokio runtime. Explicit shutdown waits
/// at most 400 ms. Drop never waits on the GUI thread: a still-running network thread is handed
/// to a background reaper, which retains and eventually releases credential-bearing state.
pub struct LiveAgentSupervisor {
    cancellation: watch::Sender<bool>,
    completion: mpsc::Receiver<Result<(), NetworkConsumerError>>,
    join: Option<thread::JoinHandle<Result<(), NetworkConsumerError>>>,
}

impl fmt::Debug for LiveAgentSupervisor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED LIVE AGENT SUPERVISOR]")
    }
}

impl LiveAgentSupervisor {
    pub fn is_finished(&self) -> bool {
        self.join
            .as_ref()
            .is_none_or(thread::JoinHandle::is_finished)
    }

    pub fn shutdown(mut self) -> Result<(), LiveAgentRuntimeError> {
        self.cancel_and_join_bounded()
    }

    fn cancel_and_join_bounded(&mut self) -> Result<(), LiveAgentRuntimeError> {
        let _ = self.cancellation.send(true);
        if self.join.is_none() {
            return Ok(());
        }
        match self.completion.recv_timeout(SHUTDOWN_WAIT) {
            Ok(result) => {
                if !self
                    .join
                    .as_ref()
                    .is_some_and(thread::JoinHandle::is_finished)
                {
                    self.reap_in_background();
                    return result.map_err(|_| LiveAgentRuntimeError::ThreadFailure);
                }
                let join = self.join.take().expect("join handle checked above");
                let joined = join
                    .join()
                    .map_err(|_| LiveAgentRuntimeError::ThreadFailure)?;
                debug_assert_eq!(result, joined);
                result.map_err(|_| LiveAgentRuntimeError::ThreadFailure)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.reap_in_background();
                Err(LiveAgentRuntimeError::ShutdownTimeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if !self
                    .join
                    .as_ref()
                    .is_some_and(thread::JoinHandle::is_finished)
                {
                    self.reap_in_background();
                    return Err(LiveAgentRuntimeError::ThreadFailure);
                }
                let join = self.join.take().expect("join handle checked above");
                match join.join() {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(_)) | Err(_) => Err(LiveAgentRuntimeError::ThreadFailure),
                }
            }
        }
    }

    fn reap_in_background(&mut self) {
        let Some(join) = self.join.take() else {
            return;
        };
        let _ = thread::Builder::new()
            .name("pal-live-agent-reaper".to_owned())
            .spawn(move || {
                let _ = join.join();
            });
    }

    fn finish_if_ready(&mut self) -> Result<(), LiveAgentRuntimeError> {
        let Some(join) = self.join.as_ref() else {
            return Ok(());
        };
        if !join.is_finished() {
            self.reap_in_background();
            return Ok(());
        }
        let join = self.join.take().expect("join handle checked above");
        match join.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) | Err(_) => Err(LiveAgentRuntimeError::ThreadFailure),
        }
    }
}

impl Drop for LiveAgentSupervisor {
    fn drop(&mut self) {
        let _ = self.cancellation.send(true);
        let _ = self.finish_if_ready();
    }
}

pub struct RunningLiveAgent {
    pub source: ServerAgentPositionSource,
    pub monotonic_epoch: MonotonicEpoch,
    pub supervisor: LiveAgentSupervisor,
}

pub fn start_live_agent(
    network_config: NetworkConsumerConfig,
    world_alias: &str,
    subject_id: [u8; 32],
) -> Result<RunningLiveAgent, LiveAgentRuntimeError> {
    let (sender, source) = server_agent_channel(world_alias, subject_id)
        .map_err(|_| LiveAgentRuntimeError::InvalidIdentity)?;
    let monotonic_epoch = MonotonicEpoch::now();
    let consumer = NetworkConsumer::new_at(network_config, sender, 1, monotonic_epoch)
        .map_err(|_| LiveAgentRuntimeError::InvalidConsumer)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| LiveAgentRuntimeError::RuntimeInitialization)?;
    let (cancellation, receiver) = watch::channel(false);
    let (completion_sender, completion) = mpsc::sync_channel(1);
    let join = thread::Builder::new()
        .name("pal-live-agent".to_owned())
        .spawn(move || {
            let result = runtime.block_on(consumer.run_until_cancelled(receiver));
            let _ = completion_sender.send(result);
            result
        })
        .map_err(|_| LiveAgentRuntimeError::ThreadStart)?;

    Ok(RunningLiveAgent {
        source,
        monotonic_epoch,
        supervisor: LiveAgentSupervisor {
            cancellation,
            completion,
            join: Some(join),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_accepts_only_a_config_path_and_redacts_it() {
        let command = LiveAgentCommand::parse(["--live-agent-config", "C:/private/pal-live.toml"])
            .expect("command");
        assert_eq!(
            command.config_path(),
            std::path::Path::new("C:/private/pal-live.toml")
        );
        assert_eq!(format!("{command:?}"), "[REDACTED LIVE AGENT COMMAND]");

        for invalid in [
            vec!["--endpoint", "https://secret.invalid"],
            vec!["--live-agent-config"],
            vec!["--live-agent-config", "relative-config.toml"],
            vec!["--live-agent-config", "config.toml", "--map", "map.bmp"],
        ] {
            assert_eq!(
                LiveAgentCommand::parse(invalid).unwrap_err(),
                LiveAgentCommandError::Invalid
            );
        }
    }
}
