use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize as DeriveDeserialize;
use serde::de::{
    self, Deserialize, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor,
};

use crate::privacy::{PlayerSelector, RawServerInfo, SelectedPlayerObservation, SelectorKind};
use crate::{
    EndpointKind, RestError, ServerLiveGameDataV1, ServerLiveInfoV1, ServerLiveMetricsV1,
    ServerLivePlayerV1, ServerLiveSettingsKnownV1, ServerLiveSettingsV1, ServerMetrics,
    ServerSettings,
};

#[derive(DeriveDeserialize)]
#[serde(deny_unknown_fields)]
struct MetricsWire {
    serverfps: u64,
    currentplayernum: u64,
    serverframetime: f64,
    maxplayernum: u64,
    uptime: u64,
    basecampnum: u64,
    days: u64,
}

#[derive(DeriveDeserialize)]
#[serde(deny_unknown_fields)]
struct InfoWire {
    version: String,
    servername: String,
    description: String,
    worldguid: String,
}

#[derive(DeriveDeserialize)]
struct SettingsWire {
    #[serde(rename = "ExpRate")]
    experience_rate: f64,
    #[serde(rename = "PalCaptureRate")]
    capture_rate: f64,
    #[serde(rename = "PalSpawnNumRate")]
    pal_spawn_rate: f64,
    #[serde(rename = "CollectionDropRate")]
    collection_drop_rate: f64,
    #[serde(rename = "CollectionObjectHpRate")]
    collection_object_hp_rate: f64,
    #[serde(rename = "CollectionObjectRespawnSpeedRate")]
    collection_respawn_rate: f64,
    #[serde(rename = "EnemyDropItemRate")]
    enemy_drop_rate: f64,
    #[serde(rename = "BaseCampWorkerMaxNum")]
    base_worker_limit: u64,
    #[serde(rename = "PalEggDefaultHatchingTime")]
    egg_hatching_hours: f64,
    #[serde(rename = "WorkSpeedRate")]
    work_speed_rate: f64,
    #[serde(rename = "bEnableFastTravel")]
    fast_travel_enabled: bool,
}

const LIVE_SETTINGS_ALLOWLIST: [&str; 66] = [
    "Difficulty",
    "DayTimeSpeedRate",
    "NightTimeSpeedRate",
    "ExpRate",
    "PalCaptureRate",
    "PalSpawnNumRate",
    "PalDamageRateAttack",
    "PalDamageRateDefense",
    "PlayerDamageRateAttack",
    "PlayerDamageRateDefense",
    "PlayerStomachDecreaceRate",
    "PlayerStaminaDecreaceRate",
    "PlayerAutoHPRegeneRate",
    "PlayerAutoHpRegeneRateInSleep",
    "PalStomachDecreaceRate",
    "PalStaminaDecreaceRate",
    "PalAutoHPRegeneRate",
    "PalAutoHpRegeneRateInSleep",
    "BuildObjectDamageRate",
    "BuildObjectDeteriorationDamageRate",
    "CollectionDropRate",
    "CollectionObjectHpRate",
    "CollectionObjectRespawnSpeedRate",
    "EnemyDropItemRate",
    "DeathPenalty",
    "bEnablePlayerToPlayerDamage",
    "bEnableFriendlyFire",
    "bEnableInvaderEnemy",
    "bActiveUNKO",
    "bEnableAimAssistPad",
    "bEnableAimAssistKeyboard",
    "DropItemMaxNum",
    "DropItemMaxNum_UNKO",
    "BaseCampMaxNum",
    "BaseCampWorkerMaxNum",
    "DropItemAliveMaxHours",
    "bAutoResetGuildNoOnlinePlayers",
    "AutoResetGuildTimeNoOnlinePlayers",
    "GuildPlayerMaxNum",
    "PalEggDefaultHatchingTime",
    "WorkSpeedRate",
    "bIsMultiplay",
    "bIsPvP",
    "bCanPickupOtherGuildDeathPenaltyDrop",
    "bEnableNonLoginPenalty",
    "bEnableFastTravel",
    "bIsStartLocationSelectByMap",
    "bExistPlayerAfterLogout",
    "bEnableDefenseOtherGuildPlayer",
    "CoopPlayerMaxNum",
    "ServerPlayerMaxNum",
    "ServerName",
    "ServerDescription",
    "PublicPort",
    "PublicIP",
    "RCONEnabled",
    "RCONPort",
    "Region",
    "bUseAuth",
    "BanListURL",
    "RESTAPIEnabled",
    "RESTAPIPort",
    "bShowPlayerList",
    "AllowConnectPlatform",
    "bIsUseBackupSaveData",
    "LogFormatType",
];

