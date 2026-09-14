use std::{
    ffi::OsString,
    fmt,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

use pal_protected_file::ProtectedFileError;
use pal_rest::{EndpointPolicy, PlayerSelector};
use serde::Deserialize;
use thiserror::Error;
use zeroize::Zeroize;

const MAX_CONFIG_BYTES: usize = 256 * 1024;
const MAX_SECRET_BYTES: usize = 64 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    world_alias: String,
    rest_base_url: String,
    #[serde(default)]
    rest_transport: RestTransport,
    listen_address: SocketAddr,
    profile_path: PathBuf,
    gate_verification_key_path: PathBuf,
    secrets_path: PathBuf,
    server_certificate_path: PathBuf,
    server_private_key_path: PathBuf,
    client_ca_certificate_path: PathBuf,
    expected_gate_profile_sha256: String,
    coordinate_profile_sha256: String,
    selected_interval_ms: u64,
    #[serde(default)]
    client_allowlist: Vec<ClientAllowlistConfig>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientAllowlistConfig {
    certificate_fingerprint_sha256: String,
    identity_uri: String,
    subject_id: String,
}

impl fmt::Debug for AgentConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentConfig")
            .field("world_alias", &"[REDACTED]")
            .field("rest_endpoint", &"[REDACTED]")
            .field("listen_address", &"[REDACTED]")
            .field("paths", &"[REDACTED]")
            .field("selected_interval_ms", &self.selected_interval_ms)
            .finish()
    }
}

impl AgentConfig {
    pub fn world_alias(&self) -> &str {
        &self.world_alias
    }

    pub fn rest_base_url(&self) -> &str {
        &self.rest_base_url
    }

    pub const fn rest_transport(&self) -> RestTransport {
        self.rest_transport
    }

    pub const fn listen_address(&self) -> SocketAddr {
        self.listen_address
    }

    pub fn profile_path(&self) -> &Path {
        &self.profile_path
    }

    pub fn gate_verification_key_path(&self) -> &Path {
        &self.gate_verification_key_path
    }

    pub fn secrets_path(&self) -> &Path {
        &self.secrets_path
    }

    pub fn server_certificate_path(&self) -> &Path {
        &self.server_certificate_path
    }

    pub fn server_private_key_path(&self) -> &Path {
        &self.server_private_key_path
    }

    pub fn client_ca_certificate_path(&self) -> &Path {
        &self.client_ca_certificate_path
    }

    pub fn startup_gate_config(&self) -> crate::StartupGateConfig {
        crate::StartupGateConfig {
            expected_profile_sha256: self.expected_gate_profile_sha256.clone(),
            coordinate_profile_sha256: self.coordinate_profile_sha256.clone(),
            selected_interval_ms: self.selected_interval_ms,
        }
    }

    pub fn endpoint_policy(&self) -> Result<EndpointPolicy, ConfigError> {
        match self.rest_transport {
            RestTransport::LocalProcess => Ok(EndpointPolicy::loopback_only()),
            RestTransport::ExplicitRemoteHttps => EndpointPolicy::loopback_only()
                .allow_exact_remote_https(&self.rest_base_url)
                .map_err(|_| ConfigError::Invalid),
        }
    }

    pub fn client_allowlist(
        &self,
        expected_subject_id: &[u8; 32],
    ) -> Result<pal_telemetry::ClientAllowlist, ConfigError> {
        let entries = self
            .client_allowlist
            .iter()
            .map(|entry| {
                let subject_id =
                    decode_nonzero_sha256(&entry.subject_id).ok_or(ConfigError::Invalid)?;
                if &subject_id != expected_subject_id {
                    return Err(ConfigError::Invalid);
                }
                pal_telemetry::AllowlistEntry::new(
                    entry.certificate_fingerprint_sha256.clone(),
                    entry.identity_uri.clone(),
                    self.world_alias.clone(),
                    subject_id,
                )
                .map_err(|_| ConfigError::Invalid)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(pal_telemetry::ClientAllowlist::new(entries))
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RestTransport {
    #[default]
    LocalProcess,
    ExplicitRemoteHttps,
}

pub fn parse_config(bytes: &[u8]) -> Result<AgentConfig, ConfigError> {
    if bytes.is_empty() || bytes.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::Invalid);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ConfigError::Invalid)?;
    let config: AgentConfig = toml::from_str(text).map_err(|_| ConfigError::Invalid)?;
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &AgentConfig) -> Result<(), ConfigError> {
    if config.world_alias.is_empty()
        || config.world_alias.len() > 64
        || !config.world_alias.is_ascii()
        || !is_private_or_loopback(config.listen_address.ip())
        || config.listen_address.port() == 0
        || !is_canonical_sha256(&config.expected_gate_profile_sha256)
        || !is_canonical_sha256(&config.coordinate_profile_sha256)
        || !matches!(config.selected_interval_ms, 500 | 1_000 | 2_000)
        || config.client_allowlist.is_empty()
        || config.client_allowlist.iter().any(|entry| {
            !is_canonical_sha256(&entry.certificate_fingerprint_sha256)
                || !entry.identity_uri.is_ascii()
                || !entry.identity_uri.starts_with("spiffe://")
                || decode_nonzero_sha256(&entry.subject_id).is_none()
        })
        || [
            &config.profile_path,
            &config.gate_verification_key_path,
            &config.secrets_path,
            &config.server_certificate_path,
            &config.server_private_key_path,
            &config.client_ca_certificate_path,
        ]
        .into_iter()
        .any(|path| path.as_os_str().is_empty())
    {
        return Err(ConfigError::Invalid);
    }
    let policy = config.endpoint_policy()?;
    pal_rest::ClientConfig::new(&config.rest_base_url, policy).map_err(|_| ConfigError::Invalid)?;
    Ok(())
}

fn is_private_or_loopback(address: IpAddr) -> bool {
    if address.is_loopback() {
        return true;
    }
    let IpAddr::V4(address) = address else {
        return false;
    };
    is_rfc1918(address)
}

fn is_rfc1918(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 10
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
}

fn is_canonical_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SelectorKind {
    UserId,
    InstanceId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretBundle {
    rest_username: String,
    rest_password: String,
    selector_kind: SelectorKind,
    selector_value: String,
    pseudonymization_key_hex: String,
    expected_player_subject_id: String,
}

impl fmt::Debug for SecretBundle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED SECRET BUNDLE]")
    }
}

