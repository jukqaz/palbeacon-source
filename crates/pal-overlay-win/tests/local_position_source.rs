#![cfg(feature = "development-local-readonly-position")]

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, VecDeque},
    rc::Rc,
};

use pal_overlay_win::local_position_source::{
    BUILD_24181527_PROFILE, LocalMemoryReadError, LocalPositionMemoryReader,
    LocalPositionReadSession, LocalPositionSourceIdentity, LocalPositionStep,
    LocalPositionTerminalReason, LocalProcessIdentity, PalworldBuild24181527PositionPump,
};
use pal_state::{PositionSource, PositionSourceEvent};

const SUBJECT: [u8; 32] = [0x31; 32];
const BOOT: [u8; 32] = [0x52; 32];

#[derive(Clone)]
struct FakeReader {
    state: Rc<RefCell<FakeState>>,
}

struct FakeState {
    identity: Result<LocalProcessIdentity, LocalMemoryReadError>,
    bytes: BTreeMap<usize, u8>,
    scripted: HashMap<usize, VecDeque<Result<Vec<u8>, LocalMemoryReadError>>>,
    reads: Vec<(usize, usize)>,
    session_starts: usize,
}

impl FakeReader {
    fn exact() -> Self {
        let profile = BUILD_24181527_PROFILE;
        Self {
            state: Rc::new(RefCell::new(FakeState {
                identity: Ok(LocalProcessIdentity::new(
                    profile.build_id(),
                    profile.executable_sha256(),
                    profile.executable_file_size(),
                    0x0000_0001_4000_0000,
                    profile.loaded_module_size(),
                )),
                bytes: BTreeMap::new(),
                scripted: HashMap::new(),
                reads: Vec::new(),
                session_starts: 0,
            })),
        }
    }

    fn identity(&self) -> LocalProcessIdentity {
        self.state.borrow().identity.unwrap()
    }

    fn set_identity(&self, identity: LocalProcessIdentity) {
        self.state.borrow_mut().identity = Ok(identity);
    }

    fn write(&self, address: usize, bytes: &[u8]) {
        let mut state = self.state.borrow_mut();
        for (offset, byte) in bytes.iter().copied().enumerate() {
            state.bytes.insert(address + offset, byte);
        }
    }

    fn write_u64(&self, address: usize, value: u64) {
        self.write(address, &value.to_le_bytes());
    }

    fn script(&self, address: usize, values: impl IntoIterator<Item = Vec<u8>>) {
        self.state
            .borrow_mut()
            .scripted
            .insert(address, values.into_iter().map(Ok).collect::<VecDeque<_>>());
    }

    fn fail_next(&self, address: usize) {
        self.state.borrow_mut().scripted.insert(
            address,
            VecDeque::from([Err(LocalMemoryReadError::Unavailable)]),
        );
    }

    fn install_chain(&self, x: f64, y: f64, z: f64, yaw: f64) -> ChainFixture {
        let profile = BUILD_24181527_PROFILE;
        let identity = self.identity();
        let root_address = identity.module_base() + profile.root_rva();
        let pointers = [
            0x0000_0002_1000_0000_u64,
            0x0000_0002_2000_0000,
            0x0000_0002_3000_0000,
            0x0000_0002_4000_0000,
            0x0000_0002_5000_0000,
            0x0000_0002_6000_0000,
            0x0000_0002_7000_0000,
        ];
        let offsets = profile.pointer_read_offsets();
        self.write_u64(root_address + offsets[0], pointers[0]);
        for index in 1..offsets.len() {
            self.write_u64(
                pointers[index - 1] as usize + offsets[index],
                pointers[index],
            );
        }
        let final_pointer_slot = pointers[pointers.len() - 2] as usize + offsets[offsets.len() - 1];
        let camera_address = pointers[pointers.len() - 1] as usize + profile.camera_offset();
        let mut camera = [0_u8; 40];
        camera[0..8].copy_from_slice(&x.to_le_bytes());
        camera[8..16].copy_from_slice(&y.to_le_bytes());
        camera[16..24].copy_from_slice(&z.to_le_bytes());
        camera[32..40].copy_from_slice(&yaw.to_le_bytes());
        self.write(camera_address, &camera);
        ChainFixture {
            root_address,
            root_pointer: pointers[0],
            final_pointer_slot,
            final_pointer: pointers[pointers.len() - 1],
            pointer_slots: {
                let mut slots = [0_usize; 7];
                slots[0] = root_address + offsets[0];
                for index in 1..slots.len() {
                    slots[index] = pointers[index - 1] as usize + offsets[index];
                }
                slots
            },
            pointers: pointers.map(|pointer| pointer as usize),
            camera_address,
            camera,
        }
    }

