use std::{
    fmt,
    path::{Path, PathBuf},
};

use pal_protected_file::read_regular_file_no_follow;
use pal_windows::{PALWORLD_IMAGE_NAME, TrackedWindow};
use thiserror::Error;

const PALWORLD_STEAM_APP_ID: &str = "1623730";
const PALWORLD_APP_MANIFEST: &str = "appmanifest_1623730.acf";
const MAX_APP_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_VDF_TOKENS: usize = 131_072;
const MAX_VDF_STRING_BYTES: usize = 64 * 1024;
const FULLY_INSTALLED_STATE_FLAGS: u64 = 4;

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SteamClientBuildError {
    #[error("the tracked game executable is not a supported Steam Palworld installation")]
    UnsupportedInstallation,
    #[error("the Steam Palworld app manifest is unavailable")]
    ManifestUnavailable,
    #[error("the Steam Palworld app manifest is malformed")]
    MalformedManifest,
    #[error("the Steam Palworld app manifest identifies a different application")]
    ApplicationMismatch,
    #[error("the Steam Palworld install directory does not match the running executable")]
    InstallDirectoryMismatch,
    #[error("the Steam Palworld installation is not in a fully installed state")]
    InstallationStateMismatch,
    #[error("the running Palworld client does not match the required data build")]
    BuildMismatch,
}

#[derive(Clone, Eq, PartialEq)]
pub struct VerifiedSteamClientBuild {
    window_id: u64,
    process_id: u32,
    executable_path: PathBuf,
    build_id: u64,
}

impl VerifiedSteamClientBuild {
    pub const fn process_id(&self) -> u32 {
        self.process_id
    }

    pub const fn build_id(&self) -> u64 {
        self.build_id
    }

    pub fn matches(&self, window: &TrackedWindow) -> bool {
        self.window_id == window.id().as_raw()
            && self.process_id == window.snapshot().process_id()
            && paths_equal_case_insensitive(&self.executable_path, Path::new(window.image_path()))
    }
}

impl fmt::Debug for VerifiedSteamClientBuild {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedSteamClientBuild")
            .field("window_id", &self.window_id)
            .field("process_id", &self.process_id)
            .field("executable_path", &"<redacted>")
            .field("build_id", &self.build_id)
            .finish()
    }
}

pub fn verify_tracked_steam_client(
    window: &TrackedWindow,
    expected_build_id: u64,
) -> Result<VerifiedSteamClientBuild, SteamClientBuildError> {
    verify_steam_client_path(
        window.id().as_raw(),
        window.snapshot().process_id(),
        Path::new(window.image_path()),
        expected_build_id,
    )
}

fn verify_steam_client_path(
    window_id: u64,
    process_id: u32,
    executable_path: &Path,
    expected_build_id: u64,
) -> Result<VerifiedSteamClientBuild, SteamClientBuildError> {
    if window_id == 0 || process_id == 0 || expected_build_id == 0 || !executable_path.is_absolute()
    {
        return Err(SteamClientBuildError::UnsupportedInstallation);
    }
    let layout = SteamInstallLayout::from_executable(executable_path)?;
    let bytes = read_regular_file_no_follow(&layout.manifest_path, MAX_APP_MANIFEST_BYTES)
        .map_err(|_| SteamClientBuildError::ManifestUnavailable)?;
    let manifest = parse_app_manifest(&bytes)?;
    if manifest.app_id != PALWORLD_STEAM_APP_ID {
        return Err(SteamClientBuildError::ApplicationMismatch);
    }
    if !manifest
        .install_dir
        .eq_ignore_ascii_case(&layout.install_dir)
    {
        return Err(SteamClientBuildError::InstallDirectoryMismatch);
    }
    if manifest.build_id != expected_build_id {
        return Err(SteamClientBuildError::BuildMismatch);
    }
    Ok(VerifiedSteamClientBuild {
        window_id,
        process_id,
        executable_path: executable_path.to_owned(),
        build_id: manifest.build_id,
    })
}

struct SteamInstallLayout {
    manifest_path: PathBuf,
    install_dir: String,
}

