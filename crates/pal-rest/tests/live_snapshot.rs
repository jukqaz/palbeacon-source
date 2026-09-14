#![recursion_limit = "256"]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pal_rest::{
    BasicAuthSecret, ClientConfig, LiveSnapshotRequest, PalRestClient, ServerLiveSectionStatusV1,
    ServerLiveSnapshotV1, ServerLiveStateV1, decode_live_game_data, decode_live_info,
    decode_live_metrics, decode_live_players, decode_live_settings,
};
use serde_json::{Value, json};

#[test]
fn players_projection_never_serializes_identity_network_or_coordinates() {
    let players = decode_live_players(
        r#"{
            "players":[{
                "name":"테스트 플레이어",
                "accountName":"ACCOUNT_SENTINEL",
                "playerId":"PLAYER_ID_SENTINEL",
                "userId":"USER_ID_SENTINEL",
                "ip":"203.0.113.77:8211",
                "ping":12.5,
                "location_x":1234.5,
                "location_y":6789.0,
                "level":42,
                "building_count":7,
                "futureField":"accepted"
            }],
            "futureEnvelopeField":true
        }"#
        .as_bytes(),
    )
    .unwrap();

    assert_eq!(players.len(), 1);
    assert_eq!(players[0].name.as_deref(), Some("테스트 플레이어"));
    assert_eq!(players[0].level, Some(42));
    assert_eq!(players[0].ping_ms, Some(12.5));
    assert_eq!(players[0].building_count, Some(7));

    let encoded = serde_json::to_string(&players).unwrap();
    for forbidden in [
        "ACCOUNT_SENTINEL",
        "PLAYER_ID_SENTINEL",
        "USER_ID_SENTINEL",
        "203.0.113.77",
        "accountName",
        "playerId",
        "userId",
        "location_x",
        "location_y",
    ] {
        assert!(!encoded.contains(forbidden), "leaked {forbidden}");
    }
}

#[test]
fn settings_preserve_all_66_official_fields_and_keep_known_projection() {
    let source = json!({
        "Difficulty":"Normal",
        "DayTimeSpeedRate":1.0,
        "NightTimeSpeedRate":1.0,
        "ExpRate":1.5,
        "PalCaptureRate":1.25,
        "PalSpawnNumRate":1.0,
        "PalDamageRateAttack":1.0,
        "PalDamageRateDefense":1.0,
        "PlayerDamageRateAttack":1.0,
        "PlayerDamageRateDefense":1.0,
        "PlayerStomachDecreaceRate":1.0,
        "PlayerStaminaDecreaceRate":1.0,
        "PlayerAutoHPRegeneRate":1.0,
        "PlayerAutoHpRegeneRateInSleep":1.0,
        "PalStomachDecreaceRate":1.0,
        "PalStaminaDecreaceRate":1.0,
        "PalAutoHPRegeneRate":1.0,
        "PalAutoHpRegeneRateInSleep":1.0,
        "BuildObjectDamageRate":1.0,
        "BuildObjectDeteriorationDamageRate":1.0,
        "CollectionDropRate":2.0,
        "CollectionObjectHpRate":1.0,
        "CollectionObjectRespawnSpeedRate":1.0,
        "EnemyDropItemRate":2.5,
        "DeathPenalty":"All",
        "bEnablePlayerToPlayerDamage":false,
        "bEnableFriendlyFire":false,
        "bEnableInvaderEnemy":true,
        "bActiveUNKO":false,
        "bEnableAimAssistPad":true,
        "bEnableAimAssistKeyboard":false,
        "DropItemMaxNum":3000,
        "DropItemMaxNum_UNKO":100,
        "BaseCampMaxNum":128,
        "BaseCampWorkerMaxNum":20,
        "DropItemAliveMaxHours":1.0,
        "bAutoResetGuildNoOnlinePlayers":false,
        "AutoResetGuildTimeNoOnlinePlayers":72.0,
        "GuildPlayerMaxNum":20,
        "PalEggDefaultHatchingTime":0.0,
        "WorkSpeedRate":1.1,
        "bIsMultiplay":true,
        "bIsPvP":false,
        "bCanPickupOtherGuildDeathPenaltyDrop":false,
        "bEnableNonLoginPenalty":true,
        "bEnableFastTravel":true,
        "bIsStartLocationSelectByMap":true,
        "bExistPlayerAfterLogout":false,
        "bEnableDefenseOtherGuildPlayer":false,
        "CoopPlayerMaxNum":4,
        "ServerPlayerMaxNum":32,
        "ServerName":"Pal Beacon Test",
        "ServerDescription":"description",
        "PublicPort":8211,
        "PublicIP":"198.51.100.10",
        "RCONEnabled":true,
        "RCONPort":25575,
        "Region":"Asia",
        "bUseAuth":true,
        "BanListURL":"https://example.invalid/ban.txt",
        "RESTAPIEnabled":true,
        "RESTAPIPort":8212,
        "bShowPlayerList":true,
        "AllowConnectPlatform":"Steam",
        "bIsUseBackupSaveData":true,
        "LogFormatType":"Text"
    });
    let source_map = source.as_object().unwrap();
    assert_eq!(source_map.len(), 66);

    let settings = decode_live_settings(&serde_json::to_vec(&source).unwrap()).unwrap();
    assert_eq!(&settings.raw, source_map);
    assert_eq!(settings.known.experience_rate, Some(1.5));
    assert_eq!(settings.known.base_worker_limit, Some(20));
    assert_eq!(settings.known.fast_travel_enabled, Some(true));

    let encoded: Value = serde_json::to_value(&settings).unwrap();
    assert_eq!(encoded.get("raw"), Some(&source));
    assert_eq!(encoded.get("experience_rate"), Some(&json!(1.5)));
}

