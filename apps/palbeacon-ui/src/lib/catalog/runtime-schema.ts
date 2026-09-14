import {
  array,
  boolean,
  literal,
  looseObject,
  nullable,
  number,
  optional,
  picklist,
  record,
  string,
} from 'valibot';

const metricSchema = looseObject({ label: string(), value: string() });
const palDropSchema = looseObject({
  item_id: string(),
  name_ko: string(),
  image_path: nullable(string()),
  rarity: optional(number()),
  variant: picklist(['normal', 'boss']),
  minimum_quantity: number(),
  maximum_quantity: number(),
  probability_ppm: number(),
});
const relatedPalSchema = looseObject({
  id: string(),
  name_ko: string(),
  image_path: nullable(string()),
  paldex_number: nullable(number()),
});
const catalogRecordSchema = looseObject({
  kind: picklist([
    'technology',
    'pal',
    'item',
    'active_skill',
    'passive_skill',
    'building',
    'shop',
  ]),
  id: string(),
  name_ko: string(),
  description_ko: nullable(string()),
  image_path: nullable(string()),
  category: string(),
  tags: array(string()),
  metrics: array(metricSchema),
  search_terms: array(string()),
  localization_fallback: boolean(),
  pal_drops: optional(array(palDropSchema)),
  learned_by_pals: optional(array(relatedPalSchema)),
  building_subcategory: optional(string()),
  building_ui_category: optional(string()),
  building_sort_order: optional(number()),
  building_requires_power: optional(boolean()),
});

const catalogCollectionsSchema = looseObject({
  technologies: array(catalogRecordSchema),
  pals: array(catalogRecordSchema),
  items: array(catalogRecordSchema),
  active_skills: array(catalogRecordSchema),
  passive_skills: array(catalogRecordSchema),
  buildings: array(catalogRecordSchema),
  shops: array(catalogRecordSchema),
});

export const unifiedCatalogSchema = looseObject({
  schema_version: literal(1),
  game_build_id: string(),
  mapping_sha256: string(),
  verified: boolean(),
  statistics: looseObject({
    technologies: number(),
    pals: number(),
    items: number(),
    active_skills: number(),
    passive_skills: number(),
    buildings: number(),
    shops: number(),
  }),
  records: catalogCollectionsSchema,
  item_relations: optional(
    record(
      string(),
      looseObject({
        recipes: array(looseObject({ recipe_id: string() })),
        drops: array(looseObject({ method_id: string(), pal_id: string() })),
        shops: array(looseObject({ product_id: string(), shop_group_id: string() })),
        technologies: array(looseObject({ technology_id: string(), level: number() })),
      }),
    ),
  ),
});