impl SteamInstallLayout {
    fn from_executable(path: &Path) -> Result<Self, SteamClientBuildError> {
        require_file_name(path, PALWORLD_IMAGE_NAME)?;
        let win64 = require_parent(path, "Win64")?;
        let binaries = require_parent(win64, "Binaries")?;
        let pal = require_parent(binaries, "Pal")?;
        let install_root = pal
            .parent()
            .ok_or(SteamClientBuildError::UnsupportedInstallation)?;
        let install_dir = install_root
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| is_valid_install_dir(name))
            .ok_or(SteamClientBuildError::UnsupportedInstallation)?
            .to_owned();
        let common = require_parent(install_root, "common")?;
        let steamapps = require_parent(common, "steamapps")?;
        Ok(Self {
            manifest_path: steamapps.join(PALWORLD_APP_MANIFEST),
            install_dir,
        })
    }
}

fn require_file_name(path: &Path, expected: &str) -> Result<(), SteamClientBuildError> {
    let actual = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(SteamClientBuildError::UnsupportedInstallation)?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(SteamClientBuildError::UnsupportedInstallation)
    }
}

fn require_parent<'a>(
    path: &'a Path,
    expected_name: &str,
) -> Result<&'a Path, SteamClientBuildError> {
    let parent = path
        .parent()
        .ok_or(SteamClientBuildError::UnsupportedInstallation)?;
    let actual = parent
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(SteamClientBuildError::UnsupportedInstallation)?;
    if actual.eq_ignore_ascii_case(expected_name) {
        Ok(parent)
    } else {
        Err(SteamClientBuildError::UnsupportedInstallation)
    }
}

fn is_valid_install_dir(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\', ':'])
        && !value.ends_with(['.', ' '])
        && !value.chars().any(char::is_control)
}