struct LiveSettingsRawWire {
    raw: serde_json::Map<String, serde_json::Value>,
}

impl<'de> Deserialize<'de> for LiveSettingsRawWire {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(LiveSettingsRawVisitor)
    }
}

struct LiveSettingsRawVisitor;

impl<'de> Visitor<'de> for LiveSettingsRawVisitor {
    type Value = LiveSettingsRawWire;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the official settings object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut raw = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if let Some(canonical_key) = canonical_live_setting(&key) {
                if raw.contains_key(canonical_key) {
                    return Err(de::Error::custom("duplicate official setting"));
                }
                let value = map.next_value::<serde_json::Value>()?;
                raw.insert(canonical_key.to_owned(), value);
            } else {
                let _ = map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(LiveSettingsRawWire { raw })
    }
}

fn canonical_live_setting(key: &str) -> Option<&'static str> {
    LIVE_SETTINGS_ALLOWLIST
        .iter()
        .copied()
        .find(|candidate| *candidate == key)
}

#[derive(DeriveDeserialize)]
struct LiveInfoWire {
    #[serde(default)]
    version: Option<String>,
    #[serde(default, rename = "servername")]
    server_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "worldguid")]
    _world_guid: Option<IgnoredAny>,
}

#[derive(DeriveDeserialize)]
struct LiveMetricsWire {
    #[serde(default)]
    serverfps: Option<f64>,
    #[serde(default)]
    currentplayernum: Option<u64>,
    #[serde(default)]
    serverframetime: Option<f64>,
    #[serde(default)]
    maxplayernum: Option<u64>,
    #[serde(default)]
    uptime: Option<u64>,
    #[serde(default)]
    basecampnum: Option<u64>,
    #[serde(default)]
    days: Option<u64>,
}

#[derive(DeriveDeserialize)]
struct LivePlayersWire {
    #[serde(default)]
    players: Option<Vec<LivePlayerWire>>,
}

#[derive(DeriveDeserialize)]
struct LivePlayerWire {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "accountName")]
    _account_name: Option<IgnoredAny>,
    #[serde(default, rename = "playerId")]
    _player_id: Option<IgnoredAny>,
    #[serde(default, rename = "userId")]
    _user_id: Option<IgnoredAny>,
    #[serde(default, rename = "ip")]
    _ip: Option<IgnoredAny>,
    #[serde(default, rename = "location_x")]
    _location_x: Option<IgnoredAny>,
    #[serde(default, rename = "location_y")]
    _location_y: Option<IgnoredAny>,
    #[serde(default)]
    ping: Option<f64>,
    #[serde(default)]
    level: Option<u64>,
    #[serde(default)]
    building_count: Option<u64>,
}

#[derive(DeriveDeserialize)]
struct LiveGameDataWire {
    #[serde(default, rename = "FPS")]
    server_fps: Option<f64>,
    #[serde(default, rename = "AverageFPS")]
    average_server_fps: Option<f64>,
    #[serde(default, rename = "ActorData")]
    actors: Option<Vec<LiveGameDataActorWire>>,
}

