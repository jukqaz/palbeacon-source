import { normalizeCompactSearchText } from '$lib/shared/search/search-core';
import { exactPoiKinds } from './overlay-filters';
import type {
  ExactMapPoi,
  MapBundle,
  MapFilterOption,
  MapPoi,
  MapRegionOption,
  MapTerm,
  MapTile,
  MapTileIndex,
  SupplementalLayer,
} from './types';

export const mapRegions: readonly [MapRegionOption, MapRegionOption] = [
  { id: 'main', mapId: 'MainMap', regionId: 'FirstRegion', label: '팰파고스 제도' },
  { id: 'tree', mapId: 'Tree', regionId: 'DummyRegion', label: '세계수' },
];

const mapGroupColors: Readonly<Record<string, string>> = {
  location: '#58cdd2',
  enemy: '#ff727c',
  collectible: '#f0c75e',
  npc: '#78d49b',
  resource: '#c7a4ff',
};

const exactPoiKindColors: Readonly<Record<string, string>> = {
  fast_travel: '#58cdd2',
  boss: '#ff727c',
  dungeon: '#c7a4ff',
  wanted: '#ffc65c',
};

const reviewedIconKinds = new Set([
  'tower',
  'oil-rig',
  'map-unlock',
  'sky-warp',
  'enemy-camp',
  'npc',
  'egg',
  'skill-fruit',
  'treasure-map',
  'chest',
  'fishing-spot',
  'oil-field',
  'ore-coal',
  'ore-crystal',
]);

const normalizeBuildId = (value: string): string => value.replace(/^steam:/, '');

export const termById = (bundle: MapBundle): Map<string, MapTerm> =>
  new Map(bundle.terminology.terms.map((term) => [term.id, term]));

const publicTermForLayer = (
  layer: SupplementalLayer,
  terms: ReadonlyMap<string, MapTerm>,
): MapTerm | null => {
  const source = terms.get(layer.id);
  if (!source || source.filter_visibility === 'hidden_until_verified') return null;
  const publicId = source.merged_into ?? source.id;
  const publicTerm = terms.get(publicId);
  if (!publicTerm || publicTerm.verification !== 'reviewed') return null;
  return publicTerm;
};

const exactPoiToMapPoi = (poi: ExactMapPoi): MapPoi => ({
  id: poi.id,
  kind: poi.kind,
  display_name: poi.display_name,
  entity_id: poi.entity_id,
  map_id: poi.map_id,
  region_id: poi.region_id,
  map_x: poi.map_x,
  map_y: poi.map_y,
  source: 'exact',
  dense: false,
});

const poiIdentity = (poi: MapPoi): string =>
  `${poi.kind}:${String(Math.round(poi.map_x))}:${String(Math.round(poi.map_y))}`;

export const mapPoisForRegion = (bundle: MapBundle, region: MapRegionOption): MapPoi[] => {
  const pois = bundle.exact.pois
    .filter((poi) => poi.map_id === region.mapId && poi.region_id === region.regionId)
    .map(exactPoiToMapPoi);
  if (!bundle.supplemental) return pois;

  const terms = termById(bundle);
  const sourceRegion = bundle.supplemental.regions.find(
    (candidate) => candidate.map_id === region.mapId && candidate.region_id === region.regionId,
  );
  if (!sourceRegion) return pois;

  const layers = new Map(bundle.supplemental.layers.map((layer) => [layer.index, layer]));
  const known = new Set(pois.map(poiIdentity));
  for (const [pointIndex, point] of sourceRegion.points.entries()) {
    const layer = layers.get(point[0]);
    if (!layer) continue;
    const publicTerm = publicTermForLayer(layer, terms);
    if (!publicTerm) continue;
    const poi: MapPoi = {
      id: `layer:${layer.id}:${region.id}:${String(pointIndex)}`,
      kind: publicTerm.id,
      display_name: publicTerm.generic_title_ko,
      entity_id: null,
      map_id: region.mapId,
      region_id: region.regionId,
      map_x: point[3],
      map_y: point[4],
      source: 'supplemental',
      dense: layer.dense,
    };
    const identity = poiIdentity(poi);
    if (known.has(identity)) continue;
    known.add(identity);
    pois.push(poi);
  }
  return pois;
};

