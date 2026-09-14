use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    Live,
    Delayed,
    Stale,
    Offline,
}

pub fn classify_freshness(sample_age_upper_bound_ms: u64, connected: bool) -> Freshness {
    if !connected {
        return Freshness::Offline;
    }

    match sample_age_upper_bound_ms {
        ..=1_500 => Freshness::Live,
        1_501..=5_000 => Freshness::Delayed,
        _ => Freshness::Stale,
    }
}

pub fn shortest_angle_lerp(from: f32, to: f32, t: f32) -> f32 {
    let delta = (to - from + 540.0).rem_euclid(360.0) - 180.0;
    (from + delta * t.clamp(0.0, 1.0)).rem_euclid(360.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleClock {
    pub age_at_receive_upper_bound_ms: u64,
    pub received_at_monotonic_ms: u64,
}

impl SampleClock {
    pub const fn received_with_age(
        age_at_receive_upper_bound_ms: u64,
        received_at_monotonic_ms: u64,
    ) -> Self {
        Self {
            age_at_receive_upper_bound_ms,
            received_at_monotonic_ms,
        }
    }

    pub const fn age_upper_bound_ms(self, now_monotonic_ms: u64) -> u64 {
        self.age_at_receive_upper_bound_ms
            .saturating_add(now_monotonic_ms.saturating_sub(self.received_at_monotonic_ms))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionValidationError {
    EmptyWorldAlias,
    EmptySubjectId,
    EmptyAgentBootId,
    NonFiniteX,
    NonFiniteY,
    NonFiniteZ,
    NonFiniteHeading,
}

impl PositionValidationError {
    pub const fn field(self) -> &'static str {
        match self {
            Self::EmptyWorldAlias => "world_alias",
            Self::EmptySubjectId => "subject_id",
            Self::EmptyAgentBootId => "agent_boot_id",
            Self::NonFiniteX => "x",
            Self::NonFiniteY => "y",
            Self::NonFiniteZ => "z",
            Self::NonFiniteHeading => "heading_degrees",
        }
    }
}

impl fmt::Display for PositionValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyWorldAlias | Self::EmptySubjectId | Self::EmptyAgentBootId => {
                write!(formatter, "{} must not be empty", self.field())
            }
            Self::NonFiniteX | Self::NonFiniteY | Self::NonFiniteZ | Self::NonFiniteHeading => {
                write!(formatter, "{} must be finite", self.field())
            }
        }
    }
}

impl Error for PositionValidationError {}

#[derive(Clone, Debug, PartialEq)]
pub struct PositionSample {
    world_alias: String,
    subject_id: Vec<u8>,
    agent_boot_id: Vec<u8>,
    source_connection_generation: u64,
    sequence: u64,
    x: f64,
    y: f64,
    z: f64,
    heading_degrees: Option<f32>,
    clock: SampleClock,
}

impl PositionSample {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        world_alias: impl Into<String>,
        subject_id: impl AsRef<[u8]>,
        agent_boot_id: impl AsRef<[u8]>,
        source_connection_generation: u64,
        sequence: u64,
        x: f64,
        y: f64,
        z: f64,
        heading_degrees: Option<f32>,
        clock: SampleClock,
    ) -> Result<Self, PositionValidationError> {
        let world_alias = world_alias.into();
        let subject_id = subject_id.as_ref().to_vec();
        let agent_boot_id = agent_boot_id.as_ref().to_vec();

        if world_alias.is_empty() {
            return Err(PositionValidationError::EmptyWorldAlias);
        }
        if subject_id.is_empty() {
            return Err(PositionValidationError::EmptySubjectId);
        }
        if agent_boot_id.is_empty() {
            return Err(PositionValidationError::EmptyAgentBootId);
        }
        if !x.is_finite() {
            return Err(PositionValidationError::NonFiniteX);
        }
        if !y.is_finite() {
            return Err(PositionValidationError::NonFiniteY);
        }
        if !z.is_finite() {
            return Err(PositionValidationError::NonFiniteZ);
        }
        if heading_degrees.is_some_and(|heading| !heading.is_finite()) {
            return Err(PositionValidationError::NonFiniteHeading);
        }

        Ok(Self {
            world_alias,
            subject_id,
            agent_boot_id,
            source_connection_generation,
            sequence,
            x,
            y,
            z,
            heading_degrees: heading_degrees.map(|heading| heading.rem_euclid(360.0)),
            clock,
        })
    }

    pub fn world_alias(&self) -> &str {
        &self.world_alias
    }

    pub fn subject_id(&self) -> &[u8] {
        &self.subject_id
    }

    pub fn agent_boot_id(&self) -> &[u8] {
        &self.agent_boot_id
    }

    pub const fn source_connection_generation(&self) -> u64 {
        self.source_connection_generation
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn x(&self) -> f64 {
        self.x
    }

    pub const fn y(&self) -> f64 {
        self.y
    }

    pub const fn z(&self) -> f64 {
        self.z
    }

    pub const fn heading_degrees(&self) -> Option<f32> {
        self.heading_degrees
    }

    pub const fn clock(&self) -> SampleClock {
        self.clock
    }
}
