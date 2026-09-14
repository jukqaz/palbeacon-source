import { normalizeSearchText } from '$lib/shared/search/search-core';
import type { CatalogRecord, UnifiedCatalog } from './types';

export const buildingCategoryOrder = [
  '생산',
  '팰 시설',
  '보관',
  '식량',
  '기반 시설',
  '조명',
  '토대',
  '방어',
  '가구',
  '기타',
] as const;

export type BuildingCategory = (typeof buildingCategoryOrder)[number];

const buildingSubcategoryLabels: Record<string, string> = {
  Product_Repair: '제작·수리',
  PalCaptureItem: '스피어',
  Weapon: '무기',
  Deforest_Mining: '벌목·채굴',
  Refine: '제련',
  Medicine: '제약',
  Milling_Crusher: '제분·분쇄',
  FishingPond: '낚시터',
  PalManagement: '팰 관리',
  PalTraining: '팰 훈련',
  PalOther: '기타 팰 시설',
  StorageFurniture: '수납 가구',
  Chest: '상자',
  Cooking: '조리',
  Farm: '농원',
  FeedBox: '먹이 상자',
  Electricity: '전력',
  Health: '회복',
  Bed: '침대',
  Freestanding: '설치형',
  Ceiling: '천장',
  WallHanging: '벽걸이',
  Wood: '나무',
  Stone: '돌',
  Metal: '금속',
  Glass: '유리',
  JapaneseStyle: '일본풍',
  Clean: '깔끔한',
  Ancient: '고대 문명',
  Interception: '요격',
  Barieer: '방벽',
  Ornament: '장식물',
  Flag: '깃발',
  Chair: '의자',
  Houseplants: '관엽 식물',
  WallDecoration: '벽 장식',
  Desk: '탁자',
  Carpet: '카펫',
  EfficiencEnhancement: '효율 강화',
  Other: '기타',
};

const buildingSubcategoryOrder: Record<BuildingCategory, readonly string[]> = {
  생산: [
    'Product_Repair',
    'PalCaptureItem',
    'Weapon',
    'Deforest_Mining',
    'Refine',
    'Medicine',
    'Milling_Crusher',
    'FishingPond',
  ],
  '팰 시설': ['PalManagement', 'PalTraining', 'PalOther'],
  보관: ['StorageFurniture', 'Chest'],
  식량: ['Cooking', 'Farm', 'FeedBox'],
  '기반 시설': ['Electricity', 'Health', 'Bed'],
  조명: ['Freestanding', 'Ceiling', 'WallHanging'],
  토대: ['Wood', 'Stone', 'Metal', 'Glass', 'JapaneseStyle', 'Clean', 'Ancient'],
  방어: ['Interception', 'Barieer'],
  가구: ['Ornament', 'Flag', 'Chair', 'Houseplants', 'WallDecoration', 'Desk', 'Carpet'],
  기타: ['EfficiencEnhancement', 'Other'],
};

export interface BuildingUnlock {
  technology: CatalogRecord;
  level: number;
  cost: number;
  ancient: boolean;
}

export interface BuildingGroup {
  id: string;
  label: string;
  records: CatalogRecord[];
}

export const buildingSubcategoryLabel = (record: CatalogRecord): string =>
  buildingSubcategoryLabels[record.building_ui_category ?? ''] ?? record.category;

export const buildingSubcategoriesFor = (
  records: readonly CatalogRecord[],
  category: BuildingCategory,
): { id: string; label: string }[] => {
  const present = new Set(
    records
      .filter((record) => record.category === category)
      .map((record) => record.building_ui_category)
      .filter((value): value is string => Boolean(value && buildingSubcategoryLabels[value])),
  );
  return buildingSubcategoryOrder[category]
    .filter((id) => present.has(id))
    .map((id) => ({ id, label: buildingSubcategoryLabels[id]! }));
};

