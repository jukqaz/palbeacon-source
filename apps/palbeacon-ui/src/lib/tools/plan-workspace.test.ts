import { get } from 'svelte/store';
import { beforeEach, describe, expect, it } from 'vitest';
import {
  planningWorkspace,
  parsePlanningWorkspace,
  resetPlanningWorkspace,
  serializePlanningWorkspace,
  updatePlanningWorkspace,
} from './plan-workspace';

describe('planning workspace', () => {
  beforeEach(() => {
    localStorage.clear();
    resetPlanningWorkspace();
  });

  it('keeps only bounded public planning selections', () => {
    updatePlanningWorkspace({
      team_goal: 'night',
      breeding_direction: 'path',
      parent_a_id: 'Anubis',
      parent_b_id: 'NightWorker',
      target_id: 'FastMount',
      work_type: 'Handcraft',
      owned_only: true,
      building_id: 'BreedingFarm',
      building_quantity: 3,
    });

    expect(get(planningWorkspace)).toMatchObject({
      team_goal: 'night',
      parent_a_id: 'Anubis',
      building_quantity: 3,
    });
    expect(localStorage.getItem('palbeacon.planning.v1')).not.toContain('owner_uid');
  });

  it('round-trips a versioned plan without personal save data', () => {
    const source = serializePlanningWorkspace(get(planningWorkspace));
    expect(parsePlanningWorkspace(source)).toEqual(get(planningWorkspace));
    expect(source).not.toMatch(/owner|save|path/iu);
  });

  it('rejects unrelated or oversized files', () => {
    expect(() => parsePlanningWorkspace('{"schema":"unknown"}')).toThrow(
      'PalBeacon 계획 파일이 아닙니다.',
    );
    expect(() => parsePlanningWorkspace('x'.repeat(16_385))).toThrow('계획 파일이 너무 큽니다.');
  });
});
