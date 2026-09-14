import { describe, expect, it } from 'vitest';
import { catalogFixture } from './catalog.fixture';
import {
  composeItemRelations,
  filterItems,
  itemDisplayDescription,
  itemRarityLabel,
  loadUnifiedCatalog,
  searchCatalog,
  summarizeItemAcquisition,
} from './catalog';
import { normalizeSearchText } from '$lib/shared/search/search-core';
import type { CatalogRecord, UnifiedCatalog } from './types';

const pal = (index: number): CatalogRecord => ({
  kind: 'pal',
  id: `Pal_${index.toString().padStart(2, '0')}`,
  name_ko: `공통 팰 ${index.toString()}`,
  description_ko: null,
  image_path: null,
  category: 'Grass',
  tags: ['Grass'],
  metrics: [],
  search_terms: ['공통'],
  localization_fallback: false,
});

describe('unified exact-build catalog', () => {
  it('filters the category before applying the visible limit', () => {
    const technology = catalogFixture.records.technologies.at(0);
    if (!technology) throw new Error('Technology fixture is missing');
    const records: CatalogRecord[] = [
      ...Array.from({ length: 45 }, (_, index) => pal(index)),
      {
        ...technology,
        name_ko: '공통 기술',
        search_terms: ['공통'],
      },
    ];

    const result = searchCatalog(records, '공통', '기술', 'all', 12);
    expect(result.total).toBe(1);
    expect(result.visible.map((entry) => entry.id)).toEqual(['Technology_Farm']);
  });

  it('normalizes Korean spacing without dropping original ID search', () => {
    expect(normalizeSearchText('  배합   목장 ')).toBe('배합 목장');
    const result = searchCatalog(
      catalogFixture.records.technologies,
      'technology_farm',
      'all',
      'technology',
      12,
    );
    expect(result.visible[0]?.name_ko).toBe('배합 목장');
  });

  it('combines exact item relations into one player-facing acquisition summary', () => {
    const relations = catalogFixture.item_relations?.['UniqueMaterial_FlowerPrince'];
    if (!relations) throw new Error('Item relation fixture is missing');

    expect(summarizeItemAcquisition(relations)).toEqual({
      routeCount: 3,
      compactLabel: '획득 3가지',
      routeSummary: '제작식 1개 · 드롭 팰 1종 · 판매 상점 1곳',
      unlockSummary: 'Lv. 19부터 · 기술 1개',
      relationLabels: ['제작 가능', '팰 드롭', '상점 판매', '기술 해금'],
    });
  });

  it('does not turn a technology gate into an additional acquisition route', () => {
    expect(
      summarizeItemAcquisition({
        recipes: [],
        drops: [],
        shops: [],
        technologies: [{ technology_id: 'Technology_Farm', level: 19, cost: 2 }],
      }),
    ).toMatchObject({
      routeCount: 0,
      compactLabel: null,
      routeSummary: null,
      unlockSummary: 'Lv. 19부터 · 기술 1개',
    });
  });

  it('filters exact item labels and relations without requiring raw values in the UI', () => {
    const results = filterItems(catalogFixture.records.items, catalogFixture.item_relations ?? {}, {
      query: '등급 5',
      categories: [],
      rarities: ['전설'],
      routes: ['recipe'],
      sort: 'rarity',
    });

    expect(results.map((item) => item.name_ko)).toEqual(['부패 독 여과막']);
    expect(itemRarityLabel(results[0]!)).toBe('전설');
  });

  it('removes sentences containing internal game identifiers from visible descriptions', () => {
    const item = {
      ...catalogFixture.records.items[0]!,
      description_ko: '복잡한 판단에 사용하는 부품. Factory_Hard_04 에서 제작할 수 있다.',
    };

    expect(itemDisplayDescription(item)).toBe('복잡한 판단에 사용하는 부품.');

    expect(
      itemDisplayDescription({
        ...item,
        description_ko: '처음 만드는 작업 시설이다. WorkBench에서 제작할 수 있다.',
      }),
    ).toBe('처음 만드는 작업 시설이다.');

    expect(itemDisplayDescription({ ...item, description_ko: '의 모습을 본뜬 투구.' })).toBeNull();
    expect(
      itemDisplayDescription({
        ...item,
        description_ko: '의 고기. 담백하고 먹기 좋다.',
      }),
    ).toBeNull();
    expect(
      itemDisplayDescription({
        ...item,
        description_ko: '이(가) 자신이 내뿜는 안개로부터 몸을 지킨다.',
      }),
    ).toBeNull();
  });

  it('groups duplicate drop sources and identical shop offers once', () => {
    const base = catalogFixture.item_relations?.['UniqueMaterial_FlowerPrince'];
    if (!base) throw new Error('Item relation fixture is missing');
    const catalog: UnifiedCatalog = {
      ...catalogFixture,
      item_relations: {
        UniqueMaterial_FlowerPrince: {
          recipes: [...base.recipes, ...base.recipes],
          drops: [
            ...base.drops,
            { ...base.drops[0]!, method_id: 'pal-drop:Anubis080:9', level: 80 },
          ],
          shops: [
            ...base.shops,
            {
              ...base.shops[0]!,
              product_id: 'shop:AnotherMoneyShop:4',
              shop_group_id: 'AnotherMoneyShop',
            },
          ],
          technologies: [...base.technologies, ...base.technologies],
        },
      },
    };

    const composed = composeItemRelations(catalog, 'UniqueMaterial_FlowerPrince');
    expect(composed.recipes).toHaveLength(1);
    expect(composed.drops).toHaveLength(1);
    expect(composed.drops[0]?.sourceCount).toBe(2);
    expect(composed.shops).toHaveLength(1);
    expect(composed.shops[0]?.shopCount).toBe(2);
    expect(composed.technologies).toHaveLength(1);
  });

  it('keeps the exact required blueprint attached to a weapon recipe', () => {
    const weapon = catalogFixture.records.items[0]!;
    const blueprint = catalogFixture.records.items[1]!;
    const base = catalogFixture.item_relations?.[weapon.id];
    if (!base?.recipes[0]) throw new Error('Item recipe fixture is missing');
    const catalog: UnifiedCatalog = {
      ...catalogFixture,
      item_relations: {
        ...catalogFixture.item_relations,
        [weapon.id]: {
          ...base,
          recipes: [{ ...base.recipes[0], unlock_item_id: blueprint.id }],
        },
      },
    };

    expect(composeItemRelations(catalog, weapon.id).recipes[0]?.unlockItem?.name_ko).toBe(
      blueprint.name_ko,
    );
  });

  it('deduplicates repeated relation rows before presenting route counts', () => {
    const relations = catalogFixture.item_relations?.['UniqueMaterial_FlowerPrince'];
    if (!relations) throw new Error('Item relation fixture is missing');

    const summary = summarizeItemAcquisition({
      recipes: [...relations.recipes, ...relations.recipes],
      drops: [...relations.drops, ...relations.drops],
      shops: [...relations.shops, ...relations.shops],
      technologies: [...relations.technologies, ...relations.technologies],
    });

    expect(summary.routeSummary).toBe('제작식 1개 · 드롭 팰 1종 · 판매 상점 1곳');
    expect(summary.unlockSummary).toBe('Lv. 19부터 · 기술 1개');
  });

  it('rejects an unverified generated catalog', async () => {
    const fetcher = () =>
      Promise.resolve(
        new Response(JSON.stringify({ ...catalogFixture, verified: false }), { status: 200 }),
      );
    await expect(loadUnifiedCatalog(fetcher as typeof fetch)).rejects.toThrow(
      'Catalog is not exact-build verified',
    );
  });
});
