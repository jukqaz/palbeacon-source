#![allow(
    linker_messages,
    reason = "MSVC reports localized import-library creation as informational output"
)]

use std::{
    ffi::OsStr,
    io::Read,
    net::{IpAddr, SocketAddr},
    os::windows::ffi::OsStrExt as _,
    os::windows::process::CommandExt as _,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use keyring::Entry;
use pal_app_settings::{
    AppSettings, AppSettingsDocument, AppSettingsError, INSECURE_REST_HTTP_CONSENT_REVISION,
    InsecureRestHttpConsentV1, RestScheme, SERVER_CREDENTIAL_SERVICE_PREFIX,
    SERVER_PROFILES_SCHEMA, ServerProfile, ServerProfilesDocument, ServerProfilesError,
    read_app_settings, read_server_profiles, write_app_settings, write_server_profiles,
};
use pal_build_contract::{
    CURRENT_BUILD_HAS_LIVE_POSITION_PROFILE, CURRENT_GAME_BUILD_ID,
    WINDOWS_LIVE_POSITION_PROFILE_BUILD_ID,
};
use pal_overlay_control::{
    ControlSettings, OverlayControlDocument, default_control_path, read_or_create_control,
    write_control,
};
use pal_rest::{
    BasicAuthSecret, ClientConfig, EndpointPolicy, LiveSnapshotRequest, PalRestClient,
    ServerLiveSnapshotV1,
};
use pal_save_ingest::{
    default_staging_root, latest_staged_save, probe_save_source, stage_save_source,
    verify_staged_save,
};
use pal_sftp_sync::{read_status as read_sftp_status, sync_profile_once};
use pal_windows_ipc::{
    CORE_MODE_APPROVED_MAP_PACK_EVENT, CORE_MODE_SAFE_EVENT, CORE_PIPE_NAME, CORE_SHUTDOWN_EVENT,
    OVERLAY_POSITION_LIVE_EVENT, OVERLAY_RUNNING_EVENT,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::Manager as _;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
use tauri_plugin_window_state::StateFlags;
use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Pipes::WaitNamedPipeW,
        Threading::{OpenEventW, SetEvent, WaitForSingleObject},
    },
};

const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;
const SAVE_PARSER_FILE_NAME: &str = "pal-save-parser-worker.exe";
const SAVE_PARSER_DATA_DIRECTORY: &str = "save-parser-data";
const SAVE_PARSER_TIMEOUT: Duration = Duration::from_secs(120);
const SAVE_PARSER_MAX_STDOUT_BYTES: u64 = 32 * 1024 * 1024;
const SAVE_PARSER_MAX_STDERR_BYTES: u64 = 64 * 1024;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const EVENT_MODIFY_STATE_ACCESS: u32 = 0x0002;
const CORE_RUNTIME_FILE_NAME: &str = "pal-core.exe";
const OVERLAY_RUNTIME_FILE_NAME: &str = "pal-overlay.exe";
const INJECTOR_RUNTIME_FILE_NAME: &str = "pal-fullscreen-injector.exe";
const RHI_PROBE_FILE_NAME: &str = "pal-fullscreen-rhi-probe.dll";
const DX11_OVERLAY_FILE_NAME: &str = "pal-fullscreen-overlay-dx11.dll";
const DX12_OVERLAY_FILE_NAME: &str = "pal-fullscreen-overlay-dx12.dll";
const MAP_PACK_DIRECTORY: &str = "map-pack";
const CORE_START_TIMEOUT: Duration = Duration::from_secs(10);
static SERVER_PROFILE_MUTATION_LOCK: Mutex<()> = Mutex::new(());
static SFTP_SYNC_LOCK: Mutex<()> = Mutex::new(());
static SAVE_STAGING_LOCK: Mutex<()> = Mutex::new(());
static SAVE_PARSER_LOCK: Mutex<()> = Mutex::new(());
static OVERLAY_CONTROL_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug)]
struct RuntimePackageLayout {
    core_executable: PathBuf,
    map_pack_root: PathBuf,
}

fn runtime_package_layout(
    package_root: &Path,
    resource_root: &Path,
) -> Result<RuntimePackageLayout, String> {
    for file_name in [
        CORE_RUNTIME_FILE_NAME,
        OVERLAY_RUNTIME_FILE_NAME,
        INJECTOR_RUNTIME_FILE_NAME,
        RHI_PROBE_FILE_NAME,
        DX11_OVERLAY_FILE_NAME,
        DX12_OVERLAY_FILE_NAME,
    ] {
        if !package_root.join(file_name).is_file() {
            return Err(format!("Windows 런타임 파일이 없습니다: {file_name}"));
        }
    }
    let map_pack_root = resource_root.join(MAP_PACK_DIRECTORY);
    let active_pointer = map_pack_root.join(format!("{CURRENT_GAME_BUILD_ID}.active.json"));
    if !active_pointer.is_file() {
        return Err("현재 게임 버전의 지도 파일이 없습니다.".to_owned());
    }
    Ok(RuntimePackageLayout {
        core_executable: package_root.join(CORE_RUNTIME_FILE_NAME),
        map_pack_root,
    })
}

struct OwnedCoreRuntime {
    child: Mutex<Option<Child>>,
}

impl OwnedCoreRuntime {
    fn start(app: &tauri::AppHandle) -> Result<Self, String> {
        let package_root = std::env::current_exe()
            .map_err(|error| format!("Windows 앱 경로를 확인하지 못했습니다: {error}"))?
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "Windows 앱 경로를 확인하지 못했습니다.".to_owned())?;
        let resource_root = app
            .path()
            .resource_dir()
            .map_err(|error| format!("Windows 리소스 경로를 확인하지 못했습니다: {error}"))?;
        let layout = runtime_package_layout(&package_root, &resource_root)?;

        if core_pipe_available() {
            log::info!(
                "PalBeacon Core is already available; this shell will not start a duplicate"
            );
            return Ok(Self {
                child: Mutex::new(None),
            });
        }

