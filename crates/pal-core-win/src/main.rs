use std::{
    error::Error,
    ffi::{OsStr, OsString},
    path::PathBuf,
    process::ExitCode,
    thread,
    time::Duration,
};

use pal_core_win::{
    approved_map_pack::{ApprovedMapPackIdentity, encode_sha256},
    control_session::ControlSession,
    ipc_runtime::CoreIpcRuntime,
    overlay_supervisor::{OverlaySupervisor, OverlaySupervisorStatus},
    settings_persistence::SettingsPersistenceRuntime,
};
use pal_domain::OverlaySettings;
use pal_overlay_control::{default_control_path, read_or_create_control};
use pal_windows_ipc::{
    CORE_MODE_APPROVED_MAP_PACK_EVENT, CORE_MODE_SAFE_EVENT, CORE_SHUTDOWN_EVENT,
};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt as _;
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
    System::Threading::{CreateEventW, OpenProcess, WaitForSingleObject},
};

const IDLE_PARK_INTERVAL: Duration = Duration::from_secs(60);
const SUPERVISOR_TICK_INTERVAL: Duration = Duration::from_millis(500);
const APPROVED_MAP_PACK_RUN_FLAG: &str = "--overlay-approved-map-pack";
const APPROVED_MAP_PACK_CHECK_FLAG: &str = "--approved-map-pack-check";
const GAME_BUILD_ID_FLAG: &str = "--game-build-id";
const PARENT_PID_FLAG: &str = "--parent-pid";
const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pal-core startup failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    match parse_command(arguments)? {
        CoreCommand::Help => {
            println!("PalBeacon core");
            println!(
                "Usage: pal-core [--help|--health-check|\
                 --approved-map-pack-check <absolute-dataset-root> \
                 --game-build-id <exact-build>|\
                 --parent-pid <pid>|\
                 --parent-pid <pid> \
                 --overlay-approved-map-pack <absolute-dataset-root> \
                 --game-build-id <exact-build>]"
            );
            Ok(())
        }
        CoreCommand::HealthCheck => {
            let _ = initial_overlay_settings()?;
            println!("pal-core: ready");
            Ok(())
        }
        CoreCommand::ApprovedMapPackCheck {
            dataset_root,
            game_build_id,
        } => {
            let (identity, _) =
                ApprovedMapPackIdentity::verify_published(dataset_root, &game_build_id)?;
            println!(
                "pal-core: approved map pack verified build={} pack_sha256={} \
                 main_transform_sha256={} tree_transform_sha256={}",
                identity.game_build_id(),
                encode_sha256(identity.canonical_pack_sha256()),
                encode_sha256(identity.main_transform_sha256()),
                encode_sha256(identity.tree_transform_sha256()),
            );
            Ok(())
        }
        CoreCommand::Run {
            source_mode,
            overlay_arguments,
            parent_pid,
        } => {
            let run_control = CoreRunControl::new(source_mode, parent_pid)?;
            let session = ControlSession::new(initial_overlay_settings()?)?;
            let shared_store = session.shared_store();
            let mut runtime = CoreIpcRuntime::start(shared_store.clone())?;
            let _persistence =
                SettingsPersistenceRuntime::start(shared_store, default_control_path())?;
            let mut overlay = OverlaySupervisor::sibling_of_current_exe_with_arguments(
                "pal-overlay.exe",
                overlay_arguments,
            )?;
            let mut fullscreen = OverlaySupervisor::sibling_of_current_exe_with_arguments(
                "pal-fullscreen-injector.exe",
                vec![
                    std::ffi::OsString::from("--parent-pid"),
                    std::ffi::OsString::from(std::process::id().to_string()),
                ],
            )?;
            loop {
                if run_control.shutdown_requested() {
                    break;
                }
                runtime.check_health()?;
                let status = overlay.tick(std::time::Instant::now());
                match status {
                    OverlaySupervisorStatus::ExecutableMissing => {
                        eprintln!("pal-core: pal-overlay.exe is not installed beside pal-core.exe");
                    }
                    OverlaySupervisorStatus::StartFailed => {
                        eprintln!("pal-core: pal-overlay.exe could not be started");
                    }
                    OverlaySupervisorStatus::Exited => {
                        eprintln!("pal-core: pal-overlay.exe exited; restart is scheduled");
                    }
                    OverlaySupervisorStatus::Started
                    | OverlaySupervisorStatus::Running
                    | OverlaySupervisorStatus::BackingOff => {}
                }
                let fullscreen_status = fullscreen.tick(std::time::Instant::now());
                match fullscreen_status {
                    OverlaySupervisorStatus::ExecutableMissing => {
                        eprintln!(
                            "pal-core: pal-fullscreen-injector.exe is not installed; \
                             external overlay fallback remains active"
                        );
                    }
                    OverlaySupervisorStatus::StartFailed => {
                        eprintln!(
                            "pal-core: fullscreen injector could not be started; \
                             external overlay fallback remains active"
                        );
                    }
                    OverlaySupervisorStatus::Exited => {
                        eprintln!(
                            "pal-core: fullscreen injector exited; restart is scheduled and \
                             external overlay fallback remains active"
                        );
                    }
                    OverlaySupervisorStatus::Started
                    | OverlaySupervisorStatus::Running
                    | OverlaySupervisorStatus::BackingOff => {}
                }
                thread::park_timeout(SUPERVISOR_TICK_INTERVAL.min(IDLE_PARK_INTERVAL));
            }
            Ok(())
        }
    }
}

