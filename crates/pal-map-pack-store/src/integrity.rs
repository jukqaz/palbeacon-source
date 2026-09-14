use std::collections::BTreeSet;
use std::fs::{self, File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::MapPackError;

pub const MAX_MANIFEST_BYTES: u64 = 1_048_576;
pub const MAX_TRANSFORM_BYTES: u64 = 1_048_576;
pub const MAX_POIS_BYTES: u64 = 16_777_216;
pub const MAX_TILE_INDEX_BYTES: u64 = 16_777_216;
pub const MAX_TILE_BYTES: u64 = 16_777_216;

const MAX_RELATIVE_PATH_BYTES: usize = 240;
const MAX_UNINDEXED_ENTRIES_PER_DIRECTORY: usize = 32;

pub fn validate_root(root: &Path) -> Result<PathBuf, MapPackError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| map_missing(error, "root"))?;
    reject_reparse(&metadata, "root")?;
    if !metadata.is_dir() {
        return Err(MapPackError::ReadFailed { component: "root" });
    }
    fs::canonicalize(root).map_err(|_| MapPackError::ReadFailed { component: "root" })
}

pub fn read_bounded(
    root: &Path,
    relative: &str,
    component: &'static str,
    max_bytes: u64,
) -> Result<Vec<u8>, MapPackError> {
    let path = resolve_existing_file(root, relative, component)?;
    let metadata = fs::metadata(&path).map_err(|error| map_missing(error, component))?;
    if metadata.len() > max_bytes {
        return Err(MapPackError::SizeLimit { component });
    }
    let mut file = File::open(path).map_err(|_| MapPackError::ReadFailed { component })?;
    let bytes = read_stream_bounded(&mut file, max_bytes, component)?;
    if bytes.len() as u64 != metadata.len() {
        return Err(MapPackError::ReadFailed { component });
    }
    Ok(bytes)
}

fn read_stream_bounded(
    reader: &mut impl Read,
    max_bytes: u64,
    component: &'static str,
) -> Result<Vec<u8>, MapPackError> {
    let read_limit = max_bytes
        .checked_add(1)
        .ok_or(MapPackError::SizeLimit { component })?;
    let mut limited = reader.take(read_limit);
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|_| MapPackError::ReadFailed { component })?;
    if bytes.len() as u64 > max_bytes {
        return Err(MapPackError::SizeLimit { component });
    }
    Ok(bytes)
}

pub fn read_hashed(
    root: &Path,
    relative: &str,
    component: &'static str,
    max_bytes: u64,
    expected_hash: &[u8; 32],
) -> Result<Vec<u8>, MapPackError> {
    let bytes = read_bounded(root, relative, component, max_bytes)?;
    let actual: [u8; 32] = Sha256::digest(&bytes).into();
    if &actual != expected_hash {
        return Err(MapPackError::HashMismatch { component });
    }
    Ok(bytes)
}

pub fn read_verified_bytes(
    root: &Path,
    relative: &str,
    component: &'static str,
    expected_size: u64,
    expected_hash: &[u8; 32],
) -> Result<Vec<u8>, MapPackError> {
    if expected_size == 0 || expected_size > MAX_TILE_BYTES {
        return Err(MapPackError::SizeLimit { component });
    }
    let path = resolve_existing_file(root, relative, component)?;
    let metadata = fs::metadata(&path).map_err(|error| map_missing(error, component))?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Err(MapPackError::HashMismatch { component });
    }
    let mut file = File::open(path).map_err(|_| MapPackError::ReadFailed { component })?;
    let bytes = read_stream_bounded(&mut file, expected_size, component)?;
    let actual: [u8; 32] = Sha256::digest(&bytes).into();
    if bytes.len() as u64 != expected_size || &actual != expected_hash {
        return Err(MapPackError::HashMismatch { component });
    }
    Ok(bytes)
}

