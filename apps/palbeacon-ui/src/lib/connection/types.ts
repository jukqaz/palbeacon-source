import type { NativeServerProfileInput } from './generated/native-command-contract';

export type {
  NativeServerProfileInput,
  NativeServerProfileMutation,
} from './generated/native-command-contract';

export interface NativeRuntimeStatus {
  core_connected: boolean;
  game_running: boolean;
  overlay_running: boolean;
  position_live: boolean;
  map_ready: boolean;
  live_position_ready: boolean;
  map_alignment_verified: boolean;
  alignment_verified: boolean;
  source_mode: string;
  freshness: string;
  supported_client_build_id: string;
  map_build_id: string | null;
  message_ko: string;
}

type NativeOverlayRotationMode = 'north_up' | 'heading_up';
type NativeOverlayInputMode = 'locked' | 'pinned_interactive';
type NativeOverlayDisplayMode = 'mini_map' | 'expanded_map';
type NativeOverlayFpsProfile = 'thirty' | 'sixty';

export interface NativeOverlayHotkeyChord {
  modifiers: {
    control: boolean;
    alt: boolean;
    shift: boolean;
    windows: boolean;
  };
  virtual_key: number;
}

interface NativeOverlayHotkeyBindings {
  overlay_visibility: NativeOverlayHotkeyChord | null;
  rotation_toggle: NativeOverlayHotkeyChord | null;
  temporary_interaction: NativeOverlayHotkeyChord | null;
  interaction_lock: NativeOverlayHotkeyChord | null;
}

interface NativeOverlayPoiFilters {
  fast_travel: boolean;
  boss: boolean;
  wanted: boolean;
  dungeon: boolean;
  enabled_layer_ids: string[];
  selected_pal_ids: string[];
  night_only: boolean;
}

export interface NativeOverlaySettings {
  enabled: boolean;
  rotation_mode: NativeOverlayRotationMode;
  input_mode: NativeOverlayInputMode;
  display_mode: NativeOverlayDisplayMode;
  opacity: number;
  diameter_px: number;
  zoom: number;
  fps_profile: NativeOverlayFpsProfile;
  poi_filters: NativeOverlayPoiFilters;
  hotkey_bindings: NativeOverlayHotkeyBindings;
}

export interface NativeOverlayControlDocument {
  schema: string;
  version: number;
  updated_at_unix_ms: number;
  settings: NativeOverlaySettings;
}

export interface NativeOverlayControlSnapshot {
  runtime: NativeRuntimeStatus;
  control: NativeOverlayControlDocument;
}

export interface NativeStagedSave {
  schema_version?: number;
  import_id: string;
  staged_root?: string;
  source_world_folder_name: string;
  staged_at_unix_ms: number;
  player_file_count: number;
  total_bytes: number;
  has_personal_data: boolean;
  files?: NativeStagedSaveFile[];
}

type NativeSaveFileRole = 'level' | 'level_meta' | 'world_option' | 'player' | 'player_dps';

interface NativeStagedSaveFile {
  role: NativeSaveFileRole;
  relative_path: string;
  encoded_length: number;
  sha256: string;
}

export interface NativeSaveSourceCandidate {
  world_root: string;
  world_folder_name: string;
  player_file_count: number;
  total_bytes: number;
  latest_modified_unix_ms: number | null;
  has_personal_data: boolean;
  files: {
    role: NativeSaveFileRole;
    relative_path: string;
    encoded_length: number;
  }[];
}

export interface NativeSaveSourceProbe {
  selected_root: string;
  status: 'ready' | 'ready_without_players' | 'no_save_world' | 'multiple_save_worlds';
  candidates: NativeSaveSourceCandidate[];
  warnings: string[];
}

export interface NativeSaveImportStatus {
  latest: NativeStagedSave | null;
  parser_ready: boolean;
  message_ko: string;
}

