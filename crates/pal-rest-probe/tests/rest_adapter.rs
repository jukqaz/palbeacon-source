use std::time::{Duration, Instant, SystemTime};

use pal_rest::{RestError, SafePlayerObservation, ServerMetrics, TimedResponse};
use pal_rest_probe::RestWindowBuilder;

fn game_data() -> TimedResponse<SafePlayerObservation> {
    TimedResponse {
        value: SafePlayerObservation {
            subject_id: "sensitive-subject".into(),
            x: 987_654_321.125,
            y: 456.0,
            z: 789.0,
            heading_degrees: Some(42.0),
            server_fps: 60.0,
            average_server_fps: 59.5,
            actor_count: 100,
        },
        response_latency: Duration::from_millis(20),
        decode_latency: Duration::from_millis(2),
        body_bytes: 64_000,
        actor_count: 100,
        rest_completed_at: SystemTime::now(),
        completed_monotonic: Instant::now(),
    }
}

fn metrics() -> TimedResponse<ServerMetrics> {
    TimedResponse {
        value: ServerMetrics {
            server_fps: 60,
            current_player_count: 1,
            server_frame_time_ms: 16.0,
            max_player_count: 8,
            uptime_seconds: 100,
            base_camp_count: 1,
            game_days: 2,
        },
        response_latency: Duration::from_millis(5),
        decode_latency: Duration::from_millis(1),
        body_bytes: 1_000,
        actor_count: 0,
        rest_completed_at: SystemTime::now(),
        completed_monotonic: Instant::now(),
    }
}

#[test]
fn consumes_only_safe_timed_response_aggregates_and_discards_identity_and_coordinates() {
    let mut builder = RestWindowBuilder::new(300_000).expect("builder");
    for _ in 0..30 {
        builder
            .record_success(&game_data(), &metrics(), 0.4, 20 * 1024 * 1024)
            .expect("sample");
    }

    let aggregate = builder.finish().expect("aggregate");
    let encoded = serde_json::to_vec(&aggregate.metrics).expect("json");

    assert_eq!(aggregate.metrics.request_successes, 30);
    assert_eq!(aggregate.metrics.selected_player_samples, 30);
    assert_eq!(aggregate.metrics.response_latency_ms.p95, 20.0);
    assert_eq!(aggregate.metrics.server_fps.p50, 60.0);
    assert!(
        !encoded
            .windows(b"sensitive-subject".len())
            .any(|window| { window == b"sensitive-subject" })
    );
    assert!(
        !encoded
            .windows(b"987654321.125".len())
            .any(|window| window == b"987654321.125")
    );
}

#[test]
fn maps_rest_errors_to_fixed_aggregate_counters_without_error_text() {
    let mut builder = RestWindowBuilder::new(300_000).expect("builder");
    builder.record_error(&RestError::Timeout);
    builder.record_error(&RestError::Unauthorized);
    builder.record_error(&RestError::SelectedPlayerMissing);
    builder.record_error(&RestError::SelectedPlayerInactive);

    let errors = builder.error_counts();

    assert_eq!(errors.timeouts, 1);
    assert_eq!(errors.auth, 1);
    assert_eq!(errors.selected_player_missing, 1);
    assert_eq!(errors.selected_player_inactive, 1);
}
