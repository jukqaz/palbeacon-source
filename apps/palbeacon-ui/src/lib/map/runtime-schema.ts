import {
  array,
  boolean,
  literal,
  looseObject,
  nullable,
  number,
  optional,
  picklist,
  string,
  tuple,
} from 'valibot';

const mapRectSchema = looseObject({
  min_x: number(),
  min_y: number(),
  max_x: number(),
  max_y: number(),
});

export const mapTileIndexSchema = looseObject({
  schema_version: number(),
  game_build_id: string(),
  map_id: string(),
  region_id: string(),
  map_width_px: number(),
  map_height_px: number(),
  tile_count: number(),
  tiles: array(
    looseObject({
      level: number(),
      x: number(),
      y: number(),
      relative_path: string(),
      size_bytes: number(),
      sha256: string(),
      map_rect: mapRectSchema,
    }),
  ),
});

export const exactMapPoiDocumentSchema = looseObject({
  schema_version: number(),
  game_build_id: string(),
  poi_count: number(),
  pois: array(
    looseObject({
      id: string(),
      kind: string(),
      display_name: string(),
      entity_id: nullable(string()),
      map_id: string(),
      region_id: string(),
      world_x: number(),
      world_y: number(),
      map_x: number(),
      map_y: number(),
      source_build_id: string(),
      verified: boolean(),
    }),
  ),
});

export const mapTerminologySchema = looseObject({
  schema_version: number(),
  language: literal('ko'),
  game_build_id: string(),
  groups: array(looseObject({ id: string(), label_ko: string(), description_ko: string() })),
  terms: array(
    looseObject({
      id: string(),
      group_id: string(),
      label_ko: string(),
      generic_title_ko: string(),
      description_ko: string(),
      aliases_ko: array(string()),
      verification: picklist(['reviewed', 'needs_game_l10n_review']),
      filter_visibility: optional(picklist(['hidden_until_verified', 'merged'])),
      merged_into: optional(string()),
    }),
  ),
});

export const supplementalMapDocumentSchema = looseObject({
  schema_version: number(),
  game_build_id: string(),
  provenance: looseObject({
    source_name: string(),
    source_page: string(),
    exact_local_build_verified: boolean(),
  }),
  layers: array(
    looseObject({
      index: number(),
      id: string(),
      label: string(),
      group: string(),
      default_enabled: boolean(),
      minimum_scale: number(),
      dense: boolean(),
      point_count: number(),
    }),
  ),
  regions: array(
    looseObject({
      map_id: string(),
      region_id: string(),
      point_count: number(),
      points: array(tuple([number(), number(), number(), number(), number()])),
    }),
  ),
});