fn paths_equal_case_insensitive(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[derive(Debug)]
struct AppManifest {
    app_id: String,
    build_id: u64,
    install_dir: String,
}

#[derive(Debug, Eq, PartialEq)]
enum Token {
    String(String),
    Open,
    Close,
}

fn parse_app_manifest(bytes: &[u8]) -> Result<AppManifest, SteamClientBuildError> {
    let text = std::str::from_utf8(bytes).map_err(|_| SteamClientBuildError::MalformedManifest)?;
    let tokens = tokenize(text)?;
    let [Token::String(root), Token::Open, rest @ ..] = tokens.as_slice() else {
        return Err(SteamClientBuildError::MalformedManifest);
    };
    if !root.eq_ignore_ascii_case("AppState") {
        return Err(SteamClientBuildError::MalformedManifest);
    }
    let mut fields = RequiredFields::default();
    let consumed = parse_object(rest, 1, &mut fields)?;
    if consumed != rest.len() {
        return Err(SteamClientBuildError::MalformedManifest);
    }
    fields.finish()
}

#[derive(Default)]
struct RequiredFields {
    app_id: Option<String>,
    build_id: Option<String>,
    target_build_id: Option<String>,
    state_flags: Option<String>,
    install_dir: Option<String>,
}

impl RequiredFields {
    fn insert(&mut self, key: &str, value: &str) -> Result<(), SteamClientBuildError> {
        let target = if key.eq_ignore_ascii_case("appid") {
            Some(&mut self.app_id)
        } else if key.eq_ignore_ascii_case("buildid") {
            Some(&mut self.build_id)
        } else if key.eq_ignore_ascii_case("TargetBuildID") {
            Some(&mut self.target_build_id)
        } else if key.eq_ignore_ascii_case("StateFlags") {
            Some(&mut self.state_flags)
        } else if key.eq_ignore_ascii_case("installdir") {
            Some(&mut self.install_dir)
        } else {
            None
        };
        if let Some(target) = target
            && target.replace(value.to_owned()).is_some()
        {
            return Err(SteamClientBuildError::MalformedManifest);
        }
        Ok(())
    }

    fn finish(self) -> Result<AppManifest, SteamClientBuildError> {
        let app_id = self
            .app_id
            .filter(|value| is_ascii_decimal(value))
            .ok_or(SteamClientBuildError::MalformedManifest)?;
        let build_id = self
            .build_id
            .filter(|value| is_ascii_decimal(value))
            .ok_or(SteamClientBuildError::MalformedManifest)?
            .parse::<u64>()
            .map_err(|_| SteamClientBuildError::MalformedManifest)?;
        let target_build_id = self
            .target_build_id
            .filter(|value| is_ascii_decimal(value))
            .ok_or(SteamClientBuildError::MalformedManifest)?
            .parse::<u64>()
            .map_err(|_| SteamClientBuildError::MalformedManifest)?;
        let state_flags = self
            .state_flags
            .filter(|value| is_ascii_decimal(value))
            .ok_or(SteamClientBuildError::MalformedManifest)?
            .parse::<u64>()
            .map_err(|_| SteamClientBuildError::MalformedManifest)?;
        let install_dir = self
            .install_dir
            .filter(|value| is_valid_install_dir(value))
            .ok_or(SteamClientBuildError::MalformedManifest)?;
        if build_id == 0 || target_build_id != build_id {
            return Err(SteamClientBuildError::MalformedManifest);
        }
        if state_flags != FULLY_INSTALLED_STATE_FLAGS {
            return Err(SteamClientBuildError::InstallationStateMismatch);
        }
        Ok(AppManifest {
            app_id,
            build_id,
            install_dir,
        })
    }
}

fn is_ascii_decimal(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn parse_object(
    tokens: &[Token],
    depth: usize,
    fields: &mut RequiredFields,
) -> Result<usize, SteamClientBuildError> {
    if depth > 32 {
        return Err(SteamClientBuildError::MalformedManifest);
    }
    let mut index = 0;
    loop {
        match tokens.get(index) {
            Some(Token::Close) => return Ok(index + 1),
            Some(Token::String(key)) => match tokens.get(index + 1) {
                Some(Token::String(value)) => {
                    if depth == 1 {
                        fields.insert(key, value)?;
                    }
                    index += 2;
                }
                Some(Token::Open) => {
                    if depth == 1 && is_required_field(key) {
                        return Err(SteamClientBuildError::MalformedManifest);
                    }
                    let consumed = parse_object(&tokens[index + 2..], depth + 1, fields)?;
                    index += 2 + consumed;
                }
                _ => return Err(SteamClientBuildError::MalformedManifest),
            },
            _ => return Err(SteamClientBuildError::MalformedManifest),
        }
    }
}

fn is_required_field(key: &str) -> bool {
    [
        "appid",
        "buildid",
        "TargetBuildID",
        "StateFlags",
        "installdir",
    ]
    .iter()
    .any(|required| key.eq_ignore_ascii_case(required))
}

fn tokenize(text: &str) -> Result<Vec<Token>, SteamClientBuildError> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            character if character.is_whitespace() => {}
            '{' => push_token(&mut tokens, Token::Open)?,
            '}' => push_token(&mut tokens, Token::Close)?,
            '"' => {
                let mut value = String::new();
                let mut terminated = false;
                while let Some(character) = chars.next() {
                    match character {
                        '"' => {
                            terminated = true;
                            break;
                        }
                        '\\' => match chars.next() {
                            Some('\\') => value.push('\\'),
                            Some('"') => value.push('"'),
                            _ => return Err(SteamClientBuildError::MalformedManifest),
                        },
                        character if character.is_control() => {
                            return Err(SteamClientBuildError::MalformedManifest);
                        }
                        character => value.push(character),
                    }
                    if value.len() > MAX_VDF_STRING_BYTES {
                        return Err(SteamClientBuildError::MalformedManifest);
                    }
                }
                if !terminated {
                    return Err(SteamClientBuildError::MalformedManifest);
                }
                push_token(&mut tokens, Token::String(value))?;
            }
            _ => return Err(SteamClientBuildError::MalformedManifest),
        }
    }
    Ok(tokens)
}

