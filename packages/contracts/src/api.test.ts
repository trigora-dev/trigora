import { describe, expect, it } from 'vitest';

import type {
  ApiErrorResponse,
  CronFlowRecord,
  DeleteFlowSecretResponse,
  FlowInvocationLogRecord,
  FlowInvocationRecord,
  FlowSecretRecord,
  FlowStatusResponse,
  GetFlowInvocationResponse,
  GetFlowResponse,
  ListFlowInvocationsQuery,
  ListFlowInvocationsResponse,
  ListFlowSecretsResponse,
  ListFlowsResponse,
  QueueFlowRecord,
  SetFlowSecretRequest,
  SetFlowSecretResponse,
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

  it('accepts hosted flow secret contracts', () => {
    const secret: FlowSecretRecord = {
      name: 'STRIPE_WEBHOOK_SECRET',
      createdAt: '2026-05-03T12:00:00.000Z',
      updatedAt: '2026-05-03T12:00:00.000Z',
    };

    const listResponse: ListFlowSecretsResponse = {
      secrets: [secret],
    };

    const setRequest: SetFlowSecretRequest = {
      name: 'STRIPE_WEBHOOK_SECRET',
      value: 'super-secret',
    };

    const setResponse: SetFlowSecretResponse = {
      ok: true,
      secret,
    };

    const deleteResponse: DeleteFlowSecretResponse = {
      ok: true,
      deleted: true,
      name: 'STRIPE_WEBHOOK_SECRET',
    };

    expect(listResponse.secrets[0]?.name).toBe('STRIPE_WEBHOOK_SECRET');
    expect(setRequest.name).toBe('STRIPE_WEBHOOK_SECRET');
    expect(setResponse.secret.updatedAt).toBe('2026-05-03T12:00:00.000Z');
    expect(deleteResponse.deleted).toBe(true);
  });

  it('accepts hosted flow invocation contracts', () => {
    const invocation: FlowInvocationRecord = {
      id: 'inv_123',
      status: 'failed',
      startedAt: '2026-05-05T12:00:00.000Z',
      completedAt: '2026-05-05T12:00:01.250Z',
      durationMs: 1250,
      httpStatus: 500,
      errorCode: 'webhook_verification_failed',
      errorMessage: 'Stripe webhook signature did not match.',
    };

    const log: FlowInvocationLogRecord = {
      sequence: 1,
      level: 'warn',
      message: 'Rejected Stripe webhook',
      timestamp: '2026-05-05T12:00:01.000Z',
      metadata: {
        source: 'stripe',
      },
    };

    const listQuery: ListFlowInvocationsQuery = {
      limit: 20,
      status: 'failed',
    };

    const listResponse: ListFlowInvocationsResponse = {
      invocations: [invocation],
    };

    const detailResponse: GetFlowInvocationResponse = {
      invocation: {
        ...invocation,
        logs: [log],
      },
    };

    expect(listQuery.status).toBe('failed');
    expect(listResponse.invocations[0]?.status).toBe('failed');
    expect(detailResponse.invocation.logs[0]?.level).toBe('warn');
    expect(detailResponse.invocation.logs[0]?.metadata).toEqual({
      source: 'stripe',
    });
  });
});