#[derive(DeriveDeserialize)]
struct LiveGameDataActorWire {
    #[serde(default, rename = "UnitType")]
    unit_type: Option<String>,
    #[serde(default, rename = "IsActive")]
    is_active: Option<serde_json::Value>,
}

impl Drop for InfoWire {
    fn drop(&mut self) {
        erase_string(&mut self.servername);
        erase_string(&mut self.description);
        erase_string(&mut self.worldguid);
    }
}

#[derive(DeriveDeserialize)]
#[serde(deny_unknown_fields)]
struct ActorWire {
    #[serde(rename = "Type")]
    actor_type: String,
    #[serde(default, rename = "InstanceID", deserialize_with = "strict_optional")]
    instance_id: Option<String>,
    #[serde(default, rename = "UnitType", deserialize_with = "strict_optional")]
    unit_type: Option<String>,
    #[serde(default, rename = "NickName", deserialize_with = "strict_optional")]
    nickname: Option<DiscardString>,
    #[serde(
        default,
        rename = "TrainerInstanceID",
        deserialize_with = "strict_optional"
    )]
    trainer_instance_id: Option<DiscardString>,
    #[serde(
        default,
        rename = "TrainerNickName",
        deserialize_with = "strict_optional"
    )]
    trainer_nickname: Option<DiscardString>,
    #[serde(default, rename = "TrainerClass", deserialize_with = "strict_optional")]
    trainer_class: Option<DiscardString>,
    #[serde(default, deserialize_with = "strict_optional")]
    userid: Option<String>,
    #[serde(default, deserialize_with = "strict_optional")]
    ip: Option<DiscardString>,
    #[serde(default, deserialize_with = "strict_optional")]
    level: Option<i64>,
    #[serde(default, rename = "HP", deserialize_with = "strict_optional")]
    hp: Option<i64>,
    #[serde(default, rename = "MaxHP", deserialize_with = "strict_optional")]
    max_hp: Option<i64>,
    #[serde(default, rename = "GuildID", deserialize_with = "strict_optional")]
    guild_id: Option<DiscardString>,
    #[serde(default, rename = "GuildName", deserialize_with = "strict_optional")]
    guild_name: Option<DiscardString>,
    #[serde(default, rename = "Class", deserialize_with = "strict_optional")]
    class: Option<DiscardString>,
    #[serde(default, rename = "Action", deserialize_with = "strict_optional")]
    action: Option<DiscardString>,
    #[serde(default, rename = "AI_Action", deserialize_with = "strict_optional")]
    ai_action: Option<DiscardString>,
    #[serde(default, rename = "LocationX", deserialize_with = "strict_optional")]
    location_x: Option<f64>,
    #[serde(default, rename = "LocationY", deserialize_with = "strict_optional")]
    location_y: Option<f64>,
    #[serde(default, rename = "LocationZ", deserialize_with = "strict_optional")]
    location_z: Option<f64>,
    #[serde(default, rename = "RotationX", deserialize_with = "strict_optional")]
    rotation_x: Option<f64>,
    #[serde(default, rename = "RotationY", deserialize_with = "strict_optional")]
    rotation_y: Option<f64>,
    #[serde(default, rename = "RotationZ", deserialize_with = "strict_optional")]
    rotation_z: Option<f64>,
    #[serde(default, rename = "Stage", deserialize_with = "strict_optional")]
    stage: Option<DiscardString>,
    #[serde(default, rename = "IsActive", deserialize_with = "strict_optional")]
    is_active: Option<String>,
}

impl Drop for ActorWire {
    fn drop(&mut self) {
        erase_optional_string(&mut self.instance_id);
        erase_optional_string(&mut self.userid);
        erase_optional_string(&mut self.is_active);
    }
}

#[derive(Clone, Copy)]
struct DiscardString;

