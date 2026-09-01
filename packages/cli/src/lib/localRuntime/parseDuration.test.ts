import { describe, expect, it } from 'vitest';

import { parseDuration } from './parseDuration';

describe('parseDuration', () => {
  it('parses numbers and duration strings', () => {
    expect(parseDuration(1500)).toBe(1500);
    expect(parseDuration('250ms')).toBe(250);
    expect(parseDuration('5s')).toBe(5000);
    expect(parseDuration('30m')).toBe(30 * 60_000);
    expect(parseDuration('7d')).toBe(7 * 86_400_000);
  });

  it('rejects invalid values', () => {
    expect(() => parseDuration('tomorrow')).toThrow(/Invalid duration/);
    expect(() => parseDuration(-1)).toThrow(/non-negative/);
  });
});
