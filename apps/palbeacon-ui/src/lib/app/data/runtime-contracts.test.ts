import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { unifiedCatalogSchema } from '$lib/catalog/runtime-schema';
import type { UnifiedCatalog } from '$lib/catalog/types';
import { exactMapPoiDocumentSchema, mapTileIndexSchema } from '$lib/map/runtime-schema';
import type { ExactMapPoiDocument, MapTileIndex } from '$lib/map/types';
import { parseRuntimePayload } from '$lib/shared/data/parse-runtime-payload';
import { technologyCatalogSchema } from '$lib/technology/runtime-schema';
import type { TechnologyCatalog } from '$lib/technology/types';
import { toolsCatalogSchema } from '$lib/tools/runtime-schema';
import type { ToolsCatalog } from '$lib/tools/types';

const generatedJson = async (path: string): Promise<unknown> =>
  JSON.parse(await readFile(resolve(process.cwd(), 'static/generated', path), 'utf8')) as unknown;

describe('runtime payload validation', () => {
  it('accepts the generated catalog, technology, tools, and map contracts', async () => {
    const catalog = parseRuntimePayload<UnifiedCatalog>(
      unifiedCatalogSchema,
      await generatedJson('game/catalog.v1.json'),
      '도감 데이터',
    );
    const technology = parseRuntimePayload<TechnologyCatalog>(
      technologyCatalogSchema,
      await generatedJson('game/technology.v1.json'),
      '기술 데이터',
    );
    const tools = parseRuntimePayload<ToolsCatalog>(
      toolsCatalogSchema,
      await generatedJson('game/tools.v1.json'),
      '계획 도구 데이터',
    );
    const map = parseRuntimePayload<MapTileIndex>(
      mapTileIndexSchema,
      await generatedJson('map/tile-index.v1.json'),
      '지도 데이터',
    );
    const pois = parseRuntimePayload<ExactMapPoiDocument>(
      exactMapPoiDocumentSchema,
      await generatedJson('map/pois.v1.json'),
      '지도 위치 데이터',
    );

    expect(catalog.statistics.pals).toBe(288);
    expect(technology.technologies).toHaveLength(588);
    expect(tools.pals.length).toBeGreaterThan(250);
    expect(map.tiles.length).toBe(map.tile_count);
    expect(pois.pois.length).toBe(pois.poi_count);
  });

  it('rejects malformed network payloads before they reach the UI', () => {
    expect(() =>
      parseRuntimePayload<UnifiedCatalog>(
        unifiedCatalogSchema,
        { schema_version: 1, verified: true, records: [] },
        '도감 데이터',
      ),
    ).toThrow('도감 데이터 형식이 올바르지 않습니다.');
    expect(() =>
      parseRuntimePayload<TechnologyCatalog>(
        technologyCatalogSchema,
        { schema_version: 1, verified: true, technologies: 'invalid' },
        '기술 데이터',
      ),
    ).toThrow('기술 데이터 형식이 올바르지 않습니다.');
    expect(() =>
      parseRuntimePayload<MapTileIndex>(
        mapTileIndexSchema,
        { schema_version: 1, tiles: [{ relative_path: 1 }] },
        '지도 데이터',
      ),
    ).toThrow('지도 데이터 형식이 올바르지 않습니다.');
  });
});
