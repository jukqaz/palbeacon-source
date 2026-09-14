use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use pal_state::{
    ClockInvalidReason, PositionSource, PositionSourceError, PositionSourceEvent, ReplayConfig,
    ReplayError, ReplayPositionSource,
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct TempFixture(PathBuf);

impl TempFixture {
    fn write(contents: &str) -> Self {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("pal-state-replay-{}-{id}.json", std::process::id()));
        fs::write(&path, contents).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn square_route() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replay/square-route.json")
}

fn config(loop_playback: bool, start_ms: u64) -> ReplayConfig {
    ReplayConfig {
        world_alias: "safe-world".to_owned(),
        subject_id: b"safe-subject".to_vec(),
        starting_generation: 7,
        base_boot_id: b"replay-boot".to_vec(),
        loop_playback,
        starting_monotonic_ms: start_ms,
    }
}

fn expect_connected(event: Option<PositionSourceEvent>, generation: u64) {
    assert_eq!(event, Some(PositionSourceEvent::Connected { generation }));
}

fn expect_sample(
    event: Option<PositionSourceEvent>,
    generation: u64,
    sequence: u64,
    x: f64,
    heading: f32,
) {
    let Some(PositionSourceEvent::Sample(sample)) = event else {
        panic!("expected a sample event");
    };
    assert_eq!(sample.source_connection_generation(), generation);
    assert_eq!(sample.sequence(), sequence);
    assert_eq!(sample.x(), x);
    assert_eq!(sample.heading_degrees(), Some(heading));
}

#[test]
fn strict_fixture_validation_distinguishes_failures() {
    let empty = TempFixture::write("[]");
    assert_eq!(
        ReplayPositionSource::from_path(empty.path(), config(false, 0)).unwrap_err(),
        ReplayError::EmptyFixture
    );

    let first_nonzero =
        TempFixture::write(r#"[{"at_ms":1,"x":0.0,"y":0.0,"z":0.0,"heading_degrees":null}]"#);
    assert_eq!(
        ReplayPositionSource::from_path(first_nonzero.path(), config(false, 0)).unwrap_err(),
        ReplayError::FirstTimestampNotZero
    );

    let nonincreasing = TempFixture::write(
        r#"[
          {"at_ms":0,"x":0.0,"y":0.0,"z":0.0,"heading_degrees":null},
          {"at_ms":0,"x":1.0,"y":0.0,"z":0.0,"heading_degrees":null}
        ]"#,
    );
    assert_eq!(
        ReplayPositionSource::from_path(nonincreasing.path(), config(false, 0)).unwrap_err(),
        ReplayError::NonIncreasingTimestamp { index: 1 }
    );

    let unknown = TempFixture::write(
        r#"[{"at_ms":0,"x":0.0,"y":0.0,"z":0.0,"heading_degrees":null,"extra":1}]"#,
    );
    assert_eq!(
        ReplayPositionSource::from_path(unknown.path(), config(false, 0)).unwrap_err(),
        ReplayError::Json
    );

    let nonfinite =
        TempFixture::write(r#"[{"at_ms":0,"x":1e999,"y":0.0,"z":0.0,"heading_degrees":null}]"#);
    assert_eq!(
        ReplayPositionSource::from_path(nonfinite.path(), config(false, 0)).unwrap_err(),
        ReplayError::NonFiniteField {
            index: 0,
            field: "x",
        }
    );

    let zero_duration =
        TempFixture::write(r#"[{"at_ms":0,"x":0.0,"y":0.0,"z":0.0,"heading_degrees":null}]"#);
    assert_eq!(
        ReplayPositionSource::from_path(zero_duration.path(), config(true, 0)).unwrap_err(),
        ReplayError::LoopRequiresPositiveDuration
    );
}

#[test]
fn io_and_json_errors_do_not_disclose_path_or_raw_content() {
    let missing = std::env::temp_dir().join("private-user-secret-replay.json");
    let error = ReplayPositionSource::from_path(&missing, config(false, 0)).unwrap_err();
    assert_eq!(error, ReplayError::Io);
    assert!(!error.to_string().contains("private-user-secret"));

    let malformed = TempFixture::write("{ raw-private-fixture-content");
    let error = ReplayPositionSource::from_path(malformed.path(), config(false, 0)).unwrap_err();
    assert_eq!(error, ReplayError::Json);
    assert!(!error.to_string().contains("raw-private-fixture-content"));
}

#[test]
fn no_loop_emits_connected_samples_and_one_disconnect() {
    let mut replay =
        ReplayPositionSource::from_path(square_route(), config(false, 10_000)).unwrap();

    expect_connected(replay.poll(10_000).unwrap(), 7);
    expect_sample(replay.poll(10_000).unwrap(), 7, 1, 0.0, 359.0);
    assert_eq!(replay.poll(10_000).unwrap(), None);
    expect_sample(replay.poll(10_400).unwrap(), 7, 2, 10.0, 1.0);
    expect_sample(replay.poll(10_400).unwrap(), 7, 3, 10.0, 90.0);
    expect_sample(replay.poll(10_400).unwrap(), 7, 4, 0.0, 180.0);
    expect_sample(replay.poll(10_400).unwrap(), 7, 5, 0.0, 270.0);
    assert_eq!(
        replay.poll(10_400).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 7 })
    );
    assert_eq!(replay.poll(10_400).unwrap(), None);
    assert_eq!(replay.poll(20_000).unwrap(), None);
}

#[test]
fn same_time_polls_drain_due_events_and_preserve_route_order() {
    let mut replay = ReplayPositionSource::from_path(square_route(), config(false, 1_000)).unwrap();
    expect_connected(replay.poll(1_250).unwrap(), 7);
    expect_sample(replay.poll(1_250).unwrap(), 7, 1, 0.0, 359.0);
    expect_sample(replay.poll(1_250).unwrap(), 7, 2, 10.0, 1.0);
    expect_sample(replay.poll(1_250).unwrap(), 7, 3, 10.0, 90.0);
    assert_eq!(replay.poll(1_250).unwrap(), None);
}

#[test]
fn fixture_is_loaded_once_and_poll_never_reads_the_file() {
    let fixture =
        TempFixture::write(r#"[{"at_ms":0,"x":4.0,"y":5.0,"z":6.0,"heading_degrees":45.0}]"#);
    let mut replay = ReplayPositionSource::from_path(fixture.path(), config(false, 100)).unwrap();
    fs::remove_file(fixture.path()).unwrap();

    expect_connected(replay.poll(100).unwrap(), 7);
    expect_sample(replay.poll(100).unwrap(), 7, 1, 4.0, 45.0);
}

#[test]
fn sample_clock_uses_receive_time_and_scheduled_age_upper_bound() {
    let mut replay = ReplayPositionSource::from_path(square_route(), config(false, 1_000)).unwrap();
    expect_connected(replay.poll(1_250).unwrap(), 7);
    let Some(PositionSourceEvent::Sample(sample)) = replay.poll(1_250).unwrap() else {
        panic!("expected sample");
    };
    assert_eq!(sample.clock().received_at_monotonic_ms, 1_250);
    assert_eq!(sample.clock().age_at_receive_upper_bound_ms, 250);
}

#[test]
fn loop_boundary_changes_generation_and_boot_and_resets_sequence() {
    let mut replay = ReplayPositionSource::from_path(square_route(), config(true, 5_000)).unwrap();
    expect_connected(replay.poll(5_400).unwrap(), 7);

    let mut first_boot = Vec::new();
    for expected_sequence in 1..=5 {
        let Some(PositionSourceEvent::Sample(sample)) = replay.poll(5_400).unwrap() else {
            panic!("expected sample");
        };
        if expected_sequence == 1 {
            first_boot = sample.agent_boot_id().to_vec();
        }
        assert_eq!(sample.sequence(), expected_sequence);
    }
    assert_eq!(
        replay.poll(5_400).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 7 })
    );
    expect_connected(replay.poll(5_400).unwrap(), 8);
    let Some(PositionSourceEvent::Sample(first_next)) = replay.poll(5_400).unwrap() else {
        panic!("expected first sample of next loop");
    };
    assert_eq!(first_next.source_connection_generation(), 8);
    assert_eq!(first_next.sequence(), 1);
    assert!(!first_next.agent_boot_id().is_empty());
    assert_ne!(first_next.agent_boot_id(), first_boot);
}

