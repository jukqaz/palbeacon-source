import type {
  NativeAppSettings,
  NativeAppSettingsDocument,
  NativeOwnedPalSnapshot,
  NativeOverlayControlDocument,
  NativeOverlayControlSnapshot,
  NativeOverlaySettings,
  NativeProfileSnapshot,
  NativeRuntimeStatus,
  NativeSaveImportStatus,
  NativeSaveSourceProbe,
  NativeStagedSave,
  NativeServerProfileMutation,
  NativeServerLiveSnapshot,
  NativeServerProfiles,
  NativeSftpSyncStatus,
} from './types';
import type { NativeReadOperation } from './generated/native-command-contract';
import { invokeNative, isTauriRuntime, requireTauriRuntime } from '$lib/shared/platform/tauri';

export type { NativeReadOperation } from './generated/native-command-contract';

const nativeRead = async <T>(operation: NativeReadOperation): Promise<T> => {
  return invokeNative<T>('native_read', { operation });
};

export const loadNativeProfileSnapshot = async (): Promise<NativeProfileSnapshot> => {
  const [runtime, save] = await Promise.all([
    nativeRead<NativeRuntimeStatus>('runtime_status'),
    nativeRead<NativeSaveImportStatus>('save_import_status'),
  ]);
  return { runtime, save };
};

const loadNativeRuntimeStatus = (): Promise<NativeRuntimeStatus> =>
  nativeRead<NativeRuntimeStatus>('runtime_status');

export const loadNativeOverlayControlDocument = (): Promise<NativeOverlayControlDocument> =>
  nativeRead<NativeOverlayControlDocument>('overlay_control');

export const loadNativeOverlayControlSnapshot = async (): Promise<NativeOverlayControlSnapshot> => {
  const [runtime, control] = await Promise.all([
    loadNativeRuntimeStatus(),
    loadNativeOverlayControlDocument(),
  ]);
  return { runtime, control };
};

export const updateNativeOverlayControl = async (
  expectedVersion: number,
  settings: NativeOverlaySettings,
): Promise<NativeOverlayControlDocument> => {
  return invokeNative<NativeOverlayControlDocument>('native_update_overlay_control', {
    expectedVersion,
    settings,
  });
};

export const loadNativeServerProfiles = (): Promise<NativeServerProfiles> =>
  nativeRead<NativeServerProfiles>('server_profiles');

export const loadNativeAppSettings = (): Promise<NativeAppSettingsDocument> =>
  nativeRead<NativeAppSettingsDocument>('app_settings');

export const updateNativeAppSettings = async (
  expectedVersion: number,
  settings: NativeAppSettings,
): Promise<NativeAppSettingsDocument> => {
  return invokeNative<NativeAppSettingsDocument>('native_update_settings', {
    expectedVersion,
    settings,
  });
};

export const loadNativeAutostartEnabled = async (): Promise<boolean> => {
  requireTauriRuntime();
  const { isEnabled } = await import('@tauri-apps/plugin-autostart');
  return isEnabled();
};

export const updateNativeAutostartEnabled = async (enabled: boolean): Promise<void> => {
  requireTauriRuntime();
  const { disable, enable } = await import('@tauri-apps/plugin-autostart');
  await (enabled ? enable() : disable());
};

export const mutateNativeServerProfiles = async (
  request: NativeServerProfileMutation,
): Promise<NativeServerProfiles> => {
  return invokeNative<NativeServerProfiles>('native_mutate_server_profiles', { request });
};

export const loadNativeServerLive = async (
  profileId: string,
): Promise<NativeServerLiveSnapshot> => {
  return invokeNative<NativeServerLiveSnapshot>('native_server_live', {
    profileId,
    request: {
      include_info: true,
      include_metrics: true,
      include_players: true,
      include_settings: true,
      include_game_data: false,
    },
  });
};

export const loadNativeSftpStatus = async (
  profileId: string,
): Promise<NativeSftpSyncStatus | null> => {
  return invokeNative<NativeSftpSyncStatus | null>('native_sftp_status', { profileId });
};

export const syncNativeSftp = async (profileId: string): Promise<NativeSftpSyncStatus> => {
  return invokeNative<NativeSftpSyncStatus>('native_sync_sftp', { profileId });
};

export const selectNativeSaveSourceDirectory = async (): Promise<string | null> => {
  requireTauriRuntime('Windows 앱에서만 세이브 폴더를 선택할 수 있습니다.');
  const { open } = await import('@tauri-apps/plugin-dialog');
  const selected = await open({
    directory: true,
    multiple: false,
    title: 'Palworld 세이브 폴더 선택',
  });
  return typeof selected === 'string' ? selected : null;
};

export const probeNativeSaveSource = async (sourcePath: string): Promise<NativeSaveSourceProbe> => {
  return invokeNative<NativeSaveSourceProbe>('native_probe_save_source', { sourcePath });
};

export const stageNativeSaveSource = async (sourcePath: string): Promise<NativeStagedSave> => {
  return invokeNative<NativeStagedSave>('native_stage_save_source', { sourcePath });
};

export const inspectNativeStagedSave = async (
  importId: string,
  ownerUid: string | null = null,
): Promise<NativeOwnedPalSnapshot> => {
  return invokeNative<NativeOwnedPalSnapshot>('native_inspect_staged_save', { importId, ownerUid });
};

export const rememberNativePersonalSelection = async (
  importId: string,
  ownerUid: string,
): Promise<void> => {
  if (!isTauriRuntime()) return;
  const document = await loadNativeAppSettings();
  if (
    document.settings.selected_import_id === importId &&
    document.settings.selected_owner_uid === ownerUid
  )
    return;
  await updateNativeAppSettings(document.version, {
    ...document.settings,
    selected_import_id: importId,
    selected_owner_uid: ownerUid,
  });
};
