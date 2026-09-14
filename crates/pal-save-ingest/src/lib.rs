//! Read-only discovery and staging for Palworld save sets.
//!
//! This crate deliberately stops before decoding Unreal save data. It gives the
//! parser a stable, private copy so a live server save is never parsed or
//! modified in place.

use std::{
    collections::VecDeque,
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt as _;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
const MAX_DISCOVERY_DEPTH: usize = 4;
const MAX_SCANNED_DIRECTORIES: usize = 4_096;
const MAX_PLAYER_FILES: usize = 1_024;
const MAX_LEVEL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_PLAYER_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 256 * 1024;
const MANIFEST_FILE_NAME: &str = "save-import-v1.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveProbeStatus {
    Ready,
    ReadyWithoutPlayers,
    NoSaveWorld,
    MultipleSaveWorlds,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveFileRole {
    Level,
    LevelMeta,
    WorldOption,
    Player,
    PlayerDps,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SaveSourceProbe {
    pub selected_root: PathBuf,
    pub status: SaveProbeStatus,
    pub candidates: Vec<SaveWorldCandidate>,
    pub warnings: Vec<String>,
}

impl SaveSourceProbe {
    pub fn can_stage(&self) -> bool {
        matches!(
            self.status,
            SaveProbeStatus::Ready | SaveProbeStatus::ReadyWithoutPlayers
        ) && self.candidates.len() == 1
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SaveWorldCandidate {
    pub world_root: PathBuf,
    pub world_folder_name: String,
    pub player_file_count: usize,
    pub total_bytes: u64,
    pub latest_modified_unix_ms: Option<u64>,
    pub has_personal_data: bool,
    pub files: Vec<SaveFileSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SaveFileSummary {
    pub role: SaveFileRole,
    pub relative_path: String,
    pub encoded_length: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StagedSaveSet {
    pub schema_version: u32,
    pub import_id: String,
    pub staged_root: PathBuf,
    pub source_world_folder_name: String,
    pub staged_at_unix_ms: u64,
    pub player_file_count: usize,
    pub total_bytes: u64,
    pub has_personal_data: bool,
    pub files: Vec<StagedSaveFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StagedSaveFile {
    pub role: SaveFileRole,
    pub relative_path: String,
    pub encoded_length: u64,
    pub sha256: String,
}

#[derive(Debug, Error)]
pub enum SaveIngestError {
    #[error("save source path must be absolute")]
    RelativePath,
    #[error("save source does not exist or is not a directory")]
    SourceNotDirectory,
    #[error("save source uses a symbolic link or Windows reparse point")]
    ReparsePoint,
    #[error("save source path is not valid Unicode")]
    NonUnicodePath,
    #[error("save discovery exceeded the bounded directory limit")]
    DiscoveryLimit,
    #[error("save file count exceeds the supported limit")]
    TooManyPlayerFiles,
    #[error("save file is larger than the supported limit: {0}")]
    FileTooLarge(String),
    #[error("save set is larger than the supported limit")]
    SaveSetTooLarge,
    #[error("selected folder does not contain exactly one Palworld world save")]
    CandidateCount,
    #[error("staging root could not be determined")]
    StagingRootUnavailable,
    #[error("staging destination escaped the configured staging root")]
    UnsafeStagingDestination,
    #[error("save import I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("save import manifest failed: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("staged save manifest is invalid")]
    InvalidStagedManifest,
    #[error("staged save content does not match its manifest")]
    StagedContentMismatch,
}

pub fn default_staging_root() -> Result<PathBuf, SaveIngestError> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|path| path.join("PalCompanion").join("save-imports"))
        .ok_or(SaveIngestError::StagingRootUnavailable)
}

pub fn probe_save_source(selected_root: &Path) -> Result<SaveSourceProbe, SaveIngestError> {
    let selected_root = canonical_safe_directory(selected_root)?;
    let mut queue = VecDeque::from([(selected_root.clone(), 0usize)]);
    let mut scanned_directories = 0usize;
    let mut candidates = Vec::new();
    let mut warnings = Vec::new();

    while let Some((directory, depth)) = queue.pop_front() {
        scanned_directories += 1;
        if scanned_directories > MAX_SCANNED_DIRECTORIES {
            return Err(SaveIngestError::DiscoveryLimit);
        }

        if find_case_insensitive_file(&directory, "Level.sav")?.is_some() {
            candidates.push(inspect_world_directory(&directory)?);
            continue;
        }
        if depth >= MAX_DISCOVERY_DEPTH {
            continue;
        }

        let mut children = fs::read_dir(&directory)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let metadata = fs::symlink_metadata(entry.path()).ok()?;
                metadata.is_dir().then_some((entry.path(), metadata))
            })
            .collect::<Vec<_>>();
        children.sort_by(|left, right| left.0.cmp(&right.0));
        for (path, metadata) in children {
            if metadata_is_reparse(&metadata) {
                warnings.push(format!(
                    "링크 또는 재분석 지점을 건너뜀: {}",
                    display_path(&path)?
                ));
                continue;
            }
            queue.push_back((path, depth + 1));
        }
    }

    candidates.sort_by(|left, right| left.world_root.cmp(&right.world_root));
    let status = match candidates.as_slice() {
        [] => SaveProbeStatus::NoSaveWorld,
        [candidate] if candidate.has_personal_data => SaveProbeStatus::Ready,
        [_] => SaveProbeStatus::ReadyWithoutPlayers,
        _ => SaveProbeStatus::MultipleSaveWorlds,
    };
    Ok(SaveSourceProbe {
        selected_root,
        status,
        candidates,
        warnings,
    })
}

pub fn stage_save_source(
    selected_root: &Path,
    staging_root: &Path,
) -> Result<StagedSaveSet, SaveIngestError> {
    let probe = probe_save_source(selected_root)?;
    if !probe.can_stage() {
        return Err(SaveIngestError::CandidateCount);
    }
    let candidate = probe
        .candidates
        .into_iter()
        .next()
        .ok_or(SaveIngestError::CandidateCount)?;
    let staging_root = prepare_staging_root(staging_root)?;
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        now_unix_ms().unwrap_or_default()
    );
    let temporary_root = staging_root.join(format!(".staging-{nonce}"));
    ensure_direct_child(&staging_root, &temporary_root)?;
    fs::create_dir(&temporary_root)?;

    let staged = (|| {
        let mut staged_files = Vec::with_capacity(candidate.files.len());
        for summary in &candidate.files {
            let source = safe_join(&candidate.world_root, &summary.relative_path)?;
            let destination = safe_join(&temporary_root, &summary.relative_path)?;
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let (encoded_length, sha256) = copy_readonly_file(&source, &destination)?;
            if encoded_length != summary.encoded_length {
                return Err(SaveIngestError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "save file changed while it was being staged",
                )));
            }
            staged_files.push(StagedSaveFile {
                role: summary.role,
                relative_path: summary.relative_path.clone(),
                encoded_length,
                sha256,
            });
        }

        let import_id = content_id(&staged_files);
        let final_root = staging_root.join(&import_id);
        ensure_direct_child(&staging_root, &final_root)?;
        let staged_at_unix_ms = now_unix_ms().unwrap_or_default();
        let manifest = StagedSaveSet {
            schema_version: 1,
            import_id,
            staged_root: final_root.clone(),
            source_world_folder_name: candidate.world_folder_name,
            staged_at_unix_ms,
            player_file_count: candidate.player_file_count,
            total_bytes: candidate.total_bytes,
            has_personal_data: candidate.has_personal_data,
            files: staged_files,
        };
        write_manifest(&temporary_root, &manifest)?;

        if final_root.exists() {
            let existing = verify_staged_save(&final_root, &staging_root)?;
            guarded_remove_dir_all(&staging_root, &temporary_root)?;
            return Ok(existing);
        }
        fs::rename(&temporary_root, &final_root)?;
        Ok(manifest)
    })();

    if staged.is_err() && temporary_root.exists() {
        let _ = guarded_remove_dir_all(&staging_root, &temporary_root);
    }
    staged
}