impl Drop for SecretBundle {
    fn drop(&mut self) {
        self.rest_username.zeroize();
        self.rest_password.zeroize();
        self.selector_value.zeroize();
        self.pseudonymization_key_hex.zeroize();
        self.expected_player_subject_id.zeroize();
    }
}

impl SecretBundle {
    pub fn parse(bytes: &[u8]) -> Result<Self, ConfigError> {
        if bytes.is_empty() || bytes.len() > MAX_SECRET_BYTES {
            return Err(ConfigError::InvalidSecrets);
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| ConfigError::InvalidSecrets)?;
        if value.rest_username.is_empty()
            || value.rest_password.is_empty()
            || value.selector_value.is_empty()
            || decode_sha256(&value.pseudonymization_key_hex)
                .is_none_or(|key| !key_has_minimum_diversity(&key))
            || decode_nonzero_sha256(&value.expected_player_subject_id).is_none()
        {
            return Err(ConfigError::InvalidSecrets);
        }
        Ok(value)
    }

    pub fn rest_auth(&self) -> Result<pal_rest::BasicAuthSecret, ConfigError> {
        pal_rest::BasicAuthSecret::new(self.rest_username.clone(), self.rest_password.clone())
            .map_err(|_| ConfigError::InvalidSecrets)
    }

    pub fn selector(&self) -> Result<PlayerSelector, ConfigError> {
        match self.selector_kind {
            SelectorKind::UserId => PlayerSelector::user_id(self.selector_value.clone()),
            SelectorKind::InstanceId => PlayerSelector::instance_id(self.selector_value.clone()),
        }
        .map_err(|_| ConfigError::InvalidSecrets)
    }

    pub fn pseudonymization_key(&self) -> Result<[u8; 32], ConfigError> {
        decode_sha256(&self.pseudonymization_key_hex).ok_or(ConfigError::InvalidSecrets)
    }

    pub fn expected_player_subject_id(&self) -> Result<[u8; 32], ConfigError> {
        decode_nonzero_sha256(&self.expected_player_subject_id).ok_or(ConfigError::InvalidSecrets)
    }
}

fn decode_nonzero_sha256(value: &str) -> Option<[u8; 32]> {
    let output = decode_sha256(value)?;
    (output != [0; 32]).then_some(output)
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if !is_canonical_sha256(value) {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Some(output)
}

pub fn parse_verification_key(bytes: &[u8]) -> Result<[u8; 32], ConfigError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ConfigError::InvalidSecrets)?;
    let key = decode_sha256(text).ok_or(ConfigError::InvalidSecrets)?;
    if key == [0; 32] {
        return Err(ConfigError::InvalidSecrets);
    }
    Ok(key)
}

pub fn load_protected_config(path: &Path) -> Result<AgentConfig, ProtectedFileError> {
    let bytes = crate::read_protected_file(path, MAX_CONFIG_BYTES)?;
    parse_config(&bytes).map_err(|_| ProtectedFileError::Invalid)
}

fn key_has_minimum_diversity(key: &[u8; 32]) -> bool {
    let mut distinct = [false; 256];
    for byte in key {
        distinct[usize::from(*byte)] = true;
    }
    distinct.into_iter().filter(|present| *present).count() >= 8
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentCommand {
    pub config_path: PathBuf,
}

pub fn parse_command<I, S>(args: I) -> Result<AgentCommand, ConfigError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next().ok_or(ConfigError::InvalidCommand)?;
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--config")) {
        return Err(ConfigError::InvalidCommand);
    }
    let config_path = PathBuf::from(args.next().ok_or(ConfigError::InvalidCommand)?);
    if config_path.as_os_str().is_empty() || args.next().is_some() {
        return Err(ConfigError::InvalidCommand);
    }
    Ok(AgentCommand { config_path })
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ConfigError {
    #[error("agent configuration is invalid")]
    Invalid,
    #[error("agent secret bundle is invalid")]
    InvalidSecrets,
    #[error("agent command is invalid")]
    InvalidCommand,
}