    fn reads(&self) -> Vec<(usize, usize)> {
        self.state.borrow().reads.clone()
    }

    fn session_starts(&self) -> usize {
        self.state.borrow().session_starts
    }
}

impl LocalPositionReadSession for FakeReader {
    fn read_exact(
        &mut self,
        address: usize,
        output: &mut [u8],
    ) -> Result<(), LocalMemoryReadError> {
        let mut state = self.state.borrow_mut();
        state.reads.push((address, output.len()));
        if let Some(scripted) = state.scripted.get_mut(&address)
            && let Some(result) = scripted.pop_front()
        {
            let bytes = result?;
            if bytes.len() != output.len() {
                return Err(LocalMemoryReadError::Unavailable);
            }
            output.copy_from_slice(&bytes);
            return Ok(());
        }
        for (offset, output_byte) in output.iter_mut().enumerate() {
            *output_byte = *state
                .bytes
                .get(&(address + offset))
                .ok_or(LocalMemoryReadError::Unavailable)?;
        }
        Ok(())
    }
}

impl LocalPositionMemoryReader for FakeReader {
    fn identity(&mut self) -> Result<LocalProcessIdentity, LocalMemoryReadError> {
        self.state.borrow().identity
    }

    fn with_read_session<T>(
        &mut self,
        operation: impl FnOnce(&mut dyn LocalPositionReadSession) -> T,
    ) -> Result<T, LocalMemoryReadError> {
        self.state.borrow_mut().session_starts += 1;
        Ok(operation(self))
    }
}

struct ChainFixture {
    root_address: usize,
    root_pointer: u64,
    final_pointer_slot: usize,
    final_pointer: u64,
    pointer_slots: [usize; 7],
    pointers: [usize; 7],
    camera_address: usize,
    camera: [u8; 40],
}

fn source_identity(generation: u64) -> LocalPositionSourceIdentity {
    LocalPositionSourceIdentity::new("local-main-map", SUBJECT, BOOT, generation).unwrap()
}

fn connected(source: &mut impl PositionSource, generation: u64) {
    assert_eq!(
        source.poll(0).unwrap(),
        Some(PositionSourceEvent::Connected { generation })
    );
}

fn sample(source: &mut impl PositionSource, now: u64) -> pal_domain::PositionSample {
    match source.poll(now).unwrap() {
        Some(PositionSourceEvent::Sample(sample)) => sample,
        other => panic!("expected sample, got {other:?}"),
    }
}

#[test]
fn exact_profile_is_immutable_and_gate_b_remains_unapproved() {
    let profile = BUILD_24181527_PROFILE;
    assert_eq!(profile.build_id(), 24_575_825);
    assert_eq!(profile.executable_file_size(), 161_397_248);
    assert_eq!(profile.loaded_module_size(), 167_432_192);
    assert_eq!(profile.root_rva(), 0x966_7260);
    assert_eq!(
        profile.pointer_read_offsets(),
        &[0, 0x1b8, 0x38, 0, 0x30, 0x348, 0x298]
    );
    assert_eq!(profile.camera_offset(), 0x128);
    assert_eq!(profile.sample_interval_ms(), 100);
    assert!(!profile.gate_b_approved());
}

#[test]
fn matching_identity_emits_one_contiguous_camera_sample_at_ten_hz() {
    let reader = FakeReader::exact();
    let chain = reader.install_chain(-400_000.0, 125_000.0, 9_000.0, -45.0);
    let (mut pump, mut source) =
        PalworldBuild24181527PositionPump::new(reader.clone(), source_identity(7)).unwrap();
    connected(&mut source, 7);

    assert_eq!(
        pump.step(10_000),
        LocalPositionStep::Published {
            generation: 7,
            sequence: 1
        }
    );
    let first = sample(&mut source, 10_000);
    assert_eq!(
        (first.x(), first.y(), first.z()),
        (-400_000.0, 125_000.0, 9_000.0)
    );
    assert_eq!(first.heading_degrees(), Some(315.0));
    assert_eq!(pump.step(10_099), LocalPositionStep::Idle);
    assert_eq!(source.poll(10_099).unwrap(), None);
    assert_eq!(
        pump.step(10_100),
        LocalPositionStep::Published {
            generation: 7,
            sequence: 2
        }
    );
    assert_eq!(sample(&mut source, 10_100).sequence(), 2);

    let reads = reader.reads();
    assert_eq!(
        reader.session_starts(),
        2,
        "each due sample must use exactly one bracketed read session"
    );
    assert_eq!(
        reads
            .iter()
            .filter(|read| **read == (chain.camera_address, 40))
            .count(),
        2,
        "each 10 Hz sample must use one contiguous camera-block read"
    );
}

