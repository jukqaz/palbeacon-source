use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use pal_rest::{
    PalRestClient, PlayerSelector, Pseudonymizer, RestError, SafePlayerObservation,
    SanitizedServerInfo, TimedResponse,
};
use thiserror::Error;
use tokio::{
    sync::watch,
    time::{MissedTickBehavior, interval},
};

use crate::{AgentPipeline, HealthMonitor, LiveProcessError, LiveServerIdentity};

const REMOTE_IDENTITY_RECHECK_INTERVAL: Duration = Duration::from_secs(30);

pub trait GameDataSource: Send + Sync + 'static {
    fn revalidate_trust(&self) -> Result<(), SourceTrustError>;

    fn game_data(
        &self,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<TimedResponse<SafePlayerObservation>, RestError>>
                + Send
                + '_,
        >,
    >;
}

pub struct RestGameDataSource {
    client: Arc<PalRestClient>,
    selector: PlayerSelector,
    pseudonymizer: Arc<Pseudonymizer>,
    world_alias: String,
    trust: RestTrustBinding,
}

impl RestGameDataSource {
    pub fn new_local_process(
        client: Arc<PalRestClient>,
        selector: PlayerSelector,
        pseudonymizer: Arc<Pseudonymizer>,
        world_alias: impl Into<String>,
        live_server: Arc<LiveServerIdentity>,
        rest_base_url: impl Into<String>,
    ) -> Self {
        Self {
            client,
            selector,
            pseudonymizer,
            world_alias: world_alias.into(),
            trust: RestTrustBinding::LocalProcess {
                live_server,
                rest_base_url: rest_base_url.into(),
            },
        }
    }

    /// Creates a source for a client that was constructed with
    /// `PalRestClient::new_explicit_remote_https`.
    ///
    /// Remote identity is enforced by the exact HTTPS endpoint policy, TLS
    /// hostname validation, and startup `/info` binding. There is no local
    /// listener process to revalidate around each request.
    pub fn new_explicit_remote_https(
        client: Arc<PalRestClient>,
        selector: PlayerSelector,
        pseudonymizer: Arc<Pseudonymizer>,
        world_alias: impl Into<String>,
        expected_info: SanitizedServerInfo,
    ) -> Self {
        Self {
            client,
            selector,
            pseudonymizer,
            world_alias: world_alias.into(),
            trust: RestTrustBinding::ExplicitRemoteHttps(RemoteIdentityCheck::new(expected_info)),
        }
    }

    async fn revalidate_remote_identity_if_due(&self) -> Result<(), RestError> {
        let RestTrustBinding::ExplicitRemoteHttps(identity) = &self.trust else {
            return Ok(());
        };
        if !identity.is_due(Instant::now()) {
            return Ok(());
        }
        let current = self
            .client
            .info(self.pseudonymizer.as_ref(), &self.world_alias)
            .await?
            .value;
        identity.confirm(&current, Instant::now())
    }
}

impl GameDataSource for RestGameDataSource {
    fn revalidate_trust(&self) -> Result<(), SourceTrustError> {
        self.trust.revalidate()
    }

    fn game_data(
        &self,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<TimedResponse<SafePlayerObservation>, RestError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            self.revalidate_remote_identity_if_due().await?;
            self.client
                .game_data(
                    &self.selector,
                    self.pseudonymizer.as_ref(),
                    &self.world_alias,
                )
                .await
        })
    }
}

enum RestTrustBinding {
    LocalProcess {
        live_server: Arc<LiveServerIdentity>,
        rest_base_url: String,
    },
    ExplicitRemoteHttps(RemoteIdentityCheck),
}

impl RestTrustBinding {
    fn revalidate(&self) -> Result<(), SourceTrustError> {
        match self {
            Self::LocalProcess {
                live_server,
                rest_base_url,
            } => live_server
                .revalidate(rest_base_url)
                .map_err(SourceTrustError::from),
            Self::ExplicitRemoteHttps(_) => Ok(()),
        }
    }
}

struct RemoteIdentityCheck {
    expected: SanitizedServerInfo,
    last_verified: Mutex<Instant>,
}

impl RemoteIdentityCheck {
    fn new(expected: SanitizedServerInfo) -> Self {
        Self::new_at(expected, Instant::now())
    }

