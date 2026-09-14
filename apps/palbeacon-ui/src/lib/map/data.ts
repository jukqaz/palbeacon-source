import { resolve } from '$app/paths';
import {
  exactMapPoiDocumentSchema,
  mapTerminologySchema,
  mapTileIndexSchema,
  supplementalMapDocumentSchema,
} from './runtime-schema';
import { parseRuntimePayload } from '$lib/shared/data/parse-runtime-payload';
import type { GenericSchema } from 'valibot';
import { validateMapBundle } from './map';
import type {
  ExactMapPoiDocument,
  MapBundle,
  MapTerminology,
  MapTileIndex,
  SupplementalMapDocument,
} from './types';

const mapAsset = (path: string): string => resolve('/[...path]', { path: `generated/map/${path}` });

const readJson = async <T>(
  fetcher: typeof fetch,
  path: string,
  schema: GenericSchema,
): Promise<T> => {
  const response = await fetcher(mapAsset(path));
  if (!response.ok) {
    throw new Error(`지도 파일을 읽지 못했습니다: ${path} (${String(response.status)})`);
  }
  return parseRuntimePayload<T>(schema, await response.json(), `지도 데이터 ${path}`);
};

export const loadMapBundle = async (fetcher: typeof fetch = fetch): Promise<MapBundle> => {
  const [main, tree, exact, terminology] = await Promise.all([
    readJson<MapTileIndex>(fetcher, 'tile-index.v1.json', mapTileIndexSchema),
    readJson<MapTileIndex>(fetcher, 'tree/tile-index.v1.json', mapTileIndexSchema),
    readJson<ExactMapPoiDocument>(fetcher, 'pois.v1.json', exactMapPoiDocumentSchema),
    readJson<MapTerminology>(fetcher, 'poi-terminology.ko.v1.json', mapTerminologySchema),
  ]);

  let supplemental: SupplementalMapDocument | null = null;
  try {
    supplemental = await readJson<SupplementalMapDocument>(
      fetcher,
      'layers.v1.json',
      supplementalMapDocumentSchema,
    );
  } catch {
    // The exact-build POIs and tiles remain usable when the third-party layer is absent.
  }

  return validateMapBundle({ indexes: [main, tree], exact, terminology, supplemental });
};
