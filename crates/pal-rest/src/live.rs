use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{RestError, TimedResponse};

pub const SERVER_LIVE_SNAPSHOT_SCHEMA_V1: &str = "pal_companion.server_live_snapshot.v1";

/// Selects the endpoints included in one live-server refresh.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveSnapshotRequest {
    pub include_info: bool,
    pub include_metrics: bool,
    pub include_players: bool,
    pub include_settings: bool,
    pub include_game_data: bool,
}

impl Default for LiveSnapshotRequest {
    fn default() -> Self {
        Self {
            include_info: true,
            include_metrics: true,
            include_players: true,
            include_settings: true,
            include_game_data: false,
        }
    }
}

/// Backwards-compatible name for the v1 collector options.
pub type ServerLiveSnapshotOptionsV1 = LiveSnapshotRequest;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerLiveStateV1 {
    ConfigurationRequired,
    Online,
    Partial,
    Offline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerLiveSectionStatusV1 {
    Ok,
    Error,
    Unsupported,
    NotConfigured,
    NotRequested,
}

/// Status and timing envelope shared by every official REST section.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ServerLiveSectionV1<T> {
    pub status: ServerLiveSectionStatusV1,
    pub observed_at_unix_ms: Option<u64>,
    pub latency_ms: Option<u64>,
    pub message_ko: String,
    pub value: Option<T>,
}

impl<T> ServerLiveSectionV1<T> {
    pub(crate) fn from_result(result: Result<TimedResponse<T>, RestError>) -> Self {
        match result {
            Ok(response) => Self::ok(response),
            Err(error) => Self::error(&error),
        }
    }

    pub(crate) fn ok(response: TimedResponse<T>) -> Self {
        Self {
            status: ServerLiveSectionStatusV1::Ok,
            observed_at_unix_ms: Some(system_time_unix_ms(response.rest_completed_at)),
            latency_ms: Some(duration_ms(
                response
                    .response_latency
                    .saturating_add(response.decode_latency),
            )),
            message_ko: "정상적으로 가져왔습니다.".to_owned(),
            value: Some(response.value),
        }
    }

    pub(crate) fn error(error: &RestError) -> Self {
        Self {
            status: ServerLiveSectionStatusV1::Error,
            observed_at_unix_ms: None,
            latency_ms: None,
            message_ko: error_message_ko(error).to_owned(),
            value: None,
        }
    }

    pub(crate) fn unsupported() -> Self {
        Self {
            status: ServerLiveSectionStatusV1::Unsupported,
            observed_at_unix_ms: None,
            latency_ms: None,
            message_ko: "이 서버 버전에서는 지원하지 않습니다.".to_owned(),
            value: None,
        }
    }

    pub(crate) fn not_configured() -> Self {
        Self {
            status: ServerLiveSectionStatusV1::NotConfigured,
            observed_at_unix_ms: None,
            latency_ms: None,
            message_ko: "서버 연결 설정이 필요합니다.".to_owned(),
            value: None,
        }
    }

