import { describe, expect, it } from 'vitest';
import {
  filterTechnologies,
  groupTechnologiesByLevel,
  technologyDisplayDescription,
  technologyDisplayPrerequisite,
  technologyDisplayUnlocks,
} from './catalog';
import type { TechnologyCatalog, TechnologyRecord } from './types';

const technology = (
  id: string,
  name: string,
  level: number,
  lane: 'normal' | 'ancient',
  unlockName: string,
): TechnologyRecord => ({
  id,
  name_ko: name,
  description_ko: `${unlockName}을 해금합니다.`,
  icon_path: null,
  level,
  cost: lane === 'ancient' ? 2 : 1,
  tier: 1,
  lane,
  localization_fallback: false,
  prerequisite: {
    technology_id: null,
    technology_name_ko: null,
    tower_boss: null,
    research_id: null,
  },
  unlocks: [{ kind: 'item', id: `Item_${id}`, name_ko: unlockName }],
});

const records = [
  technology('Technology_Bed', '푹신한 침대', 10, 'normal', '침대'),
  technology('Technology_AncientSaddle', '고대 안장', 10, 'ancient', '신비한 안장'),
  technology('Technology_Farm', '배합 목장', 19, 'normal', '배합 목장'),
];

const catalog: TechnologyCatalog = {
  schema_version: 1,
  game_build_id: 'steam:24575825',
  mapping_sha256: 'verified',
  verified: true,
  contract_review_id: null,
  source: { path: 'fixture', sha256: 'fixture' },
  statistics: { level_count: 2, technology_count: 3, ancient_count: 1, icon_count: 0 },
  technologies: records,
};

describe('technology catalog', () => {
  it('searches Korean names, original IDs, descriptions and unlocks', () => {
    expect(filterTechnologies(catalog, '목장', 'all').map((record) => record.id)).toEqual([
      'Technology_Farm',
    ]);
    expect(filterTechnologies(catalog, 'ancientsaddle', 'all').map((record) => record.id)).toEqual([
      'Technology_AncientSaddle',
    ]);
    expect(filterTechnologies(catalog, '신비한', 'all').map((record) => record.id)).toEqual([
      'Technology_AncientSaddle',
    ]);
  });

  it('filters before grouping and keeps normal and ancient lanes separate', () => {
    const groups = groupTechnologiesByLevel(filterTechnologies(catalog, '', 'ancient'));
    expect(groups).toHaveLength(1);
    expect(groups[0]?.normal).toHaveLength(0);
    expect(groups[0]?.ancient.map((record) => record.id)).toEqual(['Technology_AncientSaddle']);
  });

  it('keeps only verified Korean display content', () => {
    const unsafe = {
      ...records[0]!,
      description_ko: '편안하게 쉴 수 있다. WorkBench에서 제작한다.',
      prerequisite: {
        technology_id: null,
        technology_name_ko: null,
        tower_boss: 'TowerBoss_A',
        research_id: 'Research_A',
      },
      unlocks: [
        { kind: 'item' as const, id: 'Workbench', name_ko: 'Workbench' },
        { kind: 'building' as const, id: 'Bed', name_ko: '침대' },
      ],
    };

    expect(technologyDisplayDescription(unsafe)).toBe('편안하게 쉴 수 있다.');
    expect(technologyDisplayUnlocks(unsafe).map((unlock) => unlock.name_ko)).toEqual(['침대']);
    expect(technologyDisplayPrerequisite(unsafe)).toBeNull();
  });
});