        let mut child = Command::new(&layout.core_executable)
            .arg("--parent-pid")
            .arg(std::process::id().to_string())
            .arg("--overlay-approved-map-pack")
            .arg(&layout.map_pack_root)
            .arg("--game-build-id")
            .arg(CURRENT_GAME_BUILD_ID)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| format!("Windows 오버레이를 시작하지 못했습니다: {error}"))?;

        let started = Instant::now();
        loop {
            if core_pipe_available() {
                log::info!("PalBeacon Core started for game build {CURRENT_GAME_BUILD_ID}");
                return Ok(Self {
                    child: Mutex::new(Some(child)),
                });
            }
            if let Some(status) = child
                .try_wait()
                .map_err(|error| format!("Windows 오버레이 상태를 확인하지 못했습니다: {error}"))?
            {
                return Err(format!(
                    "Windows 오버레이가 준비되기 전에 종료됐습니다: {status}"
                ));
            }
            if started.elapsed() >= CORE_START_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Windows 오버레이 준비 시간이 초과됐습니다.".to_owned());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for OwnedCoreRuntime {
    fn drop(&mut self) {
        let Ok(child) = self.child.get_mut() else {
            return;
        };
        let Some(mut child) = child.take() else {
            return;
        };
        signal_core_shutdown();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeContext {
    platform: &'static str,
    shell: &'static str,
    app_version: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
enum NativeReadOperation {
    AppSettings,
    OverlayControl,
    RuntimeStatus,
    SaveImportStatus,
    ServerProfiles,
}

impl NativeReadOperation {
    fn read(self, app: &tauri::AppHandle) -> Result<Value, String> {
        match self {
            Self::AppSettings => app_settings(app),
            Self::OverlayControl => overlay_control(),
            Self::RuntimeStatus => Ok(runtime_status()),
            Self::SaveImportStatus => save_import_status(),
            Self::ServerProfiles => server_profiles(app),
        }
    }
}

fn ensure_overlay_control_version(
    expected_version: u64,
    current_version: u64,
) -> Result<(), String> {
    if expected_version == current_version {
        Ok(())
    } else {
        Err(
            "다른 오버레이 설정 변경이 먼저 저장되었습니다. 최신 상태를 다시 불러와 주세요."
                .to_owned(),
        )
    }
}

fn overlay_control() -> Result<Value, String> {
    let _guard = OVERLAY_CONTROL_LOCK
        .lock()
        .map_err(|_| "다른 오버레이 설정 작업이 비정상 종료됐습니다.".to_owned())?;
    let document =
        read_or_create_control(&default_control_path()).map_err(|error| error.to_string())?;
    serde_json::to_value(document).map_err(|error| error.to_string())
}

fn update_overlay_control(
    expected_version: u64,
    settings: ControlSettings,
) -> Result<Value, String> {
    let _guard = OVERLAY_CONTROL_LOCK
        .lock()
        .map_err(|_| "다른 오버레이 설정 작업이 비정상 종료됐습니다.".to_owned())?;
    let path = default_control_path();
    let current = read_or_create_control(&path).map_err(|error| error.to_string())?;
    ensure_overlay_control_version(expected_version, current.version)?;
    let next = OverlayControlDocument::next(settings, current.version)
        .map_err(|error| error.to_string())?;
    write_control(&path, &next).map_err(|error| error.to_string())?;
    serde_json::to_value(next).map_err(|error| error.to_string())
}

#[derive(Serialize)]
struct ServerProfileSummary {
    #[serde(flatten)]
    profile: ServerProfile,
    has_sftp_password: bool,
    has_rest_password: bool,
    has_insecure_rest_http_consent: bool,
}

#[derive(Serialize)]
struct ServerProfilesResponse {
    schema: &'static str,
    version: u64,
    updated_at_unix_ms: u64,
    selected_profile_id: Option<String>,
    profiles: Vec<ServerProfileSummary>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "NativeServerProfileInput"))]
#[serde(deny_unknown_fields)]
struct ServerProfileInput {
    id: String,
    display_name: String,
    host: String,
    rest_host: Option<String>,
    game_port: u16,
    query_port: Option<u16>,
    sftp_port: Option<u16>,
    rest_port: Option<u16>,
    rcon_port: Option<u16>,
    #[cfg_attr(test, ts(type = "\"http\" | \"https\""))]
    rest_scheme: RestScheme,
    rest_username: Option<String>,
    sftp_username: Option<String>,
    save_root: Option<String>,
    ssh_host_key_fingerprint: Option<String>,
}

impl ServerProfileInput {
    fn into_profile(self, current: Option<&ServerProfile>) -> ServerProfile {
        let mut profile = ServerProfile {
            id: self.id,
            display_name: self.display_name,
            host: self.host,
            rest_host: self.rest_host,
            game_port: self.game_port,
            query_port: self.query_port,
            sftp_port: self.sftp_port,
            rest_port: self.rest_port,
            rcon_port: self.rcon_port,
            rest_scheme: self.rest_scheme,
            rest_username: self.rest_username,
            sftp_username: self.sftp_username,
            save_root: self.save_root,
            ssh_host_key_fingerprint: self.ssh_host_key_fingerprint,
            insecure_rest_http_consent: None,
        };
        profile.insecure_rest_http_consent = current
            .filter(|existing| {
                existing.rest_scheme == profile.rest_scheme
                    && existing.literal_rest_socket_addr() == profile.literal_rest_socket_addr()
                    && existing.has_current_insecure_rest_http_consent()
            })
            .and_then(|existing| existing.insecure_rest_http_consent.clone());
        profile
    }
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "NativeServerProfileMutation"))]
#[serde(tag = "operation", rename_all = "snake_case")]
enum ServerProfileMutation {
    Upsert {
        expected_version: u64,
        profile: Box<ServerProfileInput>,
        sftp_password: Option<String>,
        rest_password: Option<String>,
    },
    Select {
        expected_version: u64,
        profile_id: Option<String>,
    },
    Delete {
        expected_version: u64,
        profile_id: String,
    },
    ConfirmInsecureRestHttp {
        expected_version: u64,
        profile_id: String,
        confirmed_origin: String,
        risk_revision: u16,
        accept_plaintext_credentials: bool,
        accept_untrusted_responses: bool,
    },
    RevokeInsecureRestHttp {
        expected_version: u64,
        profile_id: String,
    },
}

