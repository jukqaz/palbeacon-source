import { invokeNative, isTauriRuntime } from '$lib/shared/platform/tauri';

export type PalPlatform = 'web' | 'windows';

export interface RuntimeContext {
  platform: PalPlatform;
  shell: 'browser' | 'tauri';
  appVersion: string | null;
}

const webContext: RuntimeContext = {
  platform: 'web',
  shell: 'browser',
  appVersion: null,
};

export const detectRuntimeContext = async (): Promise<RuntimeContext> => {
  if (!isTauriRuntime()) return webContext;
  return invokeNative<RuntimeContext>('runtime_context');
};
