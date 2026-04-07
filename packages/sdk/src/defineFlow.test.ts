import { describe, expect, it, vi } from 'vitest';
import { defineFlow } from './defineFlow';

describe('defineFlow', () => {
  it('returns the provided flow definition', async () => {
    const run = vi.fn();

    const flow = defineFlow({
      id: 'hello',
      trigger: { type: 'manual' as const },
      run,
    });

    expect(flow.id).toBe('hello');
    expect(flow.trigger.type).toBe('manual');
    expect(flow.run).toBe(run);
  });

  it('preserves async run behavior', async () => {
    const run = vi.fn(async () => undefined);

    const flow = defineFlow({
      id: 'async-flow',
      trigger: { type: 'manual' as const },
      run,
    });

    await flow.run(
      {
        id: 'evt_1',
        type: 'manual',
        timestamp: new Date().toISOString(),
        payload: {},
      },
      {
        env: {},
        log: {
          info: () => undefined,
          warn: () => undefined,
          error: () => undefined,
        },
      },
    );

    expect(run).toHaveBeenCalledOnce();
  });
});
