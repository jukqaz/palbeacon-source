use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use pal_agent::{
    AgentPipeline, GameDataSource, HealthMonitor, Poller, PollerExit, RestGameDataSource,
    SourceTrustError,
};
use pal_rest::{
    BasicAuthSecret, ClientConfig, EndpointPolicy, PalRestClient, PlayerSelector, Pseudonymizer,
    RestError, SafePlayerObservation, SanitizedServerInfo, TimedResponse,
};
use pal_telemetry::LatestTelemetry;
use tokio::sync::{Notify, watch};

const SUBJECT: [u8; 32] = [0x42; 32];
const PROFILE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn explicit_remote_https_source_has_no_local_process_dependency() {
    let endpoint = "https://streamline.example.invalid/v1/api";
    let policy = EndpointPolicy::loopback_only()
        .allow_exact_remote_https(endpoint)
        .unwrap();
    let config = ClientConfig::new(endpoint, policy).unwrap();
    let client = PalRestClient::new_explicit_remote_https(
        config,
        BasicAuthSecret::new("admin", "secret").unwrap(),
    )
    .unwrap();
    let source = RestGameDataSource::new_explicit_remote_https(
        Arc::new(client),
        PlayerSelector::user_id("player-1").unwrap(),
        Arc::new(Pseudonymizer::new([0x42; 32])),
        "world-a",
        SanitizedServerInfo {
            version: "v1".to_owned(),
            server_subject_id: "42".repeat(32),
        },
    );

    assert_eq!(source.revalidate_trust(), Ok(()));
}

struct BlockingSource {
    calls: AtomicUsize,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
    release: Notify,
}

impl BlockingSource {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
            release: Notify::new(),
        })
    }
}

impl GameDataSource for BlockingSource {
    fn revalidate_trust(&self) -> Result<(), SourceTrustError> {
        Ok(())
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
            self.calls.fetch_add(1, Ordering::SeqCst);
            let in_flight = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(in_flight, Ordering::SeqCst);
            self.release.notified().await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            Err(RestError::Timeout)
        })
    }
}

fn pipeline(health: HealthMonitor) -> AgentPipeline {
    let latest = LatestTelemetry::new("world-a", SUBJECT, PROFILE).unwrap();
    AgentPipeline::new("world-a", SUBJECT, PROFILE, false, latest, health).unwrap()
}

#[tokio::test(start_paused = true)]
async fn ten_thousand_fast_ticks_never_overlap_or_build_a_backlog() {
    let source = BlockingSource::new();
    let health = HealthMonitor::default();
    let poller = Poller::new(
        Duration::from_millis(500),
        source.clone(),
        pipeline(health.clone()),
        health,
    )
    .unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let task = tokio::spawn(poller.run(shutdown_rx));
    tokio::task::yield_now().await;

    tokio::time::advance(Duration::from_millis(500 * 10_000)).await;
    tokio::task::yield_now().await;
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.max_in_flight.load(Ordering::SeqCst), 1);

    source.release.notify_one();
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(500)).await;
    tokio::task::yield_now().await;
    assert!(source.calls.load(Ordering::SeqCst) <= 2);

    shutdown_tx.send(true).unwrap();
    source.release.notify_waiters();
    task.await.unwrap();
}

struct ScriptedSource {
    results: std::sync::Mutex<Vec<Result<TimedResponse<SafePlayerObservation>, RestError>>>,
}

impl GameDataSource for ScriptedSource {
    fn revalidate_trust(&self) -> Result<(), SourceTrustError> {
        Ok(())
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
            self.results
                .lock()
                .unwrap()
                .pop()
                .unwrap_or(Err(RestError::SelectedPlayerMissing))
        })
    }
}

fn safe_sample() -> TimedResponse<SafePlayerObservation> {
    TimedResponse {
        value: SafePlayerObservation {
            subject_id: SUBJECT.iter().map(|byte| format!("{byte:02x}")).collect(),
            x: 1.0,
            y: 2.0,
            z: 3.0,
            heading_degrees: None,
            server_fps: 60.0,
            average_server_fps: 60.0,
            actor_count: 1,
        },
        response_latency: Duration::from_millis(1),
        decode_latency: Duration::from_millis(1),
        body_bytes: 1,
        actor_count: 1,
        rest_completed_at: SystemTime::now(),
        completed_monotonic: Instant::now(),
    }
}