#[test]
fn dropping_an_active_pump_immediately_disconnects_the_source() {
    let reader = FakeReader::exact();
    reader.install_chain(-400_000.0, 125_000.0, 9_000.0, 0.0);
    let (pump, mut source) =
        PalworldBuild24181527PositionPump::new(reader, source_identity(13)).unwrap();
    connected(&mut source, 13);

    drop(pump);

    assert_eq!(
        source.poll(0).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 13 })
    );
    assert_eq!(source.poll(0).unwrap(), None);
}

#[test]
fn constructor_rejects_every_exact_build_identity_mismatch() {
    let exact = FakeReader::exact().identity();
    let mismatches = [
        LocalProcessIdentity::new(
            1,
            exact.executable_sha256(),
            exact.executable_file_size(),
            exact.module_base(),
            exact.module_size(),
        ),
        LocalProcessIdentity::new(
            exact.build_id(),
            [0xAA; 32],
            exact.executable_file_size(),
            exact.module_base(),
            exact.module_size(),
        ),
        LocalProcessIdentity::new(
            exact.build_id(),
            exact.executable_sha256(),
            1,
            exact.module_base(),
            exact.module_size(),
        ),
        LocalProcessIdentity::new(
            exact.build_id(),
            exact.executable_sha256(),
            exact.executable_file_size(),
            exact.module_base(),
            1,
        ),
        LocalProcessIdentity::new(
            exact.build_id(),
            exact.executable_sha256(),
            exact.executable_file_size(),
            0,
            exact.module_size(),
        ),
    ];
    for identity in mismatches {
        let reader = FakeReader::exact();
        reader.set_identity(identity);
        let error = match PalworldBuild24181527PositionPump::new(reader, source_identity(1)) {
            Ok(_) => panic!("mismatched identity was accepted"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            pal_overlay_win::local_position_source::LocalPositionSourceError::ExactBuildMismatch
        );
    }
}

#[test]
fn pointer_arithmetic_overflow_fails_closed_without_a_sample() {
    let reader = FakeReader::exact();
    let identity = reader.identity();
    let root = identity.module_base() + BUILD_24181527_PROFILE.root_rva();
    reader.write_u64(root, u64::MAX);
    let (mut pump, mut source) =
        PalworldBuild24181527PositionPump::new(reader, source_identity(2)).unwrap();
    connected(&mut source, 2);
    assert!(matches!(
        pump.step(0),
        LocalPositionStep::Disconnected {
            generation: 2,
            reason: LocalPositionTerminalReason::InvalidPointer
        }
    ));
    assert_eq!(
        source.poll(0).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 2 })
    );
    assert_eq!(source.poll(0).unwrap(), None);
}

#[test]
fn title_screen_null_player_chain_waits_and_then_publishes_after_world_join() {
    let reader = FakeReader::exact();
    let identity = reader.identity();
    let root = identity.module_base() + BUILD_24181527_PROFILE.root_rva();
    reader.write_u64(root, 0);
    let (mut pump, mut source) =
        PalworldBuild24181527PositionPump::new(reader.clone(), source_identity(16)).unwrap();
    connected(&mut source, 16);

    assert_eq!(pump.step(0), LocalPositionStep::Idle);
    assert_eq!(pump.terminal_reason(), None);
    assert_eq!(source.poll(0).unwrap(), None);

    reader.install_chain(-325_000.0, 223_000.0, -1_300.0, 217.0);
    assert_eq!(
        pump.step(100),
        LocalPositionStep::Published {
            generation: 16,
            sequence: 1,
        }
    );
    let observed = sample(&mut source, 100);
    assert_eq!(
        (observed.x(), observed.y(), observed.z()),
        (-325_000.0, 223_000.0, -1_300.0)
    );
}

