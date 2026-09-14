use std::{
    fmt,
    path::{Path, PathBuf},
};

use pal_protected_file::{read_protected_file, read_regular_file_no_follow};
use pal_telemetry::NetworkConsumerConfig;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroizing;

const MAX_CONFIG_BYTES: usize = 64 * 1024;
const MAX_CA_CERTIFICATE_BYTES: usize = 1024 * 1024;
const MAX_CLIENT_CERTIFICATE_BYTES: usize = 1024 * 1024;
const MAX_CLIENT_PRIVATE_KEY_BYTES: usize = 256 * 1024;
const MAX_MAP_BMP_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENDPOINT_BYTES: usize = 2_048;
const MAX_PATH_BYTES: usize = 32_767;
const DEFAULT_MAXIMUM_RTT_MS: u64 = 1_000;
pub const DEV_DIAGNOSTIC_GAME_BUILD_ID: u64 = 24_181_527;
/// Exact unapproved Gate-B coordinate profile captured for the development diagnostic build.
/// Production alignment must not treat this pin as Gate-B approval.
pub const DEV_DIAGNOSTIC_COORDINATE_PROFILE_SHA256: &str =
    "9683571502240a99617aa686a4d10ea444c056ad94f4185a82b54295919fbe95";
pub const DEV_DIAGNOSTIC_MAP_BMP_SHA256: &str =
    "aa81bdddbc525898b0a70b238cc936d0f8b0416c4ead922797b066d871938d8b";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LiveSourceMode {
    /// Development-only live diagnostics. Gate B approval is still required before production
    /// map alignment may rely on captured observations.
    DevDiagnostic,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveSourceConfig {
    mode: LiveSourceMode,
    endpoint_uri: String,
    domain_name: String,
    ca_certificate_path: PathBuf,
    client_certificate_path: PathBuf,
    client_private_key_path: PathBuf,
    world_alias: String,
    subject_id: String,
    coordinate_profile_sha256: String,
    gate_profile_sha256: String,
    server_fingerprint: String,
    game_build_id: u64,
    map_bmp_path: PathBuf,
    map_bmp_sha256: String,
    #[serde(default = "default_maximum_rtt_ms")]
    maximum_rtt_ms: u64,
}

impl fmt::Debug for LiveSourceConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED LIVE SOURCE CONFIG]")
    }
}

impl LiveSourceConfig {
    pub const fn mode(&self) -> LiveSourceMode {
        self.mode
    }

    pub fn endpoint_uri(&self) -> &str {
        &self.endpoint_uri
    }

    pub fn domain_name(&self) -> &str {
        &self.domain_name
    }

    pub fn world_alias(&self) -> &str {
        &self.world_alias
    }

    pub fn subject_id(&self) -> [u8; 32] {
        decode_sha256(&self.subject_id).expect("validated subject identifier")
    }

    pub fn coordinate_profile_sha256(&self) -> &str {
        &self.coordinate_profile_sha256
    }

    pub fn gate_profile_sha256(&self) -> &str {
        &self.gate_profile_sha256
    }

    pub fn server_fingerprint(&self) -> &str {
        &self.server_fingerprint
    }

    pub const fn game_build_id(&self) -> u64 {
        self.game_build_id
    }

    pub fn map_bmp_path(&self) -> &Path {
        &self.map_bmp_path
    }

    pub fn map_bmp_sha256(&self) -> &str {
        &self.map_bmp_sha256
    }

    pub const fn maximum_rtt_ms(&self) -> u64 {
        self.maximum_rtt_ms
    }

    /// Loads owner-only mTLS material through no-follow handles and constructs the existing
    /// fail-closed network consumer configuration. Temporary key material is zeroized.
    pub fn load_network_consumer_config(
        &self,
    ) -> Result<NetworkConsumerConfig, LiveSourceConfigError> {
        self.load_network_consumer_config_with(|path, maximum| {
            read_protected_file(path, maximum).map_err(|_| ())
        })
    }

