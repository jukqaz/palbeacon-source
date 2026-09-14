use std::fmt;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::RestError;

const HMAC_BLOCK_BYTES: usize = 64;
const PLAYER_DOMAIN: &[u8] = b"pal-companion/player-subject/v1";
const SERVER_DOMAIN: &[u8] = b"pal-companion/server-subject/v1";

#[derive(Clone, Copy)]
pub(crate) enum SelectorKind {
    UserId,
    InstanceId,
}

impl SelectorKind {
    const fn domain_tag(self) -> &'static [u8] {
        match self {
            Self::UserId => b"user-id",
            Self::InstanceId => b"instance-id",
        }
    }
}

/// Exact, opaque player identity selector. Nickname selection is intentionally
/// absent because display names are neither unique nor stable.
#[derive(Eq, PartialEq)]
pub enum PlayerSelector {
    UserId(SecretSelectorValue),
    InstanceId(SecretSelectorValue),
}

impl PlayerSelector {
    pub fn user_id(value: impl Into<String>) -> Result<Self, RestError> {
        Self::from_value(value.into(), SelectorKind::UserId)
    }

    pub fn instance_id(value: impl Into<String>) -> Result<Self, RestError> {
        Self::from_value(value.into(), SelectorKind::InstanceId)
    }

    fn from_value(value: String, kind: SelectorKind) -> Result<Self, RestError> {
        let value = SecretSelectorValue(value.into_bytes());
        let value_text = value.as_str();
        if value_text.is_empty()
            || value_text.len() > 1_024
            || value_text.chars().any(char::is_control)
        {
            return Err(RestError::InvalidSelector);
        }
        Ok(match kind {
            SelectorKind::UserId => Self::UserId(value),
            SelectorKind::InstanceId => Self::InstanceId(value),
        })
    }

    pub(crate) fn kind(&self) -> SelectorKind {
        match self {
            Self::UserId(_) => SelectorKind::UserId,
            Self::InstanceId(_) => SelectorKind::InstanceId,
        }
    }
}

impl Clone for PlayerSelector {
    fn clone(&self) -> Self {
        match self {
            Self::UserId(value) => Self::UserId(value.clone()),
            Self::InstanceId(value) => Self::InstanceId(value.clone()),
        }
    }
}

impl fmt::Debug for PlayerSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Eq, PartialEq)]
pub struct SecretSelectorValue(Vec<u8>);

impl SecretSelectorValue {
    pub(crate) fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("constructed from a Rust String")
    }
}

impl Clone for SecretSelectorValue {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl fmt::Debug for SecretSelectorValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl Drop for SecretSelectorValue {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

/// Opaque result of selecting a single Player actor. It is deliberately not
/// serializable and its fields are private.
pub struct SelectedPlayerObservation {
    pub(crate) raw_selected_id: Vec<u8>,
    pub(crate) selector_kind: SelectorKind,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) z: f64,
    pub(crate) heading_degrees: Option<f32>,
    pub(crate) server_fps: f64,
    pub(crate) average_server_fps: f64,
    pub(crate) actor_count: usize,
}

impl fmt::Debug for SelectedPlayerObservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl Drop for SelectedPlayerObservation {
    fn drop(&mut self) {
        self.raw_selected_id.fill(0);
    }
}

/// Serializable player observation after all raw server identities have been
/// replaced by a keyed pseudonym.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SafePlayerObservation {
    pub subject_id: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub heading_degrees: Option<f32>,
    pub server_fps: f64,
    pub average_server_fps: f64,
    pub actor_count: usize,
}

/// Opaque raw `/info` projection. Names and description are validated and
/// discarded by the decoder; the world GUID remains private until sanitizing.
pub struct RawServerInfo {
    pub(crate) version: String,
    pub(crate) world_guid: Vec<u8>,
}

impl fmt::Debug for RawServerInfo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl Drop for RawServerInfo {
    fn drop(&mut self) {
        self.world_guid.fill(0);
    }
}

/// Serializable `/info` data with only a version and keyed server identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SanitizedServerInfo {
    pub version: String,
    pub server_subject_id: String,
}

/// Keyed pseudonymization primitive. The key is overwritten on drop and never
/// appears in Debug output.
pub struct Pseudonymizer {
    key: [u8; 32],
}

impl Pseudonymizer {
    pub const fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    fn pseudonym(
        &self,
        domain: &[u8],
        world_alias: &str,
        parts: &[&[u8]],
    ) -> Result<String, RestError> {
        validate_world_alias(world_alias)?;
        let mut message = Vec::with_capacity(
            domain.len()
                + world_alias.len()
                + parts.iter().map(|part| part.len()).sum::<usize>()
                + (parts.len() + 2) * 8,
        );
        append_part(&mut message, domain);
        append_part(&mut message, world_alias.as_bytes());
        for part in parts {
            append_part(&mut message, part);
        }
        let pseudonym = hex_lower(&hmac_sha256(&self.key, &message));
        message.fill(0);
        Ok(pseudonym)
    }
}

impl fmt::Debug for Pseudonymizer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl Drop for Pseudonymizer {
    fn drop(&mut self) {
        self.key.fill(0);
    }
}

pub fn sanitize_player(
    selected: SelectedPlayerObservation,
    pseudonymizer: &Pseudonymizer,
    world_alias: &str,
) -> Result<SafePlayerObservation, RestError> {
    let subject_id = pseudonymizer.pseudonym(
        PLAYER_DOMAIN,
        world_alias,
        &[
            selected.selector_kind.domain_tag(),
            &selected.raw_selected_id,
        ],
    )?;
    Ok(SafePlayerObservation {
        subject_id,
        x: selected.x,
        y: selected.y,
        z: selected.z,
        heading_degrees: selected.heading_degrees,
        server_fps: selected.server_fps,
        average_server_fps: selected.average_server_fps,
        actor_count: selected.actor_count,
    })
}

pub fn sanitize_server_info(
    raw: RawServerInfo,
    pseudonymizer: &Pseudonymizer,
    world_alias: &str,
) -> Result<SanitizedServerInfo, RestError> {
    let server_subject_id =
        pseudonymizer.pseudonym(SERVER_DOMAIN, world_alias, &[&raw.world_guid])?;
    Ok(SanitizedServerInfo {
        version: raw.version.clone(),
        server_subject_id,
    })
}

fn validate_world_alias(world_alias: &str) -> Result<(), RestError> {
    if world_alias.is_empty()
        || world_alias.len() > 256
        || world_alias.chars().any(char::is_control)
    {
        return Err(RestError::InvalidWorldAlias);
    }
    Ok(())
}

fn append_part(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value);
}

fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut inner_pad = [0x36_u8; HMAC_BLOCK_BYTES];
    let mut outer_pad = [0x5c_u8; HMAC_BLOCK_BYTES];
    for (index, byte) in key.iter().enumerate() {
        inner_pad[index] ^= byte;
        outer_pad[index] ^= byte;
    }

    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let mut inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);
    let result = outer.finalize().into();

    inner_pad.fill(0);
    outer_pad.fill(0);
    inner_digest.fill(0);
    result
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
