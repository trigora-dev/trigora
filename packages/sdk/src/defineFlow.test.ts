import { describe, expect, it, vi } from 'vitest';
import { defineFlow } from './defineFlow';

void defineFlow({
  id: 'valid-webhook-flow',
  trigger: { type: 'webhook' as const, event: 'orders.created' },
  async run() {
    return { ok: true };
  },
});

void defineFlow({
  id: 'invalid-webhook-flow',
  trigger: {
    type: 'webhook' as const,
    // @ts-expect-error webhook triggers must not accept cron-only fields
    cron: '* * * * *',
  },
  async run() {
    return { ok: true };
  },
});

void defineFlow({
  id: 'invalid-manual-flow',
  trigger: {
    type: 'manual' as const,
    // @ts-expect-error manual triggers must not accept webhook-only fields
    event: 'orders.created',
  },
  async run() {},
});

void defineFlow({
  id: 'invalid-cron-flow',
  trigger: {
    type: 'cron' as const,
    cron: '0 9 * * *',
    // @ts-expect-error cron triggers must not accept webhook-only fields
    event: 'orders.created',
  },
  async run() {},
});

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

  it('allows webhook flows to return HTTP-friendly values', async () => {
    const flow = defineFlow({
      id: 'webhook-flow',
      trigger: { type: 'webhook' as const },
      async run() {
        return {
          ok: true,
          userId: '123',
        };
      },
    });

    await expect(
      flow.run(
        {
          id: 'evt_2',
          type: 'webhook',
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
      ),
    ).resolves.toEqual({
      ok: true,
      userId: '123',
    });
  });
});