    /// Loads the exact regular BMP through a no-follow handle and verifies its configured hash.
    /// No decoded map may be rendered before this boundary succeeds.
    pub fn load_verified_map_bmp(&self) -> Result<VerifiedMapBmp, LiveSourceConfigError> {
        self.load_verified_map_bmp_with(|path, maximum| {
            read_regular_file_no_follow(path, maximum).map_err(|_| ())
        })
    }

    fn load_verified_map_bmp_with<E>(
        &self,
        mut read: impl FnMut(&Path, usize) -> Result<Vec<u8>, E>,
    ) -> Result<VerifiedMapBmp, LiveSourceConfigError> {
        let bytes = read(&self.map_bmp_path, MAX_MAP_BMP_BYTES)
            .map_err(|_| LiveSourceConfigError::MapFile)?;
        let actual_sha256 = canonical_sha256(&bytes);
        if actual_sha256 != self.map_bmp_sha256 {
            return Err(LiveSourceConfigError::MapIntegrity);
        }
        Ok(VerifiedMapBmp {
            bytes,
            game_build_id: self.game_build_id,
            sha256: actual_sha256,
        })
    }

    fn load_network_consumer_config_with<E>(
        &self,
        mut read: impl FnMut(&Path, usize) -> Result<Vec<u8>, E>,
    ) -> Result<NetworkConsumerConfig, LiveSourceConfigError> {
        let ca = Zeroizing::new(
            read(&self.ca_certificate_path, MAX_CA_CERTIFICATE_BYTES)
                .map_err(|_| LiveSourceConfigError::CredentialFile)?,
        );
        let certificate = Zeroizing::new(
            read(&self.client_certificate_path, MAX_CLIENT_CERTIFICATE_BYTES)
                .map_err(|_| LiveSourceConfigError::CredentialFile)?,
        );
        let private_key = Zeroizing::new(
            read(&self.client_private_key_path, MAX_CLIENT_PRIVATE_KEY_BYTES)
                .map_err(|_| LiveSourceConfigError::CredentialFile)?,
        );
        if !looks_like_certificate(&ca)
            || !looks_like_certificate(&certificate)
            || !looks_like_private_key(&private_key)
        {
            return Err(LiveSourceConfigError::InvalidCredentials);
        }

        NetworkConsumerConfig::new(
            &self.endpoint_uri,
            &self.domain_name,
            ca.as_slice(),
            certificate.as_slice(),
            private_key.as_slice(),
            &self.world_alias,
            self.subject_id(),
            &self.coordinate_profile_sha256,
            self.maximum_rtt_ms,
        )
        .map_err(|_| LiveSourceConfigError::NetworkConfiguration)?
        .with_descriptor_pins(&self.gate_profile_sha256, &self.server_fingerprint)
        .map_err(|_| LiveSourceConfigError::NetworkConfiguration)
    }
}

pub struct VerifiedMapBmp {
    bytes: Vec<u8>,
    game_build_id: u64,
    sha256: String,
}

impl fmt::Debug for VerifiedMapBmp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[VERIFIED MAP BMP]")
    }
}