export const mapFilterOptions = (
  bundle: MapBundle,
  region: MapRegionOption,
  pois: readonly MapPoi[] = mapPoisForRegion(bundle, region),
): MapFilterOption[] => {
  const counts = new Map<string, number>();
  for (const poi of pois) counts.set(poi.kind, (counts.get(poi.kind) ?? 0) + 1);

  const defaultEnabled = new Set<string>(exactPoiKinds);
  const terms = termById(bundle);
  for (const layer of bundle.supplemental?.layers ?? []) {
    if (!layer.default_enabled || layer.point_count === 0) continue;
    const publicTerm = publicTermForLayer(layer, terms);
    if (publicTerm) defaultEnabled.add(publicTerm.id);
  }

  const groupOrder = new Map(bundle.terminology.groups.map((group, index) => [group.id, index]));
  const termOrder = new Map(bundle.terminology.terms.map((term, index) => [term.id, index]));
  const groupLabels = new Map(bundle.terminology.groups.map((group) => [group.id, group.label_ko]));

  return bundle.terminology.terms
    .filter((term) => (counts.get(term.id) ?? 0) > 0)
    .filter((term) => term.verification === 'reviewed')
    .map((term) => ({
      id: term.id,
      label: term.label_ko,
      groupId: term.group_id,
      groupLabel: groupLabels.get(term.group_id) ?? '',
      count: counts.get(term.id) ?? 0,
      defaultEnabled: defaultEnabled.has(term.id),
    }))
    .toSorted((left, right) => {
      const groupDifference =
        (groupOrder.get(left.groupId) ?? Number.MAX_SAFE_INTEGER) -
        (groupOrder.get(right.groupId) ?? Number.MAX_SAFE_INTEGER);
      if (groupDifference !== 0) return groupDifference;
      return (
        (termOrder.get(left.id) ?? Number.MAX_SAFE_INTEGER) -
        (termOrder.get(right.id) ?? Number.MAX_SAFE_INTEGER)
      );
    });
};

export const publicSupplementalFilterIds = (bundle: MapBundle): string[] => {
  const terms = termById(bundle);
  const ids = new Set<string>();
  for (const layer of bundle.supplemental?.layers ?? []) {
    if (layer.point_count === 0) continue;
    const publicTerm = publicTermForLayer(layer, terms);
    if (publicTerm && !exactPoiKinds.some((kind) => kind === publicTerm.id)) {
      ids.add(publicTerm.id);
    }
  }
  return [...ids].toSorted();
};

export const defaultMapFilterIds = (bundle: MapBundle): string[] => {
  const enabled = new Set<string>(exactPoiKinds);
  const terms = termById(bundle);
  for (const layer of bundle.supplemental?.layers ?? []) {
    if (!layer.default_enabled || layer.point_count === 0) continue;
    const publicTerm = publicTermForLayer(layer, terms);
    if (publicTerm) enabled.add(publicTerm.id);
  }
  return [...enabled];
};

export const filterMapPois = (
  pois: readonly MapPoi[],
  terms: ReadonlyMap<string, MapTerm>,
  enabledFilterIds: ReadonlySet<string>,
  query: string,
): MapPoi[] => {
  const normalizedQuery = normalizeCompactSearchText(query);
  return pois.filter((poi) => {
    if (normalizedQuery.length === 0) return enabledFilterIds.has(poi.kind);
    const term = terms.get(poi.kind);
    const haystack = normalizeCompactSearchText(
      [
        poi.display_name,
        poi.entity_id,
        poi.kind,
        term?.label_ko,
        term?.generic_title_ko,
        ...(term?.aliases_ko ?? []),
      ]
        .filter(Boolean)
        .join(' '),
    );
    return haystack.includes(normalizedQuery);
  });
};

export const declutterMapPois = (
  pois: readonly MapPoi[],
  zoom: number,
  selectedPoiId: string | null,
): MapPoi[] => {
  if (pois.length <= 1) return [...pois];
  const relativeZoom = Math.max(1, zoom);
  const minimumDistance = Math.max(20, 104 / relativeZoom);
  const maximumMarkers = Math.min(3200, Math.round(850 + relativeZoom * 470));
  const occupied = new Map<string, MapPoi[]>();
  const visible: MapPoi[] = [];

  const add = (poi: MapPoi, force = false): boolean => {
    if (visible.length >= maximumMarkers && !force) return false;
    const cellX = Math.floor(poi.map_x / minimumDistance);
    const cellY = Math.floor(poi.map_y / minimumDistance);
    if (!force) {
      for (let offsetX = -1; offsetX <= 1; offsetX += 1) {
        for (let offsetY = -1; offsetY <= 1; offsetY += 1) {
          const neighbors = occupied.get(`${String(cellX + offsetX)}:${String(cellY + offsetY)}`);
          if (
            neighbors?.some(
              (candidate) =>
                Math.hypot(candidate.map_x - poi.map_x, candidate.map_y - poi.map_y) <
                minimumDistance,
            )
          ) {
            return false;
          }
        }
      }
    }
    const key = `${String(cellX)}:${String(cellY)}`;
    const cell = occupied.get(key);
    if (cell) cell.push(poi);
    else occupied.set(key, [poi]);
    visible.push(poi);
    return true;
  };

  const selected = selectedPoiId ? pois.find((poi) => poi.id === selectedPoiId) : null;
  if (selected) add(selected, true);

  const representedKinds = new Set<string>();
  for (const poi of pois) {
    if (poi.id === selectedPoiId || representedKinds.has(poi.kind)) continue;
    if (add(poi)) representedKinds.add(poi.kind);
  }
  for (const poi of pois) {
    if (poi.id !== selectedPoiId && !poi.dense) add(poi);
  }
  for (const poi of pois) {
    if (poi.id !== selectedPoiId && poi.dense) add(poi);
  }
  return visible;
};