#[tauri::command]
fn runtime_context(app: tauri::AppHandle) -> RuntimeContext {
    RuntimeContext {
        platform: "windows",
        shell: "tauri",
        app_version: app.package_info().version.to_string(),
    }
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn named_event_is_signaled(name: &str) -> bool {
    let name = wide(name);
    // SAFETY: name is a live NUL-terminated UTF-16 buffer.
    let handle = unsafe { OpenEventW(SYNCHRONIZE_ACCESS, 0, name.as_ptr()) };
    if handle.is_null() {
        return false;
    }
    // SAFETY: handle is owned and live until CloseHandle below.
    let signaled = unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0;
    // SAFETY: handle is a live owned handle and is closed exactly once.
    unsafe { CloseHandle(handle) };
    signaled
}

fn signal_core_shutdown() {
    let name = wide(CORE_SHUTDOWN_EVENT);
    // SAFETY: name is a live NUL-terminated UTF-16 buffer.
    let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE_ACCESS, 0, name.as_ptr()) };
    if handle.is_null() {
        return;
    }
    // SAFETY: handle grants EVENT_MODIFY_STATE and is owned until CloseHandle below.
    unsafe {
        SetEvent(handle);
        CloseHandle(handle);
    }
}

fn core_pipe_available() -> bool {
    let endpoint = wide(CORE_PIPE_NAME);
    // SAFETY: endpoint is a live NUL-terminated UTF-16 buffer.
    (unsafe { WaitNamedPipeW(endpoint.as_ptr(), 50) }) != 0
}

fn exact_process_exists(image_name: &str) -> bool {
    // SAFETY: the snapshot has no borrowed inputs and returns an owned handle.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return false;
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(u32::MAX),
        ..Default::default()
    };
    // SAFETY: snapshot is live and entry points to writable initialized storage.
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    let mut found = false;
    while has_entry {
        let end = entry
            .szExeFile
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(entry.szExeFile.len());
        if String::from_utf16_lossy(&entry.szExeFile[..end]).eq_ignore_ascii_case(image_name) {
            found = true;
            break;
        }
        // SAFETY: snapshot is live and entry remains writable.
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    // SAFETY: snapshot is a live owned handle.
    unsafe { CloseHandle(snapshot) };
    found
}

fn runtime_status() -> Value {
    let core_connected = core_pipe_available();
    let approved_map_pack = named_event_is_signaled(CORE_MODE_APPROVED_MAP_PACK_EVENT);
    let safe_mode = named_event_is_signaled(CORE_MODE_SAFE_EVENT);
    let source_mode = if approved_map_pack {
        "approved_map_pack"
    } else if safe_mode {
        "safe"
    } else {
        "unavailable"
    };
    let game_running = exact_process_exists("Palworld-Win64-Shipping.exe");
    let overlay_running = named_event_is_signaled(OVERLAY_RUNNING_EVENT);
    let source_streaming = approved_map_pack
        && overlay_running
        && named_event_is_signaled(OVERLAY_POSITION_LIVE_EVENT);
    let live_position_ready = CURRENT_BUILD_HAS_LIVE_POSITION_PROFILE;
    let alignment_verified = approved_map_pack && live_position_ready;
    let position_live = source_streaming && alignment_verified && game_running;
    let freshness = if position_live {
        "live"
    } else if source_streaming {
        "unverified"
    } else if game_running {
        "waiting"
    } else {
        "offline"
    };
    let message_ko = if !core_connected {
        "Windows 앱을 다시 시작해 주세요."
    } else if !game_running {
        "팰월드를 실행하면 오버레이가 표시됩니다."
    } else if !overlay_running {
        "오버레이를 준비하고 있습니다."
    } else if !approved_map_pack {
        "오버레이 지도를 준비할 수 없습니다."
    } else if !live_position_ready {
        "현재 위치를 사용할 수 없어 지도만 표시합니다."
    } else if position_live {
        "현재 위치가 지도에 표시됩니다."
    } else if source_streaming {
        "현재 위치를 확인하고 있습니다."
    } else {
        "새 위치 정보를 기다리고 있습니다."
    };
    json!({
        "core_connected": core_connected,
        "game_running": game_running,
        "overlay_running": overlay_running,
        "position_live": position_live,
        "map_ready": approved_map_pack,
        "source_mode": source_mode,
        "freshness": freshness,
        "live_position_ready": live_position_ready,
        "map_alignment_verified": approved_map_pack,
        "supported_client_build_id": WINDOWS_LIVE_POSITION_PROFILE_BUILD_ID,
        "map_build_id": approved_map_pack.then_some(CURRENT_GAME_BUILD_ID),
        "alignment_verified": alignment_verified,
        "message_ko": message_ko,
    })
}

fn save_import_status() -> Result<Value, String> {
    let staging_root = default_staging_root().map_err(|error| error.to_string())?;
    let latest = latest_staged_save(&staging_root).map_err(|error| error.to_string())?;
    let parser_ready = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .is_some_and(|path| {
            path.join(SAVE_PARSER_FILE_NAME).is_file()
                && path
                    .join(SAVE_PARSER_DATA_DIRECTORY)
                    .join("pals.json")
                    .is_file()
        });
    let has_latest = latest.is_some();
    Ok(json!({
        "latest": latest,
        "parser_ready": parser_ready,
        "message_ko": if has_latest {
            if parser_ready {
                "보호 복사본과 읽기 전용 내 팰 해석기가 준비됐습니다."
            } else {
                "보호 복사본은 준비됐지만 내 팰 해석기 파일이 없습니다."
            }
        } else {
            "가져온 서버 세이브가 없습니다."
        },
    }))
}

