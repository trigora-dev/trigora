import { describe, expect, it } from 'vitest';

import type {
  ApiErrorResponse,
  CronFlowRecord,
  FlowStatusResponse,
  GetFlowResponse,
  ListFlowsResponse,
  QueueFlowRecord,
  WebhookFlowRecord,
} from './api';

describe('API contract types', () => {
  it('accepts structured API errors with optional step and details', () => {
    const response: ApiErrorResponse = {
      error: {
        code: 'internal_error',
        message: 'Failed to activate deployment.',
        step: 'activating',
        details: {
          requestId: 'req_123',
        },
      },
    };

    expect(response.error.code).toBe('internal_error');
    expect(response.error.step).toBe('activating');
  });

  it('accepts webhook flow records in flow management responses', () => {
    const flow: WebhookFlowRecord = {
      id: '402c04b0-62c8-4d0b-942f-0ee2329436a8',
      name: 'hello',
      trigger: 'webhook',
      status: 'ready',
      createdAt: '2026-04-21T10:00:00.000Z',
      endpoint: 'https://trigora.dev/f/402c04b0-62c8-4d0b-942f-0ee2329436a8',
      route: '/hello',
    };

    const response: GetFlowResponse = {
      flow,
    };

    expect(response.flow.trigger).toBe('webhook');
    if (response.flow.trigger !== 'webhook') {
      throw new Error('Expected webhook flow');
    }
    expect(response.flow.endpoint).toContain('trigora.dev');
  });

  it('accepts cron and queue flow records in flow lists', () => {
    const cronFlow: CronFlowRecord = {
      id: '8a4c04b0-62c8-4d0b-942f-0ee2329436b9',
      name: 'nightly-sync',
      trigger: 'cron',
      status: 'ready',
      createdAt: '2026-04-21T11:00:00.000Z',
      schedule: '0 2 * * *',
    };

    const queueFlow: QueueFlowRecord = {
      id: '9b5d04b0-62c8-4d0b-942f-0ee2329436c0',
      name: 'orders-processor',
      trigger: 'queue',
      status: 'disabled',
      createdAt: '2026-04-21T12:00:00.000Z',
      queue: 'orders',
    };

    const response: ListFlowsResponse = {
      flows: [cronFlow, queueFlow],
    };

    expect(response.flows).toHaveLength(2);
    expect(response.flows[0]?.trigger).toBe('cron');
    expect(response.flows[1]?.trigger).toBe('queue');
  });

  it('accepts disable flow responses', () => {
    const response: FlowStatusResponse = {
      ok: true,
      flow: {
        id: '402c04b0-62c8-4d0b-942f-0ee2329436a8',
        status: 'disabled',
      },
    };

    expect(response.ok).toBe(true);
    expect(response.flow.status).toBe('disabled');
  });
});
