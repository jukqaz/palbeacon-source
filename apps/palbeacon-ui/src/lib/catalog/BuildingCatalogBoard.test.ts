import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { catalogFixture } from './catalog.fixture';
import BuildingCatalogBoard from './BuildingCatalogBoard.svelte';
import type { CatalogRecord, UnifiedCatalog } from './types';

const record = (
  id: string,
  name: string,
  category: string,
  uiCategory: string,
  sortOrder: number,
  requiresPower = false,
): CatalogRecord => ({
  kind: 'building',
  id,
  name_ko: name,
  description_ko: `${name}의 게임 설명`,
  image_path: `/generated/game/buildings/${id}.png`,
  category,
  tags: [category, requiresPower ? '전력 필요' : '전력 불필요'],
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
  building_requires_power: requiresPower,
  materials: [
    {
      item_id: 'Wood',
      name_ko: '목재',
      quantity: 30,
      image_path: '/generated/game/items/wood.png',
    },
  ],
});

const buildings = [
  record('Workbench', '원시적인 작업대', '생산', 'Product_Repair', 2),
  record('RepairBench', '수리대', '생산', 'Product_Repair', 1),
  record('SphereFactory', '스피어 제작대', '생산', 'PalCaptureItem', 1, true),
  record('MonsterFarm', '가축 목장', '팰 시설', 'PalManagement', 1),
];

const catalog: UnifiedCatalog = {
  ...catalogFixture,
  statistics: { ...catalogFixture.statistics, buildings: buildings.length, technologies: 1 },
  records: {
    ...catalogFixture.records,
    buildings,
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

describe('BuildingCatalogBoard', () => {
  it('uses the current game category and subgroup structure with actual slots', () => {
    render(BuildingCatalogBoard, { catalog, title: '건축물 도감' });

    expect(screen.getByRole('navigation', { name: '건축물 대분류' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '생산' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('heading', { name: '제작·수리' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: '스피어' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /수리대, 생산, 제작·수리/ })).toBeInTheDocument();
    expect(screen.queryByText('Product_Repair')).not.toBeInTheDocument();
    expect(screen.queryByText('487개 결과')).not.toBeInTheDocument();
  });

  it('searches materials and opens only useful exact-building details', async () => {
    render(BuildingCatalogBoard, { catalog, title: '건축물 도감' });
    const search = screen.getByRole('searchbox', { name: '건축물 검색' });
    await fireEvent.input(search, { target: { value: '스피어 제작대' } });
    const card = screen.getByRole('button', { name: /스피어 제작대, 생산, 스피어, 전력 필요/ });
    await fireEvent.click(card);

    const inspector = screen.getByRole('complementary', { name: '선택한 건축물 상세' });
    expect(within(inspector).getByRole('heading', { name: '스피어 제작대' })).toBeInTheDocument();
    expect(within(inspector).getByText('전력 필요')).toBeInTheDocument();
    expect(within(inspector).getByRole('heading', { name: '필요 재료' })).toBeInTheDocument();
    expect(within(inspector).getByText('목재')).toBeInTheDocument();
    expect(within(inspector).getByText('× 30')).toBeInTheDocument();
    expect(within(inspector).getByRole('link', { name: '재료 계산' })).toHaveAttribute(
      'href',
      '/plan/materials/?id=SphereFactory',
    );
    expect(within(inspector).getByRole('link', { name: '목재 아이템 상세 열기' })).toHaveAttribute(
      'href',
      '/items/?q=%EB%AA%A9%EC%9E%AC&id=Wood',
    );
    expect(within(inspector).queryByText('SphereFactory')).not.toBeInTheDocument();
    expect(within(inspector).queryByText('Prod_Craft')).not.toBeInTheDocument();
  });

  it('restores the selected slot after closing the sequential detail', async () => {
    render(BuildingCatalogBoard, { catalog, title: '건축물 도감' });
    const card = screen.getByRole('button', { name: /수리대, 생산, 제작·수리/ });
    card.focus();
    await fireEvent.click(card);
    await fireEvent.click(screen.getByRole('button', { name: '건축물 상세 닫기' }));
    await waitFor(() => expect(card).toHaveFocus());
  });
});
