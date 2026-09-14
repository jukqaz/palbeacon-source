import type {
  BreedingCombination,
  BreedingOutcome,
  BreedingPath,
  BreedingPathStep,
  PalRecommendation,
  TeamGoal,
  ToolBreedingSpecies,
  ToolBuilding,
  ToolPal,
  ToolsCatalog,
  ToolSpecialBreedingRule,
} from './types';
import { toolsCatalogSchema } from './runtime-schema';
import { parseRuntimePayload } from '$lib/shared/data/parse-runtime-payload';

const compareIdentity = (left: ToolPal, right: ToolPal): number =>
  (left.paldex_number ?? 9999) - (right.paldex_number ?? 9999) || left.id.localeCompare(right.id);

const workLevels = (pal: ToolPal): number[] => pal.work_suitability.map((work) => work.level);

const compareWork = (left: ToolPal, right: ToolPal): number => {
  const leftLevels = workLevels(left);
  const rightLevels = workLevels(right);
  return (
    Math.max(0, ...rightLevels) - Math.max(0, ...leftLevels) ||
    rightLevels.reduce((total, level) => total + level, 0) -
      leftLevels.reduce((total, level) => total + level, 0) ||
    left.food_amount - right.food_amount ||
    compareIdentity(left, right)
  );
};

const reason = (pal: ToolPal, goal: TeamGoal): string => {
  if (goal === 'balanced') {
    return `체력 ${pal.hp.toString()} · 공격 ${pal.attack.toString()} · 방어 ${pal.defense.toString()}`;
  }
  if (goal === 'travel') {
    return `탑승 질주 ${pal.ride_sprint_speed.toString()} · 달리기 ${pal.run_speed.toString()} · 스태미나 ${pal.stamina.toString()}`;
  }
  const works = [...pal.work_suitability]
    .toSorted((left, right) => right.level - left.level)
    .slice(0, 3)
    .map((work) => `${work.name_ko} ${work.level.toString()}레벨`);
  return `${goal === 'night' && pal.nocturnal ? '야행성 · ' : ''}${[...works, `식사량 ${pal.food_amount.toString()}`].join(' · ')}`;
};

const isCandidate = (pal: ToolPal, goal: TeamGoal): boolean => {
  if (goal === 'balanced') return pal.attack > 0 || pal.hp > 0 || pal.defense > 0;
  if (goal === 'base') return pal.work_suitability.length > 0;
  if (goal === 'night') return pal.nocturnal && pal.work_suitability.length > 0;
  return pal.ride_sprint_speed > 0 && pal.stamina > 0;
};

const compareCandidate = (left: ToolPal, right: ToolPal, goal: TeamGoal): number => {
  if (goal === 'balanced') {
    return (
      right.attack - left.attack ||
      right.hp - left.hp ||
      right.defense - left.defense ||
      compareIdentity(left, right)
    );
  }
  if (goal === 'base' || goal === 'night') return compareWork(left, right);
  return (
    right.ride_sprint_speed - left.ride_sprint_speed ||
    right.stamina - left.stamina ||
    right.run_speed - left.run_speed ||
    compareIdentity(left, right)
  );
};

export const recommendTeam = (
  pals: readonly ToolPal[],
  goal: TeamGoal,
  limit = 5,
): PalRecommendation[] =>
  pals
    .filter((pal) => isCandidate(pal, goal))
    .toSorted((left, right) => compareCandidate(left, right, goal))
    .map((pal) => ({ pal, reason: reason(pal, goal) }))
    .slice(0, Math.max(1, Math.min(12, limit)));

export const rankWork = (pals: readonly ToolPal[], workTypeId: string, limit = 20): ToolPal[] => {
  const level = (pal: ToolPal) =>
    pal.work_suitability.find((work) => work.id === workTypeId)?.level ?? 0;
  return pals
    .filter((pal) => level(pal) > 0)
    .toSorted(
      (left, right) =>
        level(right) - level(left) ||
        left.food_amount - right.food_amount ||
        left.id.localeCompare(right.id),
    )
    .slice(0, Math.max(1, Math.min(100, limit)));
};

export const rankTravel = (pals: readonly ToolPal[], limit = 20): ToolPal[] =>
  pals
    .filter((pal) => pal.ride_sprint_speed > 0 && pal.stamina > 0)
    .toSorted(
      (left, right) =>
        right.ride_sprint_speed - left.ride_sprint_speed ||
        right.stamina - left.stamina ||
        left.id.localeCompare(right.id),
    )
    .slice(0, Math.max(1, Math.min(100, limit)));

export const materialTotals = (building: ToolBuilding | null, quantity: number) =>
  building?.materials.map((material) => ({
    ...material,
    total: material.quantity * Math.max(1, Math.min(999, Math.trunc(quantity) || 1)),
  })) ?? [];

const pairKey = (left: string, right: string): string => {
  const a = left.toLocaleLowerCase('en-US');
  const b = right.toLocaleLowerCase('en-US');
  return a.localeCompare(b, 'en-US') <= 0 ? `${a}\u001f${b}` : `${b}\u001f${a}`;
};

