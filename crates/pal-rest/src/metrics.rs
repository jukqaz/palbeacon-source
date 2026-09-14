use serde::Serialize;

/// Aggregate-only server metrics from the official `/metrics` endpoint.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ServerMetrics {
    pub server_fps: u64,
    pub current_player_count: u64,
    pub server_frame_time_ms: f64,
    pub max_player_count: u64,
    pub uptime_seconds: u64,
    pub base_camp_count: u64,
    pub game_days: u64,
}
