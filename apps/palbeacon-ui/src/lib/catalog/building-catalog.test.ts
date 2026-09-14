import { describe, expect, it } from 'vitest';
import { catalogFixture } from './catalog.fixture';
import {
  buildingSubcategoriesFor,
  buildingUnlock,
  filterBuildings,
  groupBuildings,
} from './building-catalog';
import type { CatalogRecord, UnifiedCatalog } from './types';

const building = (
  id: string,
  name: string,
  category: CatalogRecord['category'],
  uiCategory: string,
  sortOrder: number,
): CatalogRecord => ({
  kind: 'building',
  id,
  name_ko: name,
  description_ko: `${name} 설명`,
  image_path: `/generated/game/buildings/${id}.png`,
  category,
  tags: [category, '전력 불필요'],
  metrics: [
    { label: 'HP', value: '5,000' },
    { label: '방어', value: '3' },
    { label: '재료', value: '1종' },
  ],
  search_terms: ['목재'],
  localization_fallback: false,
  building_subcategory: 'Prod_Craft',
  building_ui_category: uiCategory,
  building_sort_order: sortOrder,
  building_requires_power: false,
  materials: [
    {
      item_id: 'Wood',
      name_ko: '목재',
      quantity: 30,
      image_path: '/generated/game/items/wood.png',
    },
  ],
});

const monsterFarm = building('MonsterFarm', '가축 목장', '팰 시설', 'PalManagement', 3);
const workbench = building('Workbench', '원시적인 작업대', '생산', 'Product_Repair', 2);
const repairBench = building('RepairBench', '수리대', '생산', 'Product_Repair', 1);
const sphereFactory = building('SphereFactory', '스피어 제작대', '생산', 'PalCaptureItem', 1);

const catalog: UnifiedCatalog = {
  ...catalogFixture,
  statistics: { ...catalogFixture.statistics, buildings: 4, technologies: 1 },
  records: {
    ...catalogFixture.records,
    buildings: [monsterFarm, workbench, repairBench, sphereFactory],
    technologies: [
      {
        ...catalogFixture.records.technologies[0]!,
        id: 'Technology_MonsterFarm',
        name_ko: '가축 목장',
        category: '일반 기술',
        metrics: [
          { label: '레벨', value: '5' },
          { label: '포인트', value: '2 PT' },
        ],
        search_terms: ['MonsterFarm'],
      },
    ],
  },
};

describe('building catalog composition', () => {
  it('keeps exact game categories and sort order', () => {
    const result = filterBuildings(catalog, '', '생산', 'all');
    expect(result.map((record) => record.name_ko)).toEqual([
      '수리대',
      '스피어 제작대',
      '원시적인 작업대',
    ]);
    expect(groupBuildings(result, '생산').map((group) => group.label)).toEqual([
      '제작·수리',
      '스피어',
    ]);
    expect(buildingSubcategoriesFor(catalog.records.buildings, '생산')).toEqual([
      { id: 'Product_Repair', label: '제작·수리' },
      { id: 'PalCaptureItem', label: '스피어' },
    ]);
  });

  it('searches Korean material and verified unlock data while hiding raw grouping codes', () => {
    expect(filterBuildings(catalog, '목재', '팰 시설', 'all')).toEqual([monsterFarm]);
    expect(filterBuildings(catalog, '레벨 5', 'all', 'all')).toHaveLength(0);
    expect(buildingUnlock(catalog, monsterFarm)).toMatchObject({ level: 5, cost: 2 });
  });
});