pub fn latest_staged_save(staging_root: &Path) -> Result<Option<StagedSaveSet>, SaveIngestError> {
    if !staging_root.exists() {
        return Ok(None);
    }
    let staging_root = canonical_safe_directory(staging_root)?;
    let mut latest: Option<StagedSaveSet> = None;
    for entry in fs::read_dir(&staging_root)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_dir() || metadata_is_reparse(&metadata) {
            continue;
        }
        let manifest = match verify_staged_save(&entry.path(), &staging_root) {
            Ok(manifest) => manifest,
            Err(_) => continue,
        };
        if latest
            .as_ref()
            .is_none_or(|current| manifest.staged_at_unix_ms > current.staged_at_unix_ms)
        {
            latest = Some(manifest);
        }
    }
    Ok(latest)
}

pub fn verify_staged_save(
    staged_root: &Path,
    staging_root: &Path,
) -> Result<StagedSaveSet, SaveIngestError> {
    let staging_root = canonical_safe_directory(staging_root)?;
    let staged_root = canonical_safe_directory(staged_root)?;
    ensure_direct_child(&staging_root, &staged_root)?;

    let import_id = staged_root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| value.len() == 24 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or(SaveIngestError::InvalidStagedManifest)?;
    let manifest_path = safe_join(&staged_root, MANIFEST_FILE_NAME)?;
    let manifest_metadata = fs::symlink_metadata(&manifest_path)?;
    if !manifest_metadata.is_file()
        || metadata_is_reparse(&manifest_metadata)
        || manifest_metadata.len() > MAX_MANIFEST_BYTES
    {
        return Err(SaveIngestError::InvalidStagedManifest);
    }
    let mut bytes = Vec::with_capacity(manifest_metadata.len() as usize);
    File::open(&manifest_path)?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(SaveIngestError::InvalidStagedManifest);
    }
    let mut manifest: StagedSaveSet = serde_json::from_slice(&bytes)?;
    if manifest.schema_version != 1
        || manifest.import_id != import_id
        || manifest.files.is_empty()
        || manifest.files.len() > MAX_PLAYER_FILES + 3
    {
        return Err(SaveIngestError::InvalidStagedManifest);
    }

    manifest
        .files
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let mut total_bytes = 0u64;
    let mut player_file_count = 0usize;
    for staged_file in &manifest.files {
        if staged_file.sha256.len() != 64
            || !staged_file
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(SaveIngestError::InvalidStagedManifest);
        }
        let path = safe_join(&staged_root, &staged_file.relative_path)?;
        let limit = match staged_file.role {
            SaveFileRole::Player | SaveFileRole::PlayerDps => {
                player_file_count += 1;
                MAX_PLAYER_BYTES
            }
            _ => MAX_LEVEL_BYTES,
        };
        let (encoded_length, sha256) = hash_readonly_file(&path, limit)?;
        if encoded_length != staged_file.encoded_length
            || !sha256.eq_ignore_ascii_case(&staged_file.sha256)
        {
            return Err(SaveIngestError::StagedContentMismatch);
        }
        total_bytes = total_bytes
            .checked_add(encoded_length)
            .filter(|total| *total <= MAX_TOTAL_BYTES)
            .ok_or(SaveIngestError::SaveSetTooLarge)?;
    }
    if content_id(&manifest.files) != manifest.import_id
        || total_bytes != manifest.total_bytes
        || player_file_count != manifest.player_file_count
        || manifest.has_personal_data != (player_file_count > 0)
        || !manifest
            .files
            .iter()
            .any(|file| file.role == SaveFileRole::Level)
    {
        return Err(SaveIngestError::StagedContentMismatch);
    }
    manifest.staged_root = staged_root;
    Ok(manifest)
}

