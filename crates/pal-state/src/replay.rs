//! File-loaded deterministic replay source.

use std::fs;
use std::path::Path;

use pal_domain::{PositionSample, SampleClock};
use serde::Deserialize;
use serde_json::Number;
use thiserror::Error;

use crate::{ClockInvalidReason, PositionSource, PositionSourceError, PositionSourceEvent};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayConfig {
    pub world_alias: String,
    pub subject_id: Vec<u8>,
    pub starting_generation: u64,
    pub base_boot_id: Vec<u8>,
    pub loop_playback: bool,
    pub starting_monotonic_ms: u64,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ReplayError {
    #[error("replay fixture could not be read")]
    Io,
    #[error("replay fixture is not valid strict JSON")]
    Json,
    #[error("replay fixture must contain at least one row")]
    EmptyFixture,
    #[error("the first replay timestamp must be zero")]
    FirstTimestampNotZero,
    #[error("replay timestamp at row {index} is not strictly increasing")]
    NonIncreasingTimestamp { index: usize },
    #[error("replay field {field} at row {index} must be finite")]
    NonFiniteField { index: usize, field: &'static str },
    #[error("looping replay requires a positive cycle duration")]
    LoopRequiresPositiveDuration,
    #[error("replay connection generation overflowed")]
    GenerationOverflow,
    #[error("replay sequence overflowed")]
    SequenceOverflow,
    #[error("replay identity field {field} is invalid")]
    InvalidIdentity { field: &'static str },
    #[error("replay starting generation must be nonzero")]
    InvalidStartingGeneration,
}

#[derive(Clone, Debug)]
struct ReplayRow {
    at_ms: u64,
    x: f64,
    y: f64,
    z: f64,
    heading_degrees: Option<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReplayRow {
    at_ms: u64,
    x: Number,
    y: Number,
    z: Number,
    heading_degrees: Option<Number>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplayPhase {
    Connected,
    Samples,
    Disconnected,
    NextConnection,
    Finished,
}

#[derive(Debug)]
pub struct ReplayPositionSource {
    rows: Vec<ReplayRow>,
    world_alias: String,
    subject_id: Vec<u8>,
    base_boot_id: Vec<u8>,
    boot_id: Vec<u8>,
    generation: u64,
    loop_playback: bool,
    cycle_duration_ms: u64,
    cycle_start_ms: u64,
    row_index: usize,
    phase: ReplayPhase,
    last_valid_poll_ms: Option<u64>,
    regression_reported: bool,
}

impl ReplayPositionSource {
    pub fn from_path(path: impl AsRef<Path>, config: ReplayConfig) -> Result<Self, ReplayError> {
        validate_config(&config)?;
        let contents = fs::read_to_string(path).map_err(|_| ReplayError::Io)?;
        let raw_rows: Vec<RawReplayRow> =
            serde_json::from_str(&contents).map_err(|_| ReplayError::Json)?;
        let rows = validate_rows(raw_rows)?;
        let cycle_duration_ms = rows.last().expect("rows were validated nonempty").at_ms;
        if config.loop_playback && cycle_duration_ms == 0 {
            return Err(ReplayError::LoopRequiresPositiveDuration);
        }

        Ok(Self {
            rows,
            world_alias: config.world_alias,
            subject_id: config.subject_id,
            boot_id: config.base_boot_id.clone(),
            base_boot_id: config.base_boot_id,
            generation: config.starting_generation,
            loop_playback: config.loop_playback,
            cycle_duration_ms,
            cycle_start_ms: config.starting_monotonic_ms,
            row_index: 0,
            phase: ReplayPhase::Connected,
            last_valid_poll_ms: None,
            regression_reported: false,
        })
    }

    fn poll_valid_time(
        &mut self,
        now_monotonic_ms: u64,
    ) -> Result<Option<PositionSourceEvent>, PositionSourceError> {
        match self.phase {
            ReplayPhase::Connected => {
                self.phase = ReplayPhase::Samples;
                Ok(Some(PositionSourceEvent::Connected {
                    generation: self.generation,
                }))
            }
            ReplayPhase::Samples => self.poll_sample(now_monotonic_ms),
            ReplayPhase::Disconnected => {
                self.phase = if self.loop_playback {
                    ReplayPhase::NextConnection
                } else {
                    ReplayPhase::Finished
                };
                Ok(Some(PositionSourceEvent::Disconnected {
                    generation: self.generation,
                }))
            }
            ReplayPhase::NextConnection => {
                self.start_next_connection()?;
                Ok(Some(PositionSourceEvent::Connected {
                    generation: self.generation,
                }))
            }
            ReplayPhase::Finished => Ok(None),
        }
    }

    fn poll_sample(
        &mut self,
        now_monotonic_ms: u64,
    ) -> Result<Option<PositionSourceEvent>, PositionSourceError> {
        let Some(row) = self.rows.get(self.row_index) else {
            self.phase = ReplayPhase::Disconnected;
            return self.poll_valid_time(now_monotonic_ms);
        };
        let due_ms = self.cycle_start_ms.saturating_add(row.at_ms);
        if now_monotonic_ms < due_ms {
            return Ok(None);
        }
        let sequence = u64::try_from(self.row_index)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or(PositionSourceError::Replay(ReplayError::SequenceOverflow))?;
        let sample = PositionSample::new(
            self.world_alias.clone(),
            &self.subject_id,
            &self.boot_id,
            self.generation,
            sequence,
            row.x,
            row.y,
            row.z,
            row.heading_degrees,
            SampleClock::received_with_age(
                now_monotonic_ms.saturating_sub(due_ms),
                now_monotonic_ms,
            ),
        )
        .expect("constructor configuration and parsed rows were validated");
        self.row_index += 1;
        if self.row_index == self.rows.len() {
            self.phase = ReplayPhase::Disconnected;
        }
        Ok(Some(PositionSourceEvent::Sample(sample)))
    }

    fn start_next_connection(&mut self) -> Result<(), PositionSourceError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(PositionSourceError::Replay(ReplayError::GenerationOverflow))?;
        self.cycle_start_ms = self.cycle_start_ms.saturating_add(self.cycle_duration_ms);
        self.row_index = 0;
        self.boot_id = derived_boot_id(&self.base_boot_id, self.generation);
        self.phase = ReplayPhase::Samples;
        Ok(())
    }
}

impl PositionSource for ReplayPositionSource {
    fn poll(
        &mut self,
        now_monotonic_ms: u64,
    ) -> Result<Option<PositionSourceEvent>, PositionSourceError> {
        if self
            .last_valid_poll_ms
            .is_some_and(|last| now_monotonic_ms < last)
        {
            if self.regression_reported {
                return Ok(None);
            }
            self.regression_reported = true;
            return Ok(Some(PositionSourceEvent::ClockInvalid {
                generation: self.generation,
                reason: ClockInvalidReason::MonotonicRegression,
            }));
        }

        self.last_valid_poll_ms = Some(now_monotonic_ms);
        self.regression_reported = false;
        self.poll_valid_time(now_monotonic_ms)
    }
}

fn validate_config(config: &ReplayConfig) -> Result<(), ReplayError> {
    if config.world_alias.is_empty() {
        return Err(ReplayError::InvalidIdentity {
            field: "world_alias",
        });
    }
    if config.subject_id.is_empty() {
        return Err(ReplayError::InvalidIdentity {
            field: "subject_id",
        });
    }
    if config.base_boot_id.is_empty() {
        return Err(ReplayError::InvalidIdentity {
            field: "base_boot_id",
        });
    }
    if config.starting_generation == 0 {
        return Err(ReplayError::InvalidStartingGeneration);
    }
    Ok(())
}

fn validate_rows(raw_rows: Vec<RawReplayRow>) -> Result<Vec<ReplayRow>, ReplayError> {
    if raw_rows.is_empty() {
        return Err(ReplayError::EmptyFixture);
    }
    if raw_rows[0].at_ms != 0 {
        return Err(ReplayError::FirstTimestampNotZero);
    }

    let mut rows = Vec::with_capacity(raw_rows.len());
    let mut previous_timestamp = 0;
    for (index, raw) in raw_rows.into_iter().enumerate() {
        if index > 0 && raw.at_ms <= previous_timestamp {
            return Err(ReplayError::NonIncreasingTimestamp { index });
        }
        previous_timestamp = raw.at_ms;
        rows.push(ReplayRow {
            at_ms: raw.at_ms,
            x: finite_f64(raw.x, index, "x")?,
            y: finite_f64(raw.y, index, "y")?,
            z: finite_f64(raw.z, index, "z")?,
            heading_degrees: raw
                .heading_degrees
                .map(|value| finite_f32(value, index, "heading_degrees"))
                .transpose()?,
        });
    }
    Ok(rows)
}

fn finite_f64(value: Number, index: usize, field: &'static str) -> Result<f64, ReplayError> {
    let value = value
        .as_f64()
        .ok_or(ReplayError::NonFiniteField { index, field })?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(ReplayError::NonFiniteField { index, field })
    }
}

fn finite_f32(value: Number, index: usize, field: &'static str) -> Result<f32, ReplayError> {
    let value = finite_f64(value, index, field)?;
    let narrowed = value as f32;
    if narrowed.is_finite() {
        Ok(narrowed)
    } else {
        Err(ReplayError::NonFiniteField { index, field })
    }
}

fn derived_boot_id(base: &[u8], generation: u64) -> Vec<u8> {
    let mut boot_id = Vec::with_capacity(base.len() + 9);
    boot_id.extend_from_slice(base);
    boot_id.push(0);
    boot_id.extend_from_slice(&generation.to_be_bytes());
    boot_id
}