    pub(crate) fn not_requested() -> Self {
        Self {
            status: ServerLiveSectionStatusV1::NotRequested,
            observed_at_unix_ms: None,
            latency_ms: None,
            message_ko: "이번 조회 대상에 포함되지 않았습니다.".to_owned(),
            value: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ServerLiveInfoV1 {
    pub version: Option<String>,
    pub server_name: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ServerLiveMetricsV1 {
    pub server_fps: Option<f64>,
    pub current_player_count: Option<u64>,
    pub server_frame_time_ms: Option<f64>,
    pub max_player_count: Option<u64>,
    pub uptime_seconds: Option<u64>,
    pub base_camp_count: Option<u64>,
    pub game_days: Option<u64>,
}

/// Privacy-safe `/players` projection. The raw REST identity and network
/// fields have no representation in this serializable type.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ServerLivePlayerV1 {
    pub name: Option<String>,
    pub level: Option<u64>,
    pub ping_ms: Option<f64>,
    pub building_count: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ServerLiveSettingsKnownV1 {
    pub experience_rate: Option<f64>,
    pub capture_rate: Option<f64>,
    pub pal_spawn_rate: Option<f64>,
    pub collection_drop_rate: Option<f64>,
    pub collection_object_hp_rate: Option<f64>,
    pub collection_respawn_rate: Option<f64>,
    pub enemy_drop_rate: Option<f64>,
    pub base_worker_limit: Option<u64>,
    pub egg_hatching_hours: Option<f64>,
    pub work_speed_rate: Option<f64>,
    pub fast_travel_enabled: Option<bool>,
}

/// Reviewed official JSON settings plus stable typed projections used by the
/// UI. Unknown keys are intentionally absent from `raw`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ServerLiveSettingsV1 {
    #[serde(flatten)]
    pub known: ServerLiveSettingsKnownV1,
    pub raw: Map<String, Value>,
}

/// Safe aggregate of `/game-data`; actors and their coordinates/identifiers
/// never cross this model boundary.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ServerLiveGameDataV1 {
    pub server_fps: Option<f64>,
    pub average_server_fps: Option<f64>,
    pub total_actor_count: u64,
    pub active_actor_count: u64,
    pub unit_type_counts: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ServerLiveSnapshotV1 {
    pub schema: &'static str,
    pub profile_id: String,
    pub checked_at_unix_ms: u64,
    pub state: ServerLiveStateV1,
    pub message_ko: String,
    pub info: ServerLiveSectionV1<ServerLiveInfoV1>,
    pub metrics: ServerLiveSectionV1<ServerLiveMetricsV1>,
    pub players: ServerLiveSectionV1<Vec<ServerLivePlayerV1>>,
    pub settings: ServerLiveSectionV1<ServerLiveSettingsV1>,
    pub game_data: ServerLiveSectionV1<ServerLiveGameDataV1>,
}

impl ServerLiveSnapshotV1 {
    /// Produces a serializable, non-error snapshot before credentials and a
    /// trusted server origin have been configured by the bridge.
    pub fn not_configured(profile_id: impl Into<String>, message_ko: impl Into<String>) -> Self {
        Self {
            schema: SERVER_LIVE_SNAPSHOT_SCHEMA_V1,
            profile_id: profile_id.into(),
            checked_at_unix_ms: system_time_unix_ms(SystemTime::now()),
            state: ServerLiveStateV1::ConfigurationRequired,
            message_ko: message_ko.into(),
            info: ServerLiveSectionV1::not_configured(),
            metrics: ServerLiveSectionV1::not_configured(),
            players: ServerLiveSectionV1::not_configured(),
            settings: ServerLiveSectionV1::not_configured(),
            game_data: ServerLiveSectionV1::not_configured(),
        }
    }

    pub(crate) fn new(
        profile_id: String,
        info: ServerLiveSectionV1<ServerLiveInfoV1>,
        metrics: ServerLiveSectionV1<ServerLiveMetricsV1>,
        players: ServerLiveSectionV1<Vec<ServerLivePlayerV1>>,
        settings: ServerLiveSectionV1<ServerLiveSettingsV1>,
        game_data: ServerLiveSectionV1<ServerLiveGameDataV1>,
    ) -> Self {
        let base_statuses = [
            &info.status,
            &metrics.status,
            &players.status,
            &settings.status,
        ];
        let requested_base = base_statuses
            .into_iter()
            .filter(|status| is_requested(status))
            .count();
        let successful_base = base_statuses
            .into_iter()
            .filter(|status| **status == ServerLiveSectionStatusV1::Ok)
            .count();
        let (requested_sections, successful_sections) = if requested_base == 0 {
            (
                usize::from(is_requested(&game_data.status)),
                usize::from(game_data.status == ServerLiveSectionStatusV1::Ok),
            )
        } else {
            (requested_base, successful_base)
        };
        let state = if requested_sections > 0 && successful_sections == requested_sections {
            ServerLiveStateV1::Online
        } else if successful_sections > 0 {
            ServerLiveStateV1::Partial
        } else {
            ServerLiveStateV1::Offline
        };
        let message_ko = match state {
            ServerLiveStateV1::ConfigurationRequired => "서버 연결 설정이 필요합니다.",
            ServerLiveStateV1::Online => "실시간 서버 정보를 불러왔습니다.",
            ServerLiveStateV1::Partial => "일부 서버 정보만 불러왔습니다.",
            ServerLiveStateV1::Offline => "실시간 서버 정보를 불러오지 못했습니다.",
        }
        .to_owned();

        Self {
            schema: SERVER_LIVE_SNAPSHOT_SCHEMA_V1,
            profile_id,
            checked_at_unix_ms: system_time_unix_ms(SystemTime::now()),
            state,
            message_ko,
            info,
            metrics,
            players,
            settings,
            game_data,
        }
    }
}

fn is_requested(status: &ServerLiveSectionStatusV1) -> bool {
    !matches!(
        status,
        ServerLiveSectionStatusV1::NotConfigured | ServerLiveSectionStatusV1::NotRequested
    )
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn system_time_unix_ms(value: SystemTime) -> u64 {
    value
        .duration_since(UNIX_EPOCH)
        .map(duration_ms)
        .unwrap_or_default()
}

fn error_message_ko(error: &RestError) -> &'static str {
    match error {
        RestError::Unauthorized => "REST 인증에 실패했습니다.",
        RestError::Timeout => "서버 응답 시간이 초과되었습니다.",
        RestError::Transport | RestError::ConnectionUntrusted => {
            "서버에 안전하게 연결하지 못했습니다."
        }
        RestError::Decode { .. } => "서버 응답 형식을 해석하지 못했습니다.",
        RestError::UnexpectedStatus(_) => "서버가 요청을 정상 처리하지 않았습니다.",
        _ => "서버 정보를 가져오지 못했습니다.",
    }
}
