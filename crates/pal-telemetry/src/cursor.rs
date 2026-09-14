use std::{
    fmt,
    sync::{Arc, Mutex},
};

use pal_protocol::v2::{ResumeCursor, SubscribeResponse};
use thiserror::Error;
use tokio::sync::watch;

use crate::{ValidationError, validate_envelope};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequenceDecision {
    Accept,
    AcceptWithGap { missing: u64 },
    DropDuplicate,
    DropOutOfOrder,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SequenceDiagnostics {
    pub gap_events: u64,
    pub missing_sequences: u64,
    pub duplicate_drops: u64,
    pub out_of_order_drops: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatestConnectOutcome {
    Accepted,
    Dropped,
    OldGenerationDropped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatestPublishOutcome {
    Accepted,
    AcceptedWithGap { missing: u64 },
    DuplicateDropped,
    OutOfOrderDropped,
    OldGenerationDropped,
    RetiredBootDropped,
    IdentityMismatchDropped,
    BootMismatchDropped,
    InvalidTransitionDropped,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LatestDiagnostics {
    pub accepted_samples: u64,
    pub gap_events: u64,
    pub missing_sequences: u64,
    pub duplicate_drops: u64,
    pub out_of_order_drops: u64,
    pub old_generation_drops: u64,
    pub retired_boot_drops: u64,
    pub identity_mismatch_drops: u64,
    pub boot_mismatch_drops: u64,
    pub invalid_transition_drops: u64,
}

#[derive(Clone)]
pub struct SequenceCursor {
    boot_id: Vec<u8>,
    last_sequence: Option<u64>,
    diagnostics: SequenceDiagnostics,
}

impl fmt::Debug for SequenceCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED SEQUENCE CURSOR]")
    }
}

impl SequenceCursor {
    pub fn new(boot_id: impl AsRef<[u8]>) -> Self {
        Self {
            boot_id: boot_id.as_ref().to_vec(),
            last_sequence: None,
            diagnostics: SequenceDiagnostics::default(),
        }
    }

    pub fn decide(&mut self, sequence: u64) -> SequenceDecision {
        let Some(previous) = self.last_sequence else {
            self.last_sequence = Some(sequence);
            return SequenceDecision::Accept;
        };
        if sequence == previous {
            self.diagnostics.duplicate_drops = self.diagnostics.duplicate_drops.saturating_add(1);
            return SequenceDecision::DropDuplicate;
        }
        if sequence < previous {
            self.diagnostics.out_of_order_drops =
                self.diagnostics.out_of_order_drops.saturating_add(1);
            return SequenceDecision::DropOutOfOrder;
        }
        self.last_sequence = Some(sequence);
        let missing = sequence.saturating_sub(previous).saturating_sub(1);
        if missing == 0 {
            SequenceDecision::Accept
        } else {
            self.diagnostics.gap_events = self.diagnostics.gap_events.saturating_add(1);
            self.diagnostics.missing_sequences =
                self.diagnostics.missing_sequences.saturating_add(missing);
            SequenceDecision::AcceptWithGap { missing }
        }
    }

    pub const fn diagnostics(&self) -> SequenceDiagnostics {
        self.diagnostics
    }

    pub fn boot_id(&self) -> &[u8] {
        &self.boot_id
    }
}

struct LatestState {
    closed: bool,
    generation: u64,
    active_boot_id: Option<[u8; 16]>,
    last_sequence: Option<u64>,
    retired_boots: RetiredBootFilter,
    diagnostics: LatestDiagnostics,
}

#[cfg(test)]
type TestHook = Mutex<Option<Arc<dyn Fn(TestOperation) + Send + Sync>>>;

struct LatestInner {
    expected_world_alias: String,
    expected_subject_id: [u8; 32],
    expected_coordinate_profile_sha256: String,
    state: Mutex<LatestState>,
    sender: watch::Sender<LatestSignal>,
    #[cfg(test)]
    before_lock_hook: TestHook,
    #[cfg(test)]
    before_visible_hook: TestHook,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestOperation {
    Publish(u64),
    Close,
}

#[derive(Clone)]
enum LatestSignal {
    Open(Option<SubscribeResponse>),
    Closed,
}

#[derive(Clone)]
pub struct LatestTelemetry {
    inner: Arc<LatestInner>,
}

impl LatestTelemetry {
    pub fn new(
        expected_world_alias: impl Into<String>,
        expected_subject_id: impl AsRef<[u8]>,
        expected_coordinate_profile_sha256: impl Into<String>,
    ) -> Result<Self, LatestTelemetryError> {
        let expected_world_alias = expected_world_alias.into();
        let expected_subject_id: [u8; 32] = expected_subject_id
            .as_ref()
            .try_into()
            .map_err(|_| LatestTelemetryError::Validation)?;
        let expected_coordinate_profile_sha256 = expected_coordinate_profile_sha256.into();
        let validation_sample = SubscribeResponse {
            protocol_version: pal_domain::PROTOCOL_VERSION,
            world_alias: expected_world_alias.clone(),
            subject_id: expected_subject_id.to_vec(),
            boot_id: vec![0; 16],
            sequence: 1,
            rest_completed_at_unix_ms: 1,
            age_at_emit_ms: 0,
            position_x: 0.0,
            position_y: 0.0,
            position_z: 0.0,
            heading_degrees: None,
            heading_source: pal_protocol::v2::HeadingSource::Unspecified as i32,
            trace_id: vec![0; 16],
            coordinate_space: pal_protocol::v2::CoordinateSpace::OfficialGameDataWorldV1 as i32,
            coordinate_profile_sha256: expected_coordinate_profile_sha256.clone(),
        };
        validate_envelope(&validation_sample, &expected_coordinate_profile_sha256)?;
        let (sender, _) = watch::channel(LatestSignal::Open(None));
        Ok(Self {
            inner: Arc::new(LatestInner {
                expected_world_alias,
                expected_subject_id,
                expected_coordinate_profile_sha256,
                state: Mutex::new(LatestState {
                    closed: false,
                    generation: 0,
                    active_boot_id: None,
                    last_sequence: None,
                    retired_boots: RetiredBootFilter::default(),
                    diagnostics: LatestDiagnostics::default(),
                }),
                sender,
                #[cfg(test)]
                before_lock_hook: Mutex::new(None),
                #[cfg(test)]
                before_visible_hook: Mutex::new(None),
            }),
        })
    }

    pub fn connect(&self, generation: u64) -> LatestConnectOutcome {
        let mut state = self.inner.state.lock().expect("latest state poisoned");
        if state.closed {
            return LatestConnectOutcome::Dropped;
        }
        if generation == 0 || generation == state.generation {
            return LatestConnectOutcome::Dropped;
        }
        if generation < state.generation {
            state.diagnostics.old_generation_drops =
                state.diagnostics.old_generation_drops.saturating_add(1);
            return LatestConnectOutcome::OldGenerationDropped;
        }
        if let Some(boot_id) = state.active_boot_id.take() {
            state.retired_boots.insert(&boot_id);
        }
        state.generation = generation;
        state.last_sequence = None;
        self.inner.sender.send_replace(LatestSignal::Open(None));
        LatestConnectOutcome::Accepted
    }

    pub fn publish(
        &self,
        generation: u64,
        value: SubscribeResponse,
    ) -> Result<LatestPublishOutcome, LatestTelemetryError> {
        validate_envelope(&value, &self.inner.expected_coordinate_profile_sha256)?;
        #[cfg(test)]
        self.run_before_lock_hook(TestOperation::Publish(value.sequence));
        let mut state = self.inner.state.lock().expect("latest state poisoned");
        if state.closed {
            return Err(LatestTelemetryError::Closed);
        }
        if value.world_alias != self.inner.expected_world_alias
            || value.subject_id != self.inner.expected_subject_id
        {
            state.diagnostics.identity_mismatch_drops =
                state.diagnostics.identity_mismatch_drops.saturating_add(1);
            return Ok(LatestPublishOutcome::IdentityMismatchDropped);
        }
        if generation == 0 {
            state.diagnostics.invalid_transition_drops =
                state.diagnostics.invalid_transition_drops.saturating_add(1);
            return Ok(LatestPublishOutcome::InvalidTransitionDropped);
        }
        if generation < state.generation {
            state.diagnostics.old_generation_drops =
                state.diagnostics.old_generation_drops.saturating_add(1);
            return Ok(LatestPublishOutcome::OldGenerationDropped);
        }
        if generation != state.generation {
            state.diagnostics.invalid_transition_drops =
                state.diagnostics.invalid_transition_drops.saturating_add(1);
            return Ok(LatestPublishOutcome::InvalidTransitionDropped);
        }
        let boot_id: [u8; 16] = value
            .boot_id
            .as_slice()
            .try_into()
            .map_err(|_| LatestTelemetryError::Validation)?;
        if state.retired_boots.contains(&boot_id) {
            state.diagnostics.retired_boot_drops =
                state.diagnostics.retired_boot_drops.saturating_add(1);
            return Ok(LatestPublishOutcome::RetiredBootDropped);
        }
        if state.active_boot_id.is_some_and(|active| active != boot_id) {
            state.diagnostics.boot_mismatch_drops =
                state.diagnostics.boot_mismatch_drops.saturating_add(1);
            return Ok(LatestPublishOutcome::BootMismatchDropped);
        }
        let outcome = match state.last_sequence {
            Some(previous) if value.sequence == previous => {
                state.diagnostics.duplicate_drops =
                    state.diagnostics.duplicate_drops.saturating_add(1);
                return Ok(LatestPublishOutcome::DuplicateDropped);
            }
            Some(previous) if value.sequence < previous => {
                state.diagnostics.out_of_order_drops =
                    state.diagnostics.out_of_order_drops.saturating_add(1);
                return Ok(LatestPublishOutcome::OutOfOrderDropped);
            }
            Some(previous) if value.sequence > previous.saturating_add(1) => {
                let missing = value.sequence.saturating_sub(previous).saturating_sub(1);
                state.diagnostics.gap_events = state.diagnostics.gap_events.saturating_add(1);
                state.diagnostics.missing_sequences =
                    state.diagnostics.missing_sequences.saturating_add(missing);
                LatestPublishOutcome::AcceptedWithGap { missing }
            }
            _ => LatestPublishOutcome::Accepted,
        };
        state.active_boot_id = Some(boot_id);
        state.last_sequence = Some(value.sequence);
        state.diagnostics.accepted_samples = state.diagnostics.accepted_samples.saturating_add(1);
        #[cfg(test)]
        self.run_before_visible_hook(TestOperation::Publish(value.sequence));
        self.inner
            .sender
            .send_replace(LatestSignal::Open(Some(value)));
        drop(state);
        Ok(outcome)
    }

    pub fn current(&self) -> Option<SubscribeResponse> {
        match &*self.inner.sender.borrow() {
            LatestSignal::Open(value) => value.clone(),
            LatestSignal::Closed => None,
        }
    }

    pub fn buffered_state_count(&self) -> usize {
        usize::from(matches!(
            &*self.inner.sender.borrow(),
            LatestSignal::Open(Some(_))
        ))
    }

    pub fn close(&self) {
        #[cfg(test)]
        self.run_before_lock_hook(TestOperation::Close);
        let mut state = self.inner.state.lock().expect("latest state poisoned");
        state.closed = true;
        #[cfg(test)]
        self.run_before_visible_hook(TestOperation::Close);
        self.inner.sender.send_replace(LatestSignal::Closed);
        drop(state);
    }

    #[cfg(test)]
    fn install_test_hooks(
        &self,
        before_lock: Arc<dyn Fn(TestOperation) + Send + Sync>,
        before_visible: Arc<dyn Fn(TestOperation) + Send + Sync>,
    ) {
        *self
            .inner
            .before_lock_hook
            .lock()
            .expect("test hook poisoned") = Some(before_lock);
        *self
            .inner
            .before_visible_hook
            .lock()
            .expect("test hook poisoned") = Some(before_visible);
    }

    #[cfg(test)]
    fn run_before_lock_hook(&self, operation: TestOperation) {
        let hook = self
            .inner
            .before_lock_hook
            .lock()
            .expect("test hook poisoned")
            .clone();
        if let Some(hook) = hook {
            hook(operation);
        }
    }

    #[cfg(test)]
    fn run_before_visible_hook(&self, operation: TestOperation) {
        let hook = self
            .inner
            .before_visible_hook
            .lock()
            .expect("test hook poisoned")
            .clone();
        if let Some(hook) = hook {
            hook(operation);
        }
    }

    pub fn diagnostics(&self) -> LatestDiagnostics {
        self.inner
            .state
            .lock()
            .expect("latest state poisoned")
            .diagnostics
    }

    pub fn is_bound_to(&self, world_alias: &str, subject_id: &[u8], profile: &str) -> bool {
        self.inner.expected_world_alias == world_alias
            && self.inner.expected_subject_id.as_slice() == subject_id
            && self.inner.expected_coordinate_profile_sha256 == profile
    }

    pub async fn next_after(
        &self,
        resume: Option<ResumeCursor>,
    ) -> Result<SubscribeResponse, LatestTelemetryError> {
        let mut receiver = self.inner.sender.subscribe();
        loop {
            let current = receiver.borrow_and_update().clone();
            match current {
                LatestSignal::Open(Some(value))
                    if cursor_needs_current(resume.as_ref(), &value) =>
                {
                    return Ok(value);
                }
                LatestSignal::Closed => return Err(LatestTelemetryError::Closed),
                LatestSignal::Open(_) => {}
            }
            receiver
                .changed()
                .await
                .map_err(|_| LatestTelemetryError::Closed)?;
        }
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

fn cursor_needs_current(resume: Option<&ResumeCursor>, current: &SubscribeResponse) -> bool {
    resume.is_none_or(|resume| {
        resume.boot_id != current.boot_id || current.sequence > resume.sequence
    })
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum LatestTelemetryError {
    #[error("latest telemetry validation failed")]
    Validation,
    #[error("latest telemetry source closed")]
    Closed,
}

impl From<ValidationError> for LatestTelemetryError {
    fn from(_: ValidationError) -> Self {
        Self::Validation
    }
}

#[cfg(test)]
mod atomic_tests {
    use std::{
        sync::{Arc, Barrier, mpsc},
        thread,
    };

    use pal_protocol::v2::{CoordinateSpace, HeadingSource, SubscribeResponse};

    use super::{LatestTelemetry, TestOperation};

    const SUBJECT: [u8; 32] = [0x42; 32];
    const BOOT: [u8; 16] = [0x11; 16];
    const PROFILE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn envelope(sequence: u64) -> SubscribeResponse {
        SubscribeResponse {
            protocol_version: pal_domain::PROTOCOL_VERSION,
            world_alias: "world-a".to_owned(),
            subject_id: SUBJECT.to_vec(),
            boot_id: BOOT.to_vec(),
            sequence,
            rest_completed_at_unix_ms: 1,
            age_at_emit_ms: 0,
            position_x: sequence as f64,
            position_y: 0.0,
            position_z: 0.0,
            heading_degrees: None,
            heading_source: HeadingSource::Unspecified as i32,
            trace_id: [0x33; 16].to_vec(),
            coordinate_space: CoordinateSpace::OfficialGameDataWorldV1 as i32,
            coordinate_profile_sha256: PROFILE.to_owned(),
        }
    }

    #[test]
    fn concurrent_publications_cannot_make_the_visible_latest_roll_back() {
        let latest = LatestTelemetry::new("world-a", SUBJECT, PROFILE).unwrap();
        latest.connect(1);
        let release_first = Arc::new(Barrier::new(2));
        let (first_at_visible_tx, first_at_visible_rx) = mpsc::channel();
        let (second_at_lock_tx, second_at_lock_rx) = mpsc::channel();
        let barrier = Arc::clone(&release_first);
        latest.install_test_hooks(
            Arc::new(move |operation| {
                if operation == TestOperation::Publish(2) {
                    second_at_lock_tx.send(()).unwrap();
                }
            }),
            Arc::new(move |operation| {
                if operation == TestOperation::Publish(1) {
                    first_at_visible_tx.send(()).unwrap();
                    barrier.wait();
                }
            }),
        );

        let first = {
            let latest = latest.clone();
            thread::spawn(move || latest.publish(1, envelope(1)).unwrap())
        };
        first_at_visible_rx.recv().unwrap();
        let second = {
            let latest = latest.clone();
            thread::spawn(move || latest.publish(1, envelope(2)).unwrap())
        };
        second_at_lock_rx.recv().unwrap();
        release_first.wait();
        first.join().unwrap();
        second.join().unwrap();

        assert_eq!(latest.current().unwrap().sequence, 2);
    }

    #[test]
    fn close_and_publish_are_one_terminal_visible_transition() {
        let latest = LatestTelemetry::new("world-a", SUBJECT, PROFILE).unwrap();
        latest.connect(1);
        let release_publish = Arc::new(Barrier::new(2));
        let (publish_at_visible_tx, publish_at_visible_rx) = mpsc::channel();
        let (close_at_lock_tx, close_at_lock_rx) = mpsc::channel();
        let barrier = Arc::clone(&release_publish);
        latest.install_test_hooks(
            Arc::new(move |operation| {
                if operation == TestOperation::Close {
                    close_at_lock_tx.send(()).unwrap();
                }
            }),
            Arc::new(move |operation| {
                if operation == TestOperation::Publish(1) {
                    publish_at_visible_tx.send(()).unwrap();
                    barrier.wait();
                }
            }),
        );

        let publisher = {
            let latest = latest.clone();
            thread::spawn(move || latest.publish(1, envelope(1)))
        };
        publish_at_visible_rx.recv().unwrap();
        let closer = {
            let latest = latest.clone();
            thread::spawn(move || latest.close())
        };
        close_at_lock_rx.recv().unwrap();
        release_publish.wait();
        publisher.join().unwrap().unwrap();
        closer.join().unwrap();

        assert!(latest.current().is_none());
        assert!(latest.publish(1, envelope(2)).is_err());
    }
}
