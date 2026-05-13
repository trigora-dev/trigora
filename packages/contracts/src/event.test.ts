import { describe, expect, it } from 'vitest';

import type { CronFlowEvent, FlowEvent, ManualFlowEvent, WebhookFlowEvent } from './event';

describe('Event contract types', () => {
  it('accepts the generic base flow event shape', () => {
    const event: FlowEvent<{ ok: true }> = {
      id: 'evt_1',
      type: 'manual',
      timestamp: '2026-05-10T00:00:00.000Z',
      payload: { ok: true },
    };

    expect(event.payload.ok).toBe(true);
  });

  it('accepts trigger-specific manual and webhook events', () => {
    const manualEvent: ManualFlowEvent<{ source: 'local' }> = {
      id: 'evt_2',
      type: 'manual',
      timestamp: '2026-05-10T00:00:00.000Z',
      payload: { source: 'local' },
    };

    const webhookEvent: WebhookFlowEvent<{ action: 'opened' }> = {
      id: 'evt_3',
      type: 'webhook',
      timestamp: '2026-05-10T00:00:00.000Z',
      payload: { action: 'opened' },
      request: {
        headers: {
          'x-github-event': 'issues',
        },
        method: 'POST',
        url: 'https://trigora.dev/f/hello',
        rawBody: '{"action":"opened"}',
      },
    };

    expect(manualEvent.request).toBeUndefined();
    expect(webhookEvent.request?.method).toBe('POST');
  });

  it('accepts the hosted cron runtime event shape', () => {
    const event: CronFlowEvent = {
      id: 'inv_123',
      type: 'cron',
      timestamp: '2026-05-10T02:00:00.000Z',
      payload: {
        cron: '0 2 * * *',
        scheduledAt: '2026-05-10T02:00:00.000Z',
        timezone: 'UTC',
      },
    };

    expect(event.payload.cron).toBe('0 2 * * *');
    expect(event.payload.timezone).toBe('UTC');
    expect(event.request).toBeUndefined();
  });
});
