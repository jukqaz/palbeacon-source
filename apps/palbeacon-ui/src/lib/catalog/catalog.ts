import type {
  CatalogCollectionKey,
  CatalogItemRelations,
  CatalogKind,
  CatalogRecord,
  CatalogSearchResult,
  UnifiedCatalog,
} from './types';
import { unifiedCatalogSchema } from './runtime-schema';
import { parseRuntimePayload } from '$lib/shared/data/parse-runtime-payload';
import { safeGameDescription } from '$lib/shared/data/display-text';
import { normalizeSearchText } from '$lib/shared/search/search-core';

const catalogCollectionForKind: Record<CatalogKind, CatalogCollectionKey> = {
  technology: 'technologies',
  pal: 'pals',
  item: 'items',
  active_skill: 'active_skills',
  passive_skill: 'passive_skills',
  building: 'buildings',
  shop: 'shops',
};

export const catalogKindLabels: Record<CatalogKind, string> = {
  technology: '기술',
  pal: '팰',
  item: '아이템',
  active_skill: '액티브 스킬',
  passive_skill: '패시브 스킬',
  building: '건축물',
  shop: '상점',
};

const scoreRecord = (record: CatalogRecord, query: string): number => {
  if (query.length === 0) return 0;
  const name = normalizeSearchText(record.name_ko);
  const id = normalizeSearchText(record.id);
  if (name === query) return 0;
  if (name.startsWith(query)) return 1;
  if (name.includes(query)) return 2;
  if (id === query) return 3;
  if (id.startsWith(query)) return 4;
  if (id.includes(query)) return 5;
  return 6;
};

const searchableText = (record: CatalogRecord): string =>
  normalizeSearchText(
    [
      record.name_ko,
      record.id,
      record.description_ko ?? '',
      record.category,
      ...record.tags,
      ...record.search_terms,
      ...(record.products ?? []).map((product) => product.item_name_ko),
    ].join(' '),
  );

export const recordsForScope = (
  catalog: UnifiedCatalog,
  scope: CatalogKind | 'all',
): CatalogRecord[] => {
  if (scope !== 'all') return catalog.records[catalogCollectionForKind[scope]];
  return Object.values(catalog.records).flat();
};

export const categoriesForRecords = (
  records: readonly CatalogRecord[],
  scope: CatalogKind | 'all',
): string[] =>
  [
    ...new Set(
      records
        .map((record) => (scope === 'all' ? catalogKindLabels[record.kind] : record.category))
        .filter((category) => category.length > 0),
    ),
  ].toSorted((left, right) => left.localeCompare(right, 'ko'));

export const searchCatalog = (
  records: readonly CatalogRecord[],
  query: string,
  category: string,
  scope: CatalogKind | 'all',
  limit: number,
): CatalogSearchResult => {
  const normalizedQuery = normalizeSearchText(query);
  const matches = records
    .filter((record) => {
      const recordCategory = scope === 'all' ? catalogKindLabels[record.kind] : record.category;
      return (
        (category === 'all' || recordCategory === category) &&
        (normalizedQuery.length === 0 || searchableText(record).includes(normalizedQuery))
      );
    })
    .toSorted(
      (left, right) =>
        scoreRecord(left, normalizedQuery) - scoreRecord(right, normalizedQuery) ||
        left.name_ko.localeCompare(right.name_ko, 'ko') ||
        left.id.localeCompare(right.id),
    );
  return { total: matches.length, visible: matches.slice(0, Math.max(0, limit)) };
};

export const rarityAccent = (rarity?: number): string => {
  if (rarity === undefined || rarity < 0) return '#82909e';
  if (rarity === 0) return '#a8b9bc';
  if (rarity === 1) return '#53b224';
  if (rarity === 2) return '#009cff';
  if (rarity === 3) return '#df52ff';
  return '#ffaa00';
};

const itemRarityLabels = new Set(['일반', '비범', '희귀', '영웅', '전설']);
export type ItemRouteFilter = 'recipe' | 'drop' | 'shop' | 'technology';
export type ItemCatalogSort = 'name' | 'rarity' | 'price_desc' | 'price_asc';