impl<'de> Deserialize<'de> for DiscardString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StringVisitor;

        impl Visitor<'_> for StringVisitor {
            type Value = DiscardString;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a string")
            }

            fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
                Ok(DiscardString)
            }

            fn visit_string<E>(self, mut value: String) -> Result<Self::Value, E> {
                // Best-effort early erasure for owned sensitive values.
                let mut bytes = std::mem::take(&mut value).into_bytes();
                bytes.fill(0);
                Ok(DiscardString)
            }
        }

        deserializer.deserialize_string(StringVisitor)
    }
}

struct ActorSelection {
    selected: Option<SelectedActor>,
    match_count: usize,
    actor_count: usize,
}

struct SelectedActor {
    raw_selected_id: Vec<u8>,
    selector_kind: SelectorKind,
    active: bool,
    x: f64,
    y: f64,
    z: f64,
    heading_degrees: Option<f32>,
}

impl Drop for SelectedActor {
    fn drop(&mut self) {
        self.raw_selected_id.fill(0);
    }
}

struct ActorDataSeed<'a> {
    selector: &'a PlayerSelector,
}

impl<'de> DeserializeSeed<'de> for ActorDataSeed<'_> {
    type Value = ActorSelection;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(ActorDataVisitor {
            selector: self.selector,
        })
    }
}

struct ActorDataVisitor<'a> {
    selector: &'a PlayerSelector,
}

impl<'de> Visitor<'de> for ActorDataVisitor<'_> {
    type Value = ActorSelection;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the official ActorData array")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut selected = None;
        let mut match_count = 0_usize;
        let mut actor_count = 0_usize;

        while let Some(actor) = sequence.next_element::<ActorWire>()? {
            actor_count = actor_count
                .checked_add(1)
                .ok_or_else(|| de::Error::custom("actor count overflow"))?;
            validate_actor::<A::Error>(&actor)?;
            if let Some(candidate) = select_actor(actor, self.selector)? {
                match_count = match_count
                    .checked_add(1)
                    .ok_or_else(|| de::Error::custom("match count overflow"))?;
                if selected.is_none() {
                    selected = Some(candidate);
                }
            }
        }

        Ok(ActorSelection {
            selected,
            match_count,
            actor_count,
        })
    }
}

struct GameDataSeed<'a> {
    selector: &'a PlayerSelector,
}

impl<'de> DeserializeSeed<'de> for GameDataSeed<'_> {
    type Value = GameDataDecoded;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(GameDataVisitor {
            selector: self.selector,
        })
    }
}

struct GameDataVisitor<'a> {
    selector: &'a PlayerSelector,
}

struct GameDataDecoded {
    server_fps: f64,
    average_server_fps: f64,
    selection: ActorSelection,
}

impl<'de> Visitor<'de> for GameDataVisitor<'_> {
    type Value = GameDataDecoded;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the official game-data snapshot object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut time = None;
        let mut fps = None;
        let mut average_fps = None;
        let mut selection = None;

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "Time" => {
                    if time.is_some() {
                        return Err(de::Error::duplicate_field("Time"));
                    }
                    let value = map.next_value::<String>()?;
                    if !is_server_local_timestamp(&value) {
                        return Err(de::Error::custom("invalid server-local timestamp"));
                    }
                    time = Some(());
                }
                "FPS" => {
                    if fps.is_some() {
                        return Err(de::Error::duplicate_field("FPS"));
                    }
                    fps = Some(map.next_value::<f64>()?);
                }
                "AverageFPS" => {
                    if average_fps.is_some() {
                        return Err(de::Error::duplicate_field("AverageFPS"));
                    }
                    average_fps = Some(map.next_value::<f64>()?);
                }
                "ActorData" => {
                    if selection.is_some() {
                        return Err(de::Error::duplicate_field("ActorData"));
                    }
                    selection = Some(map.next_value_seed(ActorDataSeed {
                        selector: self.selector,
                    })?);
                }
                _ => {
                    let _ = map.next_value::<IgnoredAny>()?;
                    return Err(de::Error::unknown_field(
                        &key,
                        &["Time", "FPS", "AverageFPS", "ActorData"],
                    ));
                }
            }
        }

        time.ok_or_else(|| de::Error::missing_field("Time"))?;
        let server_fps = fps.ok_or_else(|| de::Error::missing_field("FPS"))?;
        let average_server_fps =
            average_fps.ok_or_else(|| de::Error::missing_field("AverageFPS"))?;
        if !server_fps.is_finite()
            || server_fps < 0.0
            || !average_server_fps.is_finite()
            || average_server_fps < 0.0
        {
            return Err(de::Error::custom("invalid FPS value"));
        }
        Ok(GameDataDecoded {
            server_fps,
            average_server_fps,
            selection: selection.ok_or_else(|| de::Error::missing_field("ActorData"))?,
        })
    }
}