#[test]
fn leaving_a_world_publishes_immediate_unavailable_and_recovers_in_place() {
    let reader = FakeReader::exact();
    let identity = reader.identity();
    let root = identity.module_base() + BUILD_24181527_PROFILE.root_rva();
    reader.install_chain(-325_000.0, 223_000.0, -1_300.0, 217.0);
    let (mut pump, mut source) =
        PalworldBuild24181527PositionPump::new(reader.clone(), source_identity(17)).unwrap();
    connected(&mut source, 17);

    assert!(matches!(
        pump.step(0),
        LocalPositionStep::Published {
            generation: 17,
            sequence: 1
        }
    ));
    let _ = sample(&mut source, 0);

    reader.write_u64(root, 0);
    assert_eq!(
        pump.step(100),
        LocalPositionStep::Unavailable { generation: 17 }
    );
    assert_eq!(
        source.poll(100).unwrap(),
        Some(PositionSourceEvent::Unavailable { generation: 17 })
    );
    assert_eq!(pump.step(200), LocalPositionStep::Idle);
    assert_eq!(source.poll(200).unwrap(), None);

    reader.install_chain(-300_000.0, 200_000.0, -900.0, 180.0);
    assert_eq!(
        pump.step(300),
        LocalPositionStep::Published {
            generation: 17,
            sequence: 2
        }
    );
    let recovered = sample(&mut source, 300);
    assert_eq!(recovered.sequence(), 2);
    assert_eq!((recovered.x(), recovered.y()), (-300_000.0, 200_000.0));
}

#[test]
fn changed_root_or_final_pointer_recheck_rejects_the_torn_sample() {
    for change_root in [true, false] {
        let reader = FakeReader::exact();
        let chain = reader.install_chain(-400_000.0, 125_000.0, 9_000.0, 90.0);
        if change_root {
            reader.script(
                chain.root_address,
                [
                    chain.root_pointer.to_le_bytes().to_vec(),
                    0x7777_u64.to_le_bytes().to_vec(),
                ],
            );
        } else {
            reader.script(
                chain.final_pointer_slot,
                [
                    chain.final_pointer.to_le_bytes().to_vec(),
                    0x8888_u64.to_le_bytes().to_vec(),
                ],
            );
        }
        let (mut pump, mut source) =
            PalworldBuild24181527PositionPump::new(reader, source_identity(3)).unwrap();
        connected(&mut source, 3);
        assert!(matches!(
            pump.step(0),
            LocalPositionStep::Disconnected {
                generation: 3,
                reason: LocalPositionTerminalReason::UnstablePointerChain
            }
        ));
        assert_eq!(
            source.poll(0).unwrap(),
            Some(PositionSourceEvent::Disconnected { generation: 3 })
        );
    }
}

#[test]
fn changed_intermediate_link_is_rejected_even_if_it_converges_on_the_same_camera() {
    let reader = FakeReader::exact();
    let chain = reader.install_chain(-400_000.0, 125_000.0, 9_000.0, 90.0);
    let changed_index = 3;
    let alternate_pointer = 0x0000_0002_8000_0000_usize;
    let next_offset = BUILD_24181527_PROFILE.pointer_read_offsets()[changed_index + 1];
    reader.write_u64(
        alternate_pointer + next_offset,
        chain.pointers[changed_index + 1] as u64,
    );
    reader.script(
        chain.pointer_slots[changed_index],
        [
            (chain.pointers[changed_index] as u64)
                .to_le_bytes()
                .to_vec(),
            (alternate_pointer as u64).to_le_bytes().to_vec(),
        ],
    );
    let (mut pump, mut source) =
        PalworldBuild24181527PositionPump::new(reader, source_identity(11)).unwrap();
    connected(&mut source, 11);

    assert_eq!(
        pump.step(0),
        LocalPositionStep::Disconnected {
            generation: 11,
            reason: LocalPositionTerminalReason::UnstablePointerChain,
        }
    );
    assert_eq!(
        source.poll(0).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 11 })
    );
}

