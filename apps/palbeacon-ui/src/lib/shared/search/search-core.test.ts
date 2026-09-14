import { describe, expect, it } from 'vitest';
import { compareEntityId, normalizeCompactSearchText, normalizeSearchText } from './search-core';

describe('shared search normalization', () => {
  it('normalizes Korean spacing and compatibility characters', () => {
    expect(normalizeSearchText('  배합   목장 ')).toBe('배합 목장');
    expect(normalizeSearchText('ＳｈｅｅｐＢａｌｌ')).toBe('sheepball');
  });

  it('supports separator-free map lookup', () => {
    expect(normalizeCompactSearchText('fast-travel · point')).toBe('fasttravelpoint');
  });

  it('keeps entity ID ordering deterministic', () => {
    expect(compareEntityId('A', 'B')).toBeLessThan(0);
  });
});