const paldexSort = (left: ToolPal, right: ToolPal): number =>
  (left.paldex_number ?? 9999) - (right.paldex_number ?? 9999) ||
  left.name_ko.localeCompare(right.name_ko, 'ko') ||
  left.id.localeCompare(right.id, 'en-US');

export class BreedingCalculator {
  readonly #pals = new Map<string, ToolPal>();
  readonly #species: ToolBreedingSpecies[];
  readonly #speciesById = new Map<string, ToolBreedingSpecies>();
  readonly #specialByParents = new Map<string, ToolSpecialBreedingRule[]>();
  readonly #specialOnlyChildren = new Set<string>();
  readonly #bestGeneralByRank = new Map<number, ToolBreedingSpecies>();
  readonly #generalRanks: number[];

  constructor(catalog: ToolsCatalog) {
    this.#species = catalog.breeding.species;
    for (const pal of catalog.pals) this.#pals.set(pal.id.toLocaleLowerCase('en-US'), pal);
    for (const species of this.#species) {
      this.#speciesById.set(species.internal_id.toLocaleLowerCase('en-US'), species);
    }
    for (const rule of catalog.breeding.special_rules) {
      this.#specialOnlyChildren.add(rule.child_internal_id.toLocaleLowerCase('en-US'));
      const key = pairKey(rule.parent_a_internal_id, rule.parent_b_internal_id);
      this.#specialByParents.set(key, [...(this.#specialByParents.get(key) ?? []), rule]);
    }
    for (const species of this.#species) {
      if (
        species.ignore_combi ||
        this.#specialOnlyChildren.has(species.internal_id.toLocaleLowerCase('en-US'))
      ) {
        continue;
      }
      const current = this.#bestGeneralByRank.get(species.combi_rank);
      if (
        !current ||
        current.combi_duplicate_priority < species.combi_duplicate_priority ||
        (current.combi_duplicate_priority === species.combi_duplicate_priority &&
          species.internal_id.localeCompare(current.internal_id, 'en-US') < 0)
      ) {
        this.#bestGeneralByRank.set(species.combi_rank, species);
      }
    }
    this.#generalRanks = [...this.#bestGeneralByRank.keys()].toSorted(
      (left, right) => left - right,
    );
  }

  palById(id: string): ToolPal | undefined {
    return this.#pals.get(id.trim().toLocaleLowerCase('en-US'));
  }

  forward(parentAId: string, parentBId: string): BreedingOutcome[] {
    let parentA = this.#speciesById.get(parentAId.toLocaleLowerCase('en-US'));
    let parentB = this.#speciesById.get(parentBId.toLocaleLowerCase('en-US'));
    if (!parentA || !parentB) return [];
    if (parentA.internal_id.localeCompare(parentB.internal_id, 'en-US') > 0) {
      [parentA, parentB] = [parentB, parentA];
    }

    const specialRules =
      this.#specialByParents.get(pairKey(parentA.internal_id, parentB.internal_id)) ?? [];
    const special = specialRules.flatMap((rule): BreedingOutcome[] => {
      const child = this.palById(rule.child_internal_id);
      return child
        ? [
            {
              child,
              kind: 'special',
              targetRank: null,
              parentAGender: rule.parent_a_gender,
              parentBGender: rule.parent_b_gender,
              ruleId: rule.rule_id,
            },
          ]
        : [];
    });
    if (
      specialRules.some(
        (rule) => rule.parent_a_gender === 'None' && rule.parent_b_gender === 'None',
      )
    ) {
      return special;
    }

    const targetRank = Math.floor((parentA.combi_rank + parentB.combi_rank + 1) / 2);
    const nearest = this.#nearestGeneral(targetRank);
    const child = nearest ? this.palById(nearest.internal_id) : undefined;
    return child
      ? [
          ...special,
          {
            child,
            kind: 'general',
            targetRank,
            parentAGender: 'None',
            parentBGender: 'None',
            ruleId: null,
          },
        ]
      : special;
  }

  reverse(childId: string): BreedingCombination[] {
    const child = this.palById(childId);
    if (!child) return [];
    const combinations: BreedingCombination[] = [];
    const seen = new Set<string>();
    for (let left = 0; left < this.#species.length; left += 1) {
      for (let right = left; right < this.#species.length; right += 1) {
        const parentASpecies = this.#species[left];
        const parentBSpecies = this.#species[right];
        if (!parentASpecies || !parentBSpecies) continue;
        for (const outcome of this.forward(
          parentASpecies.internal_id,
          parentBSpecies.internal_id,
        )) {
          if (outcome.child.id.toLocaleLowerCase('en-US') !== child.id.toLocaleLowerCase('en-US')) {
            continue;
          }
          const key = [
            parentASpecies.internal_id,
            parentBSpecies.internal_id,
            outcome.kind,
            outcome.parentAGender,
            outcome.parentBGender,
          ].join('\u001f');
          if (seen.has(key)) continue;
          const parentA = this.palById(parentASpecies.internal_id);
          const parentB = this.palById(parentBSpecies.internal_id);
          if (!parentA || !parentB) continue;
          seen.add(key);
          combinations.push({ parentA, parentB, outcome });
        }
      }
    }
    return combinations.toSorted(
      (left, right) =>
        Number(right.outcome.kind === 'special') - Number(left.outcome.kind === 'special') ||
        paldexSort(left.parentA, right.parentA) ||
        paldexSort(left.parentB, right.parentB),
    );
  }

  shortestPath(
    sourceIds: readonly string[],
    targetId: string,
    maximumGenerations = 6,
  ): BreedingPath | null {
    const target = this.palById(targetId);
    if (!target) return null;
    const sources = new Map<string, ToolPal>();
    for (const id of sourceIds) {
      const pal = this.palById(id);
      if (pal) sources.set(pal.id.toLocaleLowerCase('en-US'), pal);
    }
    if (sources.size === 0) return null;

    const steps = new Map<string, BreedingPathStep>();
    for (const pal of sources.values()) {
      steps.set(pal.id.toLocaleLowerCase('en-US'), {
        pal,
        generation: 0,
        parentA: null,
        parentB: null,
        kind: null,
      });
    }
    let pairEvaluations = 0;
    const generationLimit = Math.max(1, Math.min(12, Math.trunc(maximumGenerations)));
    for (let generation = 1; generation <= generationLimit; generation += 1) {
      const reachable = [...steps.values()].map((step) => step.pal);
      const additions = new Map<string, BreedingPathStep>();
      for (let left = 0; left < reachable.length; left += 1) {
        for (let right = left; right < reachable.length; right += 1) {
          const parentA = reachable[left];
          const parentB = reachable[right];
          if (!parentA || !parentB) continue;
          pairEvaluations += 1;
          for (const outcome of this.forward(parentA.id, parentB.id)) {
            const childKey = outcome.child.id.toLocaleLowerCase('en-US');
            if (steps.has(childKey) || additions.has(childKey)) continue;
            additions.set(childKey, {
              pal: outcome.child,
              generation,
              parentA,
              parentB,
              kind: outcome.kind,
            });
          }
        }
      }
      for (const [id, step] of additions) steps.set(id, step);
      if (steps.has(target.id.toLocaleLowerCase('en-US')) || additions.size === 0) break;
    }

    const targetStep = steps.get(target.id.toLocaleLowerCase('en-US'));
    if (!targetStep) {
      return {
        reachable: false,
        target,
        generations: generationLimit,
        steps: [],
        pairEvaluations,
        quality: 'exact-species-graph',
      };
    }
    const required = new Map<string, BreedingPathStep>();
    const collect = (pal: ToolPal): void => {
      const key = pal.id.toLocaleLowerCase('en-US');
      const step = steps.get(key);
      if (!step || required.has(key)) return;
      required.set(key, step);
      if (step.parentA) collect(step.parentA);
      if (step.parentB) collect(step.parentB);
    };
    collect(target);
    return {
      reachable: true,
      target,
      generations: targetStep.generation,
      steps: [...required.values()].toSorted(
        (left, right) => left.generation - right.generation || paldexSort(left.pal, right.pal),
      ),
      pairEvaluations,
      quality: 'exact-species-graph',
    };
  }

  #nearestGeneral(targetRank: number): ToolBreedingSpecies | undefined {
    if (this.#generalRanks.length === 0) return undefined;
    let low = 0;
    let high = this.#generalRanks.length;
    while (low < high) {
      const middle = (low + high) >> 1;
      if ((this.#generalRanks[middle] ?? Number.POSITIVE_INFINITY) < targetRank) low = middle + 1;
      else high = middle;
    }
    const candidates = [this.#generalRanks[low], this.#generalRanks[low - 1]]
      .flatMap((rank): ToolBreedingSpecies[] => {
        const species = rank == null ? undefined : this.#bestGeneralByRank.get(rank);
        return species ? [species] : [];
      })
      .toSorted(
        (left, right) =>
          Math.abs(left.combi_rank - targetRank) - Math.abs(right.combi_rank - targetRank) ||
          right.combi_duplicate_priority - left.combi_duplicate_priority ||
          left.internal_id.localeCompare(right.internal_id, 'en-US'),
      );
    return candidates[0];
  }
}

export const loadToolsCatalog = async (fetcher: typeof fetch = fetch): Promise<ToolsCatalog> => {
  const response = await fetcher('/generated/game/tools.v1.json');
  if (!response.ok) throw new Error(`Tools request failed: ${response.status.toString()}`);
  const catalog = parseRuntimePayload<ToolsCatalog>(
    toolsCatalogSchema,
    await response.json(),
    '계획 도구 데이터',
  );
  if (!catalog.verified) throw new Error('Tools catalog is not exact-build verified');
  return catalog;
};