fn inspect_world_directory(world_root: &Path) -> Result<SaveWorldCandidate, SaveIngestError> {
    let world_root = canonical_safe_directory(world_root)?;
    let mut files = Vec::new();
    add_named_file(&world_root, "Level.sav", SaveFileRole::Level, &mut files)?;
    add_named_file(
        &world_root,
        "LevelMeta.sav",
        SaveFileRole::LevelMeta,
        &mut files,
    )?;
    add_named_file(
        &world_root,
        "WorldOption.sav",
        SaveFileRole::WorldOption,
        &mut files,
    )?;

    let players_directory = find_case_insensitive_directory(&world_root, "Players")?;
    if let Some(players_directory) = players_directory {
        let players_directory = canonical_safe_directory(&players_directory)?;
        if !players_directory.starts_with(&world_root) {
            return Err(SaveIngestError::ReparsePoint);
        }
        let mut player_paths = fs::read_dir(&players_directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        player_paths.sort();
        if player_paths.len() > MAX_PLAYER_FILES {
            return Err(SaveIngestError::TooManyPlayerFiles);
        }
        for path in player_paths {
            let Some(role) = player_save_role(&path) else {
                continue;
            };
            let relative = path
                .strip_prefix(&world_root)
                .map_err(|_| SaveIngestError::ReparsePoint)?;
            add_file(&world_root, relative, role, &mut files)?;
        }
    }

    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let player_file_count = files
        .iter()
        .filter(|file| matches!(file.role, SaveFileRole::Player | SaveFileRole::PlayerDps))
        .count();
    let total_bytes = files.iter().try_fold(0u64, |total, file| {
        total
            .checked_add(file.encoded_length)
            .filter(|total| *total <= MAX_TOTAL_BYTES)
            .ok_or(SaveIngestError::SaveSetTooLarge)
    })?;
    let latest_modified_unix_ms = files
        .iter()
        .filter_map(|file| {
            safe_join(&world_root, &file.relative_path)
                .ok()?
                .metadata()
                .ok()?
                .modified()
                .ok()
                .and_then(system_time_to_unix_ms)
        })
        .max();
    let world_folder_name = world_root
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(SaveIngestError::NonUnicodePath)?
        .to_owned();
    Ok(SaveWorldCandidate {
        world_root,
        world_folder_name,
        player_file_count,
        total_bytes,
        latest_modified_unix_ms,
        has_personal_data: player_file_count > 0,
        files,
    })
}

fn add_named_file(
    root: &Path,
    name: &str,
    role: SaveFileRole,
    files: &mut Vec<SaveFileSummary>,
) -> Result<(), SaveIngestError> {
    if let Some(path) = find_case_insensitive_file(root, name)? {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| SaveIngestError::ReparsePoint)?;
        add_file(root, relative, role, files)?;
    }
    Ok(())
}