pub fn decode_selected_player(
    bytes: &[u8],
    selector: &PlayerSelector,
) -> Result<SelectedPlayerObservation, RestError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let decoded = GameDataSeed { selector }
        .deserialize(&mut deserializer)
        .map_err(|_| RestError::Decode {
            endpoint: EndpointKind::GameData,
        })?;
    deserializer.end().map_err(|_| RestError::Decode {
        endpoint: EndpointKind::GameData,
    })?;

    match decoded.selection.match_count {
        0 => Err(RestError::SelectedPlayerMissing),
        1 => {
            let mut selected = decoded
                .selection
                .selected
                .ok_or(RestError::SelectedPlayerMissing)?;
            if !selected.active {
                return Err(RestError::SelectedPlayerInactive);
            }
            Ok(SelectedPlayerObservation {
                raw_selected_id: std::mem::take(&mut selected.raw_selected_id),
                selector_kind: selected.selector_kind,
                x: selected.x,
                y: selected.y,
                z: selected.z,
                heading_degrees: selected.heading_degrees,
                server_fps: decoded.server_fps,
                average_server_fps: decoded.average_server_fps,
                actor_count: decoded.selection.actor_count,
            })
        }
        _ => Err(RestError::SelectedPlayerAmbiguous),
    }
}

pub fn decode_metrics(bytes: &[u8]) -> Result<ServerMetrics, RestError> {
    let wire: MetricsWire = serde_json::from_slice(bytes).map_err(|_| RestError::Decode {
        endpoint: EndpointKind::Metrics,
    })?;
    if !wire.serverframetime.is_finite() || wire.serverframetime < 0.0 {
        return Err(RestError::Decode {
            endpoint: EndpointKind::Metrics,
        });
    }
    Ok(ServerMetrics {
        server_fps: wire.serverfps,
        current_player_count: wire.currentplayernum,
        server_frame_time_ms: wire.serverframetime,
        max_player_count: wire.maxplayernum,
        uptime_seconds: wire.uptime,
        base_camp_count: wire.basecampnum,
        game_days: wire.days,
    })
}

pub fn decode_settings(bytes: &[u8]) -> Result<ServerSettings, RestError> {
    let wire: SettingsWire = serde_json::from_slice(bytes).map_err(|_| RestError::Decode {
        endpoint: EndpointKind::Settings,
    })?;
    let rates = [
        wire.experience_rate,
        wire.capture_rate,
        wire.pal_spawn_rate,
        wire.collection_drop_rate,
        wire.collection_object_hp_rate,
        wire.collection_respawn_rate,
        wire.enemy_drop_rate,
        wire.egg_hatching_hours,
        wire.work_speed_rate,
    ];
    if rates
        .into_iter()
        .any(|value| !value.is_finite() || value < 0.0)
    {
        return Err(RestError::Decode {
            endpoint: EndpointKind::Settings,
        });
    }
    Ok(ServerSettings {
        experience_rate: wire.experience_rate,
        capture_rate: wire.capture_rate,
        pal_spawn_rate: wire.pal_spawn_rate,
        collection_drop_rate: wire.collection_drop_rate,
        collection_object_hp_rate: wire.collection_object_hp_rate,
        collection_respawn_rate: wire.collection_respawn_rate,
        enemy_drop_rate: wire.enemy_drop_rate,
        base_worker_limit: wire.base_worker_limit,
        egg_hatching_hours: wire.egg_hatching_hours,
        work_speed_rate: wire.work_speed_rate,
        fast_travel_enabled: wire.fast_travel_enabled,
    })
}

