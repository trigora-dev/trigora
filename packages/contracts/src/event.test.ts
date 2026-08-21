import { describe, expect, it } from 'vitest';

import type {
  CronFlowEvent,
  FlowEvent,
  ManualFlowEvent,
  QueueFlowEvent,
  WebhookFlowEvent,
} from './event';

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
      type: 'POST',
      timestamp: '2026-05-10T00:00:00.000Z',
      payload: { action: 'opened' },
      request: {
        headers: {
          'x-github-event': 'issues',
        },
        method: 'POST',
        url: 'https://acme.trigora.dev/hello',
        rawBody: '{"action":"opened"}',
      },
    };

    expect('request' in manualEvent).toBe(false);
    expect(webhookEvent.request.method).toBe('POST');
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
    expect('request' in event).toBe(false);
  });

  it('accepts the hosted queue runtime event shape', () => {
    const event: QueueFlowEvent<{ orderId: string }> = {
      id: 'inv_456',
      type: 'queue',
      timestamp: '2026-05-10T03:00:00.000Z',
      payload: { orderId: 'ord_1' },
      queue: 'orders',
      messageId: 'msg_123',
    };

    expect(event.queue).toBe('orders');
    expect(event.messageId).toBe('msg_123');
    expect(event.payload.orderId).toBe('ord_1');
    expect('request' in event).toBe(false);
  });
});
