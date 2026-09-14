use std::{
    fs,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use pal_protocol::v2::{CoordinateSpace, GetDescriptorResponse};
use pal_rest::SanitizedServerInfo;
use pal_rest_probe::{
    GATE_A_CANDIDATE_INTERVALS_MS, GATE_A_PAIR_COUNT, GATE_A_REPORT_SCHEMA_VERSION,
    GATE_A_WINDOW_DURATION_MS, GateAEvidence, GateAReport, GateDecision, PairOrder, ProbeRunner,
    ServerFingerprintInput, evaluate,
};
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const PROFILE_SCHEMA_VERSION: u32 = 1;
const PROFILE_AUTHORITY: &str = "local_host_adapter_v1";
const MAX_PROFILE_BYTES: usize = 1024 * 1024;
const MAX_PROFILE_LIFETIME: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateProfilePayload {
    pub schema_version: u32,
    pub authority: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub process_id: u32,
    pub process_creation_time_100ns: u64,
    pub rest_server_version: String,
    pub steam_manifest_id: Option<u64>,
    pub executable_sha256: String,
    pub server_subject_id: String,
    pub rotation_z_validated: bool,
    pub report: GateAReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestedGateProfile {
    pub payload: GateProfilePayload,
    pub signature_ed25519: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupGateConfig {
    pub expected_profile_sha256: String,
    pub coordinate_profile_sha256: String,
    pub selected_interval_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedGate {
    pub gate_profile_sha256: String,
    pub coordinate_profile_sha256: String,
    pub selected_interval: Duration,
    pub server_fingerprint: String,
    pub rest_server_version: String,
    pub server_subject_id: [u8; 32],
    pub rotation_z_validated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreverifiedGate {
    payload: GateProfilePayload,
    gate_profile_sha256: String,
    coordinate_profile_sha256: String,
    selected_interval: Duration,
}

impl PreverifiedGate {
    pub const fn process_id(&self) -> u32 {
        self.payload.process_id
    }
}

pub fn parse_gate_profile(bytes: &[u8]) -> Result<AttestedGateProfile, GateProfileError> {
    if bytes.is_empty() || bytes.len() > MAX_PROFILE_BYTES {
        return Err(GateProfileError::InvalidProfile);
    }
    serde_json::from_slice(bytes).map_err(|_| GateProfileError::InvalidProfile)
}

pub fn load_gate_profile(path: &std::path::Path) -> Result<AttestedGateProfile, GateProfileError> {
    if fs::symlink_metadata(path).is_err() {
        return Err(GateProfileError::Missing);
    }
    let bytes = crate::read_protected_file(path, MAX_PROFILE_BYTES)
        .map_err(|_| GateProfileError::InvalidProfile)?;
    parse_gate_profile(&bytes)
}

pub fn canonical_profile_sha256(payload: &GateProfilePayload) -> Result<String, GateProfileError> {
    let canonical = serde_json::to_vec(payload).map_err(|_| GateProfileError::InvalidProfile)?;
    Ok(hex_lower(&Sha256::digest(canonical)))
}

pub fn enforce_startup_gate(
    config: &StartupGateConfig,
    profile: &AttestedGateProfile,
    verification_key: &[u8; 32],
    current_info: &SanitizedServerInfo,
    live_server: &crate::LiveServerIdentity,
    now: SystemTime,
) -> Result<ApprovedGate, GateProfileError> {
    let preverified = preverify_startup_gate(config, profile, verification_key, now)?;
    finalize_startup_gate(&preverified, current_info, live_server, now)
}

pub fn preverify_startup_gate(
    config: &StartupGateConfig,
    profile: &AttestedGateProfile,
    verification_key: &[u8; 32],
    now: SystemTime,
) -> Result<PreverifiedGate, GateProfileError> {
    let payload_bytes =
        serde_json::to_vec(&profile.payload).map_err(|_| GateProfileError::InvalidProfile)?;
    let signature =
        decode_ed25519_signature(&profile.signature_ed25519).ok_or(GateProfileError::Unattested)?;
    UnparsedPublicKey::new(&ED25519, verification_key)
        .verify(&payload_bytes, &signature)
        .map_err(|_| GateProfileError::Unattested)?;

    validate_payload_shape(&profile.payload, now)?;
    let profile_sha256 = hex_lower(&Sha256::digest(&payload_bytes));
    if !is_canonical_sha256(&config.expected_profile_sha256)
        || profile_sha256 != config.expected_profile_sha256
    {
        return Err(GateProfileError::ProfileHashMismatch);
    }
    if !is_canonical_sha256(&config.coordinate_profile_sha256) {
        return Err(GateProfileError::CoordinateProfileMismatch);
    }
    let selected_interval = match config.selected_interval_ms {
        500 | 1_000 | 2_000 => Duration::from_millis(config.selected_interval_ms),
        _ => return Err(GateProfileError::IntervalMismatch),
    };
    if profile.payload.report.selected_interval_ms != Some(config.selected_interval_ms) {
        return Err(GateProfileError::IntervalMismatch);
    }

    Ok(PreverifiedGate {
        payload: profile.payload.clone(),
        gate_profile_sha256: profile_sha256,
        coordinate_profile_sha256: config.coordinate_profile_sha256.clone(),
        selected_interval,
    })
}

pub fn bind_preverified_gate(
    preverified: &PreverifiedGate,
    live_server: &crate::LiveServerIdentity,
) -> Result<(), GateProfileError> {
    if preverified.payload.process_id != live_server.process_id() {
        return Err(GateProfileError::ProcessIdentityMismatch);
    }
    if preverified.payload.process_creation_time_100ns != live_server.creation_time_100ns() {
        return Err(GateProfileError::ProcessCreationMismatch);
    }
    let measured_executable = live_server.executable_sha256();
    let expected_executable = decode_nonzero_sha256(&preverified.payload.executable_sha256)
        .ok_or(GateProfileError::InvalidProfile)?;
    if measured_executable != expected_executable {
        return Err(GateProfileError::ExecutableMismatch);
    }
    Ok(())
}

pub fn finalize_startup_gate(
    preverified: &PreverifiedGate,
    current_info: &SanitizedServerInfo,
    live_server: &crate::LiveServerIdentity,
    now: SystemTime,
) -> Result<ApprovedGate, GateProfileError> {
    let payload = &preverified.payload;
    // Startup may spend time hashing the live executable and waiting for `/info`.
    // Recheck the signed validity window at the final approval boundary.
    validate_payload_shape(payload, now)?;
    bind_preverified_gate(preverified, live_server)?;
    finalize_info_binding(preverified, current_info, live_server.executable_sha256())
}

/// Finalizes a signed Gate A profile for an explicitly allowed remote HTTPS
/// endpoint.
///
/// The executable measurement and load evidence were collected by the signed
/// host-side Gate A profile. The remote client therefore binds the current
/// endpoint by TLS plus `/info` version and World GUID pseudonym rather than by
/// inspecting a process on this machine.
pub fn finalize_remote_startup_gate(
    preverified: &PreverifiedGate,
    current_info: &SanitizedServerInfo,
    now: SystemTime,
) -> Result<ApprovedGate, GateProfileError> {
    let payload = &preverified.payload;
    validate_payload_shape(payload, now)?;
    let attested_executable = decode_nonzero_sha256(&payload.executable_sha256)
        .ok_or(GateProfileError::InvalidProfile)?;
    finalize_info_binding(preverified, current_info, attested_executable)
}

fn finalize_info_binding(
    preverified: &PreverifiedGate,
    current_info: &SanitizedServerInfo,
    executable_sha256: [u8; 32],
) -> Result<ApprovedGate, GateProfileError> {
    let payload = &preverified.payload;
    if current_info.version != payload.rest_server_version {
        return Err(GateProfileError::RestServerVersionMismatch);
    }
    let server_subject_id = decode_nonzero_sha256(&current_info.server_subject_id)
        .ok_or(GateProfileError::ServerSubjectMismatch)?;
    if payload.server_subject_id != current_info.server_subject_id {
        return Err(GateProfileError::ServerSubjectMismatch);
    }

    let current_preflight = ProbeRunner::preflight(&ServerFingerprintInput {
        rest_version: current_info.version.clone(),
        steam_manifest_id: payload.steam_manifest_id,
        executable_sha256,
        server_subject_id,
        executable_hash_verified: true,
        endpoint_private_lan: true,
        auth_ok: true,
        info_ok: true,
        privacy_boundary_ok: true,
    })
    .map_err(|_| GateProfileError::FingerprintMismatch)?;
    if current_preflight.server_fingerprint != payload.report.server_fingerprint {
        return Err(GateProfileError::FingerprintMismatch);
    }

    Ok(ApprovedGate {
        gate_profile_sha256: preverified.gate_profile_sha256.clone(),
        coordinate_profile_sha256: preverified.coordinate_profile_sha256.clone(),
        selected_interval: preverified.selected_interval,
        server_fingerprint: hex_lower(payload.report.server_fingerprint.as_digest()),
        rest_server_version: current_info.version.clone(),
        server_subject_id,
        rotation_z_validated: payload.rotation_z_validated,
    })
}

pub fn build_agent_descriptor(
    world_alias: &str,
    approved: &ApprovedGate,
) -> Result<GetDescriptorResponse, GateProfileError> {
    if world_alias.is_empty()
        || world_alias.len() > 64
        || !world_alias.is_ascii()
        || !is_canonical_sha256(&approved.server_fingerprint)
        || !is_canonical_sha256(&approved.gate_profile_sha256)
        || !is_canonical_sha256(&approved.coordinate_profile_sha256)
    {
        return Err(GateProfileError::NonCanonical);
    }
    let selected_interval_ms = u32::try_from(approved.selected_interval.as_millis())
        .map_err(|_| GateProfileError::IntervalMismatch)?;
    let descriptor = GetDescriptorResponse {
        protocol_version: pal_domain::PROTOCOL_VERSION,
        agent_version: env!("CARGO_PKG_VERSION").to_owned(),
        rest_server_version: approved.rest_server_version.clone(),
        server_fingerprint: approved.server_fingerprint.clone(),
        gate_profile_sha256: approved.gate_profile_sha256.clone(),
        selected_interval_ms,
        world_alias: world_alias.to_owned(),
        coordinate_space: CoordinateSpace::OfficialGameDataWorldV1 as i32,
        coordinate_profile_sha256: approved.coordinate_profile_sha256.clone(),
    };
    pal_telemetry::validate_descriptor(&descriptor, &approved.coordinate_profile_sha256)
        .map_err(|_| GateProfileError::NonCanonical)?;
    Ok(descriptor)
}

fn validate_payload_shape(
    payload: &GateProfilePayload,
    now: SystemTime,
) -> Result<(), GateProfileError> {
    if payload.schema_version != PROFILE_SCHEMA_VERSION
        || payload.authority != PROFILE_AUTHORITY
        || payload.process_id == 0
        || payload.process_creation_time_100ns == 0
        || payload.rest_server_version.is_empty()
        || payload.rest_server_version.len() > 128
        || !payload.rest_server_version.is_ascii()
        || !is_canonical_sha256(&payload.executable_sha256)
        || decode_nonzero_sha256(&payload.server_subject_id).is_none()
    {
        return Err(GateProfileError::NonCanonical);
    }
    let now_ms = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| GateProfileError::Expired)?
        .as_millis();
    let now_ms = u64::try_from(now_ms).map_err(|_| GateProfileError::Expired)?;
    let lifetime_ms = payload
        .expires_at_unix_ms
        .checked_sub(payload.issued_at_unix_ms)
        .ok_or(GateProfileError::Expired)?;
    if payload.issued_at_unix_ms > now_ms
        || payload.expires_at_unix_ms <= now_ms
        || lifetime_ms == 0
        || lifetime_ms > MAX_PROFILE_LIFETIME.as_millis() as u64
    {
        return Err(GateProfileError::Expired);
    }
    validate_canonical_report(payload)
}

fn validate_canonical_report(payload: &GateProfilePayload) -> Result<(), GateProfileError> {
    let report = &payload.report;
    if report.schema_version != GATE_A_REPORT_SCHEMA_VERSION
        || report.decision != GateDecision::Go
        || report.failure_reasons.iter().any(|reason| {
            matches!(
                reason,
                pal_rest_probe::GateFailureReason::UnattestedEvidence
            )
        })
        || !report.failure_reasons.is_empty()
        || report.selected_interval_ms.is_none()
        || report.candidates.len() != GATE_A_CANDIDATE_INTERVALS_MS.len()
    {
        return Err(GateProfileError::NonCanonical);
    }
    for (candidate, expected_interval) in
        report.candidates.iter().zip(GATE_A_CANDIDATE_INTERVALS_MS)
    {
        if candidate.interval_ms != expected_interval
            || candidate.pairs.len() != GATE_A_PAIR_COUNT
            || candidate.safety_abort.is_some()
        {
            return Err(GateProfileError::NonCanonical);
        }
        for (index, pair) in candidate.pairs.iter().enumerate() {
            let expected_order = if index % 2 == 0 {
                PairOrder::BaselineThenCandidate
            } else {
                PairOrder::CandidateThenBaseline
            };
            if pair.pair_index != index as u32
                || pair.order != expected_order
                || pair.baseline.duration_ms != GATE_A_WINDOW_DURATION_MS
                || pair.candidate.duration_ms != GATE_A_WINDOW_DURATION_MS
            {
                return Err(GateProfileError::NonCanonical);
            }
        }
    }
    let reconstructed = evaluate(&GateAEvidence {
        schema_version: report.schema_version,
        preflight: report.preflight.clone(),
        candidates: report.candidates.clone(),
        rotation: report.rotation.clone(),
        movement: report.movement.clone(),
        privacy: report.privacy,
    });
    if reconstructed != *report {
        return Err(GateProfileError::NonCanonical);
    }
    if payload.rotation_z_validated
        && (report.rotation.present_samples == 0 || report.rotation.error_degrees.is_none())
    {
        return Err(GateProfileError::NonCanonical);
    }
    Ok(())
}

fn is_canonical_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn decode_nonzero_sha256(value: &str) -> Option<[u8; 32]> {
    let decoded = decode_sha256(value)?;
    (decoded != [0; 32]).then_some(decoded)
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

fn decode_ed25519_signature(value: &str) -> Option<[u8; 64]> {
    if value.len() != 128
        || !value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return None;
    }
    let mut output = [0_u8; 64];
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

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum GateProfileError {
    #[error("Gate A profile is missing")]
    Missing,
    #[error("Gate A profile is invalid")]
    InvalidProfile,
    #[error("Gate A profile signature is invalid")]
    Unattested,
    #[error("Gate A profile is not canonical")]
    NonCanonical,
    #[error("Gate A profile is expired")]
    Expired,
    #[error("configured Gate A profile hash does not match")]
    ProfileHashMismatch,
    #[error("configured coordinate profile is invalid")]
    CoordinateProfileMismatch,
    #[error("configured Gate A interval does not match")]
    IntervalMismatch,
    #[error("running REST server version does not match Gate A")]
    RestServerVersionMismatch,
    #[error("running server subject does not match Gate A")]
    ServerSubjectMismatch,
    #[error("running server process ID does not match Gate A")]
    ProcessIdentityMismatch,
    #[error("running server process creation time does not match Gate A")]
    ProcessCreationMismatch,
    #[error("running server executable does not match Gate A")]
    ExecutableMismatch,
    #[error("running server fingerprint does not match Gate A")]
    FingerprintMismatch,
}