fn push_token(tokens: &mut Vec<Token>, token: Token) -> Result<(), SteamClientBuildError> {
    if tokens.len() >= MAX_VDF_TOKENS {
        return Err(SteamClientBuildError::MalformedManifest);
    }
    tokens.push(token);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use tempfile::TempDir;

    use super::{SteamClientBuildError, parse_app_manifest, verify_steam_client_path};
    use pal_windows::{
        FakeWindowBackend, GameWindowTracker, MonitorId, PhysicalClientRect, WindowEvent, WindowId,
        WindowObservation, WindowObservationParts,
    };

    const BUILD_ID: u64 = 24_181_527;

    fn fixture(manifest: &str) -> (TempDir, PathBuf) {
        // The production reader deliberately refuses parent-directory reparse points. Keeping
        // this fixture in the system temp directory avoids coupling the test to whether a caller
        // checked the repository out through a junction or managed workspace.
        let temp = TempDir::new().unwrap();
        let steamapps = temp.path().join("steamapps");
        let executable = steamapps
            .join("common")
            .join("Palworld")
            .join("Pal")
            .join("Binaries")
            .join("Win64")
            .join("Palworld-Win64-Shipping.exe");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"fixture executable").unwrap();
        fs::write(steamapps.join("appmanifest_1623730.acf"), manifest).unwrap();
        (temp, executable)
    }

    fn valid_manifest() -> String {
        format!(
            "\"AppState\"\n{{\n  \"appid\" \"1623730\"\n  \"StateFlags\" \"4\"\n  \"installdir\" \"Palworld\"\n  \"buildid\" \"{BUILD_ID}\"\n  \"TargetBuildID\" \"{BUILD_ID}\"\n  \"InstalledDepots\" {{ \"1623731\" {{ \"manifest\" \"1\" }} }}\n}}"
        )
    }

    fn tracked_window(path: &Path, window_id: u64, process_id: u32) -> pal_windows::TrackedWindow {
        let observation = WindowObservation::for_test(WindowObservationParts {
            id: WindowId::from_raw(window_id),
            monitor_id: MonitorId::from_raw(1),
            process_id,
            image_path: path.to_string_lossy().into_owned(),
            window_title: "Palworld".to_owned(),
            client_rect: PhysicalClientRect::new(0, 0, 1_920, 1_080),
            dpi: 96,
            visible: true,
            top_level: true,
            foreground: true,
            minimized: false,
            inspectable: true,
        });
        let mut tracker = GameWindowTracker::new(FakeWindowBackend::with_windows([observation]));
        match tracker.poll().unwrap() {
            Some(WindowEvent::Attached(window)) => window,
            other => panic!("expected attached window, got {other:?}"),
        }
    }

    #[test]
    fn exact_steam_layout_and_build_are_accepted() {
        let (_temp, executable) = fixture(&valid_manifest());

        let verified = verify_steam_client_path(7, 44, &executable, BUILD_ID).unwrap();

        assert_eq!(verified.process_id(), 44);
        assert_eq!(verified.build_id(), BUILD_ID);
        let debug = format!("{verified:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains(temp_path_fragment(&executable)));
    }

    #[test]
    fn a_verified_binding_is_scoped_to_the_same_window_process_and_image_path() {
        let (_temp, executable) = fixture(&valid_manifest());
        let original = tracked_window(&executable, 7, 44);
        let verified = super::verify_tracked_steam_client(&original, BUILD_ID).unwrap();

        assert!(verified.matches(&original));
        assert!(!verified.matches(&tracked_window(&executable, 8, 44)));
        assert!(!verified.matches(&tracked_window(&executable, 7, 45)));
        let other_path = executable.with_file_name("PALWORLD-WIN64-SHIPPING.EXE");
        assert!(verified.matches(&tracked_window(&other_path, 7, 44)));
        let moved_path = executable
            .parent()
            .unwrap()
            .join("elsewhere")
            .join("Palworld-Win64-Shipping.exe");
        assert!(!verified.matches(&tracked_window(&moved_path, 7, 44)));
    }

    #[test]
    fn application_build_and_install_directory_mismatches_are_distinct() {
        for (needle, expected) in [
            (
                "1623730".to_owned(),
                SteamClientBuildError::ApplicationMismatch,
            ),
            (BUILD_ID.to_string(), SteamClientBuildError::BuildMismatch),
            (
                "Palworld".to_owned(),
                SteamClientBuildError::InstallDirectoryMismatch,
            ),
        ] {
            let replacement = match expected {
                SteamClientBuildError::ApplicationMismatch => "999",
                SteamClientBuildError::BuildMismatch => "1",
                SteamClientBuildError::InstallDirectoryMismatch => "OtherInstall",
                _ => unreachable!(),
            };
            let manifest = if expected == SteamClientBuildError::BuildMismatch {
                valid_manifest().replace(&needle, replacement)
            } else {
                valid_manifest().replacen(&needle, replacement, 1)
            };
            let (_temp, executable) = fixture(&manifest);
            assert_eq!(
                verify_steam_client_path(7, 44, &executable, BUILD_ID).unwrap_err(),
                expected
            );
        }
    }

    #[test]
    fn duplicate_required_fields_and_malformed_vdf_are_rejected() {
        let duplicate =
            valid_manifest().replace("\"buildid\"", "\"buildid\" \"24181527\"\n  \"buildid\"");
        assert_eq!(
            parse_app_manifest(duplicate.as_bytes()).unwrap_err(),
            SteamClientBuildError::MalformedManifest
        );
        assert_eq!(
            parse_app_manifest(b"\"AppState\" { \"appid\" \"1623730\"").unwrap_err(),
            SteamClientBuildError::MalformedManifest
        );
        assert_eq!(
            parse_app_manifest(b"\"AppState\" { bare \"value\" }").unwrap_err(),
            SteamClientBuildError::MalformedManifest
        );
        let object_then_scalar = valid_manifest().replacen(
            "\"buildid\"",
            "\"buildid\" { \"nested\" \"1\" } \"buildid\"",
            1,
        );
        assert_eq!(
            parse_app_manifest(object_then_scalar.as_bytes()).unwrap_err(),
            SteamClientBuildError::MalformedManifest
        );
    }

    #[test]
    fn required_fields_inside_nested_objects_do_not_override_root_fields() {
        let nested_only = format!(
            "\"AppState\" {{ \"appid\" \"1623730\" \"StateFlags\" \"4\" \"installdir\" \"Palworld\" \"TargetBuildID\" \"{BUILD_ID}\" \"Nested\" {{ \"buildid\" \"{BUILD_ID}\" }} }}"
        );
        assert_eq!(
            parse_app_manifest(nested_only.as_bytes()).unwrap_err(),
            SteamClientBuildError::MalformedManifest
        );
    }

    #[test]
    fn transitional_install_state_and_target_build_disagreement_are_rejected() {
        let transitional =
            valid_manifest().replacen("\"StateFlags\" \"4\"", "\"StateFlags\" \"6\"", 1);
        assert_eq!(
            parse_app_manifest(transitional.as_bytes()).unwrap_err(),
            SteamClientBuildError::InstallationStateMismatch
        );
        let mismatched_target = valid_manifest().replacen(
            &format!("\"TargetBuildID\" \"{BUILD_ID}\""),
            "\"TargetBuildID\" \"1\"",
            1,
        );
        assert_eq!(
            parse_app_manifest(mismatched_target.as_bytes()).unwrap_err(),
            SteamClientBuildError::MalformedManifest
        );
    }

    #[test]
    fn a_non_steam_executable_layout_is_rejected_without_reading_an_arbitrary_manifest() {
        let temp = TempDir::new().unwrap();
        let executable = temp
            .path()
            .join("Pal")
            .join("Binaries")
            .join("Win64")
            .join("Palworld-Win64-Shipping.exe");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();

        assert_eq!(
            verify_steam_client_path(7, 44, &executable, BUILD_ID).unwrap_err(),
            SteamClientBuildError::UnsupportedInstallation
        );
    }

    #[test]
    fn error_messages_never_include_installation_paths() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("private-user-path");
        let error = verify_steam_client_path(7, 44, &path, BUILD_ID).unwrap_err();
        let display = error.to_string();
        let debug = format!("{error:?}");

        assert!(!display.contains("private-user-path"));
        assert!(!debug.contains("private-user-path"));
    }

    fn temp_path_fragment(path: &Path) -> &str {
        path.parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .unwrap()
    }
}