impl VerifiedMapBmp {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub const fn game_build_id(&self) -> u64 {
        self.game_build_id
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

pub fn parse_live_source_config(bytes: &[u8]) -> Result<LiveSourceConfig, LiveSourceConfigError> {
    if bytes.is_empty() || bytes.len() > MAX_CONFIG_BYTES {
        return Err(LiveSourceConfigError::InvalidDocument);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| LiveSourceConfigError::InvalidDocument)?;
    let config: LiveSourceConfig =
        toml::from_str(text).map_err(|_| LiveSourceConfigError::InvalidDocument)?;
    validate_config(&config)?;
    Ok(config)
}

pub fn load_live_source_config(path: &Path) -> Result<LiveSourceConfig, LiveSourceConfigError> {
    let bytes = Zeroizing::new(
        read_protected_file(path, MAX_CONFIG_BYTES)
            .map_err(|_| LiveSourceConfigError::ConfigurationFile)?,
    );
    parse_live_source_config(&bytes)
}

fn validate_config(config: &LiveSourceConfig) -> Result<(), LiveSourceConfigError> {
    if !valid_endpoint(&config.endpoint_uri)
        || !valid_domain_name(&config.domain_name)
        || !valid_ascii_identifier(&config.world_alias, 64)
        || decode_nonzero_sha256(&config.subject_id).is_none()
        || config.coordinate_profile_sha256 != DEV_DIAGNOSTIC_COORDINATE_PROFILE_SHA256
        || decode_nonzero_sha256(&config.gate_profile_sha256).is_none()
        || decode_nonzero_sha256(&config.server_fingerprint).is_none()
        || config.game_build_id != DEV_DIAGNOSTIC_GAME_BUILD_ID
        || config.map_bmp_sha256 != DEV_DIAGNOSTIC_MAP_BMP_SHA256
        || !(1..=10_000).contains(&config.maximum_rtt_ms)
        || ![
            &config.ca_certificate_path,
            &config.client_certificate_path,
            &config.client_private_key_path,
            &config.map_bmp_path,
        ]
        .into_iter()
        .all(|path| valid_absolute_path(path))
        || !config
            .map_bmp_path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("bmp"))
    {
        return Err(LiveSourceConfigError::InvalidConfiguration);
    }
    Ok(())
}

fn valid_endpoint(value: &str) -> bool {
    if value.is_empty()
        || value.len() > MAX_ENDPOINT_BYTES
        || !value.is_ascii()
        || !value.starts_with("https://")
        || value.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return false;
    }
    let authority = &value["https://".len()..];
    !authority.is_empty()
        && !authority.contains(['/', '@', '?', '#'])
        && !authority.starts_with(':')
        && !authority.ends_with(':')
}

fn valid_domain_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.is_ascii()
        && !value.starts_with('.')
        && !value.ends_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

fn valid_ascii_identifier(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'/' | b'\\'))
}

fn valid_absolute_path(path: &Path) -> bool {
    path.is_absolute()
        && path.to_str().is_some_and(|value| {
            !value.is_empty() && value.len() <= MAX_PATH_BYTES && !value.contains('\0')
        })
}

fn looks_like_certificate(bytes: &[u8]) -> bool {
    bytes.starts_with(b"-----BEGIN CERTIFICATE-----")
        && bytes
            .windows(b"-----END CERTIFICATE-----".len())
            .any(|window| window == b"-----END CERTIFICATE-----")
}

fn looks_like_private_key(bytes: &[u8]) -> bool {
    [
        (
            b"-----BEGIN PRIVATE KEY-----".as_slice(),
            b"-----END PRIVATE KEY-----".as_slice(),
        ),
        (
            b"-----BEGIN RSA PRIVATE KEY-----".as_slice(),
            b"-----END RSA PRIVATE KEY-----".as_slice(),
        ),
        (
            b"-----BEGIN EC PRIVATE KEY-----".as_slice(),
            b"-----END EC PRIVATE KEY-----".as_slice(),
        ),
    ]
    .into_iter()
    .any(|(prefix, suffix)| {
        bytes.starts_with(prefix) && bytes.windows(suffix.len()).any(|window| window == suffix)
    })
}