fn is_import_id(value: &str) -> bool {
    value.len() == 24 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_canonical_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn read_bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut output)
        .map_err(|error| format!("내 팰 해석기 출력을 읽지 못했습니다: {error}"))?;
    if u64::try_from(output.len()).unwrap_or(u64::MAX) > limit {
        return Err("내 팰 해석 결과가 허용 크기를 초과했습니다.".to_owned());
    }
    Ok(output)
}

fn inspect_staged_save(import_id: &str, owner_uid: Option<&str>) -> Result<Value, String> {
    if !is_import_id(import_id) {
        return Err("보호 복사본 식별자가 올바르지 않습니다.".to_owned());
    }
    if owner_uid.is_some_and(|value| !is_canonical_uuid(value)) {
        return Err("플레이어 식별자가 올바르지 않습니다.".to_owned());
    }
    let _guard = SAVE_PARSER_LOCK
        .lock()
        .map_err(|_| "다른 세이브 해석 작업이 비정상 종료됐습니다.".to_owned())?;
    let staging_root = default_staging_root().map_err(|error| error.to_string())?;
    let staged_root = staging_root.join(import_id);
    let staged =
        verify_staged_save(&staged_root, &staging_root).map_err(|error| error.to_string())?;
    if staged.import_id != import_id {
        return Err("보호 복사본 식별자가 manifest와 일치하지 않습니다.".to_owned());
    }

    let package_root = std::env::current_exe()
        .map_err(|error| error.to_string())?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "내 팰 해석기 경로를 확인하지 못했습니다.".to_owned())?;
    let executable = package_root.join(SAVE_PARSER_FILE_NAME);
    let data_directory = package_root.join(SAVE_PARSER_DATA_DIRECTORY);
    if !executable.is_file() || !data_directory.join("pals.json").is_file() {
        return Err("내 팰 해석기 파일이 없습니다. 최신 패키지를 다시 설치하세요.".to_owned());
    }

    let mut command = Command::new(executable);
    command
        .arg("--staged-root")
        .arg(&staged.staged_root)
        .arg("--staging-root")
        .arg(&staging_root)
        .arg("--data-dir")
        .arg(data_directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);
    if let Some(owner_uid) = owner_uid {
        command.arg("--owner-uid").arg(owner_uid);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("내 팰 해석기를 시작하지 못했습니다: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "내 팰 해석기 출력 연결을 만들지 못했습니다.".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "내 팰 해석기 오류 연결을 만들지 못했습니다.".to_owned())?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout, SAVE_PARSER_MAX_STDOUT_BYTES));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, SAVE_PARSER_MAX_STDERR_BYTES));

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("내 팰 해석기 상태를 확인하지 못했습니다: {error}"))?
        {
            break status;
        }
        if started.elapsed() >= SAVE_PARSER_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(
                "세이브 해석이 2분을 초과해 중단했습니다. 더 작은 최신 백업으로 다시 시도하세요."
                    .to_owned(),
            );
        }
        thread::sleep(Duration::from_millis(50));
    };
    let output = stdout_reader
        .join()
        .map_err(|_| "내 팰 해석기 출력 작업이 중단됐습니다.".to_owned())??;
    let error_output = stderr_reader
        .join()
        .map_err(|_| "내 팰 해석기 오류 작업이 중단됐습니다.".to_owned())??;
    if !status.success() {
        let detail = String::from_utf8_lossy(&error_output).trim().to_owned();
        return Err(if detail.is_empty() {
            format!("내 팰 해석에 실패했습니다. 종료 코드: {status}")
        } else {
            detail
        });
    }
    if output.is_empty() {
        return Err("내 팰 해석기가 빈 결과를 반환했습니다.".to_owned());
    }
    let parsed: Value = serde_json::from_slice(&output)
        .map_err(|error| format!("내 팰 해석 결과가 올바른 JSON이 아닙니다: {error}"))?;
    if parsed["import_id"] != import_id {
        return Err("내 팰 해석 결과가 선택한 보호 복사본과 일치하지 않습니다.".to_owned());
    }
    if parsed["selected_owner_uid"].as_str() != owner_uid {
        return Err("내 팰 해석 결과의 플레이어 선택이 요청과 일치하지 않습니다.".to_owned());
    }
    Ok(parsed)
}

fn secret_exists(profile_id: &str, kind: &str) -> bool {
    Entry::new(
        &format!("{SERVER_CREDENTIAL_SERVICE_PREFIX}/{profile_id}"),
        kind,
    )
    .and_then(|entry| entry.get_password())
    .is_ok_and(|value| !value.trim().is_empty())
}

fn credential_entry(profile_id: &str, kind: &str) -> Result<Entry, String> {
    Entry::new(
        &format!("{SERVER_CREDENTIAL_SERVICE_PREFIX}/{profile_id}"),
        kind,
    )
    .map_err(|error| format!("Windows 자격 증명 저장소를 열 수 없습니다: {error}"))
}

fn read_secret(profile_id: &str, kind: &str) -> Result<Option<String>, String> {
    let entry = credential_entry(profile_id, kind)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!(
            "Windows 자격 증명 저장소에서 비밀번호를 읽지 못했습니다: {error}"
        )),
    }
}

fn set_secret(profile_id: &str, kind: &str, value: &str) -> Result<(), String> {
    credential_entry(profile_id, kind)?
        .set_password(value)
        .map_err(|error| {
            format!("Windows 자격 증명 저장소에 비밀번호를 저장하지 못했습니다: {error}")
        })
}

fn delete_secret(profile_id: &str, kind: &str) {
    if let Ok(entry) = credential_entry(profile_id, kind) {
        let _ = entry.delete_credential();
    }
}