fn add_file(
    root: &Path,
    relative: &Path,
    role: SaveFileRole,
    files: &mut Vec<SaveFileSummary>,
) -> Result<(), SaveIngestError> {
    let path = safe_join(root, relative)?;
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata_is_reparse(&metadata) {
        return Err(SaveIngestError::ReparsePoint);
    }
    let limit = match role {
        SaveFileRole::Player | SaveFileRole::PlayerDps => MAX_PLAYER_BYTES,
        _ => MAX_LEVEL_BYTES,
    };
    if metadata.len() > limit {
        return Err(SaveIngestError::FileTooLarge(display_path(&path)?));
    }
    files.push(SaveFileSummary {
        role,
        relative_path: relative_path_string(relative)?,
        encoded_length: metadata.len(),
    });
    Ok(())
}

fn find_case_insensitive_file(
    directory: &Path,
    name: &str,
) -> Result<Option<PathBuf>, SaveIngestError> {
    find_case_insensitive(directory, name, |metadata| metadata.is_file())
}

fn find_case_insensitive_directory(
    directory: &Path,
    name: &str,
) -> Result<Option<PathBuf>, SaveIngestError> {
    find_case_insensitive(directory, name, |metadata| metadata.is_dir())
}

fn find_case_insensitive(
    directory: &Path,
    name: &str,
    predicate: impl Fn(&fs::Metadata) -> bool,
) -> Result<Option<PathBuf>, SaveIngestError> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if file_name.eq_ignore_ascii_case(name) {
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata_is_reparse(&metadata) {
                return Err(SaveIngestError::ReparsePoint);
            }
            if predicate(&metadata) {
                return Ok(Some(entry.path()));
            }
        }
    }
    Ok(None)
}

