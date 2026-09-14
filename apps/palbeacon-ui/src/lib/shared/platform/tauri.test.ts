import { afterEach, describe, expect, it } from 'vitest';
import { isTauriRuntime, requireTauriRuntime } from './tauri';

describe('Tauri runtime boundary', () => {
  afterEach(() => {
    delete window.__TAURI_INTERNALS__;
  });

  it('rejects native operations in a browser', () => {
    expect(isTauriRuntime()).toBe(false);
    expect(() => requireTauriRuntime()).toThrow('Windows 앱에서만 사용할 수 있는 기능입니다.');
  });

  it('recognizes the official Tauri bridge', () => {
    window.__TAURI_INTERNALS__ = {};
    expect(isTauriRuntime()).toBe(true);
    expect(() => requireTauriRuntime()).not.toThrow();
  });
});
