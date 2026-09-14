//! Exact-build, read-only local position sampling for development diagnostics.
//!
//! This module deliberately owns no window, renderer, preview, or automatic visibility path.
//! The immutable memory profile is only a position source; Gate B map alignment remains a
//! separate, explicitly unapproved decision.

use std::fmt;

use pal_build_contract::WINDOWS_LIVE_POSITION_PROFILE_BUILD_ID_U64;
use pal_state::{
    ClockInvalidReason, ServerAgentControlOutcome, ServerAgentFrame, ServerAgentIngestOutcome,
    ServerAgentPositionSource, ServerAgentSender, server_agent_channel,
};
use thiserror::Error;

const EXECUTABLE_SHA256: [u8; 32] = [
    0xfe, 0x3c, 0x15, 0x06, 0x45, 0x24, 0xba, 0xe1, 0x94, 0x78, 0x52, 0x46, 0x7c, 0x4f, 0x92, 0xbc,
    0x22, 0x46, 0x9a, 0xcc, 0x03, 0x3a, 0x3d, 0x3c, 0x80, 0x31, 0xea, 0xb4, 0x32, 0x4e, 0x41, 0xe8,
];
const BUILD_ID: u64 = 24_575_825;
const _: () = assert!(BUILD_ID == WINDOWS_LIVE_POSITION_PROFILE_BUILD_ID_U64);
const EXECUTABLE_FILE_SIZE: u64 = 161_397_248;
const LOADED_MODULE_SIZE: usize = 167_432_192;
// Steam build 24575825: the unique RIP-relative GWorld signature
// `48 8B 1D ?? ?? ?? ?? 48 85 DB 74 33 41 B0` resolves to this slot.
const ROOT_RVA: usize = 0x966_7260;
const POINTER_READ_OFFSETS: [usize; 7] = [0, 0x1b8, 0x38, 0, 0x30, 0x348, 0x298];
const CAMERA_OFFSET: usize = 0x128;
const CAMERA_BLOCK_BYTES: usize = 40;
const SAMPLE_INTERVAL_MS: u64 = 100;
// This is deliberately broader than every currently known region and is only a corruption guard.
// Region ownership and Gate B map visibility are separate runtime decisions.
const MAX_ABSOLUTE_WORLD_COORDINATE: f64 = 2_000_000.0;
const MIN_PLAUSIBLE_YAW: f64 = -360.0;
const MAX_PLAUSIBLE_YAW: f64 = 360.0;

/// The only local process-memory profile compiled into this build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowsLivePositionProfile;

pub const WINDOWS_LIVE_POSITION_PROFILE: WindowsLivePositionProfile = WindowsLivePositionProfile;

/// Compatibility alias for callers compiled against the first exact-build profile API.
pub type Build24181527Profile = WindowsLivePositionProfile;
pub const BUILD_24181527_PROFILE: Build24181527Profile = WINDOWS_LIVE_POSITION_PROFILE;

impl WindowsLivePositionProfile {
    pub const fn build_id(self) -> u64 {
        BUILD_ID
    }

    pub const fn executable_sha256(self) -> [u8; 32] {
        EXECUTABLE_SHA256
    }

    pub const fn executable_file_size(self) -> u64 {
        EXECUTABLE_FILE_SIZE
    }

    pub const fn loaded_module_size(self) -> usize {
        LOADED_MODULE_SIZE
    }

    pub const fn root_rva(self) -> usize {
        ROOT_RVA
    }

    pub const fn pointer_read_offsets(self) -> &'static [usize; 7] {
        &POINTER_READ_OFFSETS
    }

    pub const fn camera_offset(self) -> usize {
        CAMERA_OFFSET
    }

    pub const fn sample_interval_ms(self) -> u64 {
        SAMPLE_INTERVAL_MS
    }

    /// This source profile does not approve or activate map alignment.
    pub const fn gate_b_approved(self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct LocalProcessIdentity {
    build_id: u64,
    executable_sha256: [u8; 32],
    executable_file_size: u64,
    module_base: usize,
    module_size: usize,
}

impl LocalProcessIdentity {
    pub const fn new(
        build_id: u64,
        executable_sha256: [u8; 32],
        executable_file_size: u64,
        module_base: usize,
        module_size: usize,
    ) -> Self {
        Self {
            build_id,
            executable_sha256,
            executable_file_size,
            module_base,
            module_size,
        }
    }

    pub const fn build_id(self) -> u64 {
        self.build_id
    }

    pub const fn executable_sha256(self) -> [u8; 32] {
        self.executable_sha256
    }

    pub const fn executable_file_size(self) -> u64 {
        self.executable_file_size
    }

    pub const fn module_base(self) -> usize {
        self.module_base
    }

    pub const fn module_size(self) -> usize {
        self.module_size
    }
}

impl fmt::Debug for LocalProcessIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED LOCAL PROCESS IDENTITY]")
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum LocalMemoryReadError {
    #[error("local read-only process memory is unavailable")]
    Unavailable,
}

