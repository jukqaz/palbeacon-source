import type { NativeOverlaySettings } from '$lib/connection/types';

export const exactPoiKinds = ['fast_travel', 'boss', 'dungeon', 'wanted'] as const;

export const enabledMapFilterIds = (settings: NativeOverlaySettings): string[] => [
  ...exactPoiKinds.filter((kind) => settings.poi_filters[kind]),
  ...settings.poi_filters.enabled_layer_ids,
];

export const withMapFilterIds = (
  settings: NativeOverlaySettings,
  enabledFilterIds: ReadonlySet<string>,
  availableSupplementalIds: ReadonlySet<string>,
): NativeOverlaySettings => ({
  ...settings,
  poi_filters: {
    ...settings.poi_filters,
    fast_travel: enabledFilterIds.has('fast_travel'),
    boss: enabledFilterIds.has('boss'),
    dungeon: enabledFilterIds.has('dungeon'),
    wanted: enabledFilterIds.has('wanted'),
    enabled_layer_ids: [...availableSupplementalIds]
      .filter((id) => enabledFilterIds.has(id))
      .toSorted(),
  },
});