pub fn verify_streaming_file(
    root: &Path,
    relative: &str,
    component: &'static str,
    expected_size: u64,
    expected_hash: &[u8; 32],
) -> Result<(), MapPackError> {
    if expected_size > MAX_TILE_BYTES {
        return Err(MapPackError::SizeLimit { component });
    }
    let path = resolve_existing_file(root, relative, component)?;
    let before = fs::metadata(&path).map_err(|error| map_missing(error, component))?;
    if !before.is_file() || before.len() != expected_size {
        return Err(MapPackError::HashMismatch { component });
    }
    let mut file = File::open(&path).map_err(|_| MapPackError::ReadFailed { component })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| MapPackError::ReadFailed { component })?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or(MapPackError::SizeLimit { component })?;
        if total > MAX_TILE_BYTES {
            return Err(MapPackError::SizeLimit { component });
        }
        hasher.update(&buffer[..read]);
    }
    let after = file
        .seek(SeekFrom::End(0))
        .map_err(|_| MapPackError::ReadFailed { component })?;
    let actual: [u8; 32] = hasher.finalize().into();
    if total != expected_size || after != expected_size || &actual != expected_hash {
        return Err(MapPackError::HashMismatch { component });
    }
    Ok(())
}

pub fn validate_relative_name(value: &str, field: &'static str) -> Result<(), MapPackError> {
    if value.is_empty()
        || value.len() > MAX_RELATIVE_PATH_BYTES
        || !value.is_ascii()
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("//")
        || value.chars().any(|character| {
            matches!(
                character,
                '\\' | ':' | '\0' | '<' | '>' | '"' | '|' | '?' | '*'
            )
        })
        || value.starts_with(' ')
        || value.ends_with(' ')
        || value.ends_with('.')
    {
        return Err(MapPackError::UnsafePath { field });
    }

    for component in value.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.starts_with(' ')
            || component.ends_with(' ')
            || component.ends_with('.')
            || component.len() > 128
            || component.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
            || is_dos_device(component)
        {
            return Err(MapPackError::UnsafePath { field });
        }
    }

    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(MapPackError::UnsafePath { field });
    }
    Ok(())
}

pub fn validate_tile_tree(
    root: &Path,
    tile_root_relative: &str,
    expected_files: &BTreeSet<String>,
    expected_directories: &BTreeSet<String>,
) -> Result<(), MapPackError> {
    let tiles = resolve_existing_directory(root, tile_root_relative, "tiles")?;
    let root_entries = read_directory_bounded(&tiles, expected_directories.len())?;
    let mut directories = BTreeSet::new();
    let mut folded = BTreeSet::new();
    for entry in root_entries {
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| MapPackError::ReadFailed { component: "tiles" })?;
        reject_reparse(&metadata, "tiles")?;
        if !metadata.is_dir() {
            return Err(MapPackError::OrphanTile);
        }
        let canonical = canonical_contained(root, &entry.path(), "tiles")?;
        let name = relative_name(root, &canonical)?;
        validate_relative_name(&name, "tile.directory")?;
        if !expected_directories.contains(&name) {
            return Err(MapPackError::OrphanTile);
        }
        if !directories.insert(name.clone()) || !folded.insert(name.to_ascii_lowercase()) {
            return Err(MapPackError::DuplicateEntry {
                component: "tile_path",
            });
        }
    }
    if &directories != expected_directories {
        return Err(MapPackError::OrphanTile);
    }

    let mut files = BTreeSet::new();
    for directory in expected_directories {
        let prefix = format!("{directory}/");
        let expected_in_directory: BTreeSet<_> = expected_files
            .iter()
            .filter(|path| path.starts_with(&prefix) && !path[prefix.len()..].contains('/'))
            .cloned()
            .collect();
        let path = resolve_existing_directory(root, directory, "tiles")?;
        let entries = read_directory_bounded(&path, expected_in_directory.len())?;
        let mut actual_in_directory = BTreeSet::new();
        for entry in entries {
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| MapPackError::ReadFailed { component: "tiles" })?;
            reject_reparse(&metadata, "tiles")?;
            if !metadata.is_file() {
                return Err(MapPackError::OrphanTile);
            }
            let canonical = canonical_contained(root, &entry.path(), "tiles")?;
            let name = relative_name(root, &canonical)?;
            validate_relative_name(&name, "tile.relative_path")?;
            if !expected_in_directory.contains(&name) {
                return Err(MapPackError::OrphanTile);
            }
            if !actual_in_directory.insert(name.clone())
                || !files.insert(name.clone())
                || !folded.insert(name.to_ascii_lowercase())
            {
                return Err(MapPackError::DuplicateEntry {
                    component: "tile_path",
                });
            }
        }
        if actual_in_directory != expected_in_directory {
            return Err(MapPackError::OrphanTile);
        }
    }
    if &files != expected_files {
        return Err(MapPackError::OrphanTile);
    }
    Ok(())
}

