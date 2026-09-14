import { array, boolean, literal, looseObject, nullable, number, record, string } from 'valibot';

const toolPalSchema = looseObject({
  id: string(),
  name_ko: string(),
  image_path: nullable(string()),
  paldex_number: nullable(number()),
  hp: number(),
  attack: number(),
  defense: number(),
  run_speed: number(),
  ride_sprint_speed: number(),
  stamina: number(),
  food_amount: number(),
  nocturnal: boolean(),
  work_suitability: array(looseObject({ id: string(), name_ko: string(), level: number() })),
});

export const toolsCatalogSchema = looseObject({
  schema_version: literal(1),
  game_build_id: string(),
  mapping_sha256: string(),
  verified: boolean(),
  model_notice: string(),
  work_names_ko: record(string(), string()),
  breeding: looseObject({
    contract_review_id: string(),
    general_formula: string(),
    species: array(looseObject({ internal_id: string(), name_ko: string(), combi_rank: number() })),
    special_rules: array(
      looseObject({
        rule_id: string(),
        parent_a_internal_id: string(),
        parent_b_internal_id: string(),
        child_internal_id: string(),
      }),
    ),
  }),
  pals: array(toolPalSchema),
  buildings: array(
    looseObject({
      id: string(),
      name_ko: string(),
      category: string(),
      materials: array(looseObject({ item_id: string(), name_ko: string(), quantity: number() })),
    }),
  ),
});
