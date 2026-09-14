use std::time::{Duration, Instant, SystemTime};

use pal_agent::{AgentPipeline, HealthMonitor};
use pal_protocol::v2::{CoordinateSpace, HeadingSource};
use pal_rest::{SafePlayerObservation, TimedResponse};
use pal_telemetry::LatestTelemetry;

const SUBJECT: [u8; 32] = [0x42; 32];
const PROFILE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn observation(subject: &str, heading: Option<f32>) -> TimedResponse<SafePlayerObservation> {
    TimedResponse {
        value: SafePlayerObservation {
            subject_id: subject.to_owned(),
            x: 123.5,
            y: -45.25,
            z: 9.0,
            heading_degrees: heading,
            server_fps: 60.0,
            average_server_fps: 59.9,
            actor_count: 12,
        },
        response_latency: Duration::from_millis(10),
        decode_latency: Duration::from_millis(1),
        body_bytes: 512,
        actor_count: 12,
        rest_completed_at: SystemTime::now(),
        completed_monotonic: Instant::now(),
    }
}

fn subject_hex() -> String {
    SUBJECT.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn rig(rotation_z_validated: bool) -> (AgentPipeline, LatestTelemetry, HealthMonitor) {
    let latest = LatestTelemetry::new("world-a", SUBJECT, PROFILE).unwrap();
    let health = HealthMonitor::default();
    let pipeline = AgentPipeline::new(
        "world-a",
        SUBJECT,
        PROFILE,
        rotation_z_validated,
        latest.clone(),
        health.clone(),
    )
    .unwrap();
    (pipeline, latest, health)
}

#[test]
fn sanitized_rest_position_becomes_exactly_one_latest_telemetry_sample() {
    let (mut pipeline, latest, health) = rig(true);

    pipeline
        .emit(observation(&subject_hex(), Some(270.0)))
        .unwrap();

    let envelope = latest.current().unwrap();
    assert_eq!(latest.buffered_state_count(), 1);
    assert_eq!(envelope.sequence, 1);
    assert_eq!(envelope.position_x, 123.5);
    assert_eq!(envelope.position_y, -45.25);
    assert_eq!(envelope.position_z, 9.0);
    assert_eq!(envelope.heading_degrees, Some(270.0));
    assert_eq!(
        envelope.heading_source,
        HeadingSource::RotationZValidated as i32
    );
    assert_eq!(
        envelope.coordinate_space,
        CoordinateSpace::OfficialGameDataWorldV1 as i32
    );
    assert_eq!(envelope.coordinate_profile_sha256, PROFILE);
    assert_eq!(health.snapshot().emitted_samples, 1);
}

#[test]
fn unvalidated_rotation_is_never_advertised_as_heading() {
    let (mut pipeline, latest, _) = rig(false);

    pipeline
        .emit(observation(&subject_hex(), Some(90.0)))
        .unwrap();

    let envelope = latest.current().unwrap();
    assert_eq!(envelope.heading_degrees, None);
    assert_eq!(envelope.heading_source, HeadingSource::Unspecified as i32);
}

#[test]
fn rejected_sanitized_identity_does_not_consume_sequence() {
    let (mut pipeline, latest, health) = rig(true);

    assert!(pipeline.emit(observation(&"11".repeat(32), None)).is_err());
    assert!(latest.current().is_none());
    assert_eq!(health.snapshot().publish_failures, 1);
    pipeline.emit(observation(&subject_hex(), None)).unwrap();

    assert_eq!(latest.current().unwrap().sequence, 1);
}

#[test]
fn every_emit_error_is_counted_as_a_publication_failure() {
    let (mut pipeline, latest, health) = rig(true);

    let mut invalid_time = observation(&subject_hex(), None);
    invalid_time.rest_completed_at = SystemTime::UNIX_EPOCH;
    assert!(pipeline.emit(invalid_time).is_err());

    latest.close();
    assert!(pipeline.emit(observation(&subject_hex(), None)).is_err());

    let snapshot = health.snapshot();
    assert_eq!(snapshot.publish_failures, 2);
    assert_eq!(snapshot.emitted_samples, 0);
    assert_eq!(snapshot.consecutive_failures, 2);
}

#[test]
fn restart_uses_a_distinct_boot_and_resets_sequence_to_one() {
    let (mut first, first_latest, _) = rig(true);
    first.emit(observation(&subject_hex(), None)).unwrap();
    let first_sample = first_latest.current().unwrap();

    let (mut second, second_latest, _) = rig(true);
    second.emit(observation(&subject_hex(), None)).unwrap();
    let second_sample = second_latest.current().unwrap();

    assert_ne!(first_sample.boot_id, second_sample.boot_id);
    assert_eq!(first_sample.sequence, 1);
    assert_eq!(second_sample.sequence, 1);
}

#[test]
fn age_at_emit_is_monotonic_completion_age_and_saturates() {
    let (mut pipeline, latest, _) = rig(true);
    let mut sample = observation(&subject_hex(), None);
    sample.completed_monotonic = Instant::now() - Duration::from_millis(25);

    pipeline.emit(sample).unwrap();

    assert!(latest.current().unwrap().age_at_emit_ms >= 25);
}