fn player_save_role(path: &Path) -> Option<SaveFileRole> {
    let file_name = path.file_name()?.to_str()?;
    let lower = file_name.to_ascii_lowercase();
    let (stem, role) = if let Some(stem) = lower.strip_suffix("_dps.sav") {
        (stem, SaveFileRole::PlayerDps)
    } else {
        (lower.strip_suffix(".sav")?, SaveFileRole::Player)
    };
    (stem.len() == 32 && stem.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(role)
}

fn canonical_safe_directory(path: &Path) -> Result<PathBuf, SaveIngestError> {
    if !path.is_absolute() {
        return Err(SaveIngestError::RelativePath);
    }
    reject_existing_reparse_ancestors(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| SaveIngestError::SourceNotDirectory)?;
    if !metadata.is_dir() {
        return Err(SaveIngestError::SourceNotDirectory);
    }
    if metadata_is_reparse(&metadata) {
        return Err(SaveIngestError::ReparsePoint);
    }
    let canonical = fs::canonicalize(path)?;
    reject_existing_reparse_ancestors(&canonical)?;
    if canonical.to_str().is_none() {
        return Err(SaveIngestError::NonUnicodePath);
    }
    Ok(canonical)
}

fn prepare_staging_root(path: &Path) -> Result<PathBuf, SaveIngestError> {
    if !path.is_absolute() {
        return Err(SaveIngestError::RelativePath);
    }
    reject_existing_reparse_ancestors(path)?;
    fs::create_dir_all(path)?;
    canonical_safe_directory(path)
}

fn reject_existing_reparse_ancestors(path: &Path) -> Result<(), SaveIngestError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if !current.is_absolute() || !current.exists() {
            continue;
        }
        let metadata = fs::symlink_metadata(&current)?;
        if metadata_is_reparse(&metadata) {
            return Err(SaveIngestError::ReparsePoint);
        }
    }
    Ok(())
}

fn safe_join(root: &Path, relative: impl AsRef<Path>) -> Result<PathBuf, SaveIngestError> {
    let relative = relative.as_ref();
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(SaveIngestError::ReparsePoint);
    }
    let path = root.join(relative);
    if !path.starts_with(root) {
        return Err(SaveIngestError::ReparsePoint);
    }
    Ok(path)
}

fn copy_readonly_file(source: &Path, destination: &Path) -> Result<(u64, String), SaveIngestError> {
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.is_file() || metadata_is_reparse(&metadata) {
        return Err(SaveIngestError::ReparsePoint);
    }
    let mut input = File::open(source)?;
    let opened_metadata = input.metadata()?;
    if !opened_metadata.is_file()
        || metadata_is_reparse(&opened_metadata)
        || opened_metadata.len() != metadata.len()
    {
        return Err(SaveIngestError::ReparsePoint);
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    // The management UI invokes this function on a worker isolate whose native thread has
    // a comparatively small stack. Keep the streaming buffer on the heap so
    // large saves never consume the entire native thread stack at function
    // entry.
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        total = total
            .checked_add(read as u64)
            .ok_or(SaveIngestError::SaveSetTooLarge)?;
        if total > metadata.len() || total > MAX_TOTAL_BYTES {
            return Err(SaveIngestError::SaveSetTooLarge);
        }
    }
    output.sync_all()?;
    Ok((total, hex_lower(&hasher.finalize())))
}

fn hash_readonly_file(path: &Path, limit: u64) -> Result<(u64, String), SaveIngestError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata_is_reparse(&metadata) || metadata.len() > limit {
        return Err(SaveIngestError::StagedContentMismatch);
    }
    let mut input = File::open(path)?;
    let opened_metadata = input.metadata()?;
    if !opened_metadata.is_file()
        || metadata_is_reparse(&opened_metadata)
        || opened_metadata.len() != metadata.len()
    {
        return Err(SaveIngestError::StagedContentMismatch);
    }
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total = total
            .checked_add(read as u64)
            .filter(|total| *total <= metadata.len() && *total <= limit)
            .ok_or(SaveIngestError::StagedContentMismatch)?;
    }
    if total != metadata.len() {
        return Err(SaveIngestError::StagedContentMismatch);
    }
    Ok((total, hex_lower(&hasher.finalize())))
}

