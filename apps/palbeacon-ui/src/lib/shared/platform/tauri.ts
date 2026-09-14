const WINDOWS_ONLY_MESSAGE = 'Windows 앱에서만 사용할 수 있는 기능입니다.';

export const isTauriRuntime = (): boolean =>
  typeof window !== 'undefined' && window.__TAURI_INTERNALS__ !== undefined;

export const requireTauriRuntime = (message = WINDOWS_ONLY_MESSAGE): void => {
  if (!isTauriRuntime()) throw new Error(message);
};

export const invokeNative = async <T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> => {
  requireTauriRuntime();
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
};
