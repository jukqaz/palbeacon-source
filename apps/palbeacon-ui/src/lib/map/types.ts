interface MapRect {
  min_x: number;
  min_y: number;
  max_x: number;
  max_y: number;
}

export interface MapTile {
  level: number;
  x: number;
  y: number;
  relative_path: string;
  size_bytes: number;
  sha256: string;
  map_rect: MapRect;
}

export interface MapTileIndex {
  schema_version: number;
  game_build_id: string;
  map_id: string;
  region_id: string;
  map_width_px: number;
  map_height_px: number;
  tile_count: number;
  tiles: MapTile[];
}

export interface ExactMapPoi {
  id: string;
  kind: string;
  display_name: string;
  entity_id: string | null;
  map_id: string;
  region_id: string;
  world_x: number;
  world_y: number;
  map_x: number;
  map_y: number;
  source_build_id: string;
  verified: boolean;
}

export interface ExactMapPoiDocument {
  schema_version: number;
  game_build_id: string;
  poi_count: number;
  pois: ExactMapPoi[];
}

export interface MapTerm {
  id: string;
  group_id: string;
  label_ko: string;
  generic_title_ko: string;
  description_ko: string;
  aliases_ko: string[];
  verification: 'reviewed' | 'needs_game_l10n_review';
  filter_visibility?: 'hidden_until_verified' | 'merged';
  merged_into?: string;
}

interface MapTermGroup {
  id: string;
  label_ko: string;
  description_ko: string;
}

export interface MapTerminology {
  schema_version: number;
  language: 'ko';
  game_build_id: string;
  groups: MapTermGroup[];
  terms: MapTerm[];
}

export interface SupplementalLayer {
  index: number;
  id: string;
  label: string;
  group: string;
  default_enabled: boolean;
  minimum_scale: number;
  dense: boolean;
  point_count: number;
}

type SupplementalPoint = [
  layerIndex: number,
  worldX: number,
  worldY: number,
  mapX: number,
  mapY: number,
];

interface SupplementalRegion {
  map_id: string;
  region_id: string;
  point_count: number;
  points: SupplementalPoint[];
}

export interface SupplementalMapDocument {
  schema_version: number;
  game_build_id: string;
  provenance: {
    source_name: string;
    source_page: string;
    exact_local_build_verified: boolean;
  };
  layers: SupplementalLayer[];
  regions: SupplementalRegion[];
}

export interface MapBundle {
  indexes: MapTileIndex[];
  exact: ExactMapPoiDocument;
  terminology: MapTerminology;
  supplemental: SupplementalMapDocument | null;
}

export interface MapRegionOption {
  id: string;
  mapId: string;
  regionId: string;
  label: string;
}

export interface MapPoi {
  id: string;
  kind: string;
  display_name: string;
  entity_id: string | null;
  map_id: string;
  region_id: string;
  map_x: number;
  map_y: number;
  source: 'exact' | 'supplemental';
  dense: boolean;
}

export interface MapFilterOption {
  id: string;
  label: string;
  groupId: string;
  groupLabel: string;
  count: number;
  defaultEnabled: boolean;
}