export const itemRouteLabels: Record<ItemRouteFilter, string> = {
  recipe: '제작',
  drop: '팰 드롭',
  shop: '상점',
  technology: '기술 해금',
};

export const itemRarityLabel = (item: CatalogRecord): string | null => {
  const value = item.metrics.find((metric) => metric.label === '등급')?.value;
  const label = value?.split('·', 1)[0]?.trim() ?? '';
  return itemRarityLabels.has(label) ? label : null;
};

export const itemDisplayDescription = (item: CatalogRecord): string | null => {
  return safeGameDescription(item.description_ko);
};

export const itemMetricValue = (item: CatalogRecord, label: string): string | null =>
  item.metrics.find((metric) => metric.label === label)?.value ?? null;

const itemMetricNumber = (item: CatalogRecord, label: string): number | null => {
  const value = itemMetricValue(item, label);
  if (!value) return null;
  const parsed = Number(value.replaceAll(',', '').replace(/[^0-9.-]/gu, ''));
  return Number.isFinite(parsed) ? parsed : null;
};

const itemSearchText = (item: CatalogRecord): string =>
  normalizeSearchText(
    [
      item.name_ko,
      item.id,
      itemDisplayDescription(item) ?? '',
      item.category,
      itemRarityLabel(item) ?? '',
      ...item.tags,
      ...item.search_terms,
    ].join(' '),
  );

const itemHasRoute = (
  relations: CatalogItemRelations | undefined,
  route: ItemRouteFilter,
): boolean => {
  if (!relations) return false;
  if (route === 'recipe') return relations.recipes.length > 0;
  if (route === 'drop') return relations.drops.length > 0;
  if (route === 'shop') return relations.shops.length > 0;
  return relations.technologies.length > 0;
};

export interface ItemCatalogFilter {
  query: string;
  categories: readonly string[];
  rarities: readonly string[];
  routes: readonly ItemRouteFilter[];
  sort: ItemCatalogSort;
}

export const filterItems = (
  items: readonly CatalogRecord[],
  itemRelations: Readonly<Record<string, CatalogItemRelations>>,
  filter: ItemCatalogFilter,
): CatalogRecord[] => {
  const normalizedQuery = normalizeSearchText(filter.query);
  const comparePrice = (left: CatalogRecord, right: CatalogRecord, direction: 1 | -1) => {
    const leftPrice = itemMetricNumber(left, '기준가');
    const rightPrice = itemMetricNumber(right, '기준가');
    if (leftPrice === null && rightPrice === null) return 0;
    if (leftPrice === null) return 1;
    if (rightPrice === null) return -1;
    return (leftPrice - rightPrice) * direction;
  };

  return items
    .filter((item) => {
      const rarity = itemRarityLabel(item);
      return (
        !item.localization_fallback &&
        item.name_ko.trim().length > 0 &&
        (normalizedQuery.length === 0 || itemSearchText(item).includes(normalizedQuery)) &&
        (filter.categories.length === 0 || filter.categories.includes(item.category)) &&
        (filter.rarities.length === 0 || (rarity !== null && filter.rarities.includes(rarity))) &&
        (filter.routes.length === 0 ||
          filter.routes.some((route) => itemHasRoute(itemRelations[item.id], route)))
      );
    })
    .toSorted((left, right) => {
      if (filter.sort === 'rarity') {
        return (
          (right.rarity ?? -1) - (left.rarity ?? -1) ||
          left.name_ko.localeCompare(right.name_ko, 'ko')
        );
      }
      if (filter.sort === 'price_desc') {
        return comparePrice(left, right, -1) || left.name_ko.localeCompare(right.name_ko, 'ko');
      }
      if (filter.sort === 'price_asc') {
        return comparePrice(left, right, 1) || left.name_ko.localeCompare(right.name_ko, 'ko');
      }
      return left.name_ko.localeCompare(right.name_ko, 'ko') || left.id.localeCompare(right.id);
    });
};

interface ComposedItemIngredient {
  id: string;
  name: string;
  quantity: number;
  imagePath: string | null;
  rarity: number | undefined;
}

