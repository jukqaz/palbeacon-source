export type TechnologyLane = 'normal' | 'ancient';

export interface TechnologyUnlock {
  kind: 'item' | 'building';
  id: string;
  name_ko: string;
}

export interface TechnologyRecord {
  id: string;
  name_ko: string;
  description_ko: string | null;
  icon_path: string | null;
  level: number;
  cost: number;
  tier: number;
  lane: TechnologyLane;
  localization_fallback: boolean;
  prerequisite: {
    technology_id: string | null;
    technology_name_ko: string | null;
    tower_boss: string | null;
    research_id: string | null;
  };
  unlocks: TechnologyUnlock[];
}

export interface TechnologyCatalog {
  schema_version: number;
  game_build_id: string;
  mapping_sha256: string;
  verified: boolean;
  contract_review_id: string | null;
  source: {
    path: string;
    sha256: string;
  };
  statistics: {
    level_count: number;
    technology_count: number;
    ancient_count: number;
    icon_count: number;
  };
  technologies: TechnologyRecord[];
}

export interface TechnologyLevelGroup {
  level: number;
  normal: TechnologyRecord[];
  ancient: TechnologyRecord[];
  normalCost: number;
  ancientCost: number;
}
