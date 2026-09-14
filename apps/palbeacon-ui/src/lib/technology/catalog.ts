import type {
  TechnologyCatalog,
  TechnologyLane,
  TechnologyLevelGroup,
  TechnologyRecord,
  TechnologyUnlock,
} from './types';
import { technologyCatalogSchema } from './runtime-schema';
import { hasKoreanDisplayText, safeGameDescription } from '$lib/shared/data/display-text';
import { parseRuntimePayload } from '$lib/shared/data/parse-runtime-payload';
import { normalizeSearchText } from '$lib/shared/search/search-core';

const searchableText = (record: TechnologyRecord): string =>
  normalizeSearchText(
    [
      record.name_ko,
      record.id,
      record.description_ko ?? '',
      ...record.unlocks.flatMap((unlock) => [unlock.name_ko, unlock.id]),
    ].join(' '),
  );

export const filterTechnologies = (
  catalog: TechnologyCatalog,
  query: string,
  lane: TechnologyLane | 'all',
): TechnologyRecord[] => {
  const normalizedQuery = normalizeSearchText(query);
  return catalog.technologies.filter(
    (technology) =>
      !technology.localization_fallback &&
      technology.name_ko.trim().length > 0 &&
      (lane === 'all' || technology.lane === lane) &&
      (normalizedQuery.length === 0 || searchableText(technology).includes(normalizedQuery)),
  );
};

export const groupTechnologiesByLevel = (
  technologies: readonly TechnologyRecord[],
): TechnologyLevelGroup[] => {
  const levels = new Map<number, TechnologyLevelGroup>();
  for (const technology of technologies) {
    const group = levels.get(technology.level) ?? {
      level: technology.level,
      normal: [],
      ancient: [],
      normalCost: 0,
      ancientCost: 0,
    };
    group[technology.lane].push(technology);
    if (technology.lane === 'normal') group.normalCost += technology.cost;
    else group.ancientCost += technology.cost;
    levels.set(technology.level, group);
  }
  return [...levels.values()].toSorted((left, right) => left.level - right.level);
};

export const technologyDisplayDescription = (technology: TechnologyRecord): string | null =>
  safeGameDescription(technology.description_ko);

export const technologyDisplayUnlocks = (technology: TechnologyRecord): TechnologyUnlock[] =>
  technology.unlocks.filter((unlock) => hasKoreanDisplayText(unlock.name_ko));

export const technologyDisplayPrerequisite = (technology: TechnologyRecord): string | null =>
  hasKoreanDisplayText(technology.prerequisite.technology_name_ko)
    ? technology.prerequisite.technology_name_ko
    : null;

export const loadTechnologyCatalog = async (
  fetcher: typeof fetch = fetch,
): Promise<TechnologyCatalog> => {
  const response = await fetcher('/generated/game/technology.v1.json');
  if (!response.ok) {
    throw new Error(`Technology catalog request failed: ${response.status.toString()}`);
  }
  const catalog = parseRuntimePayload<TechnologyCatalog>(
    technologyCatalogSchema,
    await response.json(),
    '기술 데이터',
  );
  if (!catalog.verified) throw new Error('Technology catalog is not exact-build verified');
  return catalog;
};
