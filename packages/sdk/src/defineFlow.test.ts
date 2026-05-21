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
  trigger: { type: 'webhook', event: 'orders.created', route: '/hooks/orders' },
  async run() {
    return { ok: true };
  },
});

const invalidWebhookTrigger: WebhookTrigger = {
  type: 'webhook',
  // @ts-expect-error webhook triggers must not accept cron-only fields
  cron: '* * * * *',
};

const invalidWebhookRouteTrigger: WebhookTrigger = {
  type: 'webhook',
  // @ts-expect-error webhook routes must start with "/"
  route: 'hooks/orders',
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
void invalidWebhookRouteTrigger;
void invalidManualTrigger;
void invalidManualReturnFlow;
void invalidCronTrigger;

void defineFlow({
  id: 'valid-webhook-inference-flow',
  trigger: { type: 'webhook', route: '/hooks/inferred' },
  async run() {
    return {
      ok: true,
      received: true,
    };
  },
});

void defineFlow<{ orderId: string }>({
  id: 'valid-webhook-generic-flow',
  trigger: { type: 'webhook' },
  async run(event) {
    const orderId: string = event.payload.orderId;
    void orderId;
    return { ok: true };
  },
});

void defineFlow({
  id: 'valid-cron-flow',
  trigger: { type: 'cron', cron: '0 2 * * *' },
  async run(event, ctx) {
    const cron: string = event.payload.cron;
    const scheduledAt: string = event.payload.scheduledAt;
    const timezone: 'UTC' = event.payload.timezone;
    void cron;
    void scheduledAt;
    void timezone;
    await ctx.log.info('Running nightly job');
  },
});

void defineFlow({
  id: 'valid-webhook-request-flow',
  trigger: { type: 'webhook' },
  async run(event) {
    const headers: Record<string, string> = event.request.headers;
    const method: string = event.type;
    void headers;
    void method;
  },
});

void defineFlow({
  id: 'invalid-manual-request-flow',
  trigger: { type: 'manual' },
  async run(event) {
    // @ts-expect-error manual flows do not receive request metadata
    const headers = event.request?.headers;
    void headers;
  },
});

void defineFlow({
  id: 'invalid-cron-request-flow',
  trigger: { type: 'cron', cron: '0 2 * * *' },
  async run(event) {
    // @ts-expect-error cron flows do not receive request metadata
    const headers = event.request?.headers;
    void headers;
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
      trigger: { type: 'webhook', route: '/hooks/test' },
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
          type: 'POST',
          timestamp: new Date().toISOString(),
          payload: {},
          request: {
            headers: {},
            method: 'POST',
            rawBody: '{}',
            url: 'https://example.com/webhooks/test',
          },
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

    expect(flow.trigger.route).toBe('/hooks/test');
  });

  it('allows cron flows to run without returning a response body', async () => {
    const run = vi.fn(async () => undefined);

    const flow = defineFlow({
      id: 'cron-flow',
      trigger: { type: 'cron', cron: '0 2 * * *' },
      run,
    });

    await expect(
      flow.run(
        {
          id: 'evt_3',
          type: 'cron',
          timestamp: new Date().toISOString(),
          payload: {
            cron: '0 2 * * *',
            scheduledAt: '2026-05-10T02:00:00.000Z',
            timezone: 'UTC',
          },
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
    ).resolves.toBeUndefined();

    expect(run).toHaveBeenCalledOnce();
  });
});