#[tokio::test(start_paused = true)]
async fn missing_inactive_ambiguous_and_transport_failures_emit_nothing_and_degrade_health() {
    let source = Arc::new(ScriptedSource {
        results: std::sync::Mutex::new(vec![
            Ok(safe_sample()),
            Err(RestError::Transport),
            Err(RestError::SelectedPlayerAmbiguous),
            Err(RestError::SelectedPlayerInactive),
            Err(RestError::SelectedPlayerMissing),
        ]),
    });
    let latest = LatestTelemetry::new("world-a", SUBJECT, PROFILE).unwrap();
    let health = HealthMonitor::default();
    let pipeline = AgentPipeline::new(
        "world-a",
        SUBJECT,
        PROFILE,
        false,
        latest.clone(),
        health.clone(),
    )
    .unwrap();
    let poller = Poller::new(Duration::from_millis(500), source, pipeline, health.clone()).unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let task = tokio::spawn(poller.run(shutdown_rx));

    for _ in 0..4 {
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(500)).await;
    }
    assert!(latest.current().is_none());
    let degraded = health.snapshot();
    assert!(degraded.degraded);
    assert_eq!(degraded.failed_polls, 4);
    assert_eq!(degraded.emitted_samples, 0);

    tokio::time::advance(Duration::from_millis(500)).await;
    tokio::task::yield_now().await;
    assert_eq!(latest.current().unwrap().sequence, 1);

    shutdown_tx.send(true).unwrap();
    task.await.unwrap();
}

struct TrustScriptSource {
    calls: AtomicUsize,
    trust_checks: AtomicUsize,
    fail_on_check: usize,
}

impl GameDataSource for TrustScriptSource {
    fn revalidate_trust(&self) -> Result<(), SourceTrustError> {
        let check = self.trust_checks.fetch_add(1, Ordering::SeqCst) + 1;
        if check >= self.fail_on_check {
            Err(SourceTrustError::LocalProcess)
        } else {
            Ok(())
        }
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
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(safe_sample()) })
    }
}

#[tokio::test(start_paused = true)]
async fn failed_pre_request_trust_check_sends_no_request_and_stops() {
    let source = Arc::new(TrustScriptSource {
        calls: AtomicUsize::new(0),
        trust_checks: AtomicUsize::new(0),
        fail_on_check: 1,
    });
    let health = HealthMonitor::default();
    let poller = Poller::new(
        Duration::from_millis(500),
        source.clone(),
        pipeline(health.clone()),
        health,
    )
    .unwrap();
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);

    assert_eq!(poller.run(shutdown_rx).await, PollerExit::TrustLost);
    assert_eq!(source.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn failed_post_response_trust_check_discards_sample_and_stops() {
    let source = Arc::new(TrustScriptSource {
        calls: AtomicUsize::new(0),
        trust_checks: AtomicUsize::new(0),
        fail_on_check: 2,
    });
    let latest = LatestTelemetry::new("world-a", SUBJECT, PROFILE).unwrap();
    let health = HealthMonitor::default();
    let pipeline = AgentPipeline::new(
        "world-a",
        SUBJECT,
        PROFILE,
        false,
        latest.clone(),
        health.clone(),
    )
    .unwrap();
    let poller = Poller::new(Duration::from_millis(500), source.clone(), pipeline, health).unwrap();
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);

    assert_eq!(poller.run(shutdown_rx).await, PollerExit::TrustLost);
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    assert!(latest.current().is_none());
}

#[tokio::test]
async fn untrusted_established_connection_stops_as_trust_loss() {
    let source = Arc::new(ScriptedSource {
        results: std::sync::Mutex::new(vec![Err(RestError::ConnectionUntrusted)]),
    });
    let health = HealthMonitor::default();
    let poller = Poller::new(
        Duration::from_millis(500),
        source,
        pipeline(health.clone()),
        health,
    )
    .unwrap();
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);

    let exit = tokio::time::timeout(Duration::from_millis(100), poller.run(shutdown_rx))
        .await
        .expect("ConnectionUntrusted must stop the poller immediately");
    assert_eq!(exit, PollerExit::TrustLost);
}
