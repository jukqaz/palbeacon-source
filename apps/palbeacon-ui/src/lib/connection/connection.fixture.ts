import type {
  NativeAppSettingsDocument,
  NativeOwnedPalSnapshot,
  NativeOverlayControlDocument,
  NativeOverlayControlSnapshot,
  NativeProfileSnapshot,
  NativeSaveSourceProbe,
  NativeSaveSourceCandidate,
  NativeStagedSave,
  NativeServerLiveSnapshot,
  NativeServerProfiles,
  NativeSftpSyncStatus,
} from './types';

export const ownedPalOwnerUid = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee';
const stagedImportId = 'b0bc52eda611399d11efe26f';

export const ownedPalSnapshotFixture = (ownerUid: string | null): NativeOwnedPalSnapshot => ({
  schema_version: 2,
  parser_id: 'psp-core-readonly-v1.2.0',
  import_id: stagedImportId,
  world_name: 'DedicatedServerWorld',
  world_pal_count: 184,
  catalog_entry_count: 288,
  passive_catalog_entry_count: 419,
  active_skill_catalog_entry_count: 324,
  selected_owner_uid: ownerUid,
  players: [{ uid: ownedPalOwnerUid, nickname: 'Arthur', level: 60, pal_count: 42 }],
  pals: ownerUid
    ? [
        {
          instance_id: 'bbbbbbbb-cccc-dddd-eeee-ffffffffffff',
          species_id: 'GrassPanda_Electric',
          species_key: 'GrassPanda_Electric',
          species_name_ko: '라이바오',
          paldex_number: 103,
          nickname: null,
          owner_uid: ownerUid,
          gender: 'male',
          level: 55,
          passive_ids: ['Legend'],
          passive_names_ko: ['전설'],
          active_skill_ids: ['ElecWave'],
          active_skill_names_ko: ['전격파'],
          learned_skill_ids: [],
          learned_skill_names_ko: [],
          iv_hp: 91,
          iv_attack: 88,
          iv_defense: 86,
          condensation_rank: 4,
          soul_hp: 0,
          soul_attack: 0,
          soul_defense: 0,
          soul_work_speed: 0,
          trust: 0,
          is_lucky: false,
          is_boss: false,
          is_predator: false,
          is_tower: false,
          storage_id: 'cccccccc-dddd-eeee-ffff-aaaaaaaaaaaa',
          storage_slot: 3,
        },
      ]
    : [],
  limitations: ['No owned Pal details are returned until a local player UID is selected.'],
});

export const stagedSaveFixture: NativeStagedSave = {
  import_id: stagedImportId,
  source_world_folder_name: 'DedicatedServerWorld',
  staged_at_unix_ms: 1_787_000_000_000,
  player_file_count: 4,
  total_bytes: 18_400_000,
  has_personal_data: true,
};

export const saveSourceCandidateFixture: NativeSaveSourceCandidate = {
  world_root: 'C:\\Palworld\\Saved\\SaveGames\\0\\DedicatedServerWorld',
  world_folder_name: 'DedicatedServerWorld',
  player_file_count: 4,
  total_bytes: 18_400_000,
  latest_modified_unix_ms: 1_787_000_000_000,
  has_personal_data: true,
  files: [
    { role: 'level', relative_path: 'Level.sav', encoded_length: 17_000_000 },
    { role: 'player', relative_path: 'Players/0001.sav', encoded_length: 350_000 },
  ],
};

export const saveSourceProbeFixture: NativeSaveSourceProbe = {
  selected_root: 'C:\\Palworld\\Saved\\SaveGames',
  status: 'ready',
  candidates: [saveSourceCandidateFixture],
  warnings: [],
};

export const nativeProfileFixture: NativeProfileSnapshot = {
  runtime: {
    core_connected: true,
    game_running: true,
    overlay_running: true,
    position_live: false,
    map_ready: true,
    live_position_ready: false,
    map_alignment_verified: true,
    alignment_verified: false,
    source_mode: 'approved_map_pack',
    freshness: 'waiting',
    supported_client_build_id: '24575825',
    map_build_id: '24575825',
    message_ko: '현재 위치를 사용할 수 없어 지도만 표시합니다.',
  },
  save: {
    latest: stagedSaveFixture,
    parser_ready: true,
    message_ko: '보호 복사본과 읽기 전용 내 팰 해석기가 준비됐습니다.',
  },
};