pub fn decode_info(bytes: &[u8]) -> Result<RawServerInfo, RestError> {
    let mut wire: InfoWire = serde_json::from_slice(bytes).map_err(|_| RestError::Decode {
        endpoint: EndpointKind::Info,
    })?;
    if wire.version.is_empty() || wire.worldguid.is_empty() {
        return Err(RestError::Decode {
            endpoint: EndpointKind::Info,
        });
    }
    Ok(RawServerInfo {
        version: std::mem::take(&mut wire.version),
        world_guid: std::mem::take(&mut wire.worldguid).into_bytes(),
    })
}

/// Tolerant, privacy-safe `/info` decoder used by live snapshots.
/// The world GUID is consumed without ever entering the output model.
pub fn decode_live_info(bytes: &[u8]) -> Result<ServerLiveInfoV1, RestError> {
    let wire: LiveInfoWire = serde_json::from_slice(bytes).map_err(|_| RestError::Decode {
        endpoint: EndpointKind::Info,
    })?;
    Ok(ServerLiveInfoV1 {
        version: wire.version,
        server_name: wire.server_name,
        description: wire.description,
    })
}

/// Tolerant `/metrics` decoder. Missing and future fields are allowed while
/// present numeric values still have to be valid server measurements.
pub fn decode_live_metrics(bytes: &[u8]) -> Result<ServerLiveMetricsV1, RestError> {
    let wire: LiveMetricsWire = serde_json::from_slice(bytes).map_err(|_| RestError::Decode {
        endpoint: EndpointKind::Metrics,
    })?;
    for value in [wire.serverfps, wire.serverframetime].into_iter().flatten() {
        if !value.is_finite() || value < 0.0 {
            return Err(RestError::Decode {
                endpoint: EndpointKind::Metrics,
            });
        }
    }
    Ok(ServerLiveMetricsV1 {
        server_fps: wire.serverfps,
        current_player_count: wire.currentplayernum,
        server_frame_time_ms: wire.serverframetime,
        max_player_count: wire.maxplayernum,
        uptime_seconds: wire.uptime,
        base_camp_count: wire.basecampnum,
        game_days: wire.days,
    })
}

/// Projects `/players` to display-only fields. Network addresses, account
/// names, platform identifiers and coordinates are ignored during decoding.
pub fn decode_live_players(bytes: &[u8]) -> Result<Vec<ServerLivePlayerV1>, RestError> {
    let wire: LivePlayersWire = serde_json::from_slice(bytes).map_err(|_| RestError::Decode {
        endpoint: EndpointKind::Players,
    })?;
    wire.players
        .unwrap_or_default()
        .into_iter()
        .map(|player| {
            if player
                .ping
                .is_some_and(|value| !value.is_finite() || value < 0.0)
            {
                return Err(RestError::Decode {
                    endpoint: EndpointKind::Players,
                });
            }
            Ok(ServerLivePlayerV1 {
                name: player.name,
                level: player.level,
                ping_ms: player.ping,
                building_count: player.building_count,
            })
        })
        .collect()
}

