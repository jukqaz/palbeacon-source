import { writable } from 'svelte/store';
import type { BreedingDirection, TeamGoal } from './types';

export interface PlanningWorkspaceState {
  team_goal: TeamGoal;
  breeding_direction: BreedingDirection;
  parent_a_id: string;
  parent_b_id: string;
  target_id: string;
  work_type: string;
  owned_only: boolean;
  building_id: string;
  building_quantity: number;
}

const STORAGE_KEY = 'palbeacon.planning.v1';
const goals = new Set<TeamGoal>(['balanced', 'base', 'night', 'travel']);
const directions = new Set<BreedingDirection>(['forward', 'reverse', 'path']);
const safeId = (value: unknown): string =>
  typeof value === 'string' && value.length <= 160 ? value : '';
const initialState = (): PlanningWorkspaceState => ({
  team_goal: 'balanced',
  breeding_direction: 'forward',
  parent_a_id: '',
  parent_b_id: '',
  target_id: '',
  work_type: 'Handcraft',
  owned_only: false,
  building_id: '',
  building_quantity: 1,
});

const normalizeState = (input: unknown): PlanningWorkspaceState => {
  const value =
    typeof input === 'object' && input !== null ? (input as Record<string, unknown>) : {};
  const quantity = Number(value['building_quantity']);
  return {
    team_goal: goals.has(value['team_goal'] as TeamGoal)
      ? (value['team_goal'] as TeamGoal)
      : 'balanced',
    breeding_direction: directions.has(value['breeding_direction'] as BreedingDirection)
      ? (value['breeding_direction'] as BreedingDirection)
      : 'forward',
    parent_a_id: safeId(value['parent_a_id']),
    parent_b_id: safeId(value['parent_b_id']),
    target_id: safeId(value['target_id']),
    work_type: safeId(value['work_type']) || 'Handcraft',
    owned_only: value['owned_only'] === true,
    building_id: safeId(value['building_id']),
    building_quantity: Number.isSafeInteger(quantity) ? Math.max(1, Math.min(999, quantity)) : 1,
  };
};

const readState = (): PlanningWorkspaceState => {
  if (typeof window === 'undefined') return initialState();
  try {
    return normalizeState(JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? '{}'));
  } catch {
    return initialState();
  }
};

export const planningWorkspace = writable<PlanningWorkspaceState>(readState());

if (typeof window !== 'undefined') {
  planningWorkspace.subscribe((state) => {
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
    } catch {
      // A full or private store must not block deterministic planning.
    }
  });
}

export const updatePlanningWorkspace = (state: PlanningWorkspaceState): void =>
  planningWorkspace.set(state);

export const resetPlanningWorkspace = (): void => planningWorkspace.set(initialState());

export const serializePlanningWorkspace = (state: PlanningWorkspaceState): string =>
  JSON.stringify({ schema: 'palbeacon.plan.v1', plan: normalizeState(state) }, null, 2);

export const parsePlanningWorkspace = (source: string): PlanningWorkspaceState => {
  if (source.length > 16_384) throw new Error('계획 파일이 너무 큽니다.');
  let document: unknown;
  try {
    document = JSON.parse(source);
  } catch {
    throw new Error('계획 파일을 읽을 수 없습니다.');
  }
  if (typeof document !== 'object' || document === null) {
    throw new Error('계획 파일을 읽을 수 없습니다.');
  }
  const record = document as Record<string, unknown>;
  if (record['schema'] !== 'palbeacon.plan.v1' || !record['plan']) {
    throw new Error('PalBeacon 계획 파일이 아닙니다.');
  }
  return normalizeState(record['plan']);
};
