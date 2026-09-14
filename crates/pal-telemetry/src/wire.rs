use pal_protocol::v2::{
    ClockProbeRequest, ClockProbeResponse, CoordinateSpace, GetDescriptorRequest,
    GetDescriptorResponse, HeadingSource, SubscribeRequest, SubscribeResponse,
};
use prost::Message;
use thiserror::Error;

pub const MAX_DECODED_MESSAGE_SIZE: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("unsupported protocol version")]
    ProtocolVersion,
    #[error("invalid telemetry identity")]
    Identity,
    #[error("invalid telemetry cursor")]
    Cursor,
    #[error("invalid telemetry sequence")]
    Sequence,
    #[error("invalid telemetry position")]
    Position,
    #[error("invalid telemetry heading")]
    Heading,
    #[error("invalid telemetry timestamp")]
    Timestamp,
    #[error("invalid canonical digest")]
    Digest,
    #[error("unsupported gate interval")]
    GateInterval,
    #[error("unsupported coordinate profile")]
    CoordinateProfile,
    #[error("invalid clock nonce")]
    ClockNonce,
    #[error("telemetry message exceeds decode limit")]
    MessageTooLarge,
    #[error("malformed telemetry message")]
    Decode,
}

pub fn validate_descriptor_request(value: &GetDescriptorRequest) -> Result<(), ValidationError> {
    validate_protocol(value.protocol_version)
}

pub fn validate_subscribe_request(value: &SubscribeRequest) -> Result<(), ValidationError> {
    validate_protocol(value.protocol_version)?;
    validate_world_alias(&value.world_alias)?;
    validate_len(&value.subject_id, 32)?;
    if let Some(resume) = &value.resume {
        validate_len(&resume.boot_id, 16)?;
        if resume.sequence == 0 {
            return Err(ValidationError::Cursor);
        }
    }
    Ok(())
}

pub fn validate_clock_probe_request(value: &ClockProbeRequest) -> Result<(), ValidationError> {
    validate_protocol(value.protocol_version)?;
    if value.nonce.len() != 16 {
        return Err(ValidationError::ClockNonce);
    }
    validate_timestamp(value.client_send_unix_ms)
}

pub fn validate_clock_probe_response(value: &ClockProbeResponse) -> Result<(), ValidationError> {
    if value.nonce.len() != 16 {
        return Err(ValidationError::ClockNonce);
    }
    validate_timestamp(value.client_send_unix_ms)?;
    validate_timestamp(value.agent_receive_unix_ms)?;
    validate_timestamp(value.agent_send_unix_ms)?;
    if value.agent_send_unix_ms < value.agent_receive_unix_ms {
        return Err(ValidationError::Timestamp);
    }
    Ok(())
}

pub fn validate_descriptor(
    value: &GetDescriptorResponse,
    expected_coordinate_profile_sha256: &str,
) -> Result<(), ValidationError> {
    validate_protocol(value.protocol_version)?;
    validate_world_alias(&value.world_alias)?;
    if value.agent_version.is_empty() || value.rest_server_version.is_empty() {
        return Err(ValidationError::Identity);
    }
    validate_canonical_sha256(&value.server_fingerprint)?;
    validate_canonical_sha256(&value.gate_profile_sha256)?;
    if !matches!(value.selected_interval_ms, 2_000 | 1_000 | 500) {
        return Err(ValidationError::GateInterval);
    }
    validate_coordinate_binding(
        value.coordinate_space,
        &value.coordinate_profile_sha256,
        expected_coordinate_profile_sha256,
    )
}

pub fn validate_envelope(
    value: &SubscribeResponse,
    expected_coordinate_profile_sha256: &str,
) -> Result<(), ValidationError> {
    validate_protocol(value.protocol_version)?;
    validate_world_alias(&value.world_alias)?;
    validate_len(&value.subject_id, 32)?;
    validate_len(&value.boot_id, 16)?;
    validate_len(&value.trace_id, 16)?;
    if value.sequence == 0 {
        return Err(ValidationError::Sequence);
    }
    validate_timestamp(value.rest_completed_at_unix_ms)?;
    if !value.position_x.is_finite()
        || !value.position_y.is_finite()
        || !value.position_z.is_finite()
    {
        return Err(ValidationError::Position);
    }
    match (
        value.heading_degrees,
        HeadingSource::try_from(value.heading_source).ok(),
    ) {
        (None, Some(HeadingSource::Unspecified)) => {}
        (Some(heading), Some(source))
            if heading.is_finite()
                && (0.0..360.0).contains(&heading)
                && source != HeadingSource::Unspecified => {}
        _ => return Err(ValidationError::Heading),
    }
    validate_coordinate_binding(
        value.coordinate_space,
        &value.coordinate_profile_sha256,
        expected_coordinate_profile_sha256,
    )
}

pub fn decode_envelope(bytes: &[u8]) -> Result<SubscribeResponse, ValidationError> {
    if bytes.len() > MAX_DECODED_MESSAGE_SIZE {
        return Err(ValidationError::MessageTooLarge);
    }
    SubscribeResponse::decode(bytes).map_err(|_| ValidationError::Decode)
}

fn validate_protocol(protocol_version: u32) -> Result<(), ValidationError> {
    if protocol_version == pal_domain::PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ValidationError::ProtocolVersion)
    }
}

fn validate_world_alias(value: &str) -> Result<(), ValidationError> {
    if !value.is_empty() && value.len() <= 64 && value.is_ascii() {
        Ok(())
    } else {
        Err(ValidationError::Identity)
    }
}

fn validate_len(value: &[u8], expected: usize) -> Result<(), ValidationError> {
    if value.len() == expected {
        Ok(())
    } else {
        Err(ValidationError::Identity)
    }
}

fn validate_timestamp(value: i64) -> Result<(), ValidationError> {
    if value > 0 {
        Ok(())
    } else {
        Err(ValidationError::Timestamp)
    }
}

fn validate_coordinate_binding(
    coordinate_space: i32,
    profile: &str,
    expected_profile: &str,
) -> Result<(), ValidationError> {
    validate_canonical_sha256(expected_profile)?;
    if CoordinateSpace::try_from(coordinate_space).ok()
        != Some(CoordinateSpace::OfficialGameDataWorldV1)
    {
        return Err(ValidationError::CoordinateProfile);
    }
    validate_canonical_sha256(profile)?;
    if profile == expected_profile {
        Ok(())
    } else {
        Err(ValidationError::CoordinateProfile)
    }
}

pub(crate) fn validate_canonical_sha256(value: &str) -> Result<(), ValidationError> {
    if value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        Ok(())
    } else {
        Err(ValidationError::Digest)
    }
}