/// Preserves only the 66 reviewed official settings and derives the stable
/// fields currently used by the application. Unknown keys are consumed and
/// discarded rather than crossing the snapshot serialization boundary.
pub fn decode_live_settings(bytes: &[u8]) -> Result<ServerLiveSettingsV1, RestError> {
    let raw = serde_json::from_slice::<LiveSettingsRawWire>(bytes)
        .map_err(|_| RestError::Decode {
            endpoint: EndpointKind::Settings,
        })?
        .raw;
    let known = ServerLiveSettingsKnownV1 {
        experience_rate: nonnegative_number(&raw, "ExpRate"),
        capture_rate: nonnegative_number(&raw, "PalCaptureRate"),
        pal_spawn_rate: nonnegative_number(&raw, "PalSpawnNumRate"),
        collection_drop_rate: nonnegative_number(&raw, "CollectionDropRate"),
        collection_object_hp_rate: nonnegative_number(&raw, "CollectionObjectHpRate"),
        collection_respawn_rate: nonnegative_number(&raw, "CollectionObjectRespawnSpeedRate"),
        enemy_drop_rate: nonnegative_number(&raw, "EnemyDropItemRate"),
        base_worker_limit: raw
            .get("BaseCampWorkerMaxNum")
            .and_then(|value| value.as_u64()),
        egg_hatching_hours: nonnegative_number(&raw, "PalEggDefaultHatchingTime"),
        work_speed_rate: nonnegative_number(&raw, "WorkSpeedRate"),
        fast_travel_enabled: raw
            .get("bEnableFastTravel")
            .and_then(|value| value.as_bool()),
    };
    Ok(ServerLiveSettingsV1 { known, raw })
}

/// Aggregates `/game-data` without materializing actor identities, network
/// addresses or coordinates in any serializable value.
pub fn decode_live_game_data(bytes: &[u8]) -> Result<ServerLiveGameDataV1, RestError> {
    let wire: LiveGameDataWire = serde_json::from_slice(bytes).map_err(|_| RestError::Decode {
        endpoint: EndpointKind::GameData,
    })?;
    for value in [wire.server_fps, wire.average_server_fps]
        .into_iter()
        .flatten()
    {
        if !value.is_finite() || value < 0.0 {
            return Err(RestError::Decode {
                endpoint: EndpointKind::GameData,
            });
        }
    }

    let actors = wire.actors.unwrap_or_default();
    let total_actor_count = u64::try_from(actors.len()).map_err(|_| RestError::Decode {
        endpoint: EndpointKind::GameData,
    })?;
    let mut active_actor_count = 0_u64;
    let mut unit_type_counts = BTreeMap::new();
    for actor in actors {
        if is_active_value(actor.is_active.as_ref()) {
            active_actor_count = active_actor_count.checked_add(1).ok_or(RestError::Decode {
                endpoint: EndpointKind::GameData,
            })?;
        }
        if let Some(unit_type) = actor.unit_type.filter(|value| is_safe_unit_type(value)) {
            let count = unit_type_counts.entry(unit_type).or_insert(0_u64);
            *count = count.checked_add(1).ok_or(RestError::Decode {
                endpoint: EndpointKind::GameData,
            })?;
        }
    }
    Ok(ServerLiveGameDataV1 {
        server_fps: wire.server_fps,
        average_server_fps: wire.average_server_fps,
        total_actor_count,
        active_actor_count,
        unit_type_counts,
    })
}