interface ComposedItemRecipe {
  key: string;
  outputQuantity: number;
  unlockItem: CatalogRecord | null;
  ingredients: ComposedItemIngredient[];
}

interface ComposedItemDrop {
  key: string;
  pal: CatalogRecord;
  variant: 'normal' | 'boss';
  minimumQuantity: number;
  maximumQuantity: number;
  probabilityPpm: number;
  sourceCount: number;
}

interface ComposedItemShopOffer {
  key: string;
  currencyName: string;
  quantity: number;
  price: number;
  stock: number | null;
  shopCount: number;
}

interface ComposedItemTechnology {
  technology: CatalogRecord;
  level: number;
  cost: number;
}

export interface ComposedItemRelations {
  recipes: ComposedItemRecipe[];
  drops: ComposedItemDrop[];
  shops: ComposedItemShopOffer[];
  technologies: ComposedItemTechnology[];
}

export const composeItemRelations = (
  catalog: UnifiedCatalog,
  itemId: string,
): ComposedItemRelations => {
  const relations = catalog.item_relations?.[itemId];
  if (!relations) return { recipes: [], drops: [], shops: [], technologies: [] };

  const itemById = new Map(catalog.records.items.map((item) => [item.id, item]));
  const palById = new Map(catalog.records.pals.map((pal) => [pal.id, pal]));
  const technologyById = new Map(
    catalog.records.technologies.map((technology) => [technology.id, technology]),
  );

  const recipes = new Map<string, ComposedItemRecipe>();
  for (const recipe of relations.recipes) {
    const ingredientSignature = recipe.ingredients
      .map((ingredient) => `${ingredient.item_id}:${ingredient.quantity.toString()}`)
      .toSorted()
      .join('|');
    const key = `${recipe.output_quantity.toString()}|${recipe.unlock_item_id ?? ''}|${ingredientSignature}`;
    if (recipes.has(key)) continue;
    const unlockItem = recipe.unlock_item_id ? (itemById.get(recipe.unlock_item_id) ?? null) : null;
    recipes.set(key, {
      key,
      outputQuantity: recipe.output_quantity,
      unlockItem:
        unlockItem && !unlockItem.localization_fallback && unlockItem.name_ko.trim().length > 0
          ? unlockItem
          : null,
      ingredients: recipe.ingredients.map((ingredient) => {
        const item = itemById.get(ingredient.item_id);
        return {
          id: ingredient.item_id,
          name: item?.name_ko ?? ingredient.name_ko,
          quantity: ingredient.quantity,
          imagePath: item?.image_path ?? null,
          rarity: item?.rarity,
        };
      }),
    });
  }

  const drops = new Map<string, ComposedItemDrop>();
  for (const drop of relations.drops) {
    const pal = palById.get(drop.pal_id);
    if (!pal || pal.localization_fallback || pal.name_ko.trim().length === 0) continue;
    const key = [
      drop.pal_id,
      drop.variant,
      drop.minimum_quantity,
      drop.maximum_quantity,
      drop.probability_ppm,
    ].join('|');
    const current = drops.get(key);
    if (current) {
      current.sourceCount += 1;
      continue;
    }
    drops.set(key, {
      key,
      pal,
      variant: drop.variant,
      minimumQuantity: drop.minimum_quantity,
      maximumQuantity: drop.maximum_quantity,
      probabilityPpm: drop.probability_ppm,
      sourceCount: 1,
    });
  }

  const shops = new Map<string, ComposedItemShopOffer & { shopIds: Set<string> }>();
  for (const shop of relations.shops) {
    const key = [
      shop.currency_item_id,
      shop.currency_name_ko,
      shop.quantity,
      shop.price,
      shop.stock ?? '',
    ].join('|');
    const current = shops.get(key);
    if (current) {
      current.shopIds.add(shop.shop_group_id);
      current.shopCount = current.shopIds.size;
      continue;
    }
    shops.set(key, {
      key,
      currencyName: shop.currency_name_ko,
      quantity: shop.quantity,
      price: shop.price,
      stock: shop.stock,
      shopCount: 1,
      shopIds: new Set([shop.shop_group_id]),
    });
  }

  const technologies = new Map<string, ComposedItemTechnology>();
  for (const relation of relations.technologies) {
    const technology = technologyById.get(relation.technology_id);
    if (!technology || technology.localization_fallback || technology.name_ko.trim().length === 0) {
      continue;
    }
    const current = technologies.get(relation.technology_id);
    if (!current || relation.level < current.level) {
      technologies.set(relation.technology_id, {
        technology,
        level: relation.level,
        cost: relation.cost,
      });
    }
  }

  return {
    recipes: [...recipes.values()],
    drops: [...drops.values()].toSorted(
      (left, right) =>
        right.probabilityPpm - left.probabilityPpm ||
        left.pal.name_ko.localeCompare(right.pal.name_ko, 'ko') ||
        right.maximumQuantity - left.maximumQuantity,
    ),
    shops: [...shops.values()]
      .map(({ shopIds: _shopIds, ...offer }) => offer)
      .toSorted(
        (left, right) =>
          left.price - right.price || left.currencyName.localeCompare(right.currencyName, 'ko'),
      ),
    technologies: [...technologies.values()].toSorted(
      (left, right) =>
        left.level - right.level ||
        left.technology.name_ko.localeCompare(right.technology.name_ko, 'ko'),
    ),
  };
};

