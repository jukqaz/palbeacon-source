export type CatalogKind =
  | 'technology'
  | 'pal'
  | 'item'
  | 'active_skill'
  | 'passive_skill'
  | 'building'
  | 'shop';

export type CatalogCollectionKey =
  | 'technologies'
  | 'pals'
  | 'items'
  | 'active_skills'
  | 'passive_skills'
  | 'buildings'
  | 'shops';

interface CatalogMetric {
  label: string;
  value: string;
}

interface ShopProduct {
  product_id: string;
  item_id: string;
  item_name_ko: string;
  quantity: number;
  price: number;
  product_type: string;
  stock: number | null;
  image_path?: string | null;
  rarity?: number;
}

interface CatalogMaterial {
  item_id: string;
  name_ko: string;
  quantity: number;
  image_path: string | null;
  rarity?: number;
}

interface CatalogItemRecipeIngredient {
  item_id: string;
  name_ko: string;
  quantity: number;
}

interface CatalogItemRecipeRelation {
  recipe_id: string;
  output_quantity: number;
  work_amount: number;
  unlock_item_id: string | null;
  ingredients: CatalogItemRecipeIngredient[];
}

interface CatalogItemDropRelation {
  method_id: string;
  pal_id: string;
  level: number;
  variant: 'normal' | 'boss';
  minimum_quantity: number;
  maximum_quantity: number;
  probability_ppm: number;
}

interface CatalogItemShopRelation {
  product_id: string;
  shop_group_id: string;
  currency_item_id: string;
  currency_name_ko: string;
  quantity: number;
  price: number;
  stock: number | null;
}

interface CatalogItemTechnologyRelation {
  technology_id: string;
  level: number;
  cost: number;
}

export interface CatalogItemRelations {
  recipes: CatalogItemRecipeRelation[];
  drops: CatalogItemDropRelation[];
  shops: CatalogItemShopRelation[];
  technologies: CatalogItemTechnologyRelation[];
}

interface CatalogElement {
  id: string;
  name_ko: string;
  icon_path: string;
}

interface CatalogWorkSuitability {
  id: string;
  name_ko: string;
  level: number;
  icon_path: string;
}

interface CatalogPalSkill {
  id: string;
  name_ko: string;
  level: number;
  element_id: string;
  element_name_ko: string;
  icon_path: string;
  power: number;
  cooldown_seconds: number;
}

interface CatalogPalDrop {
  item_id: string;
  name_ko: string;
  image_path: string | null;
  rarity?: number;
  variant: 'normal' | 'boss';
  minimum_quantity: number;
  maximum_quantity: number;
  probability_ppm: number;
}

interface CatalogRelatedPal {
  id: string;
  name_ko: string;
  image_path: string | null;
  paldex_number: number | null;
}

interface CatalogPalProfile {
  paldex_number: number | null;
  paldex_suffix: string;
  walk_speed: number;
  run_speed: number;
  ride_sprint_speed: number;
  transport_speed: number;
  stamina: number;
  food_amount: number;
  nocturnal: boolean;
}

export interface CatalogRecord {
  kind: CatalogKind;
  id: string;
  name_ko: string;
  description_ko: string | null;
  image_path: string | null;
  category: string;
  tags: string[];
  metrics: CatalogMetric[];
  search_terms: string[];
  localization_fallback: boolean;
  rarity?: number;
  element_id?: string;
  element_icon_path?: string;
  elements?: CatalogElement[];
  work_suitability?: CatalogWorkSuitability[];
  pal_profile?: CatalogPalProfile;
  pal_skills?: CatalogPalSkill[];
  pal_drops?: CatalogPalDrop[];
  learned_by_pals?: CatalogRelatedPal[];
  preview_images?: string[];
  materials?: CatalogMaterial[];
  products?: ShopProduct[];
  target_path?: string;
  building_subcategory?: string;
  building_ui_category?: string;
  building_sort_order?: number;
  building_requires_power?: boolean;
}

export interface UnifiedCatalog {
  schema_version: 1;
  game_build_id: string;
  mapping_sha256: string;
  verified: boolean;
  statistics: Record<CatalogCollectionKey, number>;
  records: Record<CatalogCollectionKey, CatalogRecord[]>;
  item_relations?: Record<string, CatalogItemRelations>;
}

export interface CatalogSearchResult {
  total: number;
  visible: CatalogRecord[];
}
