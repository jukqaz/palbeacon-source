import { describe, expect, it, vi } from 'vitest';
import { precacheInBatches, publicPrecachePages } from './precache';

describe('offline catalog installation', () => {
  it('uses the Web route contract and excludes native, retired and error pages', () => {
    expect(
      publicPrecachePages([
        '/',
        '/pals',
        '/pals/',
        '/fan-content-notice',
        '/technology',
        '/connection/profile',
        '/connection/overlay',
        '/connection/servers',
        '/settings',
        '/404',
        '/wiki',
        '/unknown',
      ]),
    ).toEqual(['/', '/pals', '/pals/', '/fan-content-notice', '/technology']);
  });

  it('bounds background requests and preserves every offline asset', async () => {
    const assets = Array.from({ length: 11 }, (_, index) => `/asset-${index.toString()}`);
    let release: (() => void) | undefined;
    const addAll = vi
      .fn<Cache['addAll']>()
      .mockImplementationOnce(
        () =>
          new Promise<void>((resolve) => {
            release = resolve;
          }),
      )
      .mockResolvedValue(undefined);

    const pending = precacheInBatches({ addAll }, assets);
    expect(addAll).toHaveBeenCalledTimes(1);
    expect(addAll.mock.calls[0]?.[0]).toEqual(assets.slice(0, 4));
    release?.();
    await pending;

    expect(addAll.mock.calls.flatMap(([batch]) => Array.from(batch))).toEqual(assets);
    expect(addAll.mock.calls.every(([batch]) => Array.from(batch).length <= 4)).toBe(true);
  });

  it('rejects a failed installation without scheduling the remaining assets', async () => {
    const addAll = vi.fn<Cache['addAll']>().mockRejectedValue(new Error('offline'));
    await expect(precacheInBatches({ addAll }, ['/a', '/b', '/c', '/d', '/e'])).rejects.toThrow(
      'offline',
    );
    expect(addAll).toHaveBeenCalledTimes(1);
  });
});