fn read_directory_bounded(
    directory: &Path,
    expected_count: usize,
) -> Result<Vec<fs::DirEntry>, MapPackError> {
    let limit = expected_count
        .checked_add(MAX_UNINDEXED_ENTRIES_PER_DIRECTORY)
        .ok_or(MapPackError::SizeLimit { component: "tiles" })?;
    let mut entries = Vec::with_capacity(expected_count.min(limit));
    for entry in
        fs::read_dir(directory).map_err(|_| MapPackError::ReadFailed { component: "tiles" })?
    {
        if entries.len() >= limit {
            return Err(MapPackError::SizeLimit { component: "tiles" });
        }
        entries.push(entry.map_err(|_| MapPackError::ReadFailed { component: "tiles" })?);
    }
    Ok(entries)
}

fn relative_name(root: &Path, path: &Path) -> Result<String, MapPackError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| MapPackError::PathEscape { component: "tiles" })?;
    Ok(relative
        .iter()
        .map(|component| component.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn resolve_existing_file(
    root: &Path,
    relative: &str,
    component: &'static str,
) -> Result<PathBuf, MapPackError> {
    validate_relative_name(relative, "relative_path")?;
    let path = walk_components(root, relative, component)?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| map_missing(error, component))?;
    reject_reparse(&metadata, component)?;
    if !metadata.is_file() {
        return Err(MapPackError::ReadFailed { component });
    }
    canonical_contained(root, &path, component)
}

pub(crate) fn resolve_existing_directory(
    root: &Path,
    relative: &str,
    component: &'static str,
) -> Result<PathBuf, MapPackError> {
    validate_relative_name(relative, "relative_path")?;
    let path = walk_components(root, relative, component)?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| map_missing(error, component))?;
    reject_reparse(&metadata, component)?;
    if !metadata.is_dir() {
        return Err(MapPackError::ReadFailed { component });
    }
    canonical_contained(root, &path, component)
}

fn walk_components(
    root: &Path,
    relative: &str,
    component: &'static str,
) -> Result<PathBuf, MapPackError> {
    let root_metadata =
        fs::symlink_metadata(root).map_err(|error| map_missing(error, component))?;
    reject_reparse(&root_metadata, component)?;
    if !root_metadata.is_dir() {
        return Err(MapPackError::ReadFailed { component });
    }
    let current_root =
        fs::canonicalize(root).map_err(|_| MapPackError::ReadFailed { component })?;
    if current_root != root {
        return Err(MapPackError::PathEscape { component });
    }

    let mut current = root.to_path_buf();
    for part in relative.split('/') {
        current.push(part);
        let metadata =
            fs::symlink_metadata(&current).map_err(|error| map_missing(error, component))?;
        reject_reparse(&metadata, component)?;
    }
    Ok(current)
}

fn canonical_contained(
    root: &Path,
    path: &Path,
    component: &'static str,
) -> Result<PathBuf, MapPackError> {
    let canonical = fs::canonicalize(path).map_err(|_| MapPackError::ReadFailed { component })?;
    if !canonical.starts_with(root) {
        return Err(MapPackError::PathEscape { component });
    }
    Ok(canonical)
}

fn reject_reparse(metadata: &Metadata, component: &'static str) -> Result<(), MapPackError> {
    if metadata.file_type().is_symlink() || windows_reparse(metadata) {
        return Err(MapPackError::ReparsePoint { component });
    }
    Ok(())
}

#[cfg(windows)]
fn windows_reparse(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
const fn windows_reparse(_metadata: &Metadata) -> bool {
    false
}

fn map_missing(error: std::io::Error, component: &'static str) -> MapPackError {
    if error.kind() == std::io::ErrorKind::NotFound {
        MapPackError::MissingComponent { component }
    } else {
        MapPackError::ReadFailed { component }
    }
}

fn is_dos_device(component: &str) -> bool {
    let stem = component
        .split_once('.')
        .map_or(component, |(stem, _)| stem)
        .trim_end_matches(&[' ', '.'][..])
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
    ) || stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::read_stream_bounded;
    use crate::MapPackError;

    #[test]
    fn bounded_stream_rejects_growth_one_byte_beyond_the_limit() {
        let mut reader = Cursor::new(vec![0_u8; 5]);
        assert!(matches!(
            read_stream_bounded(&mut reader, 4, "manifest"),
            Err(MapPackError::SizeLimit {
                component: "manifest"
            })
        ));
    }
}
