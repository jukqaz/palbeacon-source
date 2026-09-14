import { describe, expect, it } from 'vitest';
import { overlayControlSnapshotFixture } from '$lib/connection/connection.fixture';
import { enabledMapFilterIds, withMapFilterIds } from './overlay-filters';

describe('map and overlay filter bridge', () => {
  it('reads exact and public layer filters as one map selection', () => {
    expect(enabledMapFilterIds(overlayControlSnapshotFixture.control.settings)).toEqual([
      'fast_travel',
      'boss',
      'dungeon',
      'map-unlock',
      'poi',
      'tower',
    ]);
  });

  it('replaces exact and public layer filters while preserving unrelated settings', () => {
    const current = overlayControlSnapshotFixture.control.settings;
    const next = withMapFilterIds(
      current,
      new Set(['fast_travel', 'wanted', 'egg', 'ore-quartz', 'ancient-shrine']),
      new Set(['egg', 'ore-quartz', 'tower']),
    );

    expect(next.poi_filters).toMatchObject({
      fast_travel: true,
      boss: false,
      dungeon: false,
      wanted: true,
    });
    expect(next.poi_filters.enabled_layer_ids).toEqual(['egg', 'ore-quartz']);
    expect(next.poi_filters.selected_pal_ids).toEqual(current.poi_filters.selected_pal_ids);
    expect(next.poi_filters.night_only).toBe(current.poi_filters.night_only);
  });
});
