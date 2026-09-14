use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt as _;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const STABLE_CHILD_INTERVAL: Duration = Duration::from_secs(30);
const MAX_RESTART_DELAY: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlaySupervisorStatus {
    Started,
    Running,
    Exited,
    BackingOff,
    ExecutableMissing,
    StartFailed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RestartPolicy {
    consecutive_failures: u32,
}

impl RestartPolicy {
    pub const fn consecutive_failures(self) -> u32 {
        self.consecutive_failures
    }

    pub fn record_failure(&mut self) -> Duration {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        restart_delay(self.consecutive_failures)
    }

    pub fn record_stable_child(&mut self) {
        self.consecutive_failures = 0;
    }
}

pub struct OverlaySupervisor {
    executable: PathBuf,
    arguments: Vec<OsString>,
    child: Option<Child>,
    child_started_at: Option<Instant>,
    retry_at: Option<Instant>,
    policy: RestartPolicy,
}

impl OverlaySupervisor {
    pub fn sibling_of_current_exe(file_name: &str) -> io::Result<Self> {
        Self::sibling_of_current_exe_with_arguments(file_name, Vec::new())
    }

    pub fn sibling_of_current_exe_with_arguments(
        file_name: &str,
        arguments: Vec<OsString>,
    ) -> io::Result<Self> {
        let executable = std::env::current_exe()?
            .parent()
            .ok_or_else(|| io::Error::other("core executable has no parent directory"))?
            .join(file_name);
        Ok(Self::with_arguments(executable, arguments))
    }

    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self::with_arguments(executable, Vec::new())
    }

    pub fn with_arguments(
        executable: impl Into<PathBuf>,
        arguments: impl IntoIterator<Item = OsString>,
    ) -> Self {
        Self {
            executable: executable.into(),
            arguments: arguments.into_iter().collect(),
            child: None,
            child_started_at: None,
            retry_at: None,
            policy: RestartPolicy::default(),
        }
    }

    pub fn tick(&mut self, now: Instant) -> OverlaySupervisorStatus {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(None) => {
                    if self.child_started_at.is_some_and(|started| {
                        now.saturating_duration_since(started) >= STABLE_CHILD_INTERVAL
                    }) {
                        self.policy.record_stable_child();
                    }
                    return OverlaySupervisorStatus::Running;
                }
                Ok(Some(_)) | Err(_) => {
                    self.child = None;
                    self.child_started_at = None;
                    self.retry_at = Some(now + self.policy.record_failure());
                    return OverlaySupervisorStatus::Exited;
                }
            }
        }

        if self.retry_at.is_some_and(|retry_at| now < retry_at) {
            return OverlaySupervisorStatus::BackingOff;
        }
        if !is_regular_file(&self.executable) {
            self.retry_at = Some(now + self.policy.record_failure());
            return OverlaySupervisorStatus::ExecutableMissing;
        }

        let mut command = Command::new(&self.executable);
        command
            .args(&self.arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        match command.spawn() {
            Ok(child) => {
                self.child = Some(child);
                self.child_started_at = Some(now);
                self.retry_at = None;
                OverlaySupervisorStatus::Started
            }
            Err(_) => {
                self.retry_at = Some(now + self.policy.record_failure());
                OverlaySupervisorStatus::StartFailed
            }
        }
    }

    pub fn shutdown(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ = child.kill();
        let _ = child.wait();
        self.child_started_at = None;
    }

    pub const fn policy(&self) -> RestartPolicy {
        self.policy
    }
}

impl Drop for OverlaySupervisor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn restart_delay(consecutive_failures: u32) -> Duration {
    let exponent = consecutive_failures.saturating_sub(1).min(5);
    Duration::from_secs(1u64 << exponent).min(MAX_RESTART_DELAY)
}

fn is_regular_file(path: &Path) -> bool {
    path.metadata().is_ok_and(|metadata| metadata.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_backoff_is_bounded_and_resets_after_stability() {
        let mut policy = RestartPolicy::default();
        assert_eq!(policy.record_failure(), Duration::from_secs(1));
        assert_eq!(policy.record_failure(), Duration::from_secs(2));
        assert_eq!(policy.record_failure(), Duration::from_secs(4));
        for _ in 0..20 {
            assert!(policy.record_failure() <= MAX_RESTART_DELAY);
        }
        policy.record_stable_child();
        assert_eq!(policy.consecutive_failures(), 0);
        assert_eq!(policy.record_failure(), Duration::from_secs(1));
    }

    #[test]
    fn missing_overlay_enters_backoff_without_crashing_core() {
        let started = Instant::now();
        let mut supervisor = OverlaySupervisor::new("definitely-missing-pal-overlay.exe");
        assert_eq!(
            supervisor.tick(started),
            OverlaySupervisorStatus::ExecutableMissing
        );
        assert_eq!(
            supervisor.tick(started),
            OverlaySupervisorStatus::BackingOff
        );
        assert_eq!(supervisor.policy().consecutive_failures(), 1);
    }

    #[test]
    fn one_supervisor_owns_one_explicit_overlay_source_argument_set() {
        let supervisor = OverlaySupervisor::with_arguments(
            "pal-overlay.exe",
            [
                OsString::from("--development-local-readonly-position"),
                OsString::from("--real-map-bmp"),
                OsString::from(r"C:\maps\main.bmp"),
            ],
        );
        assert_eq!(supervisor.arguments.len(), 3);
        assert_eq!(
            supervisor.arguments[0],
            OsString::from("--development-local-readonly-position")
        );
    }
}
