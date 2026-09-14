import { describe, expect, it } from 'vitest';
import { toolsFixture } from './tools.fixture';
import { BreedingCalculator, materialTotals, rankTravel, rankWork, recommendTeam } from './tools';

describe('public planning tools', () => {
  it('keeps the public team formula deterministic', () => {
    expect(recommendTeam(toolsFixture.pals, 'balanced', 1)[0]?.pal.id).toBe('Anubis');
    expect(recommendTeam(toolsFixture.pals, 'night', 1)[0]?.pal.id).toBe('NightWorker');
  });

  it('ranks exact work and travel values before stable IDs', () => {
    expect(rankWork(toolsFixture.pals, 'Handcraft')[0]?.id).toBe('Anubis');
    expect(rankTravel(toolsFixture.pals)[0]?.id).toBe('FastMount');
  });

  it('multiplies building materials without mutating source quantities', () => {
    expect(materialTotals(toolsFixture.buildings[0] ?? null, 5)).toEqual([
      { item_id: 'Wood', name_ko: '목재', quantity: 100, total: 500 },
      { item_id: 'Stone', name_ko: '돌', quantity: 20, total: 100 },
    ]);
    expect(toolsFixture.buildings[0]?.materials[0]?.quantity).toBe(100);
  });

  it('keeps exact special, reverse and path breeding results consistent', () => {
    const calculator = new BreedingCalculator(toolsFixture);
    const forward = calculator.forward('Anubis', 'NightWorker');
    expect(forward).toHaveLength(1);
    expect(forward[0]?.kind).toBe('special');
    expect(forward[0]?.child.id).toBe('FastMount');

    const reverse = calculator.reverse('FastMount');
    const specialCombination = reverse.find(
      (combination) =>
        combination.parentA.id === 'Anubis' && combination.parentB.id === 'NightWorker',
    );
    expect(specialCombination?.outcome.kind).toBe('special');

    const path = calculator.shortestPath(['Anubis', 'NightWorker'], 'FastMount', 4);
    expect(path?.reachable).toBe(true);
    expect(path?.generations).toBe(1);
    expect(path?.steps.at(-1)?.pal.id).toBe('FastMount');
  });
});
