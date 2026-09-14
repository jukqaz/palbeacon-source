use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SERVER_PROFILES_SCHEMA: &str = "palbeacon.server_profiles.v1";
pub const SERVER_CREDENTIAL_SERVICE_PREFIX: &str = "PalBeacon/server";
pub const MAX_SERVER_PROFILES: usize = 16;
pub const INSECURE_REST_HTTP_CONSENT_REVISION: u16 = 1;
const MAX_PROFILE_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestScheme {
    #[default]
    Http,
    Https,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InsecureRestHttpConsentV1 {
    pub endpoint: SocketAddr,
    pub risk_revision: u16,
    pub confirmed_at_unix_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerProfile {
    pub id: String,
    pub display_name: String,
    pub host: String,
    /// Optional dedicated REST origin host. Some hosting providers expose the
    /// official API on a different origin than the game and SFTP services.
    pub rest_host: Option<String>,
    pub game_port: u16,
    pub query_port: Option<u16>,
    pub sftp_port: Option<u16>,
    pub rest_port: Option<u16>,
    pub rcon_port: Option<u16>,
    pub rest_scheme: RestScheme,
    pub rest_username: Option<String>,
    pub sftp_username: Option<String>,
    pub save_root: Option<String>,
    pub ssh_host_key_fingerprint: Option<String>,
    #[serde(default)]
    pub insecure_rest_http_consent: Option<InsecureRestHttpConsentV1>,
}

impl ServerProfile {
    pub fn effective_rest_host(&self) -> &str {
        self.rest_host.as_deref().unwrap_or(&self.host)
    }

    pub fn literal_rest_socket_addr(&self) -> Option<SocketAddr> {
        let address = self.effective_rest_host().trim().parse::<IpAddr>().ok()?;
        if !is_explicit_http_unicast(address) {
            return None;
        }
        let port = self.rest_port.filter(|port| *port != 0)?;
        Some(SocketAddr::new(address, port))
    }

    pub fn has_current_insecure_rest_http_consent(&self) -> bool {
        self.insecure_rest_http_consent
            .as_ref()
            .is_some_and(|consent| {
                self.rest_scheme == RestScheme::Http
                    && consent.risk_revision == INSECURE_REST_HTTP_CONSENT_REVISION
                    && consent.confirmed_at_unix_ms != 0
                    && self.literal_rest_socket_addr() == Some(consent.endpoint)
            })
    }

    pub fn validate(&self) -> Result<(), ServerProfilesError> {
        validate_profile_id(&self.id)?;
        validate_display_name(&self.display_name)?;
        validate_host(&self.host)?;
        if let Some(rest_host) = self.rest_host.as_deref() {
            validate_host(rest_host)?;
        }
        if self.game_port == 0
            || self.query_port == Some(0)
            || self.sftp_port == Some(0)
            || self.rest_port == Some(0)
            || self.rcon_port == Some(0)
        {
            return Err(ServerProfilesError::InvalidPort);
        }
        validate_optional_text(
            self.rest_username.as_deref(),
            128,
            ServerProfilesError::InvalidUsername,
        )?;
        validate_optional_text(
            self.sftp_username.as_deref(),
            128,
            ServerProfilesError::InvalidUsername,
        )?;
        validate_save_root(self.save_root.as_deref())?;
        validate_fingerprint(self.ssh_host_key_fingerprint.as_deref())?;
        if self.insecure_rest_http_consent.is_some()
            && !self.has_current_insecure_rest_http_consent()
        {
            return Err(ServerProfilesError::InvalidInsecureRestHttpConsent);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerProfilesDocument {
    pub schema: String,
    pub version: u64,
    pub updated_at_unix_ms: u64,
    pub selected_profile_id: Option<String>,
    pub profiles: Vec<ServerProfile>,
}

impl Default for ServerProfilesDocument {
    fn default() -> Self {
        Self {
            schema: SERVER_PROFILES_SCHEMA.to_owned(),
            version: 1,
            updated_at_unix_ms: unix_ms(),
            selected_profile_id: None,
            profiles: Vec::new(),
        }
    }
}

impl ServerProfilesDocument {
    pub fn validate(&self) -> Result<(), ServerProfilesError> {
        if self.schema != SERVER_PROFILES_SCHEMA {
            return Err(ServerProfilesError::SchemaMismatch);
        }
        if self.version == 0 {
            return Err(ServerProfilesError::InvalidVersion);
        }
        if self.profiles.len() > MAX_SERVER_PROFILES {
            return Err(ServerProfilesError::TooManyProfiles);
        }
        for profile in &self.profiles {
            profile.validate()?;
        }
        let mut ids = self
            .profiles
            .iter()
            .map(|profile| profile.id.as_str())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        if ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ServerProfilesError::DuplicateProfileId);
        }
        if self
            .selected_profile_id
            .as_deref()
            .is_some_and(|selected| !self.profiles.iter().any(|profile| profile.id == selected))
        {
            return Err(ServerProfilesError::SelectedProfileMissing);
        }
        Ok(())
    }

    pub fn upsert(
        mut self,
        profile: ServerProfile,
    ) -> Result<ServerProfilesDocument, ServerProfilesError> {
        profile.validate()?;
        if let Some(existing) = self
            .profiles
            .iter_mut()
            .find(|existing| existing.id == profile.id)
        {
            *existing = profile;
        } else {
            if self.profiles.len() >= MAX_SERVER_PROFILES {
                return Err(ServerProfilesError::TooManyProfiles);
            }
            self.profiles.push(profile);
        }
        self.profiles.sort_by(|left, right| {
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        self.version = self
            .version
            .checked_add(1)
            .ok_or(ServerProfilesError::InvalidVersion)?;
        self.updated_at_unix_ms = unix_ms();
        self.validate()?;
        Ok(self)
    }

    pub fn remove(
        mut self,
        profile_id: &str,
    ) -> Result<ServerProfilesDocument, ServerProfilesError> {
        validate_profile_id(profile_id)?;
        let original_len = self.profiles.len();
        self.profiles.retain(|profile| profile.id != profile_id);
        if self.profiles.len() == original_len {
            return Err(ServerProfilesError::ProfileNotFound);
        }
        if self.selected_profile_id.as_deref() == Some(profile_id) {
            self.selected_profile_id = None;
        }
        self.version = self
            .version
            .checked_add(1)
            .ok_or(ServerProfilesError::InvalidVersion)?;
        self.updated_at_unix_ms = unix_ms();
        self.validate()?;
        Ok(self)
    }

    pub fn select(
        mut self,
        profile_id: Option<String>,
    ) -> Result<ServerProfilesDocument, ServerProfilesError> {
        if let Some(profile_id) = profile_id.as_deref() {
            validate_profile_id(profile_id)?;
            if !self.profiles.iter().any(|profile| profile.id == profile_id) {
                return Err(ServerProfilesError::ProfileNotFound);
            }
        }
        self.selected_profile_id = profile_id;
        self.version = self
            .version
            .checked_add(1)
            .ok_or(ServerProfilesError::InvalidVersion)?;
        self.updated_at_unix_ms = unix_ms();
        self.validate()?;
        Ok(self)
    }
}

#[derive(Debug, Error)]
pub enum ServerProfilesError {
    #[error("서버 프로필 파일을 읽거나 쓸 수 없습니다")]
    Io(#[from] std::io::Error),
    #[error("서버 프로필 JSON이 올바르지 않습니다")]
    Json(#[from] serde_json::Error),
    #[error("서버 프로필 파일이 허용 크기를 초과했습니다")]
    FileTooLarge,
    #[error("서버 프로필 스키마가 일치하지 않습니다")]
    SchemaMismatch,
    #[error("서버 프로필 버전이 올바르지 않습니다")]
    InvalidVersion,
    #[error("서버 프로필은 최대 16개까지 저장할 수 있습니다")]
    TooManyProfiles,
    #[error("서버 프로필 ID가 올바르지 않습니다")]
    InvalidProfileId,
    #[error("서버 이름이 올바르지 않습니다")]
    InvalidDisplayName,
    #[error("서버 호스트가 올바르지 않습니다")]
    InvalidHost,
    #[error("서버 포트가 올바르지 않습니다")]
    InvalidPort,
    #[error("서버 계정 이름이 올바르지 않습니다")]
    InvalidUsername,
    #[error("원격 세이브 경로가 올바르지 않습니다")]
    InvalidSaveRoot,
    #[error("SSH 호스트 키 지문은 SHA256 형식이어야 합니다")]
    InvalidFingerprint,
    #[error("암호화되지 않은 REST HTTP 동의가 현재 IP 및 포트와 일치하지 않습니다")]
    InvalidInsecureRestHttpConsent,
    #[error("같은 서버 프로필 ID가 중복되었습니다")]
    DuplicateProfileId,
    #[error("선택된 서버 프로필이 존재하지 않습니다")]
    SelectedProfileMissing,
    #[error("서버 프로필을 찾을 수 없습니다")]
    ProfileNotFound,
}

pub fn default_server_profiles_path() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("PalBeacon")
        .join("config")
        .join("server-profiles-v1.json")
}

pub fn read_server_profiles(path: &Path) -> Result<ServerProfilesDocument, ServerProfilesError> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_PROFILE_BYTES {
        return Err(ServerProfilesError::FileTooLarge);
    }
    let mut bytes = Vec::new();
    file.take(MAX_PROFILE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_PROFILE_BYTES {
        return Err(ServerProfilesError::FileTooLarge);
    }
    let document: ServerProfilesDocument = serde_json::from_slice(&bytes)?;
    document.validate()?;
    Ok(document)
}

pub fn read_or_create_server_profiles(
    path: &Path,
) -> Result<ServerProfilesDocument, ServerProfilesError> {
    match read_server_profiles(path) {
        Ok(document) => Ok(document),
        Err(ServerProfilesError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            let document = ServerProfilesDocument::default();
            write_server_profiles(path, &document)?;
            Ok(document)
        }
        Err(error) => Err(error),
    }
}

pub fn write_server_profiles(
    path: &Path,
    document: &ServerProfilesDocument,
) -> Result<(), ServerProfilesError> {
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(document)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_PROFILE_BYTES {
        return Err(ServerProfilesError::FileTooLarge);
    }
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "server profiles path has no parent",
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

fn validate_profile_id(value: &str) -> Result<(), ServerProfilesError> {
    if (8..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        Ok(())
    } else {
        Err(ServerProfilesError::InvalidProfileId)
    }
}

fn validate_display_name(value: &str) -> Result<(), ServerProfilesError> {
    let value = value.trim();
    if !value.is_empty() && value.len() <= 64 && !value.chars().any(char::is_control) {
        Ok(())
    } else {
        Err(ServerProfilesError::InvalidDisplayName)
    }
}

fn validate_host(value: &str) -> Result<(), ServerProfilesError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 253
        || value.chars().any(char::is_whitespace)
        || value.contains(['/', '\\', '@', '#', '?'])
    {
        return Err(ServerProfilesError::InvalidHost);
    }
    if value.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    let valid_dns = value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    });
    if valid_dns {
        Ok(())
    } else {
        Err(ServerProfilesError::InvalidHost)
    }
}

fn is_explicit_http_unicast(address: IpAddr) -> bool {
    let address = match address {
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map_or(IpAddr::V6(address), IpAddr::V4),
        address => address,
    };
    !address.is_unspecified()
        && !address.is_multicast()
        && !matches!(address, IpAddr::V4(address) if address == std::net::Ipv4Addr::BROADCAST)
}

fn validate_optional_text(
    value: Option<&str>,
    max_bytes: usize,
    error: ServerProfilesError,
) -> Result<(), ServerProfilesError> {
    if value.is_none_or(|value| {
        let value = value.trim();
        !value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
    }) {
        Ok(())
    } else {
        Err(error)
    }
}

fn validate_save_root(value: Option<&str>) -> Result<(), ServerProfilesError> {
    if value.is_none_or(|value| {
        let normalized = value.trim().replace('\\', "/");
        !normalized.is_empty()
            && normalized.len() <= 512
            && !normalized.starts_with('/')
            && !normalized.split('/').any(|segment| segment == "..")
            && !normalized.chars().any(char::is_control)
    }) {
        Ok(())
    } else {
        Err(ServerProfilesError::InvalidSaveRoot)
    }
}

fn validate_fingerprint(value: Option<&str>) -> Result<(), ServerProfilesError> {
    if value.is_none_or(|value| {
        let suffix = value.trim().strip_prefix("SHA256:");
        suffix.is_some_and(|suffix| {
            (32..=64).contains(&suffix.len())
                && suffix.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'_' | b'-' | b'=')
                })
        })
    }) {
        Ok(())
    } else {
        Err(ServerProfilesError::InvalidFingerprint)
    }
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
    use tempfile::tempdir;

    use super::*;

    fn profile(id: &str, name: &str) -> ServerProfile {
        ServerProfile {
            id: id.to_owned(),
            display_name: name.to_owned(),
            host: "pal.example.test".to_owned(),
            rest_host: Some("rest.pal.example.test".to_owned()),
            game_port: 8211,
            query_port: Some(27015),
            sftp_port: Some(22),
            rest_port: Some(8212),
            rcon_port: Some(25575),
            rest_scheme: RestScheme::Https,
            rest_username: Some("admin".to_owned()),
            sftp_username: Some("sync".to_owned()),
            save_root: Some("Pal/Saved/SaveGames/0".to_owned()),
            ssh_host_key_fingerprint: Some(
                "SHA256:s5Uy9ardpSZIpuu0FVPWE+k9siQK+JZdwSLEJeJVafE".to_owned(),
            ),
            insecure_rest_http_consent: None,
        }
    }

    #[test]
    fn profiles_round_trip_and_select_without_secret_material() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("server-profiles.json");
        let document = ServerProfilesDocument::default()
            .upsert(profile("server-alpha", "친구 서버"))
            .unwrap()
            .select(Some("server-alpha".to_owned()))
            .unwrap();
        write_server_profiles(&path, &document).unwrap();
        let restored = read_server_profiles(&path).unwrap();
        assert_eq!(restored, document);
        let serialized = fs::read_to_string(path).unwrap();
        assert!(!serialized.contains("password"));
        assert!(!serialized.contains("secret"));
    }

    #[test]
    fn duplicate_ids_and_parent_traversal_are_rejected() {
        let document = ServerProfilesDocument {
            profiles: vec![profile("server-alpha", "A"), profile("server-alpha", "B")],
            ..ServerProfilesDocument::default()
        };
        assert!(matches!(
            document.validate(),
            Err(ServerProfilesError::DuplicateProfileId)
        ));

        let mut unsafe_profile = profile("server-beta", "B");
        unsafe_profile.save_root = Some("../outside".to_owned());
        assert!(matches!(
            unsafe_profile.validate(),
            Err(ServerProfilesError::InvalidSaveRoot)
        ));

        let mut invalid_rest_origin = profile("server-gamma", "C");
        invalid_rest_origin.rest_host = Some("https://rest.example.test".to_owned());
        assert!(matches!(
            invalid_rest_origin.validate(),
            Err(ServerProfilesError::InvalidHost)
        ));
    }

    #[test]
    fn legacy_profile_without_dedicated_rest_host_remains_valid() {
        let mut value = serde_json::to_value(profile("server-alpha", "A")).unwrap();
        value.as_object_mut().unwrap().remove("rest_host");

        let restored: ServerProfile = serde_json::from_value(value).unwrap();

        assert_eq!(restored.rest_host, None);
        assert!(restored.validate().is_ok());
    }

    #[test]
    fn legacy_profile_without_insecure_http_consent_defaults_to_none() {
        let mut value = serde_json::to_value(profile("server-alpha", "A")).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("insecure_rest_http_consent");

        let restored: ServerProfile = serde_json::from_value(value).unwrap();

        assert_eq!(restored.insecure_rest_http_consent, None);
        assert!(restored.validate().is_ok());
    }

    #[test]
    fn insecure_http_consent_is_bound_to_the_current_literal_ip_and_port() {
        let endpoint = "203.0.113.17:8212".parse().unwrap();
        let mut consented = profile("server-alpha", "A");
        consented.rest_scheme = RestScheme::Http;
        consented.rest_host = Some("203.0.113.17".to_owned());
        consented.insecure_rest_http_consent = Some(InsecureRestHttpConsentV1 {
            endpoint,
            risk_revision: INSECURE_REST_HTTP_CONSENT_REVISION,
            confirmed_at_unix_ms: 1,
        });

        assert!(consented.validate().is_ok());
        assert!(consented.has_current_insecure_rest_http_consent());

        let mutations: [fn(&mut ServerProfile); 3] = [
            |profile: &mut ServerProfile| profile.rest_port = Some(8213),
            |profile: &mut ServerProfile| profile.rest_host = Some("203.0.113.18".to_owned()),
            |profile: &mut ServerProfile| profile.rest_scheme = RestScheme::Https,
        ];
        for mutate in mutations {
            let mut changed = consented.clone();
            mutate(&mut changed);
            assert!(matches!(
                changed.validate(),
                Err(ServerProfilesError::InvalidInsecureRestHttpConsent)
            ));
        }

        let mut wrong_revision = consented.clone();
        wrong_revision
            .insecure_rest_http_consent
            .as_mut()
            .unwrap()
            .risk_revision += 1;
        assert!(matches!(
            wrong_revision.validate(),
            Err(ServerProfilesError::InvalidInsecureRestHttpConsent)
        ));
    }

    #[test]
    fn literal_rest_socket_addr_handles_ipv6_and_rejects_dns() {
        let mut profile = profile("server-alpha", "A");
        profile.rest_host = Some("2001:db8::7".to_owned());
        assert_eq!(
            profile.literal_rest_socket_addr(),
            Some("[2001:db8::7]:8212".parse().unwrap())
        );

        profile.rest_host = Some("rest.example.test".to_owned());
        assert_eq!(profile.literal_rest_socket_addr(), None);

        for forbidden in [
            "0.0.0.0",
            "::",
            "224.0.0.1",
            "ff02::1",
            "255.255.255.255",
            "::ffff:0.0.0.0",
            "::ffff:224.0.0.1",
            "::ffff:255.255.255.255",
        ] {
            profile.rest_host = Some(forbidden.to_owned());
            assert_eq!(profile.literal_rest_socket_addr(), None, "{forbidden}");
        }
    }

    #[test]
    fn deleting_selected_profile_clears_selection() {
        let document = ServerProfilesDocument::default()
            .upsert(profile("server-alpha", "A"))
            .unwrap()
            .select(Some("server-alpha".to_owned()))
            .unwrap()
            .remove("server-alpha")
            .unwrap();
        assert!(document.selected_profile_id.is_none());
        assert!(document.profiles.is_empty());
    }
}
