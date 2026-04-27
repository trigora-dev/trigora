import type {
  CronTrigger,
  ManualFlowDefinition,
  ManualTrigger,
  WebhookTrigger,
} from '@trigora/contracts';
import { describe, expect, it, vi } from 'vitest';
import { defineFlow } from './defineFlow';

void defineFlow({
  id: 'valid-webhook-flow',
  trigger: { type: 'webhook', event: 'orders.created' },
  async run() {
    return { ok: true };
  },
});

const invalidWebhookTrigger: WebhookTrigger = {
  type: 'webhook',
  // @ts-expect-error webhook triggers must not accept cron-only fields
  cron: '* * * * *',
};

const invalidManualTrigger: ManualTrigger = {
  type: 'manual',
  // @ts-expect-error manual triggers must not accept webhook-only fields
  event: 'orders.created',
};

const invalidManualReturnFlow: ManualFlowDefinition = {
  id: 'invalid-manual-return-flow',
  trigger: {
    type: 'manual',
  },
  // @ts-expect-error manual flows must not return webhook-style response bodies
  async run() {
    return { ok: true };
  },
};

const invalidCronTrigger: CronTrigger = {
  type: 'cron',
  cron: '0 9 * * *',
  // @ts-expect-error cron triggers must not accept webhook-only fields
  event: 'orders.created',
};

void invalidWebhookTrigger;
void invalidManualTrigger;
void invalidManualReturnFlow;
void invalidCronTrigger;

void defineFlow({
  id: 'valid-webhook-inference-flow',
  trigger: { type: 'webhook' },
  async run() {
    return {
      ok: true,
      received: true,
    };
  },
});

describe('defineFlow', () => {
  it('returns the provided flow definition', async () => {
    const run = vi.fn();

    const flow = defineFlow({
      id: 'hello',
      trigger: { type: 'manual' },
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
      trigger: { type: 'manual' },
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
      trigger: { type: 'webhook' },
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
