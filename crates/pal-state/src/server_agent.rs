//! Bounded, nonblocking handoff from a server-agent transport into overlay state.

use std::{
    fmt,
    sync::{Arc, Mutex},
};

use pal_domain::{PositionSample, PositionValidationError, SampleClock};
use thiserror::Error;

use crate::{ClockInvalidReason, PositionSource, PositionSourceError, PositionSourceEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerAgentControlOutcome {
    Accepted,
    Dropped,
    OldGenerationDropped,
    QueueFullDropped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerAgentIngestOutcome {
    Accepted,
    Dropped,
    DuplicateDropped,
    OutOfOrderDropped,
    OldGenerationDropped,
    RetiredBootDropped,
    IdentityMismatchDropped,
    BootMismatchDropped,
    InvalidTransitionDropped,
    ClockRegressionDropped,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServerAgentDiagnostics {
    pub accepted_samples: u64,
    pub gap_events: u64,
    pub missing_sequences: u64,
    pub superseded_samples: u64,
    pub duplicate_drops: u64,
    pub out_of_order_drops: u64,
    pub old_generation_drops: u64,
    pub retired_boot_drops: u64,
    pub identity_mismatch_drops: u64,
    pub boot_mismatch_drops: u64,
    pub invalid_transition_drops: u64,
    pub clock_regression_drops: u64,
    pub control_queue_full_drops: u64,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ServerAgentSourceError {
    #[error("position sample validation failed: {0}")]
    Position(#[from] PositionValidationError),
    #[error("expected server-agent identity is invalid")]
    InvalidExpectedIdentity,
}

#[derive(Clone, PartialEq)]
pub struct ServerAgentFrame {
    world_alias: String,
    subject_id: Vec<u8>,
    agent_boot_id: Vec<u8>,
    generation: u64,
    sequence: u64,
    x: f64,
    y: f64,
    z: f64,
    heading_degrees: Option<f32>,
    age_upper_bound_ms: u64,
}

impl fmt::Debug for ServerAgentFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED SERVER-AGENT FRAME]")
    }
}

impl ServerAgentFrame {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        world_alias: impl Into<String>,
        subject_id: impl AsRef<[u8]>,
        agent_boot_id: impl AsRef<[u8]>,
        generation: u64,
        sequence: u64,
        x: f64,
        y: f64,
        z: f64,
        heading_degrees: Option<f32>,
        age_upper_bound_ms: u64,
    ) -> Self {
        Self {
            world_alias: world_alias.into(),
            subject_id: subject_id.as_ref().to_vec(),
            agent_boot_id: agent_boot_id.as_ref().to_vec(),
            generation,
            sequence,
            x,
            y,
            z,
            heading_degrees,
            age_upper_bound_ms,
        }
    }
}

struct Inbox {
    expected_world_alias: String,
    expected_subject_id: Vec<u8>,
    generation: u64,
    connected: bool,
    generation_has_sample: bool,
    active_boot_id: Option<Vec<u8>>,
    retired_boots: RetiredBootFilter,
    last_sequence: Option<u64>,
    last_received_monotonic_ms: Option<u64>,
    pending_controls: ControlQueue,
    pending_sample: Option<PositionSourceEvent>,
    diagnostics: ServerAgentDiagnostics,
}

const CONTROL_QUEUE_CAPACITY: usize = 15;
const CONTROL_EVENTS_PER_SESSION: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ControlEvent {
    Connected {
        generation: u64,
    },
    Disconnected {
        generation: u64,
    },
    ClockInvalid {
        generation: u64,
        reason: ClockInvalidReason,
    },
}

impl ControlEvent {
    const fn into_position_source_event(self) -> PositionSourceEvent {
        match self {
            Self::Connected { generation } => PositionSourceEvent::Connected { generation },
            Self::Disconnected { generation } => PositionSourceEvent::Disconnected { generation },
            Self::ClockInvalid { generation, reason } => {
                PositionSourceEvent::ClockInvalid { generation, reason }
            }
        }
    }
}

struct ControlQueue {
    entries: [Option<ControlEvent>; CONTROL_QUEUE_CAPACITY],
    head: usize,
    len: usize,
}

impl ControlQueue {
    const fn new() -> Self {
        Self {
            entries: [None; CONTROL_QUEUE_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    const fn remaining(&self) -> usize {
        CONTROL_QUEUE_CAPACITY - self.len
    }

    fn push(&mut self, event: ControlEvent) -> bool {
        if self.len == CONTROL_QUEUE_CAPACITY {
            return false;
        }
        let index = (self.head + self.len) % CONTROL_QUEUE_CAPACITY;
        self.entries[index] = Some(event);
        self.len += 1;
        true
    }

    fn pop(&mut self) -> Option<ControlEvent> {
        if self.len == 0 {
            return None;
        }
        let event = self.entries[self.head].take();
        self.head = (self.head + 1) % CONTROL_QUEUE_CAPACITY;
        self.len -= 1;
        event
    }

    fn has_clock_invalid(&self, generation: u64) -> bool {
        (0..self.len).any(|offset| {
            matches!(
                self.entries[(self.head + offset) % CONTROL_QUEUE_CAPACITY],
                Some(ControlEvent::ClockInvalid {
                    generation: queued,
                    ..
                }) if queued == generation
            )
        })
    }
}

#[derive(Clone)]
pub struct ServerAgentSender {
    inbox: Arc<Mutex<Inbox>>,
}

pub struct ServerAgentPositionSource {
    inbox: Arc<Mutex<Inbox>>,
}

pub fn server_agent_channel(
    expected_world_alias: impl Into<String>,
    expected_subject_id: impl AsRef<[u8]>,
) -> Result<(ServerAgentSender, ServerAgentPositionSource), ServerAgentSourceError> {
    let expected_world_alias = expected_world_alias.into();
    let expected_subject_id = expected_subject_id.as_ref().to_vec();
    if expected_world_alias.is_empty()
        || expected_world_alias.len() > 64
        || !expected_world_alias.is_ascii()
        || expected_subject_id.len() != 32
    {
        return Err(ServerAgentSourceError::InvalidExpectedIdentity);
    }
    let inbox = Arc::new(Mutex::new(Inbox {
        expected_world_alias,
        expected_subject_id,
        generation: 0,
        connected: false,
        generation_has_sample: false,
        active_boot_id: None,
        retired_boots: RetiredBootFilter::default(),
        last_sequence: None,
        last_received_monotonic_ms: None,
        pending_controls: ControlQueue::new(),
        pending_sample: None,
        diagnostics: ServerAgentDiagnostics::default(),
    }));
    Ok((
        ServerAgentSender {
            inbox: Arc::clone(&inbox),
        },
        ServerAgentPositionSource { inbox },
    ))
}

impl ServerAgentSender {
    pub fn connect(&self, generation: u64) -> ServerAgentControlOutcome {
        let mut inbox = self.inbox.lock().expect("server-agent inbox poisoned");
        if generation == 0 || generation == inbox.generation {
            return ServerAgentControlOutcome::Dropped;
        }
        if generation < inbox.generation {
            inbox.diagnostics.old_generation_drops =
                inbox.diagnostics.old_generation_drops.saturating_add(1);
            return ServerAgentControlOutcome::OldGenerationDropped;
        }
        if inbox.pending_controls.remaining() < CONTROL_EVENTS_PER_SESSION {
            inbox.diagnostics.control_queue_full_drops =
                inbox.diagnostics.control_queue_full_drops.saturating_add(1);
            return ServerAgentControlOutcome::QueueFullDropped;
        }
        inbox.generation = generation;
        inbox.connected = true;
        inbox.generation_has_sample = false;
        inbox.pending_sample = None;
        let queued = inbox
            .pending_controls
            .push(ControlEvent::Connected { generation });
        debug_assert!(queued, "connect reserved the complete lifecycle");
        ServerAgentControlOutcome::Accepted
    }

    pub fn publish_position(
        &self,
        frame: ServerAgentFrame,
        received_at_monotonic_ms: u64,
    ) -> Result<ServerAgentIngestOutcome, ServerAgentSourceError> {
        let mut inbox = self.inbox.lock().expect("server-agent inbox poisoned");
        if frame.world_alias != inbox.expected_world_alias
            || frame.subject_id != inbox.expected_subject_id
        {
            inbox.diagnostics.identity_mismatch_drops =
                inbox.diagnostics.identity_mismatch_drops.saturating_add(1);
            return Ok(ServerAgentIngestOutcome::IdentityMismatchDropped);
        }
        if frame.generation == 0 || frame.sequence == 0 {
            inbox.diagnostics.invalid_transition_drops =
                inbox.diagnostics.invalid_transition_drops.saturating_add(1);
            return Ok(ServerAgentIngestOutcome::InvalidTransitionDropped);
        }
        if frame.generation < inbox.generation {
            inbox.diagnostics.old_generation_drops =
                inbox.diagnostics.old_generation_drops.saturating_add(1);
            return Ok(ServerAgentIngestOutcome::OldGenerationDropped);
        }
        if !inbox.connected || frame.generation != inbox.generation {
            return Ok(ServerAgentIngestOutcome::Dropped);
        }
        if inbox.retired_boots.contains(&frame.agent_boot_id) {
            inbox.diagnostics.retired_boot_drops =
                inbox.diagnostics.retired_boot_drops.saturating_add(1);
            return Ok(ServerAgentIngestOutcome::RetiredBootDropped);
        }
        if inbox
            .active_boot_id
            .as_deref()
            .is_some_and(|boot_id| boot_id != frame.agent_boot_id)
        {
            if inbox.generation_has_sample {
                inbox.diagnostics.boot_mismatch_drops =
                    inbox.diagnostics.boot_mismatch_drops.saturating_add(1);
                return Ok(ServerAgentIngestOutcome::BootMismatchDropped);
            }
            inbox.retire_active_boot();
            inbox.last_sequence = None;
            inbox.last_received_monotonic_ms = None;
        }
        if let Some(sequence) = inbox.last_sequence {
            if frame.sequence == sequence {
                inbox.diagnostics.duplicate_drops =
                    inbox.diagnostics.duplicate_drops.saturating_add(1);
                return Ok(ServerAgentIngestOutcome::DuplicateDropped);
            }
            if frame.sequence < sequence {
                inbox.diagnostics.out_of_order_drops =
                    inbox.diagnostics.out_of_order_drops.saturating_add(1);
                return Ok(ServerAgentIngestOutcome::OutOfOrderDropped);
            }
        }
        if inbox
            .last_received_monotonic_ms
            .is_some_and(|last| received_at_monotonic_ms < last)
        {
            inbox.diagnostics.clock_regression_drops =
                inbox.diagnostics.clock_regression_drops.saturating_add(1);
            inbox.pending_sample = None;
            inbox.queue_clock_invalid(frame.generation, ClockInvalidReason::MonotonicRegression);
            return Ok(ServerAgentIngestOutcome::ClockRegressionDropped);
        }

        let sample = PositionSample::new(
            frame.world_alias,
            &frame.subject_id,
            &frame.agent_boot_id,
            frame.generation,
            frame.sequence,
            frame.x,
            frame.y,
            frame.z,
            frame.heading_degrees,
            SampleClock::received_with_age(frame.age_upper_bound_ms, received_at_monotonic_ms),
        )?;

        let previous = inbox.last_sequence.unwrap_or(0);
        if frame.sequence > previous.saturating_add(1) {
            inbox.diagnostics.gap_events = inbox.diagnostics.gap_events.saturating_add(1);
            inbox.diagnostics.missing_sequences = inbox
                .diagnostics
                .missing_sequences
                .saturating_add(frame.sequence.saturating_sub(previous).saturating_sub(1));
        }
        if inbox.pending_sample.is_some() {
            inbox.diagnostics.superseded_samples =
                inbox.diagnostics.superseded_samples.saturating_add(1);
        }
        if inbox.active_boot_id.is_none() {
            inbox.active_boot_id = Some(frame.agent_boot_id);
        }
        inbox.generation_has_sample = true;
        inbox.last_sequence = Some(frame.sequence);
        inbox.last_received_monotonic_ms = Some(received_at_monotonic_ms);
        inbox.pending_sample = Some(PositionSourceEvent::Sample(sample));
        inbox.diagnostics.accepted_samples = inbox.diagnostics.accepted_samples.saturating_add(1);
        Ok(ServerAgentIngestOutcome::Accepted)
    }

    pub fn unavailable(&self, generation: u64) -> ServerAgentControlOutcome {
        let mut inbox = self.inbox.lock().expect("server-agent inbox poisoned");
        if generation < inbox.generation {
            inbox.diagnostics.old_generation_drops =
                inbox.diagnostics.old_generation_drops.saturating_add(1);
            return ServerAgentControlOutcome::OldGenerationDropped;
        }
        if generation == 0 || generation != inbox.generation || !inbox.connected {
            return ServerAgentControlOutcome::Dropped;
        }
        inbox.pending_sample = Some(PositionSourceEvent::Unavailable { generation });
        ServerAgentControlOutcome::Accepted
    }

    pub fn disconnect(&self, generation: u64) -> ServerAgentControlOutcome {
        let mut inbox = self.inbox.lock().expect("server-agent inbox poisoned");
        if generation < inbox.generation {
            inbox.diagnostics.old_generation_drops =
                inbox.diagnostics.old_generation_drops.saturating_add(1);
            return ServerAgentControlOutcome::OldGenerationDropped;
        }
        if generation == 0 || generation != inbox.generation || !inbox.connected {
            return ServerAgentControlOutcome::Dropped;
        }
        inbox.connected = false;
        inbox.generation_has_sample = false;
        inbox.pending_sample = None;
        if !inbox
            .pending_controls
            .push(ControlEvent::Disconnected { generation })
        {
            inbox.diagnostics.control_queue_full_drops =
                inbox.diagnostics.control_queue_full_drops.saturating_add(1);
            return ServerAgentControlOutcome::QueueFullDropped;
        }
        ServerAgentControlOutcome::Accepted
    }

    pub fn clock_invalid(
        &self,
        generation: u64,
        reason: ClockInvalidReason,
    ) -> ServerAgentControlOutcome {
        let mut inbox = self.inbox.lock().expect("server-agent inbox poisoned");
        if generation < inbox.generation {
            inbox.diagnostics.old_generation_drops =
                inbox.diagnostics.old_generation_drops.saturating_add(1);
            return ServerAgentControlOutcome::OldGenerationDropped;
        }
        if generation == 0 || generation != inbox.generation || !inbox.connected {
            return ServerAgentControlOutcome::Dropped;
        }
        inbox.pending_sample = None;
        if !inbox.queue_clock_invalid(generation, reason) {
            inbox.connected = false;
            inbox.diagnostics.control_queue_full_drops =
                inbox.diagnostics.control_queue_full_drops.saturating_add(1);
            return ServerAgentControlOutcome::QueueFullDropped;
        }
        ServerAgentControlOutcome::Accepted
    }

    pub fn diagnostics(&self) -> ServerAgentDiagnostics {
        self.inbox
            .lock()
            .expect("server-agent inbox poisoned")
            .diagnostics
    }
}

impl Inbox {
    fn retire_active_boot(&mut self) {
        let Some(boot_id) = self.active_boot_id.take() else {
            return;
        };
        self.retired_boots.insert(&boot_id);
    }

    fn queue_clock_invalid(&mut self, generation: u64, reason: ClockInvalidReason) -> bool {
        if self.pending_controls.has_clock_invalid(generation) {
            return true;
        }
        self.pending_controls
            .push(ControlEvent::ClockInvalid { generation, reason })
    }
}

#[derive(Default)]
struct RetiredBootFilter {
    bits: [u64; 32],
}

impl RetiredBootFilter {
    fn insert(&mut self, boot_id: &[u8]) {
        for bit in boot_filter_bits(boot_id) {
            self.bits[bit / u64::BITS as usize] |= 1_u64 << (bit % u64::BITS as usize);
        }
    }

    fn contains(&self, boot_id: &[u8]) -> bool {
        boot_filter_bits(boot_id).into_iter().all(|bit| {
            self.bits[bit / u64::BITS as usize] & (1_u64 << (bit % u64::BITS as usize)) != 0
        })
    }
}

fn boot_filter_bits(boot_id: &[u8]) -> [usize; 3] {
    const BIT_COUNT: u64 = 32 * u64::BITS as u64;
    let first = fnv1a(boot_id, 0xcbf2_9ce4_8422_2325);
    let step = fnv1a(boot_id, 0x8422_2325_cbf2_9ce4) | 1;
    [
        (first % BIT_COUNT) as usize,
        (first.wrapping_add(step) % BIT_COUNT) as usize,
        (first.wrapping_add(step.wrapping_mul(2)) % BIT_COUNT) as usize,
    ]
}

fn fnv1a(bytes: &[u8], seed: u64) -> u64 {
    bytes.iter().fold(seed, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

impl PositionSource for ServerAgentPositionSource {
    fn poll(
        &mut self,
        _now_monotonic_ms: u64,
    ) -> Result<Option<PositionSourceEvent>, PositionSourceError> {
        let mut inbox = self.inbox.lock().expect("server-agent inbox poisoned");
        Ok(inbox
            .pending_controls
            .pop()
            .map(ControlEvent::into_position_source_event)
            .or_else(|| inbox.pending_sample.take()))
    }
}