    fn new_at(expected: SanitizedServerInfo, verified_at: Instant) -> Self {
        Self {
            expected,
            last_verified: Mutex::new(verified_at),
        }
    }

    fn is_due(&self, now: Instant) -> bool {
        now.saturating_duration_since(*self.last_verified()) >= REMOTE_IDENTITY_RECHECK_INTERVAL
    }

    fn confirm(&self, current: &SanitizedServerInfo, now: Instant) -> Result<(), RestError> {
        if current != &self.expected {
            return Err(RestError::ConnectionUntrusted);
        }
        *self.last_verified() = now;
        Ok(())
    }

    fn last_verified(&self) -> std::sync::MutexGuard<'_, Instant> {
        self.last_verified
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub struct Poller<S> {
    poll_interval: Duration,
    source: Arc<S>,
    pipeline: AgentPipeline,
    health: HealthMonitor,
}

impl<S> Poller<S>
where
    S: GameDataSource,
{
    pub fn new(
        poll_interval: Duration,
        source: Arc<S>,
        pipeline: AgentPipeline,
        health: HealthMonitor,
    ) -> Result<Self, PollerBuildError> {
        if !matches!(poll_interval.as_millis(), 500 | 1_000 | 2_000) {
            return Err(PollerBuildError::InvalidInterval);
        }
        Ok(Self {
            poll_interval,
            source,
            pipeline,
            health,
        })
    }

    pub async fn run(mut self, mut shutdown: watch::Receiver<bool>) -> PollerExit {
        let mut ticks = interval(self.poll_interval);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = ticks.tick() => {}
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return PollerExit::Shutdown;
                    }
                    continue;
                }
            }
            if self.source.revalidate_trust().is_err() {
                return PollerExit::TrustLost;
            }
            let result = tokio::select! {
                result = self.source.game_data() => result,
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return PollerExit::Shutdown;
                    }
                    continue;
                }
            };
            if matches!(result, Err(RestError::ConnectionUntrusted)) {
                return PollerExit::TrustLost;
            }
            if self.source.revalidate_trust().is_err() {
                return PollerExit::TrustLost;
            }
            match result {
                Ok(sample) => {
                    self.health.record_poll_success();
                    if self.pipeline.emit(sample).is_err() {
                        continue;
                    }
                }
                Err(error) => self.health.record_poll_failure(&error),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PollerExit {
    Shutdown,
    TrustLost,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum PollerBuildError {
    #[error("poll interval is not approved by Gate A")]
    InvalidInterval,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SourceTrustError {
    #[error("local REST process trust was lost")]
    LocalProcess,
}

impl From<LiveProcessError> for SourceTrustError {
    fn from(_: LiveProcessError) -> Self {
        Self::LocalProcess
    }
}

#[cfg(test)]
mod tests {
    use super::{REMOTE_IDENTITY_RECHECK_INTERVAL, RemoteIdentityCheck};
    use pal_rest::{RestError, SanitizedServerInfo};
    use std::time::{Duration, Instant};

    fn info(version: &str, subject: u8) -> SanitizedServerInfo {
        SanitizedServerInfo {
            version: version.to_owned(),
            server_subject_id: format!("{subject:02x}").repeat(32),
        }
    }

    #[test]
    fn remote_identity_is_rechecked_periodically_and_mismatch_fails_closed() {
        let started = Instant::now();
        let check = RemoteIdentityCheck::new_at(info("v1", 0x42), started);
        assert!(
            !check.is_due(started + REMOTE_IDENTITY_RECHECK_INTERVAL - Duration::from_millis(1))
        );
        assert!(check.is_due(started + REMOTE_IDENTITY_RECHECK_INTERVAL));

        assert!(matches!(
            check.confirm(
                &info("v2", 0x42),
                started + REMOTE_IDENTITY_RECHECK_INTERVAL
            ),
            Err(RestError::ConnectionUntrusted)
        ));
        assert!(check.is_due(started + REMOTE_IDENTITY_RECHECK_INTERVAL));

        check
            .confirm(
                &info("v1", 0x42),
                started + REMOTE_IDENTITY_RECHECK_INTERVAL,
            )
            .unwrap();
        assert!(!check.is_due(started + REMOTE_IDENTITY_RECHECK_INTERVAL));
    }
}
