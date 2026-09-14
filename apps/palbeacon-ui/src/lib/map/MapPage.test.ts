import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { overlayControlSnapshotFixture } from '$lib/connection/connection.fixture';
import type { NativeOverlayControlDocument, NativeOverlaySettings } from '$lib/connection/types';
import MapPage from './MapPage.svelte';
import { mapFixture } from './map.fixture';

describe('MapPage overlay filter integration', () => {
  it('loads and persists one exact plus supplemental PC map selection', async () => {
    const control = structuredClone(overlayControlSnapshotFixture.control);
    const updateOverlay = vi.fn<
      (
        expectedVersion: number,
        settings: NativeOverlaySettings,
      ) => Promise<NativeOverlayControlDocument>
    >((expectedVersion, settings) =>
      Promise.resolve({
        ...control,
        version: expectedVersion + 1,
        settings,
      }),
    );

    render(MapPage, {
      loadBundle: () => Promise.resolve(mapFixture),
      loadOverlay: () => Promise.resolve(control),
      updateOverlay,
      nativeRuntime: true,
    });

    const controls = await screen.findByRole('complementary', {
      name: '지도 검색과 필터',
    });
    const quartz = within(controls).getByRole('button', { name: /순수한 석영/ });
    expect(quartz).toHaveAttribute('aria-pressed', 'false');

    await fireEvent.click(quartz);

    await waitFor(() => expect(updateOverlay).toHaveBeenCalledOnce());
    expect(updateOverlay.mock.calls[0]?.[0]).toBe(control.version);
    expect(updateOverlay.mock.calls[0]?.[1].poi_filters).toMatchObject({
      fast_travel: true,
      boss: true,
      dungeon: true,
      wanted: false,
    });
    expect(updateOverlay.mock.calls[0]?.[1].poi_filters.enabled_layer_ids).toEqual([
      'ore-quartz',
      'tower',
    ]);
  });
});
