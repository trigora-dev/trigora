import { describe, expect, it } from 'vitest';

import type { RetryPolicy } from './retry';

describe('Retry contract types', () => {
  it('accepts an exponential retry policy', () => {
    const retry: RetryPolicy = {
      attempts: 5,
      backoff: 'exponential',
    };

    expect(retry.attempts).toBe(5);
    expect(retry.backoff).toBe('exponential');
  });
});