fn restore_secret(profile_id: &str, kind: &str, previous: Option<&str>) {
    if let Some(previous) = previous {
        let _ = set_secret(profile_id, kind, previous);
    } else {
        delete_secret(profile_id, kind);
    }
}

fn app_config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|error| format!("앱 설정 경로를 확인하지 못했습니다: {error}"))
}

fn server_profiles_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app_config_dir(app).map(|directory| directory.join("server-profiles-v1.json"))
}

fn read_server_profiles_document(app: &tauri::AppHandle) -> Result<ServerProfilesDocument, String> {
    match read_server_profiles(&server_profiles_path(app)?) {
        Ok(document) => Ok(document),
        Err(ServerProfilesError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(ServerProfilesDocument::default())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn server_profiles(app: &tauri::AppHandle) -> Result<Value, String> {
    let document = read_server_profiles_document(app)?;
    server_profiles_response(document)
}

fn server_profiles_response(document: ServerProfilesDocument) -> Result<Value, String> {
    let profiles = document
        .profiles
        .into_iter()
        .map(|profile| ServerProfileSummary {
            has_sftp_password: secret_exists(&profile.id, "sftp"),
            has_rest_password: secret_exists(&profile.id, "rest"),
            has_insecure_rest_http_consent: profile.has_current_insecure_rest_http_consent(),
            profile,
        })
        .collect();
    serde_json::to_value(ServerProfilesResponse {
        schema: SERVER_PROFILES_SCHEMA,
        version: document.version,
        updated_at_unix_ms: document.updated_at_unix_ms,
        selected_profile_id: document.selected_profile_id,
        profiles,
    })
    .map_err(|error| error.to_string())
}

fn ensure_server_profile_version(actual: u64, expected: u64) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(
            "다른 서버 프로필 변경이 먼저 저장되었습니다. 최신 상태를 다시 불러와 주세요."
                .to_owned(),
        )
    }
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn apply_secret_updates(
    profile_id: &str,
    updates: [(&'static str, Option<String>); 2],
) -> Result<Vec<(&'static str, Option<String>)>, String> {
    let mut applied: Vec<(&'static str, Option<String>)> = Vec::new();
    for (kind, value) in updates {
        let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        let previous = read_secret(profile_id, kind)?;
        if let Err(error) = set_secret(profile_id, kind, value.trim()) {
            for (applied_kind, applied_previous) in applied.iter().rev() {
                restore_secret(profile_id, applied_kind, applied_previous.as_deref());
            }
            return Err(error);
        }
        applied.push((kind, previous));
    }
    Ok(applied)
}

fn mutate_server_profiles(
    app: &tauri::AppHandle,
    request: ServerProfileMutation,
) -> Result<Value, String> {
    let _guard = SERVER_PROFILE_MUTATION_LOCK
        .lock()
        .map_err(|_| "서버 프로필 변경 잠금이 손상되었습니다.".to_owned())?;
    let path = server_profiles_path(app)?;
    let current = read_server_profiles_document(app)?;

    match request {
        ServerProfileMutation::Upsert {
            expected_version,
            profile,
            sftp_password,
            rest_password,
        } => {
            ensure_server_profile_version(current.version, expected_version)?;
            let existing = current
                .profiles
                .iter()
                .find(|existing| existing.id == profile.id);
            let profile = (*profile).into_profile(existing);
            profile.validate().map_err(|error| error.to_string())?;
            let profile_id = profile.id.clone();
            let was_empty = current.profiles.is_empty();
            let mut next = current
                .clone()
                .upsert(profile)
                .map_err(|error| error.to_string())?;
            if was_empty {
                next = next
                    .select(Some(profile_id.clone()))
                    .map_err(|error| error.to_string())?;
            }
            let previous_secrets = apply_secret_updates(
                &profile_id,
                [("sftp", sftp_password), ("rest", rest_password)],
            )?;
            if let Err(error) = write_server_profiles(&path, &next) {
                for (kind, previous) in previous_secrets.iter().rev() {
                    restore_secret(&profile_id, kind, previous.as_deref());
                }
                return Err(error.to_string());
            }
            server_profiles_response(next)
        }
        ServerProfileMutation::Select {
            expected_version,
            profile_id,
        } => {
            ensure_server_profile_version(current.version, expected_version)?;
            let next = current
                .select(profile_id)
                .map_err(|error| error.to_string())?;
            write_server_profiles(&path, &next).map_err(|error| error.to_string())?;
            server_profiles_response(next)
        }
        ServerProfileMutation::Delete {
            expected_version,
            profile_id,
        } => {
            ensure_server_profile_version(current.version, expected_version)?;
            let next = current
                .remove(&profile_id)
                .map_err(|error| error.to_string())?;
            write_server_profiles(&path, &next).map_err(|error| error.to_string())?;
            delete_secret(&profile_id, "sftp");
            delete_secret(&profile_id, "rest");
            server_profiles_response(next)
        }
        ServerProfileMutation::ConfirmInsecureRestHttp {
            expected_version,
            profile_id,
            confirmed_origin,
            risk_revision,
            accept_plaintext_credentials,
            accept_untrusted_responses,
        } => {
            ensure_server_profile_version(current.version, expected_version)?;
            if risk_revision != INSECURE_REST_HTTP_CONSENT_REVISION
                || !accept_plaintext_credentials
                || !accept_untrusted_responses
            {
                return Err(
                    "평문 HTTP의 자격 증명 노출 및 응답 변조 위험을 모두 확인해 주세요.".to_owned(),
                );
            }
            let mut profile = current
                .profiles
                .iter()
                .find(|profile| profile.id == profile_id)
                .cloned()
                .ok_or_else(|| "서버 프로필을 찾을 수 없습니다.".to_owned())?;
            if profile.rest_scheme != RestScheme::Http {
                return Err("HTTPS 프로필에는 평문 HTTP 동의가 필요하지 않습니다.".to_owned());
            }
            let endpoint = profile.literal_rest_socket_addr().ok_or_else(|| {
                "평문 HTTP는 숫자로 된 단일 IP 주소와 유효한 REST 포트에서만 허용됩니다.".to_owned()
            })?;
            if confirmed_origin != format!("http://{endpoint}") {
                return Err("확인한 HTTP 주소가 현재 프로필과 일치하지 않습니다.".to_owned());
            }
            profile.insecure_rest_http_consent = Some(InsecureRestHttpConsentV1 {
                endpoint,
                risk_revision,
                confirmed_at_unix_ms: current_unix_ms(),
            });
            let next = current.upsert(profile).map_err(|error| error.to_string())?;
            write_server_profiles(&path, &next).map_err(|error| error.to_string())?;
            server_profiles_response(next)
        }
        ServerProfileMutation::RevokeInsecureRestHttp {
            expected_version,
            profile_id,
        } => {
            ensure_server_profile_version(current.version, expected_version)?;
            let mut profile = current
                .profiles
                .iter()
                .find(|profile| profile.id == profile_id)
                .cloned()
                .ok_or_else(|| "서버 프로필을 찾을 수 없습니다.".to_owned())?;
            if profile.insecure_rest_http_consent.is_none() {
                return server_profiles_response(current);
            }
            profile.insecure_rest_http_consent = None;
            let next = current.upsert(profile).map_err(|error| error.to_string())?;
            write_server_profiles(&path, &next).map_err(|error| error.to_string())?;
            server_profiles_response(next)
        }
    }
}

enum PreparedLiveRestOrigin {
    Https(String),
    ExplicitHttp(SocketAddr),
}

fn prepare_live_rest(profile: &ServerProfile) -> Result<(String, PreparedLiveRestOrigin), String> {
    let port = profile
        .rest_port
        .filter(|port| *port != 0)
        .ok_or_else(|| "REST 포트를 설정해 주세요.".to_owned())?;
    let username = profile
        .rest_username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "REST 사용자 이름을 설정해 주세요.".to_owned())?
        .to_owned();
    let host = profile.effective_rest_host().trim();
    if host.is_empty() {
        return Err("REST 연결 주소 설정을 확인해 주세요.".to_owned());
    }
    let origin = match profile.rest_scheme {
        RestScheme::Https => {
            let authority = match host.parse::<IpAddr>() {
                Ok(IpAddr::V6(_)) => format!("[{host}]:{port}"),
                _ => format!("{host}:{port}"),
            };
            PreparedLiveRestOrigin::Https(format!("https://{authority}/v1/api/"))
        }
        RestScheme::Http => {
            let endpoint = profile.literal_rest_socket_addr().ok_or_else(|| {
                "평문 HTTP는 숫자로 된 단일 IP 주소에서만 사용할 수 있습니다.".to_owned()
            })?;
            if !profile.has_current_insecure_rest_http_consent() {
                return Err("현재 IP와 포트에 대한 평문 HTTP 위험 동의가 필요합니다.".to_owned());
            }
            PreparedLiveRestOrigin::ExplicitHttp(endpoint)
        }
    };
    Ok((username, origin))
}

async fn server_live_snapshot(
    app: &tauri::AppHandle,
    profile_id: &str,
    request: LiveSnapshotRequest,
) -> Result<Value, String> {
    let document = read_server_profiles_document(app)?;
    let Some(profile) = document
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
    else {
        return serde_json::to_value(ServerLiveSnapshotV1::not_configured(
            profile_id,
            "서버 프로필을 찾을 수 없습니다.",
        ))
        .map_err(|error| error.to_string());
    };
    let (username, origin) = match prepare_live_rest(profile) {
        Ok(prepared) => prepared,
        Err(message) => {
            return serde_json::to_value(ServerLiveSnapshotV1::not_configured(profile_id, message))
                .map_err(|error| error.to_string());
        }
    };
    let password = match read_secret(&profile.id, "rest")? {
        Some(password) if !password.trim().is_empty() => password,
        _ => {
            return serde_json::to_value(ServerLiveSnapshotV1::not_configured(
                profile_id,
                "REST 비밀번호를 Windows 자격 증명에 저장해 주세요.",
            ))
            .map_err(|error| error.to_string());
        }
    };
    let auth = BasicAuthSecret::new(username, password)
        .map_err(|_| "REST 사용자 이름 또는 비밀번호 설정을 확인해 주세요.".to_owned())?;
    let client = match origin {
        PreparedLiveRestOrigin::Https(base_url) => {
            let policy = EndpointPolicy::loopback_only()
                .allow_exact_remote_https(&base_url)
                .map_err(|_| "REST HTTPS 주소를 허용 목록에 고정하지 못했습니다.".to_owned())?;
            let config = ClientConfig::new(&base_url, policy)
                .map_err(|_| "REST 클라이언트를 준비하지 못했습니다.".to_owned())?;
            PalRestClient::new_explicit_remote_https(config, auth)
        }
        PreparedLiveRestOrigin::ExplicitHttp(endpoint) => {
            let config = ClientConfig::explicit_insecure_http(endpoint)
                .map_err(|_| "평문 REST 주소를 안전하게 고정하지 못했습니다.".to_owned())?;
            PalRestClient::new_explicit_insecure_http(config, auth)
        }
    }
    .map_err(|_| "REST 클라이언트를 준비하지 못했습니다.".to_owned())?;
    let snapshot = client
        .collect_server_live_snapshot(profile_id, request)
        .await;
    serde_json::to_value(snapshot).map_err(|error| error.to_string())
}

fn sftp_status(profile_id: &str) -> Result<Value, String> {
    serde_json::to_value(read_sftp_status(profile_id).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

fn sync_sftp(app: &tauri::AppHandle, profile_id: &str) -> Result<Value, String> {
    let _guard = SFTP_SYNC_LOCK
        .lock()
        .map_err(|_| "SFTP 동기화 잠금이 손상되었습니다.".to_owned())?;
    let document = read_server_profiles_document(app)?;
    let profile = document
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "서버 프로필을 찾을 수 없습니다.".to_owned())?;
    let status = sync_profile_once(profile).map_err(|error| error.to_string())?;
    serde_json::to_value(status).map_err(|error| error.to_string())
}

fn app_settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app_config_dir(app).map(|directory| directory.join("app-settings-v1.json"))
}

fn read_app_settings_document(app: &tauri::AppHandle) -> Result<AppSettingsDocument, String> {
    match read_app_settings(&app_settings_path(app)?) {
        Ok(document) => Ok(document),
        Err(AppSettingsError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(AppSettingsDocument::default())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn app_settings(app: &tauri::AppHandle) -> Result<Value, String> {
    serde_json::to_value(read_app_settings_document(app)?).map_err(|error| error.to_string())
}

fn update_app_settings(
    app: &tauri::AppHandle,
    expected_version: u64,
    settings: AppSettings,
) -> Result<Value, String> {
    let path = app_settings_path(app)?;
    let current = read_app_settings_document(app)?;
    if current.version != expected_version {
        return Err(
            "다른 설정 변경이 먼저 저장되었습니다. 최신 상태를 다시 불러와 주세요.".to_owned(),
        );
    }
    let next =
        AppSettingsDocument::next(settings, current.version).map_err(|error| error.to_string())?;
    write_app_settings(&path, &next).map_err(|error| error.to_string())?;
    serde_json::to_value(next).map_err(|error| error.to_string())
}

#[tauri::command]
async fn native_read(
    app: tauri::AppHandle,
    operation: NativeReadOperation,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || operation.read(&app))
        .await
        .map_err(|error| format!("로컬 게이트웨이 작업이 중단됐습니다: {error}"))?
}

#[tauri::command]
async fn native_update_settings(
    app: tauri::AppHandle,
    expected_version: u64,
    settings: AppSettings,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        update_app_settings(&app, expected_version, settings)
    })
    .await
    .map_err(|error| format!("설정 저장 작업이 중단됐습니다: {error}"))?
}

#[tauri::command]
async fn native_update_overlay_control(
    expected_version: u64,
    settings: ControlSettings,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || update_overlay_control(expected_version, settings))
        .await
        .map_err(|error| format!("오버레이 설정 저장 작업이 중단됐습니다: {error}"))?
}

#[tauri::command]
async fn native_mutate_server_profiles(
    app: tauri::AppHandle,
    request: ServerProfileMutation,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || mutate_server_profiles(&app, request))
        .await
        .map_err(|error| format!("서버 프로필 변경 작업이 중단됐습니다: {error}"))?
}

#[tauri::command]
async fn native_server_live(
    app: tauri::AppHandle,
    profile_id: String,
    request: LiveSnapshotRequest,
) -> Result<Value, String> {
    server_live_snapshot(&app, &profile_id, request).await
}

#[tauri::command]
async fn native_sftp_status(profile_id: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || sftp_status(&profile_id))
        .await
        .map_err(|error| format!("SFTP 상태 조회가 중단됐습니다: {error}"))?
}

#[tauri::command]
async fn native_sync_sftp(app: tauri::AppHandle, profile_id: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || sync_sftp(&app, &profile_id))
        .await
        .map_err(|error| format!("SFTP 동기화가 중단됐습니다: {error}"))?
}