export const mapAreaLabel = (poi: Pick<MapPoi, 'map_x' | 'map_y'>): string => {
  const horizontal = poi.map_x < 2048 / 3 ? '서쪽' : poi.map_x > (2048 * 2) / 3 ? '동쪽' : '';
  const vertical = poi.map_y < 2048 / 3 ? '북쪽' : poi.map_y > (2048 * 2) / 3 ? '남쪽' : '';
  if (!horizontal && !vertical) return '중앙';
  if (!horizontal) return vertical;
  if (!vertical) return horizontal;
  return `${vertical.slice(0, 1)}${horizontal}`;
};

export const mapIndexForRegion = (bundle: MapBundle, region: MapRegionOption): MapTileIndex => {
  const index = bundle.indexes.find(
    (candidate) => candidate.map_id === region.mapId && candidate.region_id === region.regionId,
  );
  if (!index) throw new Error(`지도 인덱스가 없습니다: ${region.mapId}/${region.regionId}`);
  return index;
};

export const levelZeroTiles = (index: MapTileIndex): MapTile[] =>
  index.tiles.filter((tile) => tile.level === 0);

export const tilePublicPath = (region: MapRegionOption, tile: MapTile): string => {
  const fileName = tile.relative_path.split('/').at(-1);
  if (!fileName) throw new Error('타일 경로가 올바르지 않습니다.');
  return region.id === 'tree'
    ? `/generated/map/tree/tiles/${fileName}`
    : `/generated/map/tiles/${fileName}`;
};

export const mapFilterColor = (kind: string, terms: ReadonlyMap<string, MapTerm>): string =>
  exactPoiKindColors[kind] ?? mapGroupColors[terms.get(kind)?.group_id ?? ''] ?? '#82909e';

export const poiIconPath = (poi: MapPoi): string | null => {
  if (poi.kind === 'boss' && poi.entity_id) {
    return `/generated/game/pals/T_${poi.entity_id}_icon_normal.webp`;
  }
  const exactIcons: Readonly<Record<string, string>> = {
    fast_travel: '/generated/map/icons/compass-fast-travel.png',
    boss: '/generated/map/icons/boss-category.webp',
    dungeon: '/generated/map/icons/compass-dungeon.png',
    wanted: '/generated/map/icons/compass-bounty.png',
  };
  const exactIcon = exactIcons[poi.kind];
  if (exactIcon) return exactIcon;
  return reviewedIconKinds.has(poi.kind) ? `/generated/map/icons/reviewed-${poi.kind}.webp` : null;
};

const hasValidSupplementalContract = (bundle: MapBundle): boolean => {
  const supplemental = bundle.supplemental;
  if (!supplemental) return true;
  const terms = termById(bundle);
  const observedCounts = Array.from({ length: supplemental.layers.length }, () => 0);
  if (
    supplemental.layers.some((layer, index) => {
      if (layer.index !== index || !terms.has(layer.id)) return true;
      const source = terms.get(layer.id);
      if (!source) return true;
      if (source.filter_visibility === 'hidden_until_verified') return false;
      return publicTermForLayer(layer, terms) === null;
    })
  ) {
    return false;
  }
  for (const region of supplemental.regions) {
    if (region.point_count !== region.points.length) return false;
    for (const point of region.points) {
      const layerIndex = point[0];
      if (
        layerIndex < 0 ||
        layerIndex >= supplemental.layers.length ||
        point.slice(1).some((value) => !Number.isFinite(value))
      ) {
        return false;
      }
      observedCounts[layerIndex] = (observedCounts[layerIndex] ?? 0) + 1;
    }
  }
  return supplemental.layers.every((layer, index) => layer.point_count === observedCounts[index]);
};

export const validateMapBundle = (bundle: MapBundle): MapBundle => {
  const build = bundle.exact.game_build_id;
  if (
    bundle.exact.poi_count !== bundle.exact.pois.length ||
    bundle.exact.pois.some((poi) => !poi.verified) ||
    bundle.indexes.length !== 2 ||
    bundle.indexes.some(
      (index) =>
        normalizeBuildId(index.game_build_id) !== normalizeBuildId(build) ||
        index.map_width_px !== 2048 ||
        index.map_height_px !== 2048 ||
        levelZeroTiles(index).length !== 16,
    ) ||
    normalizeBuildId(bundle.terminology.game_build_id) !== normalizeBuildId(build) ||
    (bundle.supplemental !== null &&
      (normalizeBuildId(bundle.supplemental.game_build_id) !== normalizeBuildId(build) ||
        bundle.supplemental.provenance.exact_local_build_verified)) ||
    !hasValidSupplementalContract(bundle)
  ) {
    throw new Error('지도 데이터 계약이 일치하지 않습니다.');
  }
  return bundle;
};
