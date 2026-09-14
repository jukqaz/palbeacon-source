//! Headless collection state for the Build 24181527 one-point alignment smoke.
//!
//! This module owns no window, renderer, process access, or output. It only
//! reduces already accepted position-source events into the strict Slice A
//! evaluator input.

use pal_domain::{Freshness, classify_freshness};
use pal_state::PositionSourceEvent;
use thiserror::Error;

use crate::{
    actual_map_preview::authoritative_main_map_world_to_image,
    local_alignment_diagnostic::{
        AlignmentSample, LocalAlignmentDiagnosticError, LocalAlignmentObservation,
        LocalAlignmentResult, evaluate_local_alignment,
    },
};

const EXACT_MAP_WIDTH_PX: u32 = 2_048;
const EXACT_MAP_HEIGHT_PX: u32 = 2_048;
const REQUIRED_SAMPLE_COUNT: usize = 10;

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum LocalAlignmentCollectionError {
    #[error("the alignment source emitted a sample before connecting")]
    SampleBeforeConnection,
    #[error("the alignment source connection changed during collection")]
    ConnectionChanged,
    #[error("the alignment source terminated during collection")]
    SourceTerminated,
    #[error("the alignment sample is not live")]
    SampleNotLive,
    #[error("the alignment sample is missing yaw")]
    SampleMissingYaw,
    #[error("the alignment sample is outside the authoritative MainMap")]
    SampleOutsideMainMap,
    #[error("the alignment collection is already complete")]
    AlreadyComplete,
    #[error("the alignment samples failed strict evaluation: {0}")]
    Diagnostic(#[from] LocalAlignmentDiagnosticError),
}

#[derive(Clone, Debug, PartialEq)]
pub enum LocalAlignmentCollectionUpdate {
    Pending,
    Complete(LocalAlignmentResult),
}

pub struct LocalAlignmentCollector {
    observation: LocalAlignmentObservation,
    connection_generation: Option<u64>,
    samples: Vec<AlignmentSample>,
    complete: bool,
}

impl LocalAlignmentCollector {
    pub fn new(observation: LocalAlignmentObservation) -> Self {
        Self {
            observation,
            connection_generation: None,
            samples: Vec::with_capacity(REQUIRED_SAMPLE_COUNT),
            complete: false,
        }
    }

    pub fn consume(
        &mut self,
        event: Option<PositionSourceEvent>,
        now_monotonic_ms: u64,
    ) -> Result<LocalAlignmentCollectionUpdate, LocalAlignmentCollectionError> {
        if self.complete {
            return Err(LocalAlignmentCollectionError::AlreadyComplete);
        }
        let Some(event) = event else {
            return Ok(LocalAlignmentCollectionUpdate::Pending);
        };

        match event {
            PositionSourceEvent::Connected { generation } => {
                if self
                    .connection_generation
                    .is_some_and(|active| active != generation)
                {
                    return Err(LocalAlignmentCollectionError::ConnectionChanged);
                }
                self.connection_generation = Some(generation);
                Ok(LocalAlignmentCollectionUpdate::Pending)
            }
            PositionSourceEvent::Unavailable { .. }
            | PositionSourceEvent::Disconnected { .. }
            | PositionSourceEvent::ClockInvalid { .. } => {
                Err(LocalAlignmentCollectionError::SourceTerminated)
            }
            PositionSourceEvent::Sample(sample) => {
                let Some(generation) = self.connection_generation else {
                    return Err(LocalAlignmentCollectionError::SampleBeforeConnection);
                };
                if sample.source_connection_generation() != generation {
                    return Err(LocalAlignmentCollectionError::ConnectionChanged);
                }

                let freshness =
                    classify_freshness(sample.clock().age_upper_bound_ms(now_monotonic_ms), true);
                if freshness != Freshness::Live {
                    return Err(LocalAlignmentCollectionError::SampleNotLive);
                }
                if sample.heading_degrees().is_none() {
                    return Err(LocalAlignmentCollectionError::SampleMissingYaw);
                }

                let normalized = authoritative_main_map_world_to_image()
                    .project_within_bounds(sample.x(), sample.y())
                    .filter(|point| {
                        (0.0..1.0).contains(&point.x()) && (0.0..1.0).contains(&point.y())
                    })
                    .ok_or(LocalAlignmentCollectionError::SampleOutsideMainMap)?;
                let projected_x_px = normalized.x() * f64::from(EXACT_MAP_WIDTH_PX);
                let projected_y_px = normalized.y() * f64::from(EXACT_MAP_HEIGHT_PX);
                self.samples.push(AlignmentSample::new(
                    projected_x_px,
                    projected_y_px,
                    sample.clock().received_at_monotonic_ms,
                    freshness,
                    true,
                ));

                if self.samples.len() < REQUIRED_SAMPLE_COUNT {
                    return Ok(LocalAlignmentCollectionUpdate::Pending);
                }
                if self.samples.len() > REQUIRED_SAMPLE_COUNT {
                    return Err(LocalAlignmentCollectionError::AlreadyComplete);
                }

                let result = evaluate_local_alignment(&self.observation, &self.samples)?;
                self.complete = true;
                Ok(LocalAlignmentCollectionUpdate::Complete(result))
            }
        }
    }

    pub fn accepted_sample_count(&self) -> usize {
        self.samples.len()
    }
}
