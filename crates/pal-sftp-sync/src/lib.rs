//! Read-only Palworld save snapshot synchronization over SFTP.
//!
//! The synchronizer never writes to the remote server. It verifies a pinned
//! SSH host key before authentication, downloads only the bounded Palworld
//! save allowlist to a private temporary directory, and then hands the stable
//! copy to `pal-save-ingest`.

#![forbid(unsafe_code)]

use std::{
    collections::VecDeque,
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read as _, Write as _},
    net::{SocketAddr, TcpStream, ToSocketAddrs as _},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use keyring::Entry;
use pal_app_settings::{
    SERVER_CREDENTIAL_SERVICE_PREFIX, ServerProfile, default_server_profiles_path,
    read_or_create_server_profiles,
};
use pal_save_ingest::{StagedSaveSet, default_staging_root, stage_save_source};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use ssh2::{FileStat, HashType, Session, Sftp};
use thiserror::Error;
use zeroize::Zeroize as _;

const STATUS_SCHEMA: &str = "palbeacon.sftp_sync_status.v1";
const STATUS_FILE_NAME: &str = "status-v1.json";
const MAX_DISCOVERY_DEPTH: usize = 2;
const MAX_REMOTE_DIRECTORIES: usize = 128;
const MAX_REMOTE_ENTRIES: usize = 2_048;
const MAX_PLAYER_FILES: usize = 1_024;
const MAX_LEVEL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_PLAYER_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 256 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(12);
pub const DEFAULT_SYNC_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SftpSyncState {
    Disabled,
    AwaitingConfiguration,
    AwaitingPassword,
    Checking,
    UpToDate,
    Synced,
    Error,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SftpSyncStatus {
    pub schema: String,
    pub profile_id: Option<String>,
    pub state: SftpSyncState,
    pub checked_at_unix_ms: u64,
    pub last_success_at_unix_ms: Option<u64>,
    pub remote_revision: Option<String>,
    pub import_id: Option<String>,
    pub player_file_count: Option<usize>,
    pub total_bytes: Option<u64>,
    pub message_ko: String,
}

impl SftpSyncStatus {
    pub fn disabled(message: impl Into<String>) -> Self {
        Self {
            schema: STATUS_SCHEMA.to_owned(),
            profile_id: None,
            state: SftpSyncState::Disabled,
            checked_at_unix_ms: unix_ms(),
            last_success_at_unix_ms: None,
            remote_revision: None,
            import_id: None,
            player_file_count: None,
            total_bytes: None,
            message_ko: message.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum SftpSyncError {
    #[error("SFTP 동기화 프로필을 찾을 수 없습니다")]
    ProfileMissing,
    #[error("SFTP 사용자 이름, 세이브 경로와 호스트 키 지문이 필요합니다")]
    IncompleteProfile,
    #[error("Windows 자격 증명 저장소에 SFTP 비밀번호가 없습니다")]
    PasswordMissing,
    #[error("SFTP 서버 주소를 해석할 수 없습니다")]
    AddressResolution,
    #[error("SFTP 서버에 연결하지 못했습니다: {0}")]
    Connect(#[source] io::Error),
    #[error("SSH 연결을 시작하지 못했습니다: {0}")]
    Ssh(#[from] ssh2::Error),
    #[error("SSH 호스트 키가 등록된 지문과 일치하지 않습니다")]
    HostKeyMismatch,
    #[error("SSH 서버가 SHA-256 호스트 키 지문을 제공하지 않았습니다")]
    HostKeyUnavailable,
    #[error("SFTP 비밀번호 인증에 실패했습니다")]
    Authentication,
    #[error("원격 세이브 경로에서 Palworld 월드를 정확히 하나 찾지 못했습니다")]
    WorldDiscovery,
    #[error("원격 세이브 파일 수 또는 디렉터리 탐색 범위를 초과했습니다")]
    DiscoveryLimit,
    #[error("허용되지 않은 원격 파일 이름입니다")]
    UnsafeRemoteName,
    #[error("원격 세이브 파일이 허용 크기를 초과했습니다: {0}")]
    FileTooLarge(String),
    #[error("원격 세이브 전체 크기가 허용 범위를 초과했습니다")]
    SaveSetTooLarge,
    #[error("원격 세이브가 내려받는 동안 변경되었습니다")]
    RemoteChanged,
    #[error("로컬 동기화 경로를 만들 수 없습니다")]
    LocalRootUnavailable,
    #[error("SFTP 동기화 로컬 I/O 실패: {0}")]
    Io(#[from] io::Error),
    #[error("SFTP 상태 파일 처리 실패: {0}")]
    Status(#[from] serde_json::Error),
    #[error("세이브 보호 복사본 생성 실패: {0}")]
    Ingest(#[from] pal_save_ingest::SaveIngestError),
    #[error("Windows 자격 증명 저장소를 읽지 못했습니다")]
    CredentialStore,
}

#[derive(Clone, Debug)]
struct RemoteSaveFile {
    relative_path: String,
    remote_path: PathBuf,
    size: u64,
    mtime: u64,
    limit: u64,
}

pub struct SftpSyncRuntime {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl SftpSyncRuntime {
    pub fn start_selected(interval: Duration) -> Result<Self, SftpSyncError> {
        let root = default_sync_root()?;
        fs::create_dir_all(&root)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("pal-sftp-sync".to_owned())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    let _ = sync_selected_once();
                    let mut remaining = interval.max(Duration::from_secs(5));
                    while !worker_stop.load(Ordering::Acquire) && !remaining.is_zero() {
                        let slice = remaining.min(Duration::from_millis(250));
                        thread::sleep(slice);
                        remaining = remaining.saturating_sub(slice);
                    }
                }
            })?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }
}

impl Drop for SftpSyncRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn default_sync_root() -> Result<PathBuf, SftpSyncError> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|path| path.join("PalBeacon").join("sftp-sync"))
        .ok_or(SftpSyncError::LocalRootUnavailable)
}

pub fn status_path(profile_id: &str) -> Result<PathBuf, SftpSyncError> {
    validate_profile_component(profile_id)?;
    Ok(default_sync_root()?.join(profile_id).join(STATUS_FILE_NAME))
}

pub fn read_status(profile_id: &str) -> Result<Option<SftpSyncStatus>, SftpSyncError> {
    let path = status_path(profile_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path)?;
    let status = serde_json::from_slice(&bytes)?;
    Ok(Some(status))
}

pub fn sync_selected_once() -> Result<SftpSyncStatus, SftpSyncError> {
    let document = read_or_create_server_profiles(&default_server_profiles_path())
        .map_err(|_| SftpSyncError::ProfileMissing)?;
    let Some(profile_id) = document.selected_profile_id else {
        let status = SftpSyncStatus::disabled("선택된 서버가 없어 SFTP 동기화를 기다립니다.");
        write_global_status(&status)?;
        return Ok(status);
    };
    let profile = document
        .profiles
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or(SftpSyncError::ProfileMissing)?;
    sync_profile_once(&profile)
}

pub fn sync_profile_once(profile: &ServerProfile) -> Result<SftpSyncStatus, SftpSyncError> {
    let profile_id = profile.id.clone();
    let checked_at = unix_ms();
    let previous = read_status(&profile_id).ok().flatten();
    let result = sync_profile_inner(profile, previous.as_ref());
    let status = match result {
        Ok((state, revision, staged)) => SftpSyncStatus {
            schema: STATUS_SCHEMA.to_owned(),
            profile_id: Some(profile_id.clone()),
            state,
            checked_at_unix_ms: checked_at,
            last_success_at_unix_ms: Some(checked_at),
            remote_revision: Some(revision),
            import_id: staged
                .as_ref()
                .map(|value| value.import_id.clone())
                .or_else(|| previous.as_ref().and_then(|value| value.import_id.clone())),
            player_file_count: staged
                .as_ref()
                .map(|value| value.player_file_count)
                .or_else(|| previous.as_ref().and_then(|value| value.player_file_count)),
            total_bytes: staged
                .as_ref()
                .map(|value| value.total_bytes)
                .or_else(|| previous.as_ref().and_then(|value| value.total_bytes)),
            message_ko: if state == SftpSyncState::Synced {
                "새 서버 세이브를 읽기 전용 보호 복사본으로 동기화했습니다.".to_owned()
            } else {
                "서버 세이브가 최신 상태입니다.".to_owned()
            },
        },
        Err(error) => {
            let state = match error {
                SftpSyncError::PasswordMissing => SftpSyncState::AwaitingPassword,
                SftpSyncError::IncompleteProfile => SftpSyncState::AwaitingConfiguration,
                _ => SftpSyncState::Error,
            };
            SftpSyncStatus {
                schema: STATUS_SCHEMA.to_owned(),
                profile_id: Some(profile_id.clone()),
                state,
                checked_at_unix_ms: checked_at,
                last_success_at_unix_ms: previous
                    .as_ref()
                    .and_then(|value| value.last_success_at_unix_ms),
                remote_revision: previous
                    .as_ref()
                    .and_then(|value| value.remote_revision.clone()),
                import_id: previous.as_ref().and_then(|value| value.import_id.clone()),
                player_file_count: previous.as_ref().and_then(|value| value.player_file_count),
                total_bytes: previous.as_ref().and_then(|value| value.total_bytes),
                message_ko: error.to_string(),
            }
        }
    };
    write_profile_status(&profile_id, &status)?;
    Ok(status)
}

fn sync_profile_inner(
    profile: &ServerProfile,
    previous: Option<&SftpSyncStatus>,
) -> Result<(SftpSyncState, String, Option<StagedSaveSet>), SftpSyncError> {
    let port = profile.sftp_port.ok_or(SftpSyncError::IncompleteProfile)?;
    let username = profile
        .sftp_username
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or(SftpSyncError::IncompleteProfile)?;
    let save_root = profile
        .save_root
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or(SftpSyncError::IncompleteProfile)?;
    let fingerprint = profile
        .ssh_host_key_fingerprint
        .as_deref()
        .filter(|value| value.starts_with("SHA256:"))
        .ok_or(SftpSyncError::IncompleteProfile)?;
    let mut password = credential_entry(&profile.id)?
        .get_password()
        .map_err(|_| SftpSyncError::PasswordMissing)?;

    let result = (|| {
        let address = resolve(&profile.host, port)?;
        let stream =
            TcpStream::connect_timeout(&address, IO_TIMEOUT).map_err(SftpSyncError::Connect)?;
        stream.set_read_timeout(Some(IO_TIMEOUT)).ok();
        stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
        let mut session = Session::new()?;
        session.set_timeout(IO_TIMEOUT.as_millis() as u32);
        session.set_tcp_stream(stream);
        session.handshake()?;
        verify_host_key(&session, fingerprint)?;
        session
            .userauth_password(username, &password)
            .map_err(|_| SftpSyncError::Authentication)?;
        if !session.authenticated() {
            return Err(SftpSyncError::Authentication);
        }
        let sftp = session.sftp()?;
        let world_root = discover_world_root(&sftp, Path::new(save_root))?;
        let files = list_save_files(&sftp, &world_root)?;
        let revision = remote_revision(&files);
        if previous
            .and_then(|value| value.remote_revision.as_deref())
            .is_some_and(|value| value == revision)
            && previous
                .and_then(|value| value.import_id.as_ref())
                .is_some()
        {
            return Ok((SftpSyncState::UpToDate, revision, None));
        }
        let staged = download_and_stage(&sftp, &profile.id, &files)?;
        Ok((SftpSyncState::Synced, revision, Some(staged)))
    })();
    password.zeroize();
    result
}

fn credential_entry(profile_id: &str) -> Result<Entry, SftpSyncError> {
    Entry::new(
        &format!("{SERVER_CREDENTIAL_SERVICE_PREFIX}/{profile_id}"),
        "sftp",
    )
    .map_err(|_| SftpSyncError::CredentialStore)
}

fn resolve(host: &str, port: u16) -> Result<SocketAddr, SftpSyncError> {
    (host, port)
        .to_socket_addrs()
        .map_err(|_| SftpSyncError::AddressResolution)?
        .next()
        .ok_or(SftpSyncError::AddressResolution)
}

fn verify_host_key(session: &Session, expected: &str) -> Result<(), SftpSyncError> {
    let raw = session
        .host_key_hash(HashType::Sha256)
        .ok_or(SftpSyncError::HostKeyUnavailable)?;
    let actual = format!("SHA256:{}", STANDARD_NO_PAD.encode(raw));
    if normalize_fingerprint(&actual) == normalize_fingerprint(expected) {
        Ok(())
    } else {
        Err(SftpSyncError::HostKeyMismatch)
    }
}

fn normalize_fingerprint(value: &str) -> &str {
    value.trim().trim_end_matches('=')
}

fn discover_world_root(sftp: &Sftp, root: &Path) -> Result<PathBuf, SftpSyncError> {
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut candidates = Vec::new();
    let mut scanned = 0usize;
    while let Some((directory, depth)) = queue.pop_front() {
        scanned += 1;
        if scanned > MAX_REMOTE_DIRECTORIES {
            return Err(SftpSyncError::DiscoveryLimit);
        }
        if sftp.stat(&directory.join("Level.sav")).is_ok() {
            candidates.push(directory);
            continue;
        }
        if depth >= MAX_DISCOVERY_DEPTH {
            continue;
        }
        let entries = sftp.readdir(&directory)?;
        if entries.len() > MAX_REMOTE_ENTRIES {
            return Err(SftpSyncError::DiscoveryLimit);
        }
        for (returned_path, stat) in entries {
            if !stat.file_type().is_dir() {
                continue;
            }
            let name = returned_path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or(SftpSyncError::UnsafeRemoteName)?;
            if name == "." || name == ".." {
                continue;
            }
            validate_remote_name(name)?;
            queue.push_back((directory.join(name), depth + 1));
        }
    }
    if candidates.len() == 1 {
        Ok(candidates.remove(0))
    } else {
        Err(SftpSyncError::WorldDiscovery)
    }
}

fn list_save_files(sftp: &Sftp, world_root: &Path) -> Result<Vec<RemoteSaveFile>, SftpSyncError> {
    let mut files = Vec::new();
    for name in ["Level.sav", "LevelMeta.sav", "WorldOption.sav"] {
        let remote_path = world_root.join(name);
        if let Ok(stat) = sftp.stat(&remote_path) {
            push_remote_file(
                &mut files,
                name.to_owned(),
                remote_path,
                stat,
                MAX_LEVEL_BYTES,
            )?;
        }
    }
    if !files.iter().any(|file| file.relative_path == "Level.sav") {
        return Err(SftpSyncError::WorldDiscovery);
    }

    let players_root = world_root.join("Players");
    if let Ok(entries) = sftp.readdir(&players_root) {
        if entries.len() > MAX_PLAYER_FILES + 2 {
            return Err(SftpSyncError::DiscoveryLimit);
        }
        for (returned_path, stat) in entries {
            if !stat.file_type().is_file() {
                continue;
            }
            let name = returned_path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or(SftpSyncError::UnsafeRemoteName)?;
            if !is_player_save_name(name) {
                continue;
            }
            let relative_path = format!("Players/{name}");
            push_remote_file(
                &mut files,
                relative_path,
                players_root.join(name),
                stat,
                MAX_PLAYER_BYTES,
            )?;
        }
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let total = files.iter().try_fold(0u64, |sum, file| {
        sum.checked_add(file.size)
            .filter(|value| *value <= MAX_TOTAL_BYTES)
            .ok_or(SftpSyncError::SaveSetTooLarge)
    })?;
    if total == 0 {
        return Err(SftpSyncError::WorldDiscovery);
    }
    Ok(files)
}

fn push_remote_file(
    files: &mut Vec<RemoteSaveFile>,
    relative_path: String,
    remote_path: PathBuf,
    stat: FileStat,
    limit: u64,
) -> Result<(), SftpSyncError> {
    if !stat.file_type().is_file() {
        return Err(SftpSyncError::UnsafeRemoteName);
    }
    let size = stat.size.unwrap_or(u64::MAX);
    if size > limit {
        return Err(SftpSyncError::FileTooLarge(relative_path));
    }
    files.push(RemoteSaveFile {
        relative_path,
        remote_path,
        size,
        mtime: stat.mtime.unwrap_or_default(),
        limit,
    });
    Ok(())
}

fn is_player_save_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".sav") else {
        return false;
    };
    let stem = stem.strip_suffix("_dps").unwrap_or(stem);
    (8..=64).contains(&stem.len()) && stem.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_remote_name(name: &str) -> Result<(), SftpSyncError> {
    if name.is_empty()
        || name.len() > 128
        || name.contains(['/', '\\'])
        || name == "."
        || name == ".."
        || name.chars().any(char::is_control)
    {
        Err(SftpSyncError::UnsafeRemoteName)
    } else {
        Ok(())
    }
}

fn validate_profile_component(value: &str) -> Result<(), SftpSyncError> {
    if (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Ok(())
    } else {
        Err(SftpSyncError::LocalRootUnavailable)
    }
}

fn remote_revision(files: &[RemoteSaveFile]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.relative_path.as_bytes());
        hasher.update([0]);
        hasher.update(file.size.to_le_bytes());
        hasher.update(file.mtime.to_le_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn download_and_stage(
    sftp: &Sftp,
    profile_id: &str,
    files: &[RemoteSaveFile],
) -> Result<StagedSaveSet, SftpSyncError> {
    validate_profile_component(profile_id)?;
    let profile_root = default_sync_root()?.join(profile_id);
    fs::create_dir_all(&profile_root)?;
    let temporary_root =
        profile_root.join(format!(".incoming-{}-{}", std::process::id(), unix_ms()));
    fs::create_dir(&temporary_root)?;
    let result = (|| {
        for remote in files {
            let destination = safe_local_join(&temporary_root, &remote.relative_path)?;
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut source = sftp.open(&remote.remote_path)?;
            let mut target = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)?;
            let mut bounded = io::Read::by_ref(&mut source).take(remote.limit + 1);
            let copied = copy_buffered(&mut bounded, &mut target)?;
            target.flush()?;
            target.sync_all()?;
            if copied != remote.size || copied > remote.limit {
                return Err(SftpSyncError::RemoteChanged);
            }
        }
        Ok(stage_save_source(
            &temporary_root,
            &default_staging_root()?,
        )?)
    })();
    let _ = fs::remove_dir_all(&temporary_root);
    result
}

fn safe_local_join(root: &Path, relative: &str) -> Result<PathBuf, SftpSyncError> {
    let normalized = relative.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(SftpSyncError::UnsafeRemoteName);
    }
    Ok(root.join(normalized))
}

fn copy_buffered(source: &mut dyn io::Read, target: &mut File) -> io::Result<u64> {
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut total = 0u64;
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            return Ok(total);
        }
        target.write_all(&buffer[..read])?;
        total = total.saturating_add(read as u64);
    }
}

fn write_profile_status(profile_id: &str, status: &SftpSyncStatus) -> Result<(), SftpSyncError> {
    let path = status_path(profile_id)?;
    write_status_atomic(&path, status)
}

fn write_global_status(status: &SftpSyncStatus) -> Result<(), SftpSyncError> {
    let path = default_sync_root()?.join(STATUS_FILE_NAME);
    write_status_atomic(&path, status)
}

fn write_status_atomic(path: &Path, status: &SftpSyncStatus) -> Result<(), SftpSyncError> {
    let parent = path.parent().ok_or(SftpSyncError::LocalRootUnavailable)?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".status-{}-{}.tmp", std::process::id(), unix_ms()));
    let bytes = serde_json::to_vec_pretty(status)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.flush()?;
    file.sync_all()?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_key_fingerprint_padding_is_ignored() {
        assert_eq!(
            normalize_fingerprint("SHA256:abc="),
            normalize_fingerprint("SHA256:abc")
        );
    }

    #[test]
    fn player_save_allowlist_is_strict() {
        assert!(is_player_save_name("00000000000000000000000000000001.sav"));
        assert!(is_player_save_name(
            "00000000000000000000000000000001_dps.sav"
        ));
        assert!(!is_player_save_name("../Level.sav"));
        assert!(!is_player_save_name("friend.sav"));
        assert!(!is_player_save_name("00000000.exe"));
    }

    #[test]
    fn local_join_never_accepts_parent_or_absolute_paths() {
        let root = Path::new("C:/safe");
        assert_eq!(
            safe_local_join(root, "Players/00000000.sav").unwrap(),
            root.join("Players/00000000.sav")
        );
        assert!(safe_local_join(root, "../Level.sav").is_err());
        assert!(safe_local_join(root, "/Level.sav").is_err());
    }

    #[test]
    fn remote_revision_is_stable_and_metadata_sensitive() {
        let file = RemoteSaveFile {
            relative_path: "Level.sav".to_owned(),
            remote_path: PathBuf::from("remote/Level.sav"),
            size: 12,
            mtime: 34,
            limit: MAX_LEVEL_BYTES,
        };
        let first = remote_revision(std::slice::from_ref(&file));
        assert_eq!(first, remote_revision(std::slice::from_ref(&file)));
        let mut changed = file;
        changed.mtime += 1;
        assert_ne!(first, remote_revision(&[changed]));
    }
}