#[test]
fn tolerant_live_decoders_accept_missing_and_future_fields() {
    let info =
        decode_live_info(br#"{"version":"v0.6.7","worldguid":"WORLD_GUID_SENTINEL","future":1}"#)
            .unwrap();
    assert_eq!(info.version.as_deref(), Some("v0.6.7"));
    assert_eq!(info.server_name, None);
    assert!(
        !serde_json::to_string(&info)
            .unwrap()
            .contains("WORLD_GUID_SENTINEL")
    );

    let metrics = decode_live_metrics(br#"{"serverfps":59,"future":true}"#).unwrap();
    assert_eq!(metrics.server_fps, Some(59.0));
    assert_eq!(metrics.current_player_count, None);

    let players =
        decode_live_players(r#"{"players":[{"name":"이름"}],"future":0}"#.as_bytes()).unwrap();
    assert_eq!(players[0].level, None);

    let settings = decode_live_settings(
        br#"{"FutureSetting":{"nested":"UNKNOWN_SENTINEL"},"AdminPassword":"ADMIN_PASSWORD_SENTINEL"}"#,
    )
    .unwrap();
    assert_eq!(settings.known.experience_rate, None);
    assert!(settings.raw.is_empty());
    let encoded = serde_json::to_string(&settings).unwrap();
    for forbidden in [
        "FutureSetting",
        "UNKNOWN_SENTINEL",
        "AdminPassword",
        "ADMIN_PASSWORD_SENTINEL",
    ] {
        assert!(!encoded.contains(forbidden), "leaked {forbidden}");
    }
}

#[test]
fn game_data_returns_only_safe_aggregates() {
    let aggregate = decode_live_game_data(
        br#"{
            "Time":"2026-08-09 12:00:00",
            "FPS":58.5,
            "AverageFPS":57.25,
            "ActorData":[
                {"Type":"Character","UnitType":"Player","IsActive":"true","userid":"USER_SENTINEL","InstanceID":"INSTANCE_SENTINEL","ip":"203.0.113.20","LocationX":12.0,"LocationY":34.0},
                {"Type":"Character","UnitType":"WildPal","IsActive":false,"LocationX":56.0},
                {"Type":"Character","UnitType":"FutureUnit","IsActive":true,"NickName":"NICK_SENTINEL"}
            ],
            "future":true
        }"#,
    )
    .unwrap();

    assert_eq!(aggregate.server_fps, Some(58.5));
    assert_eq!(aggregate.average_server_fps, Some(57.25));
    assert_eq!(aggregate.total_actor_count, 3);
    assert_eq!(aggregate.active_actor_count, 2);
    assert_eq!(aggregate.unit_type_counts.get("Player"), Some(&1));
    assert_eq!(aggregate.unit_type_counts.get("WildPal"), Some(&1));
    assert_eq!(aggregate.unit_type_counts.get("FutureUnit"), Some(&1));

    let encoded = serde_json::to_string(&aggregate).unwrap();
    for forbidden in [
        "USER_SENTINEL",
        "INSTANCE_SENTINEL",
        "203.0.113.20",
        "NICK_SENTINEL",
        "LocationX",
        "LocationY",
        "userid",
        "InstanceID",
        "ip",
    ] {
        assert!(!encoded.contains(forbidden), "leaked {forbidden}");
    }
}

#[test]
fn not_configured_snapshot_is_a_complete_non_error_contract() {
    let snapshot =
        ServerLiveSnapshotV1::not_configured("profile-new", "REST 연결을 설정해 주세요.");

    assert_eq!(snapshot.state, ServerLiveStateV1::ConfigurationRequired);
    assert_eq!(snapshot.profile_id, "profile-new");
    for status in [
        snapshot.info.status,
        snapshot.metrics.status,
        snapshot.players.status,
        snapshot.settings.status,
        snapshot.game_data.status,
    ] {
        assert_eq!(status, ServerLiveSectionStatusV1::NotConfigured);
    }
    let encoded = serde_json::to_value(snapshot).unwrap();
    assert_eq!(encoded["state"], "configuration_required");
    assert!(encoded["info"]["value"].is_null());
}

#[test]
fn request_json_defaults_to_full_poll_without_game_data() {
    let request: LiveSnapshotRequest = serde_json::from_value(json!({})).unwrap();
    assert_eq!(request, LiveSnapshotRequest::default());

    let request: LiveSnapshotRequest =
        serde_json::from_value(json!({"include_players": false})).unwrap();
    assert!(request.include_info);
    assert!(!request.include_players);
    assert!(!request.include_game_data);
}

struct RouteServer {
    base_url: String,
    requests: mpsc::Receiver<String>,
    thread: Option<thread::JoinHandle<()>>,
}

impl RouteServer {
    fn start(expected_requests: usize, responder: fn(&str) -> Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, requests) = mpsc::channel();
        let thread = thread::spawn(move || {
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_request(&mut stream);
                let path = String::from_utf8_lossy(&request)
                    .lines()
                    .next()
                    .and_then(|line| line.split_ascii_whitespace().nth(1))
                    .unwrap_or_default()
                    .to_owned();
                sender.send(path.clone()).unwrap();
                let response = responder(&path);
                stream.write_all(&response).unwrap();
                stream.flush().unwrap();
            }
        });
        Self {
            base_url: format!("http://{address}/v1/api"),
            requests,
            thread: Some(thread),
        }
    }

    fn finish(mut self, expected_requests: usize) -> Vec<String> {
        let mut paths = Vec::with_capacity(expected_requests);
        for _ in 0..expected_requests {
            paths.push(self.requests.recv_timeout(Duration::from_secs(2)).unwrap());
        }
        self.thread.take().unwrap().join().unwrap();
        paths.sort();
        paths
    }
}

