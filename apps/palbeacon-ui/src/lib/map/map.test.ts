import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { mapFixture } from './map.fixture';
import {
  declutterMapPois,
  filterMapPois,
  mapAreaLabel,
  mapFilterOptions,
  mapPoisForRegion,
  mapRegions,
  publicSupplementalFilterIds,
  termById,
  validateMapBundle,
} from './map';
import type { MapBundle } from './types';

const readMapAsset = <T>(path: string): T =>
  JSON.parse(
    readFileSync(resolve(process.cwd(), '..', '..', 'assets', 'palbeacon', 'game', 'map', path), {
      encoding: 'utf8',
    }),
  ) as T;

const activeMapBundle = (): MapBundle =>
  validateMapBundle({
    indexes: [readMapAsset('tile-index.v1.json'), readMapAsset('tree/tile-index.v1.json')],
    exact: readMapAsset('pois.v1.json'),
    terminology: readMapAsset('poi-terminology.ko.v1.json'),
    supplemental: readMapAsset('layers.v1.json'),
  });

describe('map data contract', () => {
  it('searches exact and supplemental Korean data without dropping internal search aliases', () => {
    const terms = termById(mapFixture);
    const pois = mapPoisForRegion(mapFixture, mapRegions[0]);
    const filters = new Set(['fast_travel', 'boss', 'dungeon', 'ore-quartz']);

    expect(filterMapPois(pois, terms, filters, '라이바오').map((poi) => poi.id)).toEqual([
      'boss:raibao',
    ]);
    expect(filterMapPois(pois, terms, filters, 'GrassPanda_Electric')).toHaveLength(1);
    expect(filterMapPois(pois, terms, filters, '워프').map((poi) => poi.kind)).toEqual([
      'fast_travel',
    ]);
    expect(filterMapPois(pois, terms, filters, '석영')).toHaveLength(2);
    expect(filterMapPois(pois, terms, new Set(), '석영')).toHaveLength(2);
  });

  it('offers every public data-backed filter, merges aliases and hides unverified layers', () => {
    const pois = mapPoisForRegion(mapFixture, mapRegions[0]);
    const options = mapFilterOptions(mapFixture, mapRegions[0], pois);

    expect(options.map((option) => option.label)).toEqual([
      '참수리 상',
      '던전',
      '필드 보스',
      '보스 타워',
      '순수한 석영',
    ]);
    expect(options.find((option) => option.id === 'boss')?.count).toBe(2);
    expect(options.some((option) => option.id === 'sealed-realm')).toBe(false);
    expect(options.some((option) => option.id === 'ancient-shrine')).toBe(false);
    expect(publicSupplementalFilterIds(mapFixture)).toEqual(['egg', 'ore-quartz', 'tower']);
  });

  it('covers all 37 public filters in the active map data', () => {
    const bundle = activeMapBundle();
    const options = mapRegions.flatMap((region) => mapFilterOptions(bundle, region));
    const ids = new Set(options.map((option) => option.id));

    expect(ids.size).toBe(37);
    expect(ids).toEqual(
      new Set(['fast_travel', 'boss', 'dungeon', 'wanted', ...publicSupplementalFilterIds(bundle)]),
    );
    for (const hiddenId of [
      'respawn',
      'dimensional-warp',
      'dimensional-distortion',
      'captured-pal',
      'anti-air-turret',
      'medal',
      'ancient-shrine',
      'healing-spring',
    ]) {
      expect(ids.has(hiddenId)).toBe(false);
    }
  });

  it('declutters individual markers without creating aggregate counts', () => {
    const basePoi = mapPoisForRegion(mapFixture, mapRegions[0])[0];
    if (!basePoi) throw new Error('지도 fixture POI가 필요합니다.');
    const pois = Array.from({ length: 24 }, (_, index) => ({
      ...basePoi,
      id: `boss:${String(index)}`,
      map_x: 100 + index * 4,
      map_y: 100 + index * 3,
    }));

    const overview = declutterMapPois(pois, 1, null);
    const zoomed = declutterMapPois(pois, 5, null);
    expect(overview.every((poi) => poi.id.startsWith('boss:'))).toBe(true);
    expect(zoomed.length).toBeGreaterThan(overview.length);
    expect(declutterMapPois(pois, 1, 'boss:23').some((poi) => poi.id === 'boss:23')).toBe(true);
  });

  it('turns verified map coordinates into readable area labels without exposing coordinates', () => {
    expect(mapAreaLabel({ map_x: 120, map_y: 120 })).toBe('북서쪽');
    expect(mapAreaLabel({ map_x: 1024, map_y: 1024 })).toBe('중앙');
    expect(mapAreaLabel({ map_x: 1900, map_y: 1900 })).toBe('남동쪽');
  });

  it('rejects a mismatched exact-build map bundle', () => {
    expect(validateMapBundle(mapFixture)).toBe(mapFixture);
    expect(() =>
      validateMapBundle({
        ...mapFixture,
        exact: { ...mapFixture.exact, poi_count: 999 },
      }),
    ).toThrow('지도 데이터 계약이 일치하지 않습니다.');
  });
});