/// A deterministic, testable boundary around an already read-only process handle.
///
/// One implementation-defined read session must bracket every complete pointer-chain sample.
pub trait LocalPositionReadSession {
    fn read_exact(&mut self, address: usize, output: &mut [u8])
    -> Result<(), LocalMemoryReadError>;
}

/// Implementations must observe identity from the same process instance used by the session.
/// The pump checks identity before and after every session.
pub trait LocalPositionMemoryReader {
    fn identity(&mut self) -> Result<LocalProcessIdentity, LocalMemoryReadError>;

    fn with_read_session<T>(
        &mut self,
        operation: impl FnOnce(&mut dyn LocalPositionReadSession) -> T,
    ) -> Result<T, LocalMemoryReadError>;
}

#[derive(Clone, Eq, PartialEq)]
pub struct LocalPositionSourceIdentity {
    world_alias: String,
    subject_id: [u8; 32],
    boot_id: [u8; 32],
    starting_generation: u64,
}

impl LocalPositionSourceIdentity {
    pub fn new(
        world_alias: impl Into<String>,
        subject_id: [u8; 32],
        boot_id: [u8; 32],
        starting_generation: u64,
    ) -> Result<Self, LocalPositionSourceError> {
        let world_alias = world_alias.into();
        if world_alias.is_empty()
            || world_alias.len() > 64
            || !world_alias.is_ascii()
            || subject_id == [0; 32]
            || boot_id == [0; 32]
            || starting_generation == 0
        {
            return Err(LocalPositionSourceError::InvalidSourceIdentity);
        }
        Ok(Self {
            world_alias,
            subject_id,
            boot_id,
            starting_generation,
        })
    }
}

