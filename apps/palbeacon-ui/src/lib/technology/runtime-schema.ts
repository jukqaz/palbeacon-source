import { array, boolean, looseObject, nullable, number, picklist, string } from 'valibot';

const technologyRecordSchema = looseObject({
  id: string(),
  name_ko: string(),
  description_ko: nullable(string()),
  icon_path: nullable(string()),
  level: number(),
  cost: number(),
  tier: number(),
  lane: picklist(['normal', 'ancient']),
  localization_fallback: boolean(),
  prerequisite: looseObject({
    technology_id: nullable(string()),
    technology_name_ko: nullable(string()),
    tower_boss: nullable(string()),
    research_id: nullable(string()),
  }),
  unlocks: array(
    looseObject({ kind: picklist(['item', 'building']), id: string(), name_ko: string() }),
  ),
});

export const technologyCatalogSchema = looseObject({
  schema_version: number(),
  game_build_id: string(),
  mapping_sha256: string(),
  verified: boolean(),
  contract_review_id: nullable(string()),
  source: looseObject({ path: string(), sha256: string() }),
  statistics: looseObject({
    level_count: number(),
    technology_count: number(),
    ancient_count: number(),
    icon_count: number(),
  }),
  technologies: array(technologyRecordSchema),
});