export const buildingUnlock = (
  catalog: UnifiedCatalog,
  building: CatalogRecord,
): BuildingUnlock | null => {
  const technologies = catalog.records.technologies.filter(
    (technology) => !technology.localization_fallback && technology.name_ko.trim().length > 0,
  );
  const exact = technologies.find((technology) => technology.id === building.id);
  const technology =
    exact ?? technologies.find((candidate) => candidate.search_terms.includes(building.id));
  if (!technology) return null;
  const level = Number(
    technology.metrics.find((metric) => metric.label === '레벨')?.value ?? Number.NaN,
  );
  const cost = Number.parseInt(
    technology.metrics.find((metric) => metric.label === '포인트')?.value ?? '',
    10,
  );
  if (!Number.isFinite(level) || !Number.isFinite(cost)) return null;
  return {
    technology,
    level,
    cost,
    ancient: technology.category === '고대 기술',
  };
};

const buildingSearchText = (record: CatalogRecord, unlock: BuildingUnlock | null) =>
  normalizeSearchText(
    [
      record.name_ko,
      record.id,
      record.description_ko ?? '',
      record.category,
      buildingSubcategoryLabel(record),
      ...record.tags,
      ...record.search_terms,
      ...(record.materials ?? []).map((material) => material.name_ko),
      unlock?.technology.name_ko ?? '',
    ].join(' '),
  );

export const filterBuildings = (
  catalog: UnifiedCatalog,
  query: string,
  category: BuildingCategory | 'all',
  subcategory: string,
): CatalogRecord[] => {
  const normalizedQuery = normalizeSearchText(query);
  return catalog.records.buildings
    .filter(
      (record) =>
        !record.localization_fallback &&
        record.name_ko.trim().length > 0 &&
        (category === 'all' || record.category === category) &&
        (subcategory === 'all' || record.building_ui_category === subcategory) &&
        (normalizedQuery.length === 0 ||
          buildingSearchText(record, buildingUnlock(catalog, record)).includes(normalizedQuery)),
    )
    .toSorted((left, right) => {
      const leftCategoryIndex = buildingCategoryOrder.indexOf(left.category as BuildingCategory);
      const rightCategoryIndex = buildingCategoryOrder.indexOf(right.category as BuildingCategory);
      return (
        (leftCategoryIndex === -1 ? Number.MAX_SAFE_INTEGER : leftCategoryIndex) -
          (rightCategoryIndex === -1 ? Number.MAX_SAFE_INTEGER : rightCategoryIndex) ||
        (left.building_sort_order ?? Number.MAX_SAFE_INTEGER) -
          (right.building_sort_order ?? Number.MAX_SAFE_INTEGER) ||
        left.name_ko.localeCompare(right.name_ko, 'ko') ||
        left.id.localeCompare(right.id)
      );
    });
};

export const groupBuildings = (
  records: readonly CatalogRecord[],
  category: BuildingCategory | 'all',
): BuildingGroup[] => {
  if (category === 'all') {
    return buildingCategoryOrder.flatMap((categoryLabel) => {
      const matches = records.filter((record) => record.category === categoryLabel);
      return matches.length > 0
        ? [{ id: categoryLabel, label: categoryLabel, records: matches }]
        : [];
    });
  }

  const order = buildingSubcategoryOrder[category];
  const bySubcategory = new Map<string, CatalogRecord[]>();
  for (const record of records) {
    const id = record.building_ui_category ?? 'Other';
    bySubcategory.set(id, [...(bySubcategory.get(id) ?? []), record]);
  }
  return [...bySubcategory.entries()]
    .toSorted(
      ([left], [right]) =>
        (order.indexOf(left) === -1 ? Number.MAX_SAFE_INTEGER : order.indexOf(left)) -
          (order.indexOf(right) === -1 ? Number.MAX_SAFE_INTEGER : order.indexOf(right)) ||
        (buildingSubcategoryLabels[left] ?? category).localeCompare(
          buildingSubcategoryLabels[right] ?? category,
          'ko',
        ),
    )
    .map(([id, groupRecords]) => ({
      id,
      label: buildingSubcategoryLabels[id] ?? category,
      records: groupRecords,
    }));
};

export const buildingMetric = (record: CatalogRecord, label: string): string | null =>
  record.metrics.find((metric) => metric.label === label)?.value ?? null;