fn decode_nonzero_sha256(value: &str) -> Option<[u8; 32]> {
    let decoded = decode_sha256(value)?;
    (decoded != [0; 32]).then_some(decoded)
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Some(output)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn canonical_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

const fn default_maximum_rtt_ms() -> u64 {
    DEFAULT_MAXIMUM_RTT_MS
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum LiveSourceConfigError {
    #[error("live source configuration document is invalid")]
    InvalidDocument,
    #[error("live source configuration is invalid")]
    InvalidConfiguration,
    #[error("live source configuration file was refused")]
    ConfigurationFile,
    #[error("live source credential file was refused")]
    CredentialFile,
    #[error("live source credentials are invalid")]
    InvalidCredentials,
    #[error("live source network configuration is invalid")]
    NetworkConfiguration,
    #[error("live source map file was refused")]
    MapFile,
    #[error("live source map integrity check failed")]
    MapIntegrity,
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::Path};

    use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};

    use super::*;

    fn valid_tls_material() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut ca_parameters = CertificateParams::default();
        ca_parameters.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = CertifiedIssuer::self_signed(ca_parameters, KeyPair::generate().unwrap()).unwrap();
        let client_key = KeyPair::generate().unwrap();
        let client_certificate = CertificateParams::default()
            .signed_by(&client_key, &ca)
            .unwrap();
        (
            ca.pem().into_bytes(),
            client_certificate.pem().into_bytes(),
            client_key.serialize_pem().into_bytes(),
        )
    }

    fn absolute(value: &str) -> String {
        std::env::temp_dir()
            .join(value)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn valid_document() -> Vec<u8> {
        format!(
            r#"mode = "dev_diagnostic"
endpoint_uri = "https://127.0.0.1:45123"
domain_name = "pal-agent.local"
ca_certificate_path = "{}"
client_certificate_path = "{}"
client_private_key_path = "{}"
world_alias = "private-world"
subject_id = "{}"
coordinate_profile_sha256 = "{}"
gate_profile_sha256 = "{}"
server_fingerprint = "{}"
game_build_id = 24181527
map_bmp_path = "{}"
map_bmp_sha256 = "{}"
maximum_rtt_ms = 750
"#,
            absolute("ca.pem"),
            absolute("client.pem"),
            absolute("client.key"),
            "11".repeat(32),
            DEV_DIAGNOSTIC_COORDINATE_PROFILE_SHA256,
            "33".repeat(32),
            "44".repeat(32),
            absolute("palworld-map.bmp"),
            DEV_DIAGNOSTIC_MAP_BMP_SHA256,
        )
        .into_bytes()
    }

    #[test]
    fn strict_config_parses_all_identity_and_map_bindings() {
        let config = parse_live_source_config(&valid_document()).unwrap();

        assert_eq!(config.mode(), LiveSourceMode::DevDiagnostic);
        assert_eq!(config.endpoint_uri(), "https://127.0.0.1:45123");
        assert_eq!(config.domain_name(), "pal-agent.local");
        assert_eq!(config.world_alias(), "private-world");
        assert_eq!(config.subject_id(), [0x11; 32]);
        assert_eq!(
            config.coordinate_profile_sha256(),
            DEV_DIAGNOSTIC_COORDINATE_PROFILE_SHA256
        );
        assert_eq!(config.gate_profile_sha256(), "33".repeat(32));
        assert_eq!(config.server_fingerprint(), "44".repeat(32));
        assert_eq!(config.game_build_id(), 24_181_527);
        assert_eq!(config.map_bmp_sha256(), DEV_DIAGNOSTIC_MAP_BMP_SHA256);
        assert_eq!(config.maximum_rtt_ms(), 750);
    }

    #[test]
    fn unknown_fields_and_unsafe_endpoint_forms_are_refused() {
        let mut unknown = String::from_utf8(valid_document()).unwrap();
        unknown.push_str("private_key = \"secret\"\n");
        assert_eq!(
            parse_live_source_config(unknown.as_bytes()).unwrap_err(),
            LiveSourceConfigError::InvalidDocument
        );

        for endpoint in [
            "http://127.0.0.1:45123",
            "https://user:password@agent.local:45123",
            "https://agent.local:45123/service",
            "https://agent.local:45123?token=secret",
            "https://agent.local:45123#fragment",
        ] {
            let document = String::from_utf8(valid_document())
                .unwrap()
                .replace("https://127.0.0.1:45123", endpoint);
            assert_eq!(
                parse_live_source_config(document.as_bytes()).unwrap_err(),
                LiveSourceConfigError::InvalidConfiguration,
                "endpoint should be refused: {endpoint}"
            );
        }
    }

    #[test]
    fn build_and_map_identity_are_mandatory_and_canonical() {
        for (needle, replacement) in [
            ("game_build_id = 24181527", "game_build_id = 0"),
            ("game_build_id = 24181527", "game_build_id = 24181528"),
            (&"11".repeat(32), &"00".repeat(32)),
            (DEV_DIAGNOSTIC_COORDINATE_PROFILE_SHA256, &"22".repeat(32)),
            (&"33".repeat(32), &"00".repeat(32)),
            (&"44".repeat(32), "short"),
            (DEV_DIAGNOSTIC_MAP_BMP_SHA256, "short"),
            (DEV_DIAGNOSTIC_MAP_BMP_SHA256, &"44".repeat(32)),
            ("palworld-map.bmp", "palworld-map.png"),
        ] {
            let document =
                String::from_utf8(valid_document())
                    .unwrap()
                    .replacen(needle, replacement, 1);
            assert_eq!(
                parse_live_source_config(document.as_bytes()).unwrap_err(),
                LiveSourceConfigError::InvalidConfiguration
            );
        }
    }

    #[test]
    fn debug_and_errors_never_render_configuration_values() {
        let document = valid_document();
        let config = parse_live_source_config(&document).unwrap();
        let debug = format!("{config:?}");
        assert_eq!(debug, "[REDACTED LIVE SOURCE CONFIG]");

        let mut malformed = document;
        malformed.extend_from_slice(b"secret = 'private-key-value'");
        let error = parse_live_source_config(&malformed).unwrap_err();
        let rendered = format!("{error:?} {error}");
        for forbidden in [
            "private-key-value",
            "127.0.0.1",
            "private-world",
            "client.key",
        ] {
            assert!(!rendered.contains(forbidden));
        }
    }

    #[test]
    fn protected_material_is_bounded_and_constructs_redacted_network_config() {
        let config = parse_live_source_config(&valid_document()).unwrap();
        let (ca, certificate, private_key) = valid_tls_material();
        let mut files = HashMap::new();
        files.insert(config.ca_certificate_path.clone(), ca);
        files.insert(config.client_certificate_path.clone(), certificate);
        files.insert(config.client_private_key_path.clone(), private_key);
        let mut requested = Vec::new();

        let network = config
            .load_network_consumer_config_with(|path, maximum| {
                requested.push((path.to_path_buf(), maximum));
                files.get(path).cloned().ok_or(())
            })
            .unwrap();

        assert_eq!(format!("{network:?}"), "[REDACTED NETWORK CONSUMER CONFIG]");
        assert_eq!(requested.len(), 3);
        assert_eq!(requested[0].1, MAX_CA_CERTIFICATE_BYTES);
        assert_eq!(requested[1].1, MAX_CLIENT_CERTIFICATE_BYTES);
        assert_eq!(requested[2].1, MAX_CLIENT_PRIVATE_KEY_BYTES);
    }

    #[test]
    fn invalid_pem_does_not_appear_in_error() {
        let config = parse_live_source_config(&valid_document()).unwrap();
        let error = config
            .load_network_consumer_config_with(|path: &Path, _| {
                if path == config.client_private_key_path {
                    Ok::<_, ()>(b"SUPER-SECRET-INVALID-KEY".to_vec())
                } else {
                    Ok::<_, ()>(
                        b"-----BEGIN CERTIFICATE-----\nx\n-----END CERTIFICATE-----".to_vec(),
                    )
                }
            })
            .unwrap_err();

        assert_eq!(error, LiveSourceConfigError::InvalidCredentials);
        assert!(!format!("{error:?} {error}").contains("SUPER-SECRET"));
    }

    #[test]
    fn map_bytes_are_bounded_and_hash_checked_before_decode() {
        let mut config = parse_live_source_config(&valid_document()).unwrap();
        let exact_bytes = b"BM-exact-private-map-fixture";
        config.map_bmp_sha256 = canonical_sha256(exact_bytes);
        let mut requested_limit = 0;

        let verified = config
            .load_verified_map_bmp_with(|_, maximum| {
                requested_limit = maximum;
                Ok::<_, ()>(exact_bytes.to_vec())
            })
            .unwrap();

        assert_eq!(requested_limit, MAX_MAP_BMP_BYTES);
        assert_eq!(verified.bytes(), exact_bytes);
        assert_eq!(verified.game_build_id(), DEV_DIAGNOSTIC_GAME_BUILD_ID);
        assert_eq!(verified.sha256(), canonical_sha256(exact_bytes));
        assert_eq!(format!("{verified:?}"), "[VERIFIED MAP BMP]");

        let error = config
            .load_verified_map_bmp_with(|_, _| Ok::<_, ()>(b"different-map".to_vec()))
            .unwrap_err();
        assert_eq!(error, LiveSourceConfigError::MapIntegrity);
    }
}