#[test]
fn camera_block_is_read_once_and_invalid_values_fail_closed() {
    let invalid = [
        (f64::NAN, 0.0, 0.0, 0.0),
        (-2_000_001.0, 0.0, 0.0, 0.0),
        (2_000_001.0, 0.0, 0.0, 0.0),
        (0.0, -2_000_001.0, 0.0, 0.0),
        (0.0, 2_000_001.0, 0.0, 0.0),
        (0.0, 0.0, f64::INFINITY, 0.0),
        (0.0, 0.0, 0.0, 361.0),
    ];
    for (x, y, z, yaw) in invalid {
        let reader = FakeReader::exact();
        let chain = reader.install_chain(x, y, z, yaw);
        let (mut pump, mut source) =
            PalworldBuild24181527PositionPump::new(reader.clone(), source_identity(4)).unwrap();
        connected(&mut source, 4);
        assert!(matches!(
            pump.step(0),
            LocalPositionStep::Disconnected {
                generation: 4,
                reason: LocalPositionTerminalReason::InvalidPosition
            }
        ));
        assert_eq!(
            reader
                .reads()
                .iter()
                .filter(|read| **read == (chain.camera_address, chain.camera.len()))
                .count(),
            1
        );
    }
}

#[test]
fn title_screen_zero_camera_waits_and_then_publishes_after_world_join() {
    let reader = FakeReader::exact();
    let chain = reader.install_chain(0.0, 0.0, 0.0, 0.0);
    let (mut pump, mut source) =
        PalworldBuild24181527PositionPump::new(reader.clone(), source_identity(15)).unwrap();
    connected(&mut source, 15);

    assert_eq!(pump.step(0), LocalPositionStep::Idle);
    assert_eq!(pump.terminal_reason(), None);
    assert_eq!(source.poll(0).unwrap(), None);

    let mut live_camera = chain.camera;
    live_camera[0..8].copy_from_slice(&(-400_000.0_f64).to_le_bytes());
    live_camera[8..16].copy_from_slice(&(125_000.0_f64).to_le_bytes());
    live_camera[16..24].copy_from_slice(&(9_000.0_f64).to_le_bytes());
    live_camera[32..40].copy_from_slice(&(45.0_f64).to_le_bytes());
    reader.write(chain.camera_address, &live_camera);

    assert_eq!(
        pump.step(100),
        LocalPositionStep::Published {
            generation: 15,
            sequence: 1,
        }
    );
    let observed = sample(&mut source, 100);
    assert_eq!(
        (observed.x(), observed.y(), observed.z()),
        (-400_000.0, 125_000.0, 9_000.0)
    );
}

#[test]
fn raw_position_collection_is_not_coupled_to_the_unapproved_main_map_bounds() {
    let reader = FakeReader::exact();
    reader.install_chain(500_000.0, -800_000.0, 9_000.0, 0.0);
    let (mut pump, mut source) =
        PalworldBuild24181527PositionPump::new(reader, source_identity(12)).unwrap();
    connected(&mut source, 12);

    assert!(matches!(
        pump.step(0),
        LocalPositionStep::Published {
            generation: 12,
            sequence: 1
        }
    ));
    let observed = sample(&mut source, 0);
    assert_eq!((observed.x(), observed.y()), (500_000.0, -800_000.0));
}

#[test]
fn read_failure_and_live_identity_change_disconnect_once_and_never_leave_stale_data() {
    for identity_change in [false, true] {
        let reader = FakeReader::exact();
        let chain = reader.install_chain(-400_000.0, 125_000.0, 9_000.0, 45.0);
        let (mut pump, mut source) =
            PalworldBuild24181527PositionPump::new(reader.clone(), source_identity(5)).unwrap();
        connected(&mut source, 5);
        assert!(matches!(pump.step(0), LocalPositionStep::Published { .. }));
        let _ = sample(&mut source, 0);
        if identity_change {
            let identity = reader.identity();
            reader.set_identity(LocalProcessIdentity::new(
                identity.build_id(),
                [0xAB; 32],
                identity.executable_file_size(),
                identity.module_base(),
                identity.module_size(),
            ));
        } else {
            reader.fail_next(chain.root_address);
        }
        assert!(matches!(
            pump.step(100),
            LocalPositionStep::Disconnected { generation: 5, .. }
        ));
        assert_eq!(
            source.poll(100).unwrap(),
            Some(PositionSourceEvent::Disconnected { generation: 5 })
        );
        assert_eq!(pump.step(200), LocalPositionStep::Idle);
        assert_eq!(source.poll(200).unwrap(), None);
    }
}