#[test]
fn monotonic_regression_emits_once_without_advancing_and_then_recovers() {
    let mut replay = ReplayPositionSource::from_path(square_route(), config(false, 1_000)).unwrap();
    expect_connected(replay.poll(1_000).unwrap(), 7);
    expect_sample(replay.poll(1_000).unwrap(), 7, 1, 0.0, 359.0);

    assert_eq!(
        replay.poll(999).unwrap(),
        Some(PositionSourceEvent::ClockInvalid {
            generation: 7,
            reason: ClockInvalidReason::MonotonicRegression,
        })
    );
    assert_eq!(replay.poll(999).unwrap(), None);
    expect_sample(replay.poll(1_100).unwrap(), 7, 2, 10.0, 1.0);
}

#[test]
fn loop_generation_overflow_is_a_typed_runtime_error() {
    let mut overflow = config(true, 0);
    overflow.starting_generation = u64::MAX;
    let mut replay = ReplayPositionSource::from_path(square_route(), overflow).unwrap();
    expect_connected(replay.poll(400).unwrap(), u64::MAX);
    for _ in 0..5 {
        assert!(matches!(
            replay.poll(400).unwrap(),
            Some(PositionSourceEvent::Sample(_))
        ));
    }
    assert_eq!(
        replay.poll(400).unwrap(),
        Some(PositionSourceEvent::Disconnected {
            generation: u64::MAX,
        })
    );
    assert_eq!(
        replay.poll(400).unwrap_err(),
        PositionSourceError::Replay(ReplayError::GenerationOverflow)
    );
}