fn initial_overlay_settings() -> Result<OverlaySettings, Box<dyn Error + Send + Sync>> {
    Ok(read_or_create_control(&default_control_path())?
        .settings
        .to_domain()?)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CoreSourceMode {
    Safe,
    ApprovedMapPack,
}

#[derive(Debug, Eq, PartialEq)]
enum CoreCommand {
    Help,
    HealthCheck,
    ApprovedMapPackCheck {
        dataset_root: PathBuf,
        game_build_id: String,
    },
    Run {
        source_mode: CoreSourceMode,
        overlay_arguments: Vec<OsString>,
        parent_pid: Option<u32>,
    },
}

fn parse_command(arguments: Vec<OsString>) -> Result<CoreCommand, Box<dyn Error + Send + Sync>> {
    match arguments.as_slice() {
        [argument] if argument == "--help" => Ok(CoreCommand::Help),
        [argument] if argument == "--health-check" => Ok(CoreCommand::HealthCheck),
        [flag, dataset_root, build_flag, game_build_id]
            if flag == APPROVED_MAP_PACK_CHECK_FLAG && build_flag == GAME_BUILD_ID_FLAG =>
        {
            let dataset_root = PathBuf::from(dataset_root);
            if !dataset_root.is_absolute() {
                return Err("approved map pack dataset root must be absolute".into());
            }
            let game_build_id = game_build_id
                .to_str()
                .filter(|value| {
                    !value.is_empty() && value.as_bytes().iter().all(u8::is_ascii_digit)
                })
                .ok_or("approved map pack Build ID must contain only ASCII digits")?
                .to_owned();
            Ok(CoreCommand::ApprovedMapPackCheck {
                dataset_root,
                game_build_id,
            })
        }
        [flag, dataset_root, build_flag, game_build_id]
            if flag == APPROVED_MAP_PACK_RUN_FLAG && build_flag == GAME_BUILD_ID_FLAG =>
        {
            approved_map_pack_run_command(dataset_root, game_build_id, None)
        }
        [parent_flag, parent_pid] if parent_flag == PARENT_PID_FLAG => Ok(CoreCommand::Run {
            source_mode: CoreSourceMode::Safe,
            overlay_arguments: Vec::new(),
            parent_pid: Some(parse_parent_pid(parent_pid)?),
        }),
        [
            parent_flag,
            parent_pid,
            flag,
            dataset_root,
            build_flag,
            game_build_id,
        ] if parent_flag == PARENT_PID_FLAG
            && flag == APPROVED_MAP_PACK_RUN_FLAG
            && build_flag == GAME_BUILD_ID_FLAG =>
        {
            approved_map_pack_run_command(
                dataset_root,
                game_build_id,
                Some(parse_parent_pid(parent_pid)?),
            )
        }
        [] => Ok(CoreCommand::Run {
            source_mode: CoreSourceMode::Safe,
            overlay_arguments: Vec::new(),
            parent_pid: None,
        }),
        _ => Err("unrecognized pal-core arguments".into()),
    }
}

fn parse_parent_pid(value: &OsStr) -> Result<u32, Box<dyn Error + Send + Sync>> {
    value
        .to_str()
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value != 0)
        .ok_or_else(|| "pal-core parent PID must be a positive decimal integer".into())
}