fn content_id(files: &[StagedSaveFile]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.relative_path.as_bytes());
        hasher.update([0]);
        hasher.update(file.sha256.as_bytes());
        hasher.update(file.encoded_length.to_le_bytes());
    }
    hex_lower(&hasher.finalize())[..24].to_owned()
}

fn write_manifest(root: &Path, manifest: &StagedSaveSet) -> Result<(), SaveIngestError> {
    let bytes = serde_json::to_vec_pretty(manifest)?;
    let path = root.join(MANIFEST_FILE_NAME);
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn ensure_direct_child(root: &Path, child: &Path) -> Result<(), SaveIngestError> {
    if child.parent() == Some(root) && child.starts_with(root) {
        Ok(())
    } else {
        Err(SaveIngestError::UnsafeStagingDestination)
    }
}

fn guarded_remove_dir_all(root: &Path, target: &Path) -> Result<(), SaveIngestError> {
    ensure_direct_child(root, target)?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(SaveIngestError::UnsafeStagingDestination)?;
    if !name.starts_with(".staging-") {
        return Err(SaveIngestError::UnsafeStagingDestination);
    }
    fs::remove_dir_all(target)?;
    Ok(())
}

fn relative_path_string(path: &Path) -> Result<String, SaveIngestError> {
    path.to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or(SaveIngestError::NonUnicodePath)
}

fn display_path(path: &Path) -> Result<String, SaveIngestError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(SaveIngestError::NonUnicodePath)
}

fn metadata_is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        let _ = FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_type().is_symlink()
    }
}

fn now_unix_ms() -> Option<u64> {
    system_time_to_unix_ms(SystemTime::now())
}

