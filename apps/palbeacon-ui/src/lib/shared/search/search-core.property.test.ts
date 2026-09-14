import { assert, pre, property, string } from 'fast-check';
import { describe, expect, it } from 'vitest';
import { normalizeCompactSearchText, normalizeSearchText } from './search-core';

describe('shared search normalization properties', () => {
  it('is idempotent for arbitrary Unicode input', () => {
    assert(
      property(string(), (value) => {
        const normalized = normalizeSearchText(value);
        expect(normalizeSearchText(normalized)).toBe(normalized);
      }),
    );
  });

  it('always removes compact search separators', () => {
    assert(
      property(string(), (value) => {
        expect(normalizeCompactSearchText(value)).not.toMatch(/[\s_\-·]/u);
      }),
    );
  });

  it('preserves normalized text when no compact separators remain', () => {
    assert(
      property(string(), (value) => {
        const normalized = normalizeSearchText(value);
        pre(!/[\s_\-·]/u.test(normalized));
        expect(normalizeCompactSearchText(value)).toBe(normalized);
      }),
    );
  });
});