export interface ItemAcquisitionSummary {
  routeCount: number;
  compactLabel: string | null;
  routeSummary: string | null;
  unlockSummary: string | null;
  relationLabels: string[];
}

/**
 * Joins exact item relations once so list and detail views can present distinct,
 * human-facing summaries without independently recounting the same records.
 */
export const summarizeItemAcquisition = (
  relations: CatalogItemRelations,
): ItemAcquisitionSummary => {
  const recipeCount = new Set(relations.recipes.map((recipe) => recipe.recipe_id)).size;
  const dropPalCount = new Set(relations.drops.map((drop) => drop.pal_id)).size;
  const shopCount = new Set(relations.shops.map((shop) => shop.shop_group_id)).size;
  const technologyIds = new Set(
    relations.technologies.map((technology) => technology.technology_id),
  );
  const routeParts = [
    recipeCount > 0 ? `제작식 ${recipeCount.toString()}개` : null,
    dropPalCount > 0 ? `드롭 팰 ${dropPalCount.toString()}종` : null,
    shopCount > 0 ? `판매 상점 ${shopCount.toString()}곳` : null,
  ].filter((part): part is string => part !== null);
  const relationLabels = [
    recipeCount > 0 ? '제작 가능' : null,
    dropPalCount > 0 ? '팰 드롭' : null,
    shopCount > 0 ? '상점 판매' : null,
    technologyIds.size > 0 ? '기술 해금' : null,
  ].filter((label): label is string => label !== null);
  const earliestUnlockLevel = relations.technologies.reduce<number | null>(
    (earliest, technology) =>
      earliest === null ? technology.level : Math.min(earliest, technology.level),
    null,
  );

  return {
    routeCount: routeParts.length,
    compactLabel: routeParts.length > 0 ? `획득 ${routeParts.length.toString()}가지` : null,
    routeSummary: routeParts.length > 0 ? routeParts.join(' · ') : null,
    unlockSummary:
      earliestUnlockLevel === null
        ? null
        : `Lv. ${earliestUnlockLevel.toString()}부터 · 기술 ${technologyIds.size.toString()}개`,
    relationLabels,
  };
};

export const loadUnifiedCatalog = async (
  fetcher: typeof fetch = fetch,
): Promise<UnifiedCatalog> => {
  const response = await fetcher('/generated/game/catalog.v1.json');
  if (!response.ok) throw new Error(`Catalog request failed: ${response.status.toString()}`);
  const catalog = parseRuntimePayload<UnifiedCatalog>(
    unifiedCatalogSchema,
    await response.json(),
    '도감 데이터',
  );
  if (!catalog.verified) throw new Error('Catalog is not exact-build verified');
  return catalog;
};