impl fmt::Debug for LocalPositionSourceIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED LOCAL POSITION SOURCE IDENTITY]")
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum LocalPositionSourceError {
    #[error("local position source identity is invalid")]
    InvalidSourceIdentity,
    #[error("the local process does not match the exact supported build profile")]
    ExactBuildMismatch,
    #[error("the local position source is still active")]
    StillActive,
    #[error("the local position source generation overflowed")]
    GenerationOverflow,
    #[error("the bounded position handoff rejected a lifecycle transition")]
    HandoffRejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalPositionTerminalReason {
    IdentityMismatch,
    ReadFailed,
    NoActiveWorld,
    InvalidPointer,
    UnstablePointerChain,
    InvalidPosition,
    MonotonicRegression,
    SequenceOverflow,
    HandoffRejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalPositionStep {
    Idle,
    Unavailable {
        generation: u64,
    },
    Published {
        generation: u64,
        sequence: u64,
    },
    ClockInvalid {
        generation: u64,
    },
    Disconnected {
        generation: u64,
        reason: LocalPositionTerminalReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PumpPhase {
    Active,
    ClockInvalidPendingDisconnect,
    Disconnected,
}

pub struct PalworldWindowsLivePositionPump<R> {
    reader: Option<R>,
    bound_process: LocalProcessIdentity,
    sender: ServerAgentSender,
    source_identity: LocalPositionSourceIdentity,
    active_boot_id: [u8; 32],
    generation: u64,
    sequence: u64,
    next_sample_at_ms: Option<u64>,
    last_step_at_ms: Option<u64>,
    phase: PumpPhase,
    terminal_reason: Option<LocalPositionTerminalReason>,
    world_available: bool,
}

impl<R> fmt::Debug for PalworldWindowsLivePositionPump<R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED EXACT-BUILD LOCAL POSITION PUMP]")
    }
}

impl<R> Drop for PalworldWindowsLivePositionPump<R> {
    fn drop(&mut self) {
        if self.phase != PumpPhase::Disconnected {
            let _ = self.sender.disconnect(self.generation);
            self.phase = PumpPhase::Disconnected;
        }
    }
}

impl<R: LocalPositionMemoryReader> PalworldWindowsLivePositionPump<R> {
    pub fn new(
        mut reader: R,
        source_identity: LocalPositionSourceIdentity,
    ) -> Result<(Self, ServerAgentPositionSource), LocalPositionSourceError> {
        let bound_process = reader
            .identity()
            .map_err(|_| LocalPositionSourceError::ExactBuildMismatch)?;
        validate_exact_process(bound_process)?;
        let generation = source_identity.starting_generation;
        let (sender, source) = server_agent_channel(
            source_identity.world_alias.clone(),
            source_identity.subject_id,
        )
        .map_err(|_| LocalPositionSourceError::InvalidSourceIdentity)?;
        if sender.connect(generation) != ServerAgentControlOutcome::Accepted {
            return Err(LocalPositionSourceError::HandoffRejected);
        }
        Ok((
            Self {
                reader: Some(reader),
                bound_process,
                sender,
                active_boot_id: source_identity.boot_id,
                source_identity,
                generation,
                sequence: 0,
                next_sample_at_ms: None,
                last_step_at_ms: None,
                phase: PumpPhase::Active,
                terminal_reason: None,
                world_available: false,
            },
            source,
        ))
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn terminal_reason(&self) -> Option<LocalPositionTerminalReason> {
        self.terminal_reason
    }

    pub fn step(&mut self, now_monotonic_ms: u64) -> LocalPositionStep {
        if self.phase == PumpPhase::ClockInvalidPendingDisconnect {
            return self.disconnect(LocalPositionTerminalReason::MonotonicRegression);
        }
        if self.phase == PumpPhase::Disconnected {
            return LocalPositionStep::Idle;
        }
        if self
            .last_step_at_ms
            .is_some_and(|last| now_monotonic_ms < last)
        {
            self.phase = PumpPhase::ClockInvalidPendingDisconnect;
            self.terminal_reason = Some(LocalPositionTerminalReason::MonotonicRegression);
            let _ = self
                .sender
                .clock_invalid(self.generation, ClockInvalidReason::MonotonicRegression);
            return LocalPositionStep::ClockInvalid {
                generation: self.generation,
            };
        }
        self.last_step_at_ms = Some(now_monotonic_ms);

        if self
            .next_sample_at_ms
            .is_some_and(|due| now_monotonic_ms < due)
        {
            return LocalPositionStep::Idle;
        }

        let observation = match self.read_stable_position() {
            Ok(Some(observation)) => observation,
            Ok(None) => {
                // The process exists before a world is joined. Keep the exact-build reader
                // alive and retry instead of forcing the supervisor to restart the overlay.
                self.next_sample_at_ms = Some(now_monotonic_ms.saturating_add(SAMPLE_INTERVAL_MS));
                if self.world_available {
                    self.world_available = false;
                    if self.sender.unavailable(self.generation)
                        != ServerAgentControlOutcome::Accepted
                    {
                        return self.disconnect(LocalPositionTerminalReason::HandoffRejected);
                    }
                    return LocalPositionStep::Unavailable {
                        generation: self.generation,
                    };
                }
                return LocalPositionStep::Idle;
            }
            Err(reason) => return self.disconnect(reason),
        };
        let Some(sequence) = self.sequence.checked_add(1) else {
            return self.disconnect(LocalPositionTerminalReason::SequenceOverflow);
        };
        let frame = ServerAgentFrame::new(
            self.source_identity.world_alias.clone(),
            self.source_identity.subject_id,
            self.active_boot_id,
            self.generation,
            sequence,
            observation.x,
            observation.y,
            observation.z,
            Some(observation.yaw_degrees as f32),
            0,
        );
        match self.sender.publish_position(frame, now_monotonic_ms) {
            Ok(ServerAgentIngestOutcome::Accepted) => {
                self.world_available = true;
                self.sequence = sequence;
                self.next_sample_at_ms = Some(now_monotonic_ms.saturating_add(SAMPLE_INTERVAL_MS));
                LocalPositionStep::Published {
                    generation: self.generation,
                    sequence,
                }
            }
            _ => self.disconnect(LocalPositionTerminalReason::HandoffRejected),
        }
    }

    pub fn reconnect(
        &mut self,
        mut reader: R,
        now_monotonic_ms: u64,
    ) -> Result<(), LocalPositionSourceError> {
        if self.phase != PumpPhase::Disconnected {
            return Err(LocalPositionSourceError::StillActive);
        }
        let identity = reader
            .identity()
            .map_err(|_| LocalPositionSourceError::ExactBuildMismatch)?;
        validate_exact_process(identity)?;
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(LocalPositionSourceError::GenerationOverflow)?;
        if self.sender.connect(generation) != ServerAgentControlOutcome::Accepted {
            return Err(LocalPositionSourceError::HandoffRejected);
        }
        self.reader = Some(reader);
        self.bound_process = identity;
        self.generation = generation;
        self.active_boot_id = derive_connection_boot_id(self.source_identity.boot_id, generation);
        self.next_sample_at_ms = Some(now_monotonic_ms);
        self.last_step_at_ms = Some(now_monotonic_ms);
        self.phase = PumpPhase::Active;
        self.terminal_reason = None;
        self.world_available = false;
        Ok(())
    }

    fn disconnect(&mut self, reason: LocalPositionTerminalReason) -> LocalPositionStep {
        if self.phase != PumpPhase::Disconnected {
            let _ = self.sender.disconnect(self.generation);
            self.reader.take();
            self.phase = PumpPhase::Disconnected;
            self.terminal_reason = Some(reason);
        }
        LocalPositionStep::Disconnected {
            generation: self.generation,
            reason,
        }
    }

    fn read_stable_position(
        &mut self,
    ) -> Result<Option<LocalPositionObservation>, LocalPositionTerminalReason> {
        let reader = self
            .reader
            .as_mut()
            .ok_or(LocalPositionTerminalReason::ReadFailed)?;
        let before = reader
            .identity()
            .map_err(|_| LocalPositionTerminalReason::ReadFailed)?;
        if before != self.bound_process || validate_exact_process(before).is_err() {
            return Err(LocalPositionTerminalReason::IdentityMismatch);
        }

        let observation = reader
            .with_read_session(|session| read_stable_position_in_session(session, before))
            .map_err(|_| LocalPositionTerminalReason::ReadFailed)?;
        let after = reader
            .identity()
            .map_err(|_| LocalPositionTerminalReason::ReadFailed)?;
        if after != before || validate_exact_process(after).is_err() {
            return Err(LocalPositionTerminalReason::IdentityMismatch);
        }

        match observation {
            Ok(observation) => Ok(observation),
            // Palworld creates the process and top-level game window before a world owns the
            // local player chain. A null link is therefore a retryable "not in a world yet"
            // state. The source remains connected, publishes no coordinates, and the renderer
            // stays fail-closed until a complete stable chain appears.
            Err(LocalPositionTerminalReason::NoActiveWorld) => Ok(None),
            Err(reason) => Err(reason),
        }
    }
}

/// Compatibility alias for the original exact-build profile API.
pub type PalworldBuild24181527PositionPump<R> = PalworldWindowsLivePositionPump<R>;

#[derive(Clone, Copy)]
struct LocalPositionObservation {
    x: f64,
    y: f64,
    z: f64,
    yaw_degrees: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ResolvedPointerChain {
    pointers: [usize; POINTER_READ_OFFSETS.len()],
    camera_address: usize,
}

fn read_stable_position_in_session(
    session: &mut dyn LocalPositionReadSession,
    identity: LocalProcessIdentity,
) -> Result<Option<LocalPositionObservation>, LocalPositionTerminalReason> {
    let root_slot = checked_add(identity.module_base, ROOT_RVA)?;
    let chain_before = resolve_pointer_chain(session, root_slot)?;
    chain_before
        .camera_address
        .checked_add(CAMERA_BLOCK_BYTES)
        .ok_or(LocalPositionTerminalReason::InvalidPointer)?;

    let mut camera = [0_u8; CAMERA_BLOCK_BYTES];
    session
        .read_exact(chain_before.camera_address, &mut camera)
        .map_err(|_| LocalPositionTerminalReason::ReadFailed)?;
    verify_pointer_chain(session, root_slot, chain_before)?;
    decode_camera_block(&camera)
}

fn resolve_pointer_chain(
    session: &mut dyn LocalPositionReadSession,
    root_slot: usize,
) -> Result<ResolvedPointerChain, LocalPositionTerminalReason> {
    let mut pointers = [0_usize; POINTER_READ_OFFSETS.len()];
    let mut pointer_base = root_slot;
    for (index, offset) in POINTER_READ_OFFSETS.iter().copied().enumerate() {
        let pointer_slot = checked_add(pointer_base, offset)?;
        let pointer = read_pointer(session, pointer_slot)?;
        pointers[index] = pointer;
        pointer_base = pointer;
    }
    let camera_address = checked_add(pointer_base, CAMERA_OFFSET)?;
    Ok(ResolvedPointerChain {
        pointers,
        camera_address,
    })
}

fn verify_pointer_chain(
    session: &mut dyn LocalPositionReadSession,
    root_slot: usize,
    expected: ResolvedPointerChain,
) -> Result<(), LocalPositionTerminalReason> {
    let mut pointer_base = root_slot;
    for (index, offset) in POINTER_READ_OFFSETS.iter().copied().enumerate() {
        let pointer_slot = checked_add(pointer_base, offset)?;
        let observed = read_pointer(session, pointer_slot)?;
        if observed != expected.pointers[index] {
            return Err(LocalPositionTerminalReason::UnstablePointerChain);
        }
        pointer_base = expected.pointers[index];
    }
    if checked_add(pointer_base, CAMERA_OFFSET)? != expected.camera_address {
        return Err(LocalPositionTerminalReason::UnstablePointerChain);
    }
    Ok(())
}

fn read_pointer(
    session: &mut dyn LocalPositionReadSession,
    address: usize,
) -> Result<usize, LocalPositionTerminalReason> {
    if address == 0 {
        return Err(LocalPositionTerminalReason::InvalidPointer);
    }
    let mut bytes = [0_u8; 8];
    session
        .read_exact(address, &mut bytes)
        .map_err(|_| LocalPositionTerminalReason::ReadFailed)?;
    let pointer = usize::try_from(u64::from_le_bytes(bytes))
        .map_err(|_| LocalPositionTerminalReason::InvalidPointer)?;
    if pointer == 0 {
        return Err(LocalPositionTerminalReason::NoActiveWorld);
    }
    Ok(pointer)
}

fn validate_exact_process(identity: LocalProcessIdentity) -> Result<(), LocalPositionSourceError> {
    let root_end = ROOT_RVA.checked_add(8);
    if identity.build_id != BUILD_ID
        || identity.executable_sha256 != EXECUTABLE_SHA256
        || identity.executable_file_size != EXECUTABLE_FILE_SIZE
        || identity.module_base == 0
        || identity.module_size != LOADED_MODULE_SIZE
        || identity
            .module_base
            .checked_add(identity.module_size)
            .is_none()
        || root_end.is_none_or(|end| end > identity.module_size)
    {
        return Err(LocalPositionSourceError::ExactBuildMismatch);
    }
    Ok(())
}

fn checked_add(base: usize, offset: usize) -> Result<usize, LocalPositionTerminalReason> {
    let address = base
        .checked_add(offset)
        .ok_or(LocalPositionTerminalReason::InvalidPointer)?;
    if address == 0 {
        return Err(LocalPositionTerminalReason::InvalidPointer);
    }
    Ok(address)
}

fn decode_camera_block(
    camera: &[u8; CAMERA_BLOCK_BYTES],
) -> Result<Option<LocalPositionObservation>, LocalPositionTerminalReason> {
    let x = read_f64(camera, 0);
    let y = read_f64(camera, 8);
    let z = read_f64(camera, 16);
    let yaw_degrees = read_f64(camera, 32);
    if !x.is_finite()
        || !y.is_finite()
        || !z.is_finite()
        || !yaw_degrees.is_finite()
        || x.abs() > MAX_ABSOLUTE_WORLD_COORDINATE
        || y.abs() > MAX_ABSOLUTE_WORLD_COORDINATE
        || z.abs() > MAX_ABSOLUTE_WORLD_COORDINATE
        || !(MIN_PLAUSIBLE_YAW..=MAX_PLAUSIBLE_YAW).contains(&yaw_degrees)
    {
        return Err(LocalPositionTerminalReason::InvalidPosition);
    }
    // The exact build exposes an all-zero camera block while the process is at the title
    // screen. It is a retryable "no active world" state, not a player at the map origin.
    if x == 0.0 && y == 0.0 && z == 0.0 && yaw_degrees == 0.0 {
        return Ok(None);
    }
    Ok(Some(LocalPositionObservation {
        x,
        y,
        z,
        yaw_degrees,
    }))
}

fn read_f64(bytes: &[u8; CAMERA_BLOCK_BYTES], offset: usize) -> f64 {
    let mut value = [0_u8; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    f64::from_le_bytes(value)
}

fn derive_connection_boot_id(mut base: [u8; 32], generation: u64) -> [u8; 32] {
    let generation = generation.to_le_bytes();
    for (index, byte) in generation.into_iter().enumerate() {
        base[24 + index] ^= byte;
    }
    base
}