#[tauri::command]
async fn native_probe_save_source(source_path: PathBuf) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let probe = probe_save_source(&source_path).map_err(|error| error.to_string())?;
        serde_json::to_value(probe).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("세이브 폴더 검사가 중단됐습니다: {error}"))?
}

#[tauri::command]
async fn native_stage_save_source(source_path: PathBuf) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = SAVE_STAGING_LOCK
            .lock()
            .map_err(|_| "다른 보호 복사 작업이 비정상 종료됐습니다.".to_owned())?;
        let staging_root = default_staging_root().map_err(|error| error.to_string())?;
        let staged =
            stage_save_source(&source_path, &staging_root).map_err(|error| error.to_string())?;
        serde_json::to_value(staged).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("세이브 보호 복사가 중단됐습니다: {error}"))?
}

#[tauri::command]
async fn native_inspect_staged_save(
    import_id: String,
    owner_uid: Option<String>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        inspect_staged_save(&import_id, owner_uid.as_deref())
    })
    .await
    .map_err(|error| format!("내 팰 해석 작업이 중단됐습니다: {error}"))?
}

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--startup"])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::SIZE | StateFlags::MAXIMIZED)
                .build(),
        )
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .target(Target::new(TargetKind::LogDir {
                    file_name: Some("palbeacon".to_owned()),
                }))
                .max_file_size(2_000_000)
                .rotation_strategy(RotationStrategy::KeepOne)
                .level(log::LevelFilter::Info)
                .build(),
        )
        .setup(|app| {
            let runtime = OwnedCoreRuntime::start(app.handle()).map_err(std::io::Error::other)?;
            app.manage(runtime);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            runtime_context,
            native_read,
            native_update_settings,
            native_update_overlay_control,
            native_mutate_server_profiles,
            native_server_live,
            native_sftp_status,
            native_sync_sftp,
            native_probe_save_source,
            native_stage_save_source,
            native_inspect_staged_save
        ])
        .run(tauri::generate_context!())
        .expect("PalBeacon Tauri runtime failed");
}