impl Drop for RouteServer {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[tokio::test]
async fn collector_keeps_partial_results_and_skips_unrequested_game_data() {
    let server = RouteServer::start(4, partial_responder);
    let snapshot = client(&server)
        .collect_server_live_snapshot("profile-a", LiveSnapshotRequest::default())
        .await;

    assert_eq!(snapshot.profile_id, "profile-a");
    assert_eq!(snapshot.state, ServerLiveStateV1::Partial);
    assert_eq!(snapshot.info.status, ServerLiveSectionStatusV1::Ok);
    assert_eq!(snapshot.metrics.status, ServerLiveSectionStatusV1::Error);
    assert_eq!(snapshot.players.status, ServerLiveSectionStatusV1::Ok);
    assert_eq!(snapshot.settings.status, ServerLiveSectionStatusV1::Error);
    assert_eq!(
        snapshot.game_data.status,
        ServerLiveSectionStatusV1::NotRequested
    );
    assert_eq!(
        snapshot.players.value.unwrap()[0].name.as_deref(),
        Some("Player")
    );

    let paths = server.finish(4);
    assert_eq!(
        paths,
        vec![
            "/v1/api/info",
            "/v1/api/metrics",
            "/v1/api/players",
            "/v1/api/settings"
        ]
    );
}

#[tokio::test]
async fn selected_poll_calls_only_selected_endpoint() {
    let server = RouteServer::start(1, metrics_responder);
    let snapshot = client(&server)
        .collect_server_live_snapshot(
            "profile-b",
            LiveSnapshotRequest {
                include_info: false,
                include_metrics: true,
                include_players: false,
                include_settings: false,
                include_game_data: false,
            },
        )
        .await;

    assert_eq!(snapshot.state, ServerLiveStateV1::Online);
    assert_eq!(snapshot.metrics.status, ServerLiveSectionStatusV1::Ok);
    assert_eq!(
        snapshot.info.status,
        ServerLiveSectionStatusV1::NotRequested
    );
    assert_eq!(server.finish(1), vec!["/v1/api/metrics"]);
}

#[tokio::test]
async fn explicit_game_data_404_is_reported_as_unsupported() {
    let server = RouteServer::start(1, game_data_unsupported_responder);
    let snapshot = client(&server)
        .collect_server_live_snapshot(
            "profile-c",
            LiveSnapshotRequest {
                include_info: false,
                include_metrics: false,
                include_players: false,
                include_settings: false,
                include_game_data: true,
            },
        )
        .await;

    assert_eq!(snapshot.state, ServerLiveStateV1::Offline);
    assert_eq!(
        snapshot.game_data.status,
        ServerLiveSectionStatusV1::Unsupported
    );
    assert_eq!(server.finish(1), vec!["/v1/api/game-data"]);
}

#[tokio::test]
async fn snapshot_settings_serialize_only_reviewed_official_keys() {
    let server = RouteServer::start(1, settings_allowlist_responder);
    let snapshot = client(&server)
        .collect_server_live_snapshot(
            "profile-settings",
            LiveSnapshotRequest {
                include_info: false,
                include_metrics: false,
                include_players: false,
                include_settings: true,
                include_game_data: false,
            },
        )
        .await;

    assert_eq!(snapshot.settings.status, ServerLiveSectionStatusV1::Ok);
    let settings = snapshot.settings.value.as_ref().unwrap();
    assert_eq!(settings.known.experience_rate, Some(2.0));
    assert_eq!(settings.raw.get("ExpRate"), Some(&json!(2.0)));
    assert_eq!(
        settings.raw.get("ServerName"),
        Some(&json!("Allowed Server"))
    );
    assert_eq!(settings.raw.len(), 2);

    let encoded = serde_json::to_string(&snapshot).unwrap();
    for forbidden in [
        "FutureSetting",
        "UNKNOWN_SENTINEL",
        "AdminPassword",
        "ADMIN_PASSWORD_SENTINEL",
    ] {
        assert!(!encoded.contains(forbidden), "snapshot leaked {forbidden}");
    }
    assert_eq!(server.finish(1), vec!["/v1/api/settings"]);
}

fn client(server: &RouteServer) -> PalRestClient {
    PalRestClient::new_unverified_for_probe(
        ClientConfig::loopback(&server.base_url).unwrap(),
        BasicAuthSecret::new("admin", "test-password").unwrap(),
    )
    .unwrap()
}

fn partial_responder(path: &str) -> Vec<u8> {
    match path {
        "/v1/api/info" => json_response(
            "200 OK",
            br#"{"version":"v0.6.7","servername":"Server","description":"Description","worldguid":"PRIVATE"}"#,
        ),
        "/v1/api/players" => json_response(
            "200 OK",
            br#"{"players":[{"name":"Player","userId":"PRIVATE","level":10,"ping":5,"building_count":2}]}"#,
        ),
        "/v1/api/metrics" => json_response("500 Internal Server Error", br#"{"error":true}"#),
        "/v1/api/settings" => json_response("404 Not Found", br#"{"error":true}"#),
        _ => json_response("404 Not Found", br#"{}"#),
    }
}

fn metrics_responder(path: &str) -> Vec<u8> {
    assert_eq!(path, "/v1/api/metrics");
    json_response(
        "200 OK",
        br#"{"serverfps":60,"currentplayernum":1,"serverframetime":16.6,"maxplayernum":32,"uptime":10,"basecampnum":1,"days":2}"#,
    )
}

fn game_data_unsupported_responder(path: &str) -> Vec<u8> {
    assert_eq!(path, "/v1/api/game-data");
    json_response("404 Not Found", br#"{}"#)
}

fn settings_allowlist_responder(path: &str) -> Vec<u8> {
    assert_eq!(path, "/v1/api/settings");
    json_response(
        "200 OK",
        br#"{"ExpRate":2.0,"ServerName":"Allowed Server","FutureSetting":"UNKNOWN_SENTINEL","AdminPassword":"ADMIN_PASSWORD_SENTINEL"}"#,
    )
}

fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1_024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
    }
    request
}

fn json_response(status: &str, body: &[u8]) -> Vec<u8> {
    let mut output = format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    output.extend_from_slice(body);
    output
}