const overlayControlFixture: NativeOverlayControlDocument = {
  schema: 'palbeacon.overlay_control.v2',
  version: 9,
  updated_at_unix_ms: 1_787_000_000_000,
  settings: {
    enabled: true,
    rotation_mode: 'north_up',
    input_mode: 'locked',
    display_mode: 'mini_map',
    opacity: 0.92,
    diameter_px: 320,
    zoom: 1,
    fps_profile: 'sixty',
    poi_filters: {
      fast_travel: true,
      boss: true,
      wanted: false,
      dungeon: true,
      enabled_layer_ids: ['map-unlock', 'poi', 'tower'],
      selected_pal_ids: [],
      night_only: false,
    },
    hotkey_bindings: {
      overlay_visibility: {
        modifiers: { control: false, alt: false, shift: false, windows: false },
        virtual_key: 0x77,
      },
      rotation_toggle: {
        modifiers: { control: true, alt: false, shift: false, windows: false },
        virtual_key: 0x78,
      },
      temporary_interaction: null,
      interaction_lock: null,
    },
  },
};

export const overlayControlSnapshotFixture: NativeOverlayControlSnapshot = {
  runtime: nativeProfileFixture.runtime,
  control: overlayControlFixture,
};

export const serverProfilesFixture: NativeServerProfiles = {
  schema: 'palbeacon.server_profiles.v1',
  version: 7,
  updated_at_unix_ms: 1_787_000_000_000,
  selected_profile_id: 'main-server',
  profiles: [
    {
      id: 'main-server',
      display_name: '메인 전용 서버',
      host: '192.0.2.10',
      rest_host: null,
      game_port: 8211,
      query_port: 27015,
      sftp_port: 22,
      rest_port: 8212,
      rcon_port: null,
      rest_scheme: 'https',
      rest_username: 'palbeacon',
      sftp_username: 'palworld',
      save_root: '/Pal/Saved/SaveGames/0',
      ssh_host_key_fingerprint: 'SHA256:verified',
      has_sftp_password: true,
      has_rest_password: true,
      has_insecure_rest_http_consent: false,
    },
  ],
};

export const appSettingsFixture: NativeAppSettingsDocument = {
  schema: 'palbeacon.app_settings.v1',
  version: 4,
  updated_at_unix_ms: 1_787_000_000_000,
  settings: {
    launch_game_on_manual_start: true,
    open_app_on_manual_start: false,
    selected_import_id: stagedImportId,
    selected_owner_uid: '123e4567-e89b-12d3-a456-426614174000',
  },
};

export const serverLiveFixture: NativeServerLiveSnapshot = {
  schema: 'pal_companion.server_live_snapshot.v1',
  profile_id: 'main-server',
  checked_at_unix_ms: 1_787_000_000_000,
  state: 'online',
  message_ko: '공식 REST 상태를 확인했습니다.',
  info: {
    status: 'ok',
    observed_at_unix_ms: 1_787_000_000_000,
    latency_ms: 18,
    message_ko: '정상적으로 가져왔습니다.',
    value: { version: 'v0.7.0', server_name: '메인 전용 서버', description: null },
  },
  metrics: {
    status: 'ok',
    observed_at_unix_ms: 1_787_000_000_000,
    latency_ms: 22,
    message_ko: '정상적으로 가져왔습니다.',
    value: {
      server_fps: 60,
      current_player_count: 4,
      server_frame_time_ms: 16.7,
      max_player_count: 16,
      uptime_seconds: 7200,
      base_camp_count: 3,
      game_days: 41,
    },
  },
  players: {
    status: 'ok',
    observed_at_unix_ms: 1_787_000_000_000,
    latency_ms: 24,
    message_ko: '정상적으로 가져왔습니다.',
    value: [],
  },
  settings: {
    status: 'not_requested',
    observed_at_unix_ms: null,
    latency_ms: null,
    message_ko: '이번 조회 대상에 포함되지 않았습니다.',
    value: null,
  },
  game_data: {
    status: 'not_requested',
    observed_at_unix_ms: null,
    latency_ms: null,
    message_ko: '이번 조회 대상에 포함되지 않았습니다.',
    value: null,
  },
};

export const sftpSyncFixture: NativeSftpSyncStatus = {
  schema: 'palbeacon.sftp_sync_status.v1',
  profile_id: 'main-server',
  state: 'up_to_date',
  checked_at_unix_ms: 1_787_000_000_000,
  last_success_at_unix_ms: 1_787_000_000_000,
  remote_revision: 'sha256:verified',
  import_id: stagedSaveFixture.import_id,
  player_file_count: 4,
  total_bytes: 18_400_000,
  message_ko: '서버 세이브가 최신 상태입니다.',
};