#[test]
fn terminal_disconnect_releases_the_reader_before_the_pump_is_dropped() {
    struct DropProbeReader {
        inner: FakeReader,
        dropped: Rc<RefCell<bool>>,
    }

    impl Drop for DropProbeReader {
        fn drop(&mut self) {
            *self.dropped.borrow_mut() = true;
        }
    }

    impl LocalPositionMemoryReader for DropProbeReader {
        fn identity(&mut self) -> Result<LocalProcessIdentity, LocalMemoryReadError> {
            LocalPositionMemoryReader::identity(&mut self.inner)
        }

        fn with_read_session<T>(
            &mut self,
            operation: impl FnOnce(&mut dyn LocalPositionReadSession) -> T,
        ) -> Result<T, LocalMemoryReadError> {
            self.inner.with_read_session(operation)
        }
    }

    let dropped = Rc::new(RefCell::new(false));
    let reader = FakeReader::exact();
    let chain = reader.install_chain(-400_000.0, 125_000.0, 9_000.0, 45.0);
    let probe = DropProbeReader {
        inner: reader.clone(),
        dropped: Rc::clone(&dropped),
    };
    let (mut pump, mut source) =
        PalworldBuild24181527PositionPump::new(probe, source_identity(14)).unwrap();
    connected(&mut source, 14);
    reader.fail_next(chain.root_address);

    assert!(matches!(
        pump.step(0),
        LocalPositionStep::Disconnected { generation: 14, .. }
    ));
    assert!(
        *dropped.borrow(),
        "terminal disconnect must release the executable/process handles immediately"
    );
    assert_eq!(
        source.poll(0).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 14 })
    );
}

#[test]
fn explicit_reconnect_increments_generation_and_keeps_sequence_monotonic() {
    let first_reader = FakeReader::exact();
    let first_chain = first_reader.install_chain(-400_000.0, 125_000.0, 9_000.0, 0.0);
    let (mut pump, mut source) =
        PalworldBuild24181527PositionPump::new(first_reader.clone(), source_identity(8)).unwrap();
    connected(&mut source, 8);
    assert_eq!(
        pump.step(0),
        LocalPositionStep::Published {
            generation: 8,
            sequence: 1
        }
    );
    let first = sample(&mut source, 0);
    assert_eq!(first.sequence(), 1);
    first_reader.fail_next(first_chain.root_address);
    assert!(matches!(
        pump.step(100),
        LocalPositionStep::Disconnected { .. }
    ));
    assert_eq!(
        source.poll(100).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 8 })
    );

    let second_reader = FakeReader::exact();
    second_reader.install_chain(-399_000.0, 126_000.0, 10_000.0, 10.0);
    pump.reconnect(second_reader, 200).unwrap();
    assert_eq!(
        source.poll(200).unwrap(),
        Some(PositionSourceEvent::Connected { generation: 9 })
    );
    assert_eq!(
        pump.step(200),
        LocalPositionStep::Published {
            generation: 9,
            sequence: 2
        }
    );
    let second = sample(&mut source, 200);
    assert_eq!(second.source_connection_generation(), 9);
    assert_eq!(second.sequence(), 2);
    assert_ne!(first.agent_boot_id(), second.agent_boot_id());
}

#[test]
fn monotonic_clock_regression_hides_then_disconnects() {
    let reader = FakeReader::exact();
    reader.install_chain(-400_000.0, 125_000.0, 9_000.0, 0.0);
    let (mut pump, mut source) =
        PalworldBuild24181527PositionPump::new(reader, source_identity(10)).unwrap();
    connected(&mut source, 10);
    assert!(matches!(
        pump.step(100),
        LocalPositionStep::Published { .. }
    ));
    let _ = sample(&mut source, 100);
    assert_eq!(
        pump.step(99),
        LocalPositionStep::ClockInvalid { generation: 10 }
    );
    assert!(matches!(
        source.poll(99).unwrap(),
        Some(PositionSourceEvent::ClockInvalid { generation: 10, .. })
    ));
    assert_eq!(
        pump.step(100),
        LocalPositionStep::Disconnected {
            generation: 10,
            reason: LocalPositionTerminalReason::MonotonicRegression
        }
    );
    assert_eq!(
        source.poll(100).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 10 })
    );
}
