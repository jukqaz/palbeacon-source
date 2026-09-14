//! Worker boundary for the exact-build local read-only position reader.
//!
//! Process discovery, executable hashing, pointer walking, and every process-memory read happen on
//! the spawned worker. The window/render thread receives only a bounded `PositionSource` handoff.

use std::{
    ffi::{OsStr, OsString},
    fmt::{self, Write as _},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, TryRecvError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use pal_domain::{Freshness, PositionSample, RotationMode, classify_freshness};
use pal_state::ServerAgentPositionSource;
use pal_windows::TrackedWindow;
use thiserror::Error;

use crate::{
    client_build_binding::VerifiedSteamClientBuild,
    local_position_source::{
        LocalPositionMemoryReader, LocalPositionSourceError, LocalPositionSourceIdentity,
        PalworldWindowsLivePositionPump, WINDOWS_LIVE_POSITION_PROFILE,
    },
    local_position_win32::open_windows_live_position_reader,
};

const MODE_FLAG: &str = "--development-local-readonly-position";
const MAP_FLAG: &str = "--real-map-bmp";
const POI_CATALOG_FLAG: &str = "--poi-catalog";
const SMOKE_OPT_IN_FLAG: &str = "--allow-unapproved-map-smoke";
const ROTATION_FLAG: &str = "--rotation-mode";
const DURATION_FLAG: &str = "--duration-seconds";
const MAX_DURATION_SECONDS: u64 = 3_600;
#[cfg(feature = "development-live-performance-diagnostic")]
const LIVE_PERFORMANCE_DIAGNOSTIC_MODE_FLAG: &str =
    "--development-local-live-performance-diagnostic-json";
const DIAGNOSTIC_MODE_FLAG: &str = "--development-local-position-diagnostic-json";
const DIAGNOSTIC_TIMEOUT_FLAG: &str = "--timeout-seconds";
const DEFAULT_DIAGNOSTIC_TIMEOUT_SECONDS: u64 = 15;
const MAX_DIAGNOSTIC_TIMEOUT_SECONDS: u64 = 30;
const WORKER_SAMPLE_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DevelopmentLocalPositionCliError {
    #[error("the local read-only position mode flag is required")]
    MissingModeOptIn,
    #[error("--real-map-bmp requires a path")]
    MissingMapPath,
    #[error("--poi-catalog requires a path")]
    MissingPoiCatalogPath,
    #[error("local position map rendering requires --real-map-bmp")]
    MapRequired,
    #[error("unapproved map rendering requires --allow-unapproved-map-smoke")]
    UnapprovedMapSmokeOptInRequired,
    #[error("--rotation-mode requires north-up or heading-up")]
    MissingRotationMode,
    #[error("--rotation-mode must be north-up or heading-up")]
    InvalidRotationMode,
    #[error("--duration-seconds requires a value")]
    MissingDuration,
    #[error("--duration-seconds must be an integer")]
    InvalidDuration,
    #[error("--duration-seconds must be between 1 and 3600 seconds")]
    DurationOutOfRange,
    #[cfg(feature = "development-live-performance-diagnostic")]
    #[error("the live performance diagnostic requires --duration-seconds")]
    LivePerformanceDurationRequired,
    #[error("a development local-position argument was provided more than once")]
    DuplicateArgument,
    #[error("unknown development local-position argument")]
    UnknownArgument,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevelopmentLocalPositionCommand {
    map_path: PathBuf,
    poi_catalog_path: Option<PathBuf>,
    rotation_mode: RotationMode,
    duration: Option<Duration>,
    #[cfg(feature = "development-live-performance-diagnostic")]
    live_performance_diagnostic: bool,
}

impl DevelopmentLocalPositionCommand {
    pub fn parse<I, S>(arguments: I) -> Result<Self, DevelopmentLocalPositionCliError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut mode_opt_in = false;
        let mut map_path = None;
        let mut poi_catalog_path = None;
        let mut smoke_opt_in = false;
        let mut rotation_mode = None;
        let mut duration = None;
        #[cfg(feature = "development-live-performance-diagnostic")]
        let mut live_performance_diagnostic = false;
        let mut arguments = arguments.into_iter().map(Into::into);

        while let Some(argument) = arguments.next() {
            #[cfg(feature = "development-live-performance-diagnostic")]
            if argument == OsStr::new(LIVE_PERFORMANCE_DIAGNOSTIC_MODE_FLAG) {
                set_once(&mut live_performance_diagnostic)?;
                continue;
            }

            if argument == OsStr::new(MODE_FLAG) {
                set_once(&mut mode_opt_in)?;
            } else if argument == OsStr::new(MAP_FLAG) {
                if map_path.is_some() {
                    return Err(DevelopmentLocalPositionCliError::DuplicateArgument);
                }
                let raw = arguments
                    .next()
                    .ok_or(DevelopmentLocalPositionCliError::MissingMapPath)?;
                if raw.is_empty() || raw.to_string_lossy().starts_with("--") {
                    return Err(DevelopmentLocalPositionCliError::MissingMapPath);
                }
                map_path = Some(PathBuf::from(raw));
            } else if argument == OsStr::new(POI_CATALOG_FLAG) {
                if poi_catalog_path.is_some() {
                    return Err(DevelopmentLocalPositionCliError::DuplicateArgument);
                }
                let raw = arguments
                    .next()
                    .ok_or(DevelopmentLocalPositionCliError::MissingPoiCatalogPath)?;
                if raw.is_empty() || raw.to_string_lossy().starts_with("--") {
                    return Err(DevelopmentLocalPositionCliError::MissingPoiCatalogPath);
                }
                poi_catalog_path = Some(PathBuf::from(raw));
            } else if argument == OsStr::new(SMOKE_OPT_IN_FLAG) {
                set_once(&mut smoke_opt_in)?;
            } else if argument == OsStr::new(ROTATION_FLAG) {
                if rotation_mode.is_some() {
                    return Err(DevelopmentLocalPositionCliError::DuplicateArgument);
                }
                let raw = arguments
                    .next()
                    .ok_or(DevelopmentLocalPositionCliError::MissingRotationMode)?;
                rotation_mode = Some(match raw.to_str() {
                    Some("north-up") => RotationMode::NorthUp,
                    Some("heading-up") => RotationMode::HeadingUp,
                    _ => return Err(DevelopmentLocalPositionCliError::InvalidRotationMode),
                });
            } else if argument == OsStr::new(DURATION_FLAG) {
                if duration.is_some() {
                    return Err(DevelopmentLocalPositionCliError::DuplicateArgument);
                }
                let raw = arguments
                    .next()
                    .ok_or(DevelopmentLocalPositionCliError::MissingDuration)?;
                let seconds = raw
                    .to_str()
                    .ok_or(DevelopmentLocalPositionCliError::InvalidDuration)?
                    .parse::<u64>()
                    .map_err(|_| DevelopmentLocalPositionCliError::InvalidDuration)?;
                if !(1..=MAX_DURATION_SECONDS).contains(&seconds) {
                    return Err(DevelopmentLocalPositionCliError::DurationOutOfRange);
                }
                duration = Some(Duration::from_secs(seconds));
            } else {
                return Err(DevelopmentLocalPositionCliError::UnknownArgument);
            }
        }

        if !mode_opt_in {
            return Err(DevelopmentLocalPositionCliError::MissingModeOptIn);
        }
        let map_path = map_path.ok_or(DevelopmentLocalPositionCliError::MapRequired)?;
        if !smoke_opt_in {
            return Err(DevelopmentLocalPositionCliError::UnapprovedMapSmokeOptInRequired);
        }
        #[cfg(feature = "development-live-performance-diagnostic")]
        if live_performance_diagnostic && duration.is_none() {
            return Err(DevelopmentLocalPositionCliError::LivePerformanceDurationRequired);
        }
        Ok(Self {
            map_path,
            poi_catalog_path,
            rotation_mode: rotation_mode.unwrap_or(RotationMode::NorthUp),
            duration,
            #[cfg(feature = "development-live-performance-diagnostic")]
            live_performance_diagnostic,
        })
    }

    pub fn map_path(&self) -> &Path {
        &self.map_path
    }

    pub fn poi_catalog_path(&self) -> Option<&Path> {
        self.poi_catalog_path.as_deref()
    }

    pub const fn rotation_mode(&self) -> RotationMode {
        self.rotation_mode
    }

    pub const fn duration(&self) -> Option<Duration> {
        self.duration
    }

    #[cfg(feature = "development-live-performance-diagnostic")]
    pub const fn live_performance_diagnostic(&self) -> bool {
        self.live_performance_diagnostic
    }
}

fn set_once(value: &mut bool) -> Result<(), DevelopmentLocalPositionCliError> {
    if *value {
        return Err(DevelopmentLocalPositionCliError::DuplicateArgument);
    }
    *value = true;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DevelopmentLocalPositionDiagnosticCliError {
    #[error("the development local-position diagnostic mode flag is required")]
    MissingModeOptIn,
    #[error("--timeout-seconds requires a value")]
    MissingTimeout,
    #[error("--timeout-seconds must be an integer")]
    InvalidTimeout,
    #[error("--timeout-seconds must be between 1 and 30 seconds")]
    TimeoutOutOfRange,
    #[error("a development local-position diagnostic argument was provided more than once")]
    DuplicateArgument,
    #[error("unknown development local-position diagnostic argument")]
    UnknownArgument,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DevelopmentLocalPositionDiagnosticCommand {
    timeout: Duration,
}

impl DevelopmentLocalPositionDiagnosticCommand {
    pub fn parse<I, S>(arguments: I) -> Result<Self, DevelopmentLocalPositionDiagnosticCliError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut mode_opt_in = false;
        let mut timeout = None;
        let mut arguments = arguments.into_iter().map(Into::into);

        while let Some(argument) = arguments.next() {
            if argument == OsStr::new(DIAGNOSTIC_MODE_FLAG) {
                if mode_opt_in {
                    return Err(DevelopmentLocalPositionDiagnosticCliError::DuplicateArgument);
                }
                mode_opt_in = true;
            } else if argument == OsStr::new(DIAGNOSTIC_TIMEOUT_FLAG) {
                if timeout.is_some() {
                    return Err(DevelopmentLocalPositionDiagnosticCliError::DuplicateArgument);
                }
                let raw = arguments
                    .next()
                    .ok_or(DevelopmentLocalPositionDiagnosticCliError::MissingTimeout)?;
                if raw.to_string_lossy().starts_with("--") {
                    return Err(DevelopmentLocalPositionDiagnosticCliError::MissingTimeout);
                }
                let seconds = raw
                    .to_str()
                    .ok_or(DevelopmentLocalPositionDiagnosticCliError::InvalidTimeout)?
                    .parse::<u64>()
                    .map_err(|_| DevelopmentLocalPositionDiagnosticCliError::InvalidTimeout)?;
                if !(1..=MAX_DIAGNOSTIC_TIMEOUT_SECONDS).contains(&seconds) {
                    return Err(DevelopmentLocalPositionDiagnosticCliError::TimeoutOutOfRange);
                }
                timeout = Some(Duration::from_secs(seconds));
            } else {
                return Err(DevelopmentLocalPositionDiagnosticCliError::UnknownArgument);
            }
        }

        if !mode_opt_in {
            return Err(DevelopmentLocalPositionDiagnosticCliError::MissingModeOptIn);
        }
        Ok(Self {
            timeout: timeout
                .unwrap_or_else(|| Duration::from_secs(DEFAULT_DIAGNOSTIC_TIMEOUT_SECONDS)),
        })
    }

    pub const fn timeout(self) -> Duration {
        self.timeout
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DevelopmentLocalPositionDiagnosticError {
    #[error("the local position diagnostic sample is stale")]
    StaleSample,
    #[error("the local position diagnostic sample has no yaw")]
    MissingYaw,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DevelopmentLocalPositionDiagnosticRecord {
    sequence: u64,
    age_upper_bound_ms: u64,
    x: f64,
    y: f64,
    z: f64,
    yaw_degrees: f32,
}

impl DevelopmentLocalPositionDiagnosticRecord {
    pub fn from_fresh_sample(
        sample: &PositionSample,
        now_monotonic_ms: u64,
    ) -> Result<Self, DevelopmentLocalPositionDiagnosticError> {
        let age_upper_bound_ms = sample.clock().age_upper_bound_ms(now_monotonic_ms);
        if classify_freshness(age_upper_bound_ms, true) != Freshness::Live {
            return Err(DevelopmentLocalPositionDiagnosticError::StaleSample);
        }
        let yaw_degrees = sample
            .heading_degrees()
            .ok_or(DevelopmentLocalPositionDiagnosticError::MissingYaw)?;
        Ok(Self {
            sequence: sample.sequence(),
            age_upper_bound_ms,
            x: sample.x(),
            y: sample.y(),
            z: sample.z(),
            yaw_degrees,
        })
    }

    pub fn to_json_line(self) -> String {
        let profile = WINDOWS_LIVE_POSITION_PROFILE;
        let mut sha256 = String::with_capacity(64);
        for byte in profile.executable_sha256() {
            write!(&mut sha256, "{byte:02x}").expect("writing to a String cannot fail");
        }
        format!(
            concat!(
                "{{\"schema\":\"pal_companion.gate_b_position.v1\",",
                "\"source\":\"development_local_readonly_exact_build\",",
                "\"build\":{{\"steam_build_id\":{},\"executable_sha256\":\"{}\",",
                "\"executable_file_size\":{},\"loaded_module_size\":{},",
                "\"gate_b_approved\":{}}},",
                "\"sample\":{{\"sequence\":{},\"age_upper_bound_ms\":{},",
                "\"x\":{},\"y\":{},\"z\":{},\"yaw_degrees\":{}}}}}"
            ),
            profile.build_id(),
            sha256,
            profile.executable_file_size(),
            profile.loaded_module_size(),
            profile.gate_b_approved(),
            self.sequence,
            self.age_upper_bound_ms,
            self.x,
            self.y,
            self.z,
            self.yaw_degrees,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DevelopmentLocalPositionWorkerInitError {
    #[error("the exact-build local read-only process could not be opened")]
    ReaderUnavailable,
    #[error("the bounded local position source could not be initialized")]
    PositionSourceUnavailable,
}

pub enum DevelopmentLocalPositionWorkerStart {
    Pending,
    Ready(ServerAgentPositionSource),
    Failed(DevelopmentLocalPositionWorkerInitError),
    Closed,
}

pub struct DevelopmentLocalPositionWorker {
    start_receiver: Option<
        Receiver<Result<ServerAgentPositionSource, DevelopmentLocalPositionWorkerInitError>>,
    >,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    monotonic_epoch: Instant,
}

impl fmt::Debug for DevelopmentLocalPositionWorker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED DEVELOPMENT LOCAL POSITION WORKER]")
    }
}

impl DevelopmentLocalPositionWorker {
    pub fn spawn(
        window: TrackedWindow,
        verified_build: VerifiedSteamClientBuild,
    ) -> Result<Self, std::io::Error> {
        Self::spawn_with_cancellable_reader_and_loop_observer(
            move |cancelled| {
                open_windows_live_position_reader(&window, &verified_build, cancelled)
                    .map_err(|_| DevelopmentLocalPositionWorkerInitError::ReaderUnavailable)
            },
            || {},
        )
    }

    #[cfg(test)]
    fn spawn_with_reader<R, F>(open_reader: F) -> Result<Self, std::io::Error>
    where
        R: LocalPositionMemoryReader + Send + 'static,
        F: FnOnce() -> Result<R, DevelopmentLocalPositionWorkerInitError> + Send + 'static,
    {
        Self::spawn_with_cancellable_reader_and_loop_observer(move |_| open_reader(), || {})
    }

    #[cfg(test)]
    fn spawn_with_reader_and_loop_observer<R, F, O>(
        open_reader: F,
        observe_loop: O,
    ) -> Result<Self, std::io::Error>
    where
        R: LocalPositionMemoryReader + Send + 'static,
        F: FnOnce() -> Result<R, DevelopmentLocalPositionWorkerInitError> + Send + 'static,
        O: FnMut() + Send + 'static,
    {
        Self::spawn_with_cancellable_reader_and_loop_observer(move |_| open_reader(), observe_loop)
    }

    fn spawn_with_cancellable_reader_and_loop_observer<R, F, O>(
        open_reader: F,
        mut observe_loop: O,
    ) -> Result<Self, std::io::Error>
    where
        R: LocalPositionMemoryReader + Send + 'static,
        F: FnOnce(&AtomicBool) -> Result<R, DevelopmentLocalPositionWorkerInitError>
            + Send
            + 'static,
        O: FnMut() + Send + 'static,
    {
        let (start_sender, start_receiver) = sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let monotonic_epoch = Instant::now();
        let worker_epoch = monotonic_epoch;
        let worker = thread::Builder::new()
            .name("pal-local-position-current-build".to_owned())
            .spawn(move || {
                let reader = match open_reader(worker_stop.as_ref()) {
                    Ok(reader) => reader,
                    Err(error) => {
                        let _ = start_sender.send(Err(error));
                        return;
                    }
                };
                if worker_stop.load(Ordering::Acquire) {
                    return;
                }
                let identity = LocalPositionSourceIdentity::new(
                    "development-local-main-map",
                    [0x24; 32],
                    [0x81; 32],
                    1,
                )
                .expect("the static development source identity is valid");
                let (mut pump, source) =
                    match PalworldWindowsLivePositionPump::new(reader, identity) {
                        Ok(value) => value,
                        Err(
                            LocalPositionSourceError::InvalidSourceIdentity
                            | LocalPositionSourceError::ExactBuildMismatch
                            | LocalPositionSourceError::StillActive
                            | LocalPositionSourceError::GenerationOverflow
                            | LocalPositionSourceError::HandoffRejected,
                        ) => {
                            let _ = start_sender.send(Err(
                                DevelopmentLocalPositionWorkerInitError::PositionSourceUnavailable,
                            ));
                            return;
                        }
                    };
                if start_sender.send(Ok(source)).is_err() {
                    return;
                }
                let mut next_sample_deadline = Instant::now();
                while !worker_stop.load(Ordering::Acquire) {
                    observe_loop();
                    let now = Instant::now();
                    if now < next_sample_deadline {
                        thread::park_timeout(next_sample_deadline.duration_since(now));
                        continue;
                    }
                    let now_ms =
                        u64::try_from(worker_epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
                    if matches!(
                        pump.step(now_ms),
                        crate::local_position_source::LocalPositionStep::Disconnected { .. }
                    ) {
                        break;
                    }
                    next_sample_deadline =
                        next_worker_sample_deadline(next_sample_deadline, Instant::now());
                }
            })?;
        Ok(Self {
            start_receiver: Some(start_receiver),
            stop,
            worker: Some(worker),
            monotonic_epoch,
        })
    }

    pub fn poll_start(&mut self) -> DevelopmentLocalPositionWorkerStart {
        let Some(receiver) = self.start_receiver.as_ref() else {
            return DevelopmentLocalPositionWorkerStart::Closed;
        };
        match receiver.try_recv() {
            Ok(Ok(source)) => {
                self.start_receiver = None;
                DevelopmentLocalPositionWorkerStart::Ready(source)
            }
            Ok(Err(error)) => {
                self.start_receiver = None;
                DevelopmentLocalPositionWorkerStart::Failed(error)
            }
            Err(TryRecvError::Empty) => DevelopmentLocalPositionWorkerStart::Pending,
            Err(TryRecvError::Disconnected) => {
                self.start_receiver = None;
                DevelopmentLocalPositionWorkerStart::Closed
            }
        }
    }

    pub fn shutdown(mut self) {
        self.stop_worker();
    }

    pub fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.monotonic_epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn stop_worker(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.as_ref() {
            worker.thread().unpark();
        }
        if let Some(worker) = self.worker.take() {
            if worker.is_finished() {
                let _ = worker.join();
            } else {
                let _ = thread::Builder::new()
                    .name("pal-local-position-reaper".to_owned())
                    .spawn(move || {
                        let _ = worker.join();
                    });
            }
        }
        self.start_receiver = None;
    }
}

fn next_worker_sample_deadline(previous: Instant, now: Instant) -> Instant {
    previous
        .checked_add(WORKER_SAMPLE_INTERVAL)
        .filter(|deadline| *deadline > now)
        .or_else(|| now.checked_add(WORKER_SAMPLE_INTERVAL))
        .unwrap_or(now)
}

impl Drop for DevelopmentLocalPositionWorker {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant},
    };

    use pal_state::{PositionSource, PositionSourceEvent};

    use super::*;
    use crate::local_position_source::{
        BUILD_24181527_PROFILE, LocalMemoryReadError, LocalPositionReadSession,
        LocalProcessIdentity,
    };

    #[test]
    fn local_mode_requires_both_explicit_opt_ins_and_a_map() {
        assert_eq!(
            DevelopmentLocalPositionCommand::parse([MODE_FLAG, MAP_FLAG, "map.bmp"]),
            Err(DevelopmentLocalPositionCliError::UnapprovedMapSmokeOptInRequired)
        );
        assert_eq!(
            DevelopmentLocalPositionCommand::parse([MAP_FLAG, "map.bmp", SMOKE_OPT_IN_FLAG,]),
            Err(DevelopmentLocalPositionCliError::MissingModeOptIn)
        );
        assert_eq!(
            DevelopmentLocalPositionCommand::parse([MODE_FLAG, SMOKE_OPT_IN_FLAG]),
            Err(DevelopmentLocalPositionCliError::MapRequired)
        );
    }

    #[test]
    fn local_mode_preserves_non_unicode_map_paths() {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};

        let path = OsString::from_wide(&[b'C' as u16, b':' as u16, b'\\' as u16, 0xd800]);
        let command = DevelopmentLocalPositionCommand::parse([
            OsString::from(MODE_FLAG),
            OsString::from(MAP_FLAG),
            path.clone(),
            OsString::from(SMOKE_OPT_IN_FLAG),
        ])
        .unwrap();
        assert_eq!(
            command
                .map_path()
                .as_os_str()
                .encode_wide()
                .collect::<Vec<_>>(),
            path.encode_wide().collect::<Vec<_>>()
        );
    }

    #[test]
    fn local_mode_parses_required_opt_ins_regardless_of_argument_order() {
        let command = DevelopmentLocalPositionCommand::parse([
            "--duration-seconds",
            "3",
            "--allow-unapproved-map-smoke",
            "--real-map-bmp",
            "map.bmp",
            "--development-local-readonly-position",
            "--rotation-mode",
            "heading-up",
        ])
        .expect("explicit local smoke command");

        assert_eq!(command.map_path(), Path::new("map.bmp"));
        assert_eq!(command.rotation_mode(), RotationMode::HeadingUp);
        assert_eq!(command.duration(), Some(Duration::from_secs(3)));
    }

    #[cfg(feature = "development-live-performance-diagnostic")]
    #[test]
    fn live_performance_diagnostic_requires_existing_smoke_opt_ins_and_duration() {
        const LIVE_DIAGNOSTIC_FLAG: &str = "--development-local-live-performance-diagnostic-json";

        assert!(DevelopmentLocalPositionCommand::parse([LIVE_DIAGNOSTIC_FLAG]).is_err());
        assert!(
            DevelopmentLocalPositionCommand::parse([
                MODE_FLAG,
                LIVE_DIAGNOSTIC_FLAG,
                MAP_FLAG,
                "map.bmp",
                DURATION_FLAG,
                "3",
            ])
            .is_err()
        );
        assert!(
            DevelopmentLocalPositionCommand::parse([
                MODE_FLAG,
                LIVE_DIAGNOSTIC_FLAG,
                MAP_FLAG,
                "map.bmp",
                SMOKE_OPT_IN_FLAG,
            ])
            .is_err()
        );

        let command = DevelopmentLocalPositionCommand::parse([
            MODE_FLAG,
            LIVE_DIAGNOSTIC_FLAG,
            MAP_FLAG,
            "map.bmp",
            SMOKE_OPT_IN_FLAG,
            DURATION_FLAG,
            "3",
        ])
        .expect("fully opted-in bounded live performance diagnostic");

        assert!(command.live_performance_diagnostic());
        assert_eq!(command.duration(), Some(Duration::from_secs(3)));
    }

    #[test]
    fn every_read_and_reader_open_happen_on_the_worker_thread() {
        let caller = thread::current().id();
        let observed_threads = Arc::new(Mutex::new(Vec::new()));
        let worker_threads = Arc::clone(&observed_threads);
        let reader = FixtureReader::new(Arc::clone(&observed_threads));
        let mut worker = DevelopmentLocalPositionWorker::spawn_with_reader(move || {
            worker_threads.lock().unwrap().push(thread::current().id());
            Ok(reader)
        })
        .unwrap();

        let mut source = wait_for_source(&mut worker);
        assert_eq!(
            source.poll(0).unwrap(),
            Some(PositionSourceEvent::Connected { generation: 1 })
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        let sample = loop {
            if let Some(PositionSourceEvent::Sample(sample)) = source.poll(0).unwrap() {
                break sample;
            }
            assert!(Instant::now() < deadline, "worker did not publish a sample");
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(sample.x(), -327_711.295_4);
        assert_eq!(sample.y(), 216_974.095_1);
        assert_eq!(sample.heading_degrees(), Some(136.8173_f32));
        worker.shutdown();

        let observed = observed_threads.lock().unwrap();
        assert!(!observed.is_empty());
        assert!(observed.iter().all(|thread_id| *thread_id != caller));
        assert!(observed.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn dropping_a_worker_does_not_wait_for_a_blocked_reader_open() {
        let (open_started_sender, open_started) = mpsc::sync_channel(1);
        let (release_sender, release) = mpsc::sync_channel(1);
        let release_after_assertion = thread::spawn(move || {
            thread::sleep(Duration::from_millis(250));
            let _ = release_sender.send(());
        });
        let worker = DevelopmentLocalPositionWorker::spawn_with_reader(move || {
            open_started_sender.send(()).unwrap();
            let _ = release.recv();
            Err::<FixtureReader, _>(DevelopmentLocalPositionWorkerInitError::ReaderUnavailable)
        })
        .unwrap();
        open_started
            .recv_timeout(Duration::from_secs(1))
            .expect("reader open started");

        let shutdown_started = Instant::now();
        drop(worker);

        assert!(
            shutdown_started.elapsed() < Duration::from_millis(100),
            "dropping the UI-owned worker must not wait for a slow hash/open"
        );
        release_after_assertion.join().unwrap();
    }

    #[test]
    fn dropping_a_worker_cancels_a_cooperative_exact_image_open_within_100ms() {
        let (open_started_sender, open_started) = mpsc::sync_channel(1);
        let (released_sender, released) = mpsc::sync_channel(1);
        let worker =
            DevelopmentLocalPositionWorker::spawn_with_cancellable_reader_and_loop_observer(
                move |cancelled| {
                    open_started_sender.send(()).unwrap();
                    while !cancelled.load(Ordering::Acquire) {
                        thread::yield_now();
                    }
                    let _ = released_sender.send(());
                    Err::<FixtureReader, _>(
                        DevelopmentLocalPositionWorkerInitError::ReaderUnavailable,
                    )
                },
                || {},
            )
            .unwrap();
        open_started
            .recv_timeout(Duration::from_secs(1))
            .expect("exact image open started");

        let shutdown_started = Instant::now();
        drop(worker);

        released
            .recv_timeout(Duration::from_millis(100))
            .expect("cooperative exact image open released promptly after cancellation");
        assert!(shutdown_started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn worker_waits_until_the_next_100ms_sampling_deadline() {
        let start = Instant::now();
        assert_eq!(
            next_worker_sample_deadline(start, start),
            start + Duration::from_millis(100)
        );
        assert_eq!(
            next_worker_sample_deadline(start, start + Duration::from_millis(101)),
            start + Duration::from_millis(201)
        );
    }

    #[test]
    fn worker_does_not_wake_at_100hz_between_100ms_sample_deadlines() {
        let loop_count = Arc::new(AtomicUsize::new(0));
        let observed_loops = Arc::clone(&loop_count);
        let observed_threads = Arc::new(Mutex::new(Vec::new()));
        let reader = FixtureReader::new(observed_threads);
        let mut worker = DevelopmentLocalPositionWorker::spawn_with_reader_and_loop_observer(
            move || Ok(reader),
            move || {
                observed_loops.fetch_add(1, Ordering::Relaxed);
            },
        )
        .unwrap();
        let _source = wait_for_source(&mut worker);

        thread::sleep(Duration::from_millis(250));
        worker.shutdown();

        assert!(
            loop_count.load(Ordering::Relaxed) <= 7,
            "worker woke more often than the 100ms sampling cadence"
        );
    }

    #[test]
    fn shutdown_unparks_the_deadline_wait_and_releases_the_reader_promptly() {
        let (dropped_sender, dropped) = mpsc::sync_channel(1);
        let observed_threads = Arc::new(Mutex::new(Vec::new()));
        let reader = FixtureReader::new(observed_threads).with_drop_signal(dropped_sender);
        let mut worker =
            DevelopmentLocalPositionWorker::spawn_with_reader(move || Ok(reader)).unwrap();
        let _source = wait_for_source(&mut worker);
        thread::sleep(Duration::from_millis(10));

        worker.shutdown();

        dropped
            .recv_timeout(Duration::from_millis(50))
            .expect("worker cancellation releases the reader without waiting for its deadline");
    }

    fn wait_for_source(worker: &mut DevelopmentLocalPositionWorker) -> ServerAgentPositionSource {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match worker.poll_start() {
                DevelopmentLocalPositionWorkerStart::Ready(source) => return source,
                DevelopmentLocalPositionWorkerStart::Pending => {}
                DevelopmentLocalPositionWorkerStart::Failed(error) => {
                    panic!("worker initialization failed: {error}")
                }
                DevelopmentLocalPositionWorkerStart::Closed => panic!("worker closed"),
            }
            assert!(Instant::now() < deadline, "worker did not initialize");
            thread::sleep(Duration::from_millis(5));
        }
    }

    struct FixtureReader {
        identity: LocalProcessIdentity,
        memory: BTreeMap<usize, Vec<u8>>,
        observed_threads: Arc<Mutex<Vec<thread::ThreadId>>>,
        drop_signal: Option<mpsc::SyncSender<()>>,
    }

    impl FixtureReader {
        fn new(observed_threads: Arc<Mutex<Vec<thread::ThreadId>>>) -> Self {
            let profile = BUILD_24181527_PROFILE;
            let module_base = 0x1000_0000_usize;
            let identity = LocalProcessIdentity::new(
                profile.build_id(),
                profile.executable_sha256(),
                profile.executable_file_size(),
                module_base,
                profile.loaded_module_size(),
            );
            let mut memory = BTreeMap::new();
            let mut base = module_base + profile.root_rva();
            for (index, offset) in profile.pointer_read_offsets().iter().enumerate() {
                let pointer = 0x3000_0000_usize + index * 0x10_000;
                memory.insert(base + offset, (pointer as u64).to_le_bytes().to_vec());
                base = pointer;
            }
            let camera_address = base + profile.camera_offset();
            let mut camera = vec![0_u8; 40];
            camera[0..8].copy_from_slice(&(-327_711.295_4_f64).to_le_bytes());
            camera[8..16].copy_from_slice(&216_974.095_1_f64.to_le_bytes());
            camera[16..24].copy_from_slice(&(-1_234.5_f64).to_le_bytes());
            camera[32..40].copy_from_slice(&136.8173_f64.to_le_bytes());
            memory.insert(camera_address, camera);
            Self {
                identity,
                memory,
                observed_threads,
                drop_signal: None,
            }
        }

        fn with_drop_signal(mut self, drop_signal: mpsc::SyncSender<()>) -> Self {
            self.drop_signal = Some(drop_signal);
            self
        }

        fn observe_thread(&self) {
            self.observed_threads
                .lock()
                .unwrap()
                .push(thread::current().id());
        }
    }

    impl LocalPositionMemoryReader for FixtureReader {
        fn identity(&mut self) -> Result<LocalProcessIdentity, LocalMemoryReadError> {
            self.observe_thread();
            Ok(self.identity)
        }

        fn with_read_session<T>(
            &mut self,
            operation: impl FnOnce(&mut dyn LocalPositionReadSession) -> T,
        ) -> Result<T, LocalMemoryReadError> {
            self.observe_thread();
            let mut session = FixtureSession {
                memory: &self.memory,
                observed_threads: &self.observed_threads,
            };
            Ok(operation(&mut session))
        }
    }

    impl Drop for FixtureReader {
        fn drop(&mut self) {
            if let Some(drop_signal) = self.drop_signal.take() {
                let _ = drop_signal.send(());
            }
        }
    }

    struct FixtureSession<'a> {
        memory: &'a BTreeMap<usize, Vec<u8>>,
        observed_threads: &'a Arc<Mutex<Vec<thread::ThreadId>>>,
    }

    impl LocalPositionReadSession for FixtureSession<'_> {
        fn read_exact(
            &mut self,
            address: usize,
            output: &mut [u8],
        ) -> Result<(), LocalMemoryReadError> {
            self.observed_threads
                .lock()
                .unwrap()
                .push(thread::current().id());
            let bytes = self
                .memory
                .get(&address)
                .ok_or(LocalMemoryReadError::Unavailable)?;
            if bytes.len() != output.len() {
                return Err(LocalMemoryReadError::Unavailable);
            }
            output.copy_from_slice(bytes);
            Ok(())
        }
    }
}