fn system_time_to_unix_ms(value: SystemTime) -> Option<u64> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn finds_nested_world_and_counts_only_valid_player_files() {
        let temp = tempdir().unwrap();
        let world = temp.path().join("backup").join("0").join("world-guid");
        fs::create_dir_all(world.join("Players")).unwrap();
        fs::write(world.join("Level.sav"), b"level").unwrap();
        fs::write(
            world
                .join("Players")
                .join("0123456789abcdef0123456789abcdef.sav"),
            b"player",
        )
        .unwrap();
        fs::write(world.join("Players").join("notes.txt"), b"ignored").unwrap();

        let probe = probe_save_source(temp.path()).unwrap();

        assert_eq!(probe.status, SaveProbeStatus::Ready);
        assert!(probe.can_stage());
        assert_eq!(probe.candidates[0].player_file_count, 1);
        assert_eq!(probe.candidates[0].files.len(), 2);
    }

    #[test]
    fn reports_multiple_worlds_without_guessing() {
        let temp = tempdir().unwrap();
        for name in ["world-a", "world-b"] {
            let world = temp.path().join(name);
            fs::create_dir_all(&world).unwrap();
            fs::write(world.join("Level.sav"), b"level").unwrap();
        }

        let probe = probe_save_source(temp.path()).unwrap();

        assert_eq!(probe.status, SaveProbeStatus::MultipleSaveWorlds);
        assert!(!probe.can_stage());
        assert_eq!(probe.candidates.len(), 2);
    }

    #[test]
    fn stages_a_private_content_addressed_copy() {
        let source = tempdir().unwrap();
        let staging = tempdir().unwrap();
        fs::create_dir(source.path().join("Players")).unwrap();
        fs::write(source.path().join("Level.sav"), b"level").unwrap();
        fs::write(
            source
                .path()
                .join("Players")
                .join("0123456789abcdef0123456789abcdef.sav"),
            b"player",
        )
        .unwrap();

        let staged = stage_save_source(source.path(), staging.path()).unwrap();

        assert_eq!(staged.schema_version, 1);
        assert!(staged.has_personal_data);
        assert_eq!(staged.player_file_count, 1);
        assert!(staged.staged_root.join("Level.sav").is_file());
        assert!(
            staged
                .staged_root
                .join("Players/0123456789abcdef0123456789abcdef.sav")
                .is_file()
        );
        assert!(staged.staged_root.join(MANIFEST_FILE_NAME).is_file());
        assert!(staged.files.iter().all(|file| file.sha256.len() == 64));

        let verified = verify_staged_save(&staged.staged_root, staging.path()).unwrap();
        assert_eq!(verified.import_id, staged.import_id);
        assert_eq!(verified.files, staged.files);
    }

    #[test]
    fn rejects_a_staged_file_that_changed_after_import() {
        let source = tempdir().unwrap();
        let staging = tempdir().unwrap();
        fs::write(source.path().join("Level.sav"), b"level").unwrap();
        let staged = stage_save_source(source.path(), staging.path()).unwrap();
        fs::write(staged.staged_root.join("Level.sav"), b"tampered").unwrap();

        assert!(matches!(
            verify_staged_save(&staged.staged_root, staging.path()),
            Err(SaveIngestError::StagedContentMismatch)
        ));
    }

    #[test]
    fn a_repeated_import_reuses_only_a_verified_existing_copy() {
        let source = tempdir().unwrap();
        let staging = tempdir().unwrap();
        fs::write(source.path().join("Level.sav"), b"level").unwrap();
        let first = stage_save_source(source.path(), staging.path()).unwrap();
        let second = stage_save_source(source.path(), staging.path()).unwrap();

        assert_eq!(second.import_id, first.import_id);
        assert_eq!(second.staged_at_unix_ms, first.staged_at_unix_ms);

        fs::write(first.staged_root.join("Level.sav"), b"tampered").unwrap();
        assert!(matches!(
            stage_save_source(source.path(), staging.path()),
            Err(SaveIngestError::StagedContentMismatch)
        ));
    }

    #[test]
    fn stages_on_a_bounded_worker_stack() {
        let source = tempdir().unwrap();
        let staging = tempdir().unwrap();
        fs::create_dir(source.path().join("Players")).unwrap();
        fs::write(source.path().join("Level.sav"), b"level").unwrap();
        fs::write(
            source
                .path()
                .join("Players")
                .join("0123456789abcdef0123456789abcdef.sav"),
            b"player",
        )
        .unwrap();
        let source_path = source.path().to_owned();
        let staging_path = staging.path().to_owned();

        let staged = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(move || stage_save_source(&source_path, &staging_path))
            .unwrap()
            .join()
            .unwrap()
            .unwrap();

        assert_eq!(staged.player_file_count, 1);
        assert!(staged.staged_root.join(MANIFEST_FILE_NAME).is_file());
    }

    #[test]
    fn rejects_relative_sources() {
        assert!(matches!(
            probe_save_source(Path::new("relative")),
            Err(SaveIngestError::RelativePath)
        ));
    }

    #[test]
    fn player_filename_is_bounded_and_explicit() {
        assert_eq!(
            player_save_role(Path::new("0123456789abcdef0123456789abcdef_dps.sav")),
            Some(SaveFileRole::PlayerDps)
        );
        assert_eq!(player_save_role(Path::new("player.sav")), None);
        assert_eq!(
            player_save_role(Path::new("0123456789abcdef0123456789abcdef.sav.exe")),
            None
        );
    }

    #[test]
    fn latest_staged_save_survives_process_restart() {
        let source = tempdir().unwrap();
        let staging = tempdir().unwrap();
        fs::create_dir(source.path().join("Players")).unwrap();
        fs::write(source.path().join("Level.sav"), b"level").unwrap();
        fs::write(
            source
                .path()
                .join("Players")
                .join("0123456789abcdef0123456789abcdef.sav"),
            b"player",
        )
        .unwrap();
        let staged = stage_save_source(source.path(), staging.path()).unwrap();

        let restored = latest_staged_save(staging.path()).unwrap().unwrap();

        assert_eq!(restored.import_id, staged.import_id);
        assert_eq!(restored.player_file_count, 1);
        assert_eq!(restored.staged_root, staged.staged_root);
    }
}