interface NativeSavePlayerChoice {
  uid: string;
  nickname: string;
  level: number | null;
  pal_count: number;
}

interface NativeOwnedPal {
  instance_id: string;
  species_id: string;
  species_key: string;
  species_name_ko: string | null;
  paldex_number: number | null;
  nickname: string | null;
  owner_uid: string | null;
  gender: string;
  level: number;
  passive_ids: string[];
  passive_names_ko: string[];
  active_skill_ids: string[];
  active_skill_names_ko: string[];
  learned_skill_ids: string[];
  learned_skill_names_ko: string[];
  iv_hp: number | null;
  iv_attack: number | null;
  iv_defense: number | null;
  condensation_rank: number;
  soul_hp: number;
  soul_attack: number;
  soul_defense: number;
  soul_work_speed: number;
  trust: number;
  is_lucky: boolean;
  is_boss: boolean;
  is_predator: boolean;
  is_tower: boolean;
  storage_id: string;
  storage_slot: number;
}

export interface NativeOwnedPalSnapshot {
  schema_version: number;
  parser_id: string;
  import_id: string;
  world_name: string;
  world_pal_count: number;
  catalog_entry_count: number;
  passive_catalog_entry_count: number;
  active_skill_catalog_entry_count: number;
  selected_owner_uid: string | null;
  players: NativeSavePlayerChoice[];
  pals: NativeOwnedPal[];
  limitations: string[];
}

export interface NativeServerProfile extends NativeServerProfileInput {
  has_sftp_password: boolean;
  has_rest_password: boolean;
  has_insecure_rest_http_consent: boolean;
}

export interface NativeServerProfiles {
  schema: string;
  version: number;
  updated_at_unix_ms: number;
  selected_profile_id: string | null;
  profiles: NativeServerProfile[];
}

export interface NativeProfileSnapshot {
  runtime: NativeRuntimeStatus;
  save: NativeSaveImportStatus;
}

export interface NativeAppSettings {
  launch_game_on_manual_start: boolean;
  open_app_on_manual_start: boolean;
  selected_import_id: string | null;
  selected_owner_uid: string | null;
}

export interface NativeAppSettingsDocument {
  schema: string;
  version: number;
  updated_at_unix_ms: number;
  settings: NativeAppSettings;
}

interface NativeServerLiveSection<T> {
  status: 'ok' | 'error' | 'unsupported' | 'not_configured' | 'not_requested';
  observed_at_unix_ms: number | null;
  latency_ms: number | null;
  message_ko: string;
  value: T | null;
}

export interface NativeServerLiveSnapshot {
  schema: string;
  profile_id: string;
  checked_at_unix_ms: number;
  state: 'configuration_required' | 'online' | 'partial' | 'offline';
  message_ko: string;
  info: NativeServerLiveSection<{
    version: string | null;
    server_name: string | null;
    description: string | null;
  }>;
  metrics: NativeServerLiveSection<{
    server_fps: number | null;
    current_player_count: number | null;
    server_frame_time_ms: number | null;
    max_player_count: number | null;
    uptime_seconds: number | null;
    base_camp_count: number | null;
    game_days: number | null;
  }>;
  players: NativeServerLiveSection<
    {
      name: string | null;
      level: number | null;
      ping_ms: number | null;
      building_count: number | null;
    }[]
  >;
  settings: NativeServerLiveSection<Record<string, unknown>>;
  game_data: NativeServerLiveSection<Record<string, unknown>>;
}

export interface NativeSftpSyncStatus {
  schema: string;
  profile_id: string | null;
  state:
    | 'disabled'
    | 'awaiting_configuration'
    | 'awaiting_password'
    | 'checking'
    | 'up_to_date'
    | 'synced'
    | 'error';
  checked_at_unix_ms: number;
  last_success_at_unix_ms: number | null;
  remote_revision: string | null;
  import_id: string | null;
  player_file_count: number | null;
  total_bytes: number | null;
  message_ko: string;
}