#[cfg(test)]
mod tests {
    use super::{
        CORE_RUNTIME_FILE_NAME, DX11_OVERLAY_FILE_NAME, DX12_OVERLAY_FILE_NAME,
        INJECTOR_RUNTIME_FILE_NAME, MAP_PACK_DIRECTORY, NativeReadOperation,
        OVERLAY_RUNTIME_FILE_NAME, RHI_PROBE_FILE_NAME, RuntimeContext, ServerProfileMutation,
        ensure_overlay_control_version, ensure_server_profile_version, is_canonical_uuid,
        is_import_id, runtime_package_layout, runtime_status,
    };
    use pal_save_ingest::{SaveIngestError, probe_save_source};
    use std::path::Path;
    use ts_rs::TS as _;

    #[test]
    fn typescript_native_command_contract_is_current() {
        let config = ts_rs::Config::default().with_large_int("number");
        let expected = format!(
            "// @generated by ts-rs from the Tauri command boundary. Do not edit manually.\n\nexport {}\n\nexport {}\n\nexport {}\n",
            NativeReadOperation::decl(&config),
            super::ServerProfileInput::decl(&config),
            ServerProfileMutation::decl(&config),
        );
        let actual = include_str!(
            "../../../palbeacon-ui/src/lib/connection/generated/native-command-contract.ts"
        );
        let canonical = |source: &str| {
            source
                .split_whitespace()
                .collect::<String>()
                .replace('"', "'")
                .replace(',', ";")
                .replace("{'operation':", "{operation:")
                .replace(";}", "}")
                .replace("=|", "=")
        };
        assert_eq!(canonical(actual), canonical(&expected));
    }

