import { describe, expect, it } from 'vitest';
import { formatModelScore, formatScale, formatUserNumber } from './user-number';

describe('user-facing number formatting', () => {
  it('removes meaningless trailing zeroes without discarding useful precision', () => {
    expect(formatModelScore(200)).toBe('200');
    expect(formatModelScore(133.5)).toBe('133.5');
    expect(formatScale(1)).toBe('1×');
    expect(formatScale(1.25)).toBe('1.25×');
  });

  it('groups large numbers and normalizes negative zero', () => {
    expect(formatUserNumber(12_345)).toBe('12,345');
    expect(formatUserNumber(-0)).toBe('0');
  });

  it('uses a safe fallback for missing and non-finite values', () => {
    expect(formatUserNumber(null)).toBe('—');
    expect(formatUserNumber(Number.NaN)).toBe('—');
    expect(formatUserNumber(Number.POSITIVE_INFINITY)).toBe('—');
  });
});
