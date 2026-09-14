use std::{
    fmt,
    time::{Instant, UNIX_EPOCH},
};

use pal_protocol::v2::{CoordinateSpace, HeadingSource, SubscribeResponse};
use pal_rest::{SafePlayerObservation, TimedResponse};
use pal_telemetry::{
    LatestConnectOutcome, LatestPublishOutcome, LatestTelemetry, LatestTelemetryError,
};
use thiserror::Error;

use crate::HealthMonitor;

pub struct AgentPipeline {
    world_alias: String,
    subject_id: [u8; 32],
    coordinate_profile_sha256: String,
    rotation_z_validated: bool,
    boot_id: [u8; 16],
    next_sequence: u64,
    generation: u64,
    latest: LatestTelemetry,
    health: HealthMonitor,
}

impl fmt::Debug for AgentPipeline {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED AGENT PIPELINE]")
    }
}

impl AgentPipeline {
    pub fn new(
        world_alias: impl Into<String>,
        subject_id: impl AsRef<[u8]>,
        coordinate_profile_sha256: impl Into<String>,
        rotation_z_validated: bool,
        latest: LatestTelemetry,
        health: HealthMonitor,
    ) -> Result<Self, PipelineError> {
        let world_alias = world_alias.into();
        let subject_id: [u8; 32] = subject_id
            .as_ref()
            .try_into()
            .map_err(|_| PipelineError::InvalidConfiguration)?;
        let coordinate_profile_sha256 = coordinate_profile_sha256.into();
        if !latest.is_bound_to(&world_alias, &subject_id, &coordinate_profile_sha256) {
            return Err(PipelineError::InvalidConfiguration);
        }
        let mut boot_id = [0_u8; 16];
        getrandom::fill(&mut boot_id).map_err(|_| PipelineError::RandomUnavailable)?;
        if boot_id == [0; 16] {
            return Err(PipelineError::RandomUnavailable);
        }
        let generation = 1;
        if latest.connect(generation) != LatestConnectOutcome::Accepted {
            return Err(PipelineError::PublisherUnavailable);
        }
        Ok(Self {
            world_alias,
            subject_id,
            coordinate_profile_sha256,
            rotation_z_validated,
            boot_id,
            next_sequence: 1,
            generation,
            latest,
            health,
        })
    }

    pub fn emit(
        &mut self,
        response: TimedResponse<SafePlayerObservation>,
    ) -> Result<(), PipelineError> {
        let result = self.emit_inner(response);
        if result.is_err() {
            self.health.record_publish_failure();
        }
        result
    }

    fn emit_inner(
        &mut self,
        response: TimedResponse<SafePlayerObservation>,
    ) -> Result<(), PipelineError> {
        let observed_subject =
            decode_sha256(&response.value.subject_id).ok_or(PipelineError::IdentityMismatch)?;
        if observed_subject != self.subject_id {
            return Err(PipelineError::IdentityMismatch);
        }
        if self.next_sequence == u64::MAX {
            return Err(PipelineError::SequenceExhausted);
        }
        let rest_completed_at_unix_ms = response
            .rest_completed_at
            .duration_since(UNIX_EPOCH)
            .map_err(|_| PipelineError::InvalidTimestamp)?
            .as_millis();
        let rest_completed_at_unix_ms = i64::try_from(rest_completed_at_unix_ms)
            .map_err(|_| PipelineError::InvalidTimestamp)?;
        if rest_completed_at_unix_ms <= 0 {
            return Err(PipelineError::InvalidTimestamp);
        }
        let age_millis = Instant::now()
            .saturating_duration_since(response.completed_monotonic)
            .as_millis();
        let age_at_emit_ms = u32::try_from(age_millis).unwrap_or(u32::MAX);
        let (heading_degrees, heading_source) = if self.rotation_z_validated {
            match response.value.heading_degrees {
                Some(heading) => (Some(heading), HeadingSource::RotationZValidated as i32),
                None => (None, HeadingSource::Unspecified as i32),
            }
        } else {
            (None, HeadingSource::Unspecified as i32)
        };
        let mut trace_id = [0_u8; 16];
        getrandom::fill(&mut trace_id).map_err(|_| PipelineError::RandomUnavailable)?;
        let envelope = SubscribeResponse {
            protocol_version: pal_domain::PROTOCOL_VERSION,
            world_alias: self.world_alias.clone(),
            subject_id: self.subject_id.to_vec(),
            boot_id: self.boot_id.to_vec(),
            sequence: self.next_sequence,
            rest_completed_at_unix_ms,
            age_at_emit_ms,
            position_x: response.value.x,
            position_y: response.value.y,
            position_z: response.value.z,
            heading_degrees,
            heading_source,
            trace_id: trace_id.to_vec(),
            coordinate_space: CoordinateSpace::OfficialGameDataWorldV1 as i32,
            coordinate_profile_sha256: self.coordinate_profile_sha256.clone(),
        };
        match self.latest.publish(self.generation, envelope)? {
            LatestPublishOutcome::Accepted | LatestPublishOutcome::AcceptedWithGap { .. } => {
                self.next_sequence = self.next_sequence.saturating_add(1);
                self.health.record_emitted();
                Ok(())
            }
            _ => Err(PipelineError::PublisherRejected),
        }
    }

    pub const fn boot_id(&self) -> &[u8; 16] {
        &self.boot_id
    }
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64
        || value
            .as_bytes()
            .iter()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(byte))
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

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum PipelineError {
    #[error("agent pipeline configuration is invalid")]
    InvalidConfiguration,
    #[error("secure random source is unavailable")]
    RandomUnavailable,
    #[error("latest telemetry publisher is unavailable")]
    PublisherUnavailable,
    #[error("sanitized player identity did not match configuration")]
    IdentityMismatch,
    #[error("telemetry sequence is exhausted")]
    SequenceExhausted,
    #[error("REST completion timestamp is invalid")]
    InvalidTimestamp,
    #[error("latest telemetry publisher rejected the sample")]
    PublisherRejected,
}

impl From<LatestTelemetryError> for PipelineError {
    fn from(_: LatestTelemetryError) -> Self {
        Self::PublisherUnavailable
    }
}