fn approved_map_pack_run_command(
    dataset_root: &OsStr,
    game_build_id: &OsStr,
    parent_pid: Option<u32>,
) -> Result<CoreCommand, Box<dyn Error + Send + Sync>> {
    let dataset_root = PathBuf::from(dataset_root);
    if !dataset_root.is_absolute() {
        return Err("approved map pack dataset root must be absolute".into());
    }
    let game_build_id = game_build_id
        .to_str()
        .filter(|value| !value.is_empty() && value.as_bytes().iter().all(u8::is_ascii_digit))
        .ok_or("approved map pack Build ID must contain only ASCII digits")?
        .to_owned();
    ApprovedMapPackIdentity::verify_published(&dataset_root, &game_build_id)?;
    Ok(CoreCommand::Run {
        source_mode: CoreSourceMode::ApprovedMapPack,
        overlay_arguments: vec![
            OsString::from("--approved-map-pack"),
            dataset_root.into_os_string(),
            OsString::from("--game-build-id"),
            OsString::from(game_build_id),
        ],
        parent_pid,
    })
}

struct CoreRunControl {
    mode_event: HANDLE,
    shutdown_event: HANDLE,
    parent_process: Option<HANDLE>,
}

impl CoreRunControl {
    fn new(mode: CoreSourceMode, parent_pid: Option<u32>) -> std::io::Result<Self> {
        let mode_name = match mode {
            CoreSourceMode::Safe => CORE_MODE_SAFE_EVENT,
            CoreSourceMode::ApprovedMapPack => CORE_MODE_APPROVED_MAP_PACK_EVENT,
        };
        let mode_name = wide(mode_name);
        let shutdown_name = wide(CORE_SHUTDOWN_EVENT);
        // SAFETY: both names are NUL-terminated and no security descriptor is borrowed.
        let mode_event = unsafe { CreateEventW(std::ptr::null(), 1, 1, mode_name.as_ptr()) };
        if mode_event.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: the name is NUL-terminated and no security descriptor is borrowed.
        let shutdown_event =
            unsafe { CreateEventW(std::ptr::null(), 1, 0, shutdown_name.as_ptr()) };
        if shutdown_event.is_null() {
            // SAFETY: mode_event is owned and live.
            unsafe { CloseHandle(mode_event) };
            return Err(std::io::Error::last_os_error());
        }
        let parent_process = if let Some(parent_pid) = parent_pid {
            // SAFETY: the PID is parsed from a positive decimal value and no handle is inherited.
            let handle = unsafe { OpenProcess(SYNCHRONIZE_ACCESS, 0, parent_pid) };
            if handle.is_null() {
                // SAFETY: both event handles are owned and live on this error path.
                unsafe {
                    CloseHandle(shutdown_event);
                    CloseHandle(mode_event);
                }
                return Err(std::io::Error::last_os_error());
            }
            Some(handle)
        } else {
            None
        };
        Ok(Self {
            mode_event,
            shutdown_event,
            parent_process,
        })
    }

    fn shutdown_requested(&self) -> bool {
        // SAFETY: shutdown_event is owned and live for this call.
        if unsafe { WaitForSingleObject(self.shutdown_event, 0) == WAIT_OBJECT_0 } {
            return true;
        }
        self.parent_process.is_some_and(|parent| {
            // SAFETY: parent is an owned live synchronization handle.
            unsafe { WaitForSingleObject(parent, 0) == WAIT_OBJECT_0 }
        })
    }
}

impl Drop for CoreRunControl {
    fn drop(&mut self) {
        // SAFETY: both handles are owned and closed once here.
        unsafe {
            if let Some(parent_process) = self.parent_process.take() {
                CloseHandle(parent_process);
            }
            CloseHandle(self.mode_event);
            CloseHandle(self.shutdown_event);
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_core_starts_the_safe_overlay_without_a_position_source() {
        assert_eq!(
            parse_command(Vec::new()).unwrap(),
            CoreCommand::Run {
                source_mode: CoreSourceMode::Safe,
                overlay_arguments: Vec::new(),
                parent_pid: None,
            }
        );
    }

    #[test]
    fn parent_pid_contract_is_explicit_and_rejects_invalid_values() {
        assert_eq!(
            parse_command(vec![OsString::from("--parent-pid"), OsString::from("42"),]).unwrap(),
            CoreCommand::Run {
                source_mode: CoreSourceMode::Safe,
                overlay_arguments: Vec::new(),
                parent_pid: Some(42),
            }
        );
        for value in ["", "0", "-1", "not-a-pid"] {
            assert!(parse_parent_pid(OsStr::new(value)).is_err());
        }
    }

    #[test]
    fn approved_map_run_rejects_relative_roots_and_malformed_builds_before_spawn() {
        assert!(
            approved_map_pack_run_command(OsStr::new("relative"), OsStr::new("24181527"), None,)
                .is_err()
        );
        let root = std::env::current_dir().unwrap();
        assert!(
            approved_map_pack_run_command(root.as_os_str(), OsStr::new("not-a-build"), None)
                .is_err()
        );
    }
}