fn nonnegative_number(
    values: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<f64> {
    values
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn is_active_value(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::Bool(value)) => *value,
        Some(serde_json::Value::String(value)) => value.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn is_safe_unit_type(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

fn validate_actor<E>(actor: &ActorWire) -> Result<(), E>
where
    E: de::Error,
{
    match actor.actor_type.as_str() {
        "Character" => {
            let unit_type = actor
                .unit_type
                .as_deref()
                .ok_or_else(|| E::missing_field("UnitType"))?;
            if !matches!(
                unit_type,
                "Player" | "OtomoPal" | "BaseCampPal" | "WildPal" | "NPC"
            ) {
                return Err(E::custom("unknown Character UnitType"));
            }
            for (name, coordinate) in [
                ("LocationX", actor.location_x),
                ("LocationY", actor.location_y),
                ("LocationZ", actor.location_z),
            ] {
                let value = coordinate.ok_or_else(|| E::missing_field(name))?;
                if !value.is_finite() {
                    return Err(E::custom("non-finite actor coordinate"));
                }
            }
            for rotation in [actor.rotation_x, actor.rotation_y, actor.rotation_z]
                .into_iter()
                .flatten()
            {
                if !rotation.is_finite() {
                    return Err(E::custom("non-finite actor rotation"));
                }
            }
            if actor
                .is_active
                .as_deref()
                .is_some_and(|value| !matches!(value, "true" | "false"))
            {
                return Err(E::custom("invalid IsActive value"));
            }
        }
        "PalBox" => {
            if actor.unit_type.is_some() || actor.instance_id.is_some() || actor.userid.is_some() {
                return Err(E::custom("PalBox contained Character identity fields"));
            }
            for coordinate in [actor.location_x, actor.location_y, actor.location_z]
                .into_iter()
                .flatten()
            {
                if !coordinate.is_finite() {
                    return Err(E::custom("non-finite PalBox coordinate"));
                }
            }
        }
        _ => return Err(E::custom("unknown actor Type")),
    }

    // Touch type-checked discarded fields so dead-code analysis documents that
    // they are deliberately parsed and then discarded.
    let _ = (
        actor.nickname,
        actor.trainer_instance_id,
        actor.trainer_nickname,
        actor.trainer_class,
        actor.ip,
        actor.level,
        actor.hp,
        actor.max_hp,
        actor.guild_id,
        actor.guild_name,
        actor.class,
        actor.action,
        actor.ai_action,
        actor.stage,
    );
    Ok(())
}

fn select_actor<E>(
    mut actor: ActorWire,
    selector: &PlayerSelector,
) -> Result<Option<SelectedActor>, E>
where
    E: de::Error,
{
    if actor.actor_type != "Character" || actor.unit_type.as_deref() != Some("Player") {
        return Ok(None);
    }
    let matches = match selector {
        PlayerSelector::UserId(expected) => actor.userid.as_deref() == Some(expected.as_str()),
        PlayerSelector::InstanceId(expected) => {
            actor.instance_id.as_deref() == Some(expected.as_str())
        }
    };
    if !matches {
        return Ok(None);
    }

    let raw_selected_id = match selector.kind() {
        SelectorKind::UserId => actor
            .userid
            .take()
            .ok_or_else(|| E::missing_field("userid"))?
            .into_bytes(),
        SelectorKind::InstanceId => actor
            .instance_id
            .take()
            .ok_or_else(|| E::missing_field("InstanceID"))?
            .into_bytes(),
    };
    let rotation_z = actor.rotation_z;
    let heading_degrees = match rotation_z {
        Some(value) if value.abs() <= f64::from(f32::MAX) => Some(value as f32),
        Some(_) => return Err(E::custom("RotationZ exceeded f32 range")),
        None => None,
    };
    Ok(Some(SelectedActor {
        raw_selected_id,
        selector_kind: selector.kind(),
        active: actor.is_active.as_deref() == Some("true"),
        x: actor
            .location_x
            .ok_or_else(|| E::missing_field("LocationX"))?,
        y: actor
            .location_y
            .ok_or_else(|| E::missing_field("LocationY"))?,
        z: actor
            .location_z
            .ok_or_else(|| E::missing_field("LocationZ"))?,
        heading_degrees,
    }))
}

fn is_server_local_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 19
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b' '
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7 | 10 | 13 | 16) || byte.is_ascii_digit())
}

fn erase_optional_string(value: &mut Option<String>) {
    if let Some(value) = value.take() {
        let mut bytes = value.into_bytes();
        bytes.fill(0);
    }
}

fn erase_string(value: &mut String) {
    let mut bytes = std::mem::take(value).into_bytes();
    bytes.fill(0);
}

fn strict_optional<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