    #[test]
    fn runtime_context_serializes_with_frontend_contract_names() {
        let json = serde_json::to_value(RuntimeContext {
            platform: "windows",
            shell: "tauri",
            app_version: "0.1.0".to_owned(),
        })
        .expect("runtime context should serialize");

        assert_eq!(json["platform"], "windows");
        assert_eq!(json["shell"], "tauri");
        assert_eq!(json["appVersion"], "0.1.0");
    }

    #[test]
    fn native_read_operations_are_explicitly_allowlisted() {
        for name in [
            "app_settings",
            "overlay_control",
            "runtime_status",
            "save_import_status",
            "server_profiles",
        ] {
            serde_json::from_str::<NativeReadOperation>(&format!("\"{name}\""))
                .expect("allowlisted native read operation");
        }
        assert!(serde_json::from_str::<NativeReadOperation>("\"put_overlay_settings\"").is_err());
    }

    #[test]
    fn windows_runtime_layout_requires_every_owned_binary_and_exact_map_pointer() {
        let root =
            std::env::temp_dir().join(format!("palbeacon-runtime-layout-{}", std::process::id()));
        let package_root = root.join("package");
        let resource_root = root.join("resources");
        std::fs::create_dir_all(&package_root).unwrap();
        std::fs::create_dir_all(resource_root.join(MAP_PACK_DIRECTORY)).unwrap();
        for file_name in [
            CORE_RUNTIME_FILE_NAME,
            OVERLAY_RUNTIME_FILE_NAME,
            INJECTOR_RUNTIME_FILE_NAME,
            RHI_PROBE_FILE_NAME,
            DX11_OVERLAY_FILE_NAME,
            DX12_OVERLAY_FILE_NAME,
        ] {
            std::fs::write(package_root.join(file_name), b"fixture").unwrap();
        }
        std::fs::write(
            resource_root.join(MAP_PACK_DIRECTORY).join(format!(
                "{}.active.json",
                pal_build_contract::CURRENT_GAME_BUILD_ID
            )),
            b"{}",
        )
        .unwrap();

        let layout = runtime_package_layout(&package_root, &resource_root).unwrap();
        assert_eq!(
            layout.core_executable,
            package_root.join(CORE_RUNTIME_FILE_NAME)
        );
        assert_eq!(layout.map_pack_root, resource_root.join(MAP_PACK_DIRECTORY));

        std::fs::remove_file(package_root.join(DX12_OVERLAY_FILE_NAME)).unwrap();
        assert!(runtime_package_layout(&package_root, &resource_root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn overlay_control_mutations_require_the_current_version() {
        assert!(ensure_overlay_control_version(7, 7).is_ok());
        assert!(ensure_overlay_control_version(6, 7).is_err());
        assert!(ensure_overlay_control_version(8, 7).is_err());
    }

    #[test]
    fn runtime_status_always_keeps_build_and_visibility_contracts() {
        let status = runtime_status();
        assert!(status["supported_client_build_id"].is_string());
        assert!(status["position_live"].is_boolean());
        let message = status["message_ko"]
            .as_str()
            .expect("runtime message should be Korean user guidance");
        for hidden_term in ["Core", "빌드", "검증", "좌표"] {
            assert!(!message.contains(hidden_term));
        }
    }

    #[test]
    fn server_profile_mutations_are_versioned_and_explicitly_allowlisted() {
        let select = r#"{"operation":"select","expected_version":7,"profile_id":"main-server"}"#;
        assert!(serde_json::from_str::<ServerProfileMutation>(select).is_ok());
        let delete = r#"{"operation":"delete","expected_version":7,"profile_id":"main-server"}"#;
        assert!(serde_json::from_str::<ServerProfileMutation>(delete).is_ok());
        let arbitrary = r#"{"operation":"invoke_bridge","expected_version":7}"#;
        assert!(serde_json::from_str::<ServerProfileMutation>(arbitrary).is_err());
        let consent = r#"{"operation":"confirm_insecure_rest_http","expected_version":7,"profile_id":"main-server","confirmed_origin":"http://192.0.2.10:8212","risk_revision":1,"accept_plaintext_credentials":true,"accept_untrusted_responses":true}"#;
        assert!(serde_json::from_str::<ServerProfileMutation>(consent).is_ok());
        assert!(ensure_server_profile_version(7, 7).is_ok());
        assert!(ensure_server_profile_version(8, 7).is_err());
    }

    #[test]
    fn save_source_probe_rejects_relative_paths_before_file_access() {
        assert!(matches!(
            probe_save_source(Path::new("relative-save-folder")),
            Err(SaveIngestError::RelativePath)
        ));
    }

    #[test]
    fn save_parser_identifiers_are_strictly_bounded() {
        assert!(is_import_id("b0bc52eda611399d11efe26f"));
        assert!(!is_import_id("../another-users-save"));
        assert!(is_canonical_uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"));
        assert!(!is_canonical_uuid("../not-an-owner"));
    }
}
