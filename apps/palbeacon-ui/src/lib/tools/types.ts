export type TeamGoal = 'balanced' | 'base' | 'night' | 'travel';
export type ToolMode = 'breeding' | 'team' | 'work' | 'travel' | 'materials';
export type BreedingDirection = 'forward' | 'reverse' | 'path';

interface WorkSuitability {
  id: string;
  name_ko: string;
  level: number;
}

export interface ToolPal {
  id: string;
  name_ko: string;
  image_path: string | null;
  paldex_number: number | null;
  hp: number;
  attack: number;
  defense: number;
  run_speed: number;
  ride_sprint_speed: number;
  stamina: number;
  food_amount: number;
  nocturnal: boolean;
  work_suitability: WorkSuitability[];
}

interface ToolBuildingMaterial {
  item_id: string;
  name_ko: string;
  quantity: number;
}

export interface ToolBuilding {
  id: string;
  name_ko: string;
  category: string;
  materials: ToolBuildingMaterial[];
}

export interface ToolBreedingSpecies {
  internal_id: string;
  name_ko: string;
  name_en: string;
  paldex_number: number | null;
  paldex_suffix: string;
  combi_rank: number;
  combi_duplicate_priority: number;
  ignore_combi: boolean;
  source_row_id: string;
}

export interface ToolSpecialBreedingRule {
  rule_id: string;
  parent_a_internal_id: string;
  parent_a_gender: string;
  parent_b_internal_id: string;
  parent_b_gender: string;
  child_internal_id: string;
  source_row_ids: string[];
}

interface ToolBreedingCatalog {
  contract_review_id: string;
  general_formula: string;
  species: ToolBreedingSpecies[];
  special_rules: ToolSpecialBreedingRule[];
}

export interface ToolsCatalog {
  schema_version: 1;
  game_build_id: string;
  mapping_sha256: string;
  verified: boolean;
  model_notice: string;
  work_names_ko: Record<string, string>;
  breeding: ToolBreedingCatalog;
  pals: ToolPal[];
  buildings: ToolBuilding[];
}

export interface PalRecommendation {
  pal: ToolPal;
  reason: string;
}

export interface BreedingOutcome {
  child: ToolPal;
  kind: 'general' | 'special';
  targetRank: number | null;
  parentAGender: string;
  parentBGender: string;
  ruleId: string | null;
}

export interface BreedingCombination {
  parentA: ToolPal;
  parentB: ToolPal;
  outcome: BreedingOutcome;
}

export interface BreedingPathStep {
  pal: ToolPal;
  generation: number;
  parentA: ToolPal | null;
  parentB: ToolPal | null;
  kind: 'general' | 'special' | null;
}

export interface BreedingPath {
  reachable: boolean;
  target: ToolPal;
  generations: number;
  steps: BreedingPathStep[];
  pairEvaluations: number;
  quality: 'exact-species-graph';
}
