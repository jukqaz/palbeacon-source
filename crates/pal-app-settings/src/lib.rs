#![forbid(unsafe_op_in_unsafe_fn)]

mod server_profiles;

pub use server_profiles::{
    INSECURE_REST_HTTP_CONSENT_REVISION, InsecureRestHttpConsentV1, MAX_SERVER_PROFILES,
    RestScheme, SERVER_CREDENTIAL_SERVICE_PREFIX, SERVER_PROFILES_SCHEMA, ServerProfile,
    ServerProfilesDocument, ServerProfilesError, default_server_profiles_path,
    read_or_create_server_profiles, read_server_profiles, write_server_profiles,
};

use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const APP_SETTINGS_SCHEMA: &str = "palbeacon.app_settings.v1";
const MAX_SETTINGS_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppSettings {
    pub launch_game_on_manual_start: bool,
    pub open_app_on_manual_start: bool,
    pub selected_import_id: Option<String>,
    pub selected_owner_uid: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            launch_game_on_manual_start: true,
            open_app_on_manual_start: true,
            selected_import_id: None,
            selected_owner_uid: None,
        }
    }
}

impl AppSettings {
    fn validate(&self) -> Result<(), AppSettingsError> {
        if self
            .selected_import_id
            .as_deref()
            .is_some_and(|value| !is_import_id(value))
        {
            return Err(AppSettingsError::InvalidImportId);
        }
        if self
            .selected_owner_uid
            .as_deref()
            .is_some_and(|value| !is_canonical_uuid(value))
        {
            return Err(AppSettingsError::InvalidOwnerUid);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppSettingsDocument {
    pub schema: String,
    pub version: u64,
    pub updated_at_unix_ms: u64,
    pub settings: AppSettings,
}

impl Default for AppSettingsDocument {
    fn default() -> Self {
        Self {
            schema: APP_SETTINGS_SCHEMA.to_owned(),
            version: 1,
            updated_at_unix_ms: unix_ms(),
            settings: AppSettings::default(),
        }
    }
}

impl AppSettingsDocument {
    pub fn validate(&self) -> Result<(), AppSettingsError> {
        if self.schema != APP_SETTINGS_SCHEMA {
            return Err(AppSettingsError::SchemaMismatch);
        }
        if self.version == 0 {
            return Err(AppSettingsError::InvalidVersion);
        }
        self.settings.validate()?;
        Ok(())
    }

    pub fn next(settings: AppSettings, current_version: u64) -> Result<Self, AppSettingsError> {
        let version = current_version
            .checked_add(1)
            .ok_or(AppSettingsError::InvalidVersion)?;
        let document = Self {
            schema: APP_SETTINGS_SCHEMA.to_owned(),
            version,
            updated_at_unix_ms: unix_ms(),
            settings,
        };
        document.validate()?;
        Ok(document)
    }
}

#[derive(Debug, Error)]
pub enum AppSettingsError {
    #[error("앱 설정 파일을 읽거나 쓸 수 없습니다")]
    Io(#[from] std::io::Error),
    #[error("앱 설정 JSON이 올바르지 않습니다")]
    Json(#[from] serde_json::Error),
    #[error("앱 설정 파일이 허용 크기를 초과했습니다")]
    FileTooLarge,
    #[error("앱 설정 스키마가 일치하지 않습니다")]
    SchemaMismatch,
    #[error("앱 설정 버전이 올바르지 않습니다")]
    InvalidVersion,
    #[error("선택한 세이브 가져오기 ID가 올바른 형식이 아닙니다")]
    InvalidImportId,
    #[error("선택한 캐릭터 UID가 올바른 UUID 형식이 아닙니다")]
    InvalidOwnerUid,
}

pub fn read_app_settings(path: &Path) -> Result<AppSettingsDocument, AppSettingsError> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_SETTINGS_BYTES {
        return Err(AppSettingsError::FileTooLarge);
    }
    let mut bytes = Vec::new();
    file.take(MAX_SETTINGS_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SETTINGS_BYTES {
        return Err(AppSettingsError::FileTooLarge);
    }
    let document: AppSettingsDocument = serde_json::from_slice(&bytes)?;
    document.validate()?;
    Ok(document)
}

pub fn write_app_settings(
    path: &Path,
    document: &AppSettingsDocument,
) -> Result<(), AppSettingsError> {
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(document)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SETTINGS_BYTES {
        return Err(AppSettingsError::FileTooLarge);
    }
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "app settings path has no parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut file = File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
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

fn is_canonical_uuid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    value.bytes().enumerate().all(|(index, byte)| {
        if matches!(index, 8 | 13 | 18 | 23) {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

fn is_import_id(value: &str) -> bool {
    value.len() == 24 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_document_is_safe_and_round_trips() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("app-settings.json");
        let document = AppSettingsDocument::default();
        assert!(document.settings.launch_game_on_manual_start);
        assert!(document.settings.open_app_on_manual_start);
        assert!(document.settings.selected_import_id.is_none());
        assert!(document.settings.selected_owner_uid.is_none());
        write_app_settings(&path, &document).expect("write");
        assert_eq!(read_app_settings(&path).expect("read"), document);
    }

    #[test]
    fn selected_import_id_is_local_and_strictly_validated() {
        let mut settings = AppSettings {
            selected_import_id: Some("b0bc52eda611399d11efe26f".to_owned()),
            ..AppSettings::default()
        };
        AppSettingsDocument::next(settings.clone(), 1).expect("canonical import ID");

        settings.selected_import_id = Some("../another-users-save".to_owned());
        assert!(matches!(
            AppSettingsDocument::next(settings, 1),
            Err(AppSettingsError::InvalidImportId)
        ));
    }

    #[test]
    fn selected_owner_uid_is_local_and_strictly_validated() {
        let mut settings = AppSettings {
            selected_owner_uid: Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned()),
            ..AppSettings::default()
        };
        AppSettingsDocument::next(settings.clone(), 1).expect("canonical owner UID");

        settings.selected_owner_uid = Some("../not-an-owner".to_owned());
        assert!(matches!(
            AppSettingsDocument::next(settings, 1),
            Err(AppSettingsError::InvalidOwnerUid)
        ));
    }
}
