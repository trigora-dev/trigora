import { describe, expect, it } from 'vitest';

import type {
  ApiErrorResponse,
  CreateBillingCheckoutRequest,
  CreateBillingCheckoutResponse,
  CreateBillingPortalResponse,
  CreateWorkspaceDeployTokenResponse,
  CreateWorkspaceRequest,
  CronFlowRecord,
  CurrentWorkspaceResponse,
  DeleteFlowSecretResponse,
  DeleteFlowResponse,
  FlowInvocationLogRecord,
  FlowInvocationRecord,
  FlowSecretRecord,
  FlowStatusResponse,
  GetUsageResponse,
  GetInvocationResponse,
  GetFlowResponse,
  InvocationExecutionContext,
  ListFlowInvocationsQuery,
  ListInvocationsResponse,
  ListSecretsQuery,
  ListSecretsResponse,
  ListFlowsResponse,
  ListWorkspaceDeployTokensResponse,
  ListWorkspacesResponse,
  QueueFlowRecord,
  SetFlowSecretRequest,
  SetFlowSecretResponse,
  UpdateWorkspaceRequest,
  WebhookFlowRecord,
  WhoAmIResponse,
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
      id: 'hello',
      slug: 'hello',
      trigger: 'webhook',
      status: 'ready',
      createdAt: '2026-04-21T10:00:00.000Z',
      routePath: '/hello',
      endpoint: 'https://acme.trigora.dev/hello',
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
      id: 'nightly-sync',
      slug: 'nightly-sync',
      trigger: 'cron',
      status: 'ready',
      createdAt: '2026-04-21T11:00:00.000Z',
      schedule: '0 2 * * *',
      timezone: 'UTC',
    };

    const queueFlow: QueueFlowRecord = {
      id: 'orders-processor',
      slug: 'orders-processor',
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
    if (response.flows[0]?.trigger !== 'cron') {
      throw new Error('Expected cron flow');
    }
    expect(response.flows[0].timezone).toBe('UTC');
  });

  it('accepts disable flow responses', () => {
    const response: FlowStatusResponse = {
      ok: true,
      flow: {
        id: 'hello',
        slug: 'hello',
        trigger: 'webhook',
        status: 'disabled',
        routePath: '/hello',
        endpoint: 'https://acme.trigora.dev/hello',
      },
    };

    expect(response.ok).toBe(true);
    expect(response.flow.status).toBe('disabled');
    if (response.flow.trigger !== 'webhook') {
      throw new Error('Expected webhook flow status response');
    }
    expect(response.flow.routePath).toBe('/hello');
  });

  it('accepts delete flow responses', () => {
    const response: DeleteFlowResponse = {
      deleted: true,
    };

    expect(response.deleted).toBe(true);
  });

  it('accepts deploy-token whoami responses', () => {
    const response: WhoAmIResponse = {
      actorType: 'deploy_token',
      workspace: {
        id: 'ws_123',
        name: 'Acme',
        plan: 'pro',
        planStatus: 'active',
        slug: 'acme',
      },
      token: {
        id: 'tok_123',
        label: 'local-dev',
        status: 'active',
        createdAt: '2026-05-17T00:00:00.000Z',
      },
    };

    expect(response.workspace.slug).toBe('acme');
    if (response.actorType !== 'deploy_token') {
      throw new Error('Expected deploy token identity');
    }
    expect(response.token.label).toBe('local-dev');
  });

  it('accepts user whoami and workspace management contracts', () => {
    const whoAmI: WhoAmIResponse = {
      actorType: 'user',
      user: {
        id: 'user_123',
        email: 'user@example.com',
        emailVerified: true,
        image: null,
        name: 'Omar',
      },
      workspace: {
        id: 'ws_123',
        name: 'Acme',
        plan: 'pro',
        planStatus: 'active',
        slug: 'acme',
        role: 'owner',
      },
    };

    const createRequest: CreateWorkspaceRequest = {
      name: 'Acme',
      slug: 'acme',
    };

    const updateRequest: UpdateWorkspaceRequest = {
      name: 'Acme Inc.',
      slug: 'acme',
    };

    const currentWorkspace: CurrentWorkspaceResponse = {
      user: whoAmI.user,
      workspace: whoAmI.workspace,
      activity: {
        createdAt: '2026-05-18T00:00:00.000Z',
        updatedAt: '2026-05-18T01:00:00.000Z',
        flowCount: 3,
        tokenCount: 2,
      },
      stats: {
        recentInvocationCount: 8,
      },
      flows: [],
    };

    const workspaces: ListWorkspacesResponse = {
      workspaces: [
        {
          ...whoAmI.workspace,
          isCurrent: true,
        },
      ],
    };

    expect(whoAmI.workspace.role).toBe('owner');
    expect(createRequest.slug).toBe('acme');
    expect(updateRequest.name).toBe('Acme Inc.');
    expect(currentWorkspace.activity?.flowCount).toBe(3);
    expect(workspaces.workspaces[0]?.isCurrent).toBe(true);
  });

  it('accepts hosted flow secret contracts', () => {
    const secret: FlowSecretRecord = {
      name: 'STRIPE_WEBHOOK_SECRET',
      createdAt: '2026-05-03T12:00:00.000Z',
      updatedAt: '2026-05-03T12:00:00.000Z',
    };

    const listResponse: ListSecretsResponse = {
      secrets: [
        {
          ...secret,
          flowSlug: 'stripe-checkout',
        },
      ],
    };

    const listQuery: ListSecretsQuery = {
      flow: 'stripe-checkout',
    };

    const setRequest: SetFlowSecretRequest = {
      flow: 'stripe-checkout',
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
    expect(listResponse.secrets[0]?.flowSlug).toBe('stripe-checkout');
    expect(listQuery.flow).toBe('stripe-checkout');
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
    const executionContext: InvocationExecutionContext = {
      attempt: 1,
      deploymentId: 'dep_123',
      flowSlug: 'stripe-checkout',
      invocationId: invocation.id,
      trigger: {
        type: 'webhook',
        endpoint: 'https://acme.trigora.dev/stripe-checkout',
        routePath: '/stripe-checkout',
      },
      triggerType: 'webhook',
      workspaceSlug: 'acme',
    };

    const listQuery: ListFlowInvocationsQuery = {
      flow: 'stripe-checkout',
      limit: 20,
      range: '7d',
      status: 'failed',
    };

    const listResponse: ListInvocationsResponse = {
      invocations: [
        {
          ...invocation,
          flowSlug: 'stripe-checkout',
        },
      ],
    };

    const detailResponse: GetInvocationResponse = {
      invocation: {
        ...invocation,
        flowSlug: 'stripe-checkout',
        triggerType: 'webhook',
        executionContext,
        logs: [log],
      },
    };

    expect(listQuery.status).toBe('failed');
    expect(listQuery.range).toBe('7d');
    expect(listResponse.invocations[0]?.status).toBe('failed');
    expect(listResponse.invocations[0]?.flowSlug).toBe('stripe-checkout');
    expect(detailResponse.invocation.triggerType).toBe('webhook');
    expect(detailResponse.invocation.executionContext.trigger).toEqual({
      type: 'webhook',
      endpoint: 'https://acme.trigora.dev/stripe-checkout',
      routePath: '/stripe-checkout',
    });
    expect(detailResponse.invocation.logs[0]?.level).toBe('warn');
    expect(detailResponse.invocation.logs[0]?.metadata).toEqual({
      source: 'stripe',
    });
  });

  it('accepts workspace deploy token contracts', () => {
    const tokens: ListWorkspaceDeployTokensResponse = {
      tokens: [
        {
          id: 'tok_123',
          label: 'local-dev',
          status: 'active',
          lastUsedAt: null,
          createdAt: '2026-05-18T00:00:00.000Z',
        },
      ],
    };

    const createResponse: CreateWorkspaceDeployTokenResponse = {
      ok: true,
      rawToken: 'trg_secret',
      token: tokens.tokens[0]!,
      workspace: {
        id: 'ws_123',
        name: 'Acme',
        plan: 'pro',
        planStatus: 'active',
        slug: 'acme',
        role: 'owner',
      },
    };

    expect(tokens.tokens[0]?.lastUsedAt).toBeNull();
    expect(createResponse.workspace.role).toBe('owner');
    expect(createResponse.workspace.plan).toBe('pro');
  });

  it('accepts usage and billing contracts without public CPU fields', () => {
    const usage: GetUsageResponse = {
      period: {
        month: '2026-05',
        startsAt: '2026-05-01T00:00:00.000Z',
        endsAt: '2026-06-01T00:00:00.000Z',
      },
      plan: {
        name: 'pro',
        status: 'active',
        executionLimit: 100_000,
        logRetentionDays: 30,
        enforcement: 'soft',
      },
      usage: {
        executions: 82_000,
        succeededExecutions: 80_000,
        failedExecutions: 2_000,
        logsWritten: 410_000,
        storedLogRows: 390_000,
        logBytes: 24_000_000,
        activeHostedFlows: 12,
        activeCronSchedules: 4,
        activeDeployTokens: 3,
        secrets: 18,
      },
      warnings: [
        {
          metric: 'executions',
          thresholdPercent: 80,
          current: 82_000,
          limit: 100_000,
        },
      ],
    };

    const checkoutRequest: CreateBillingCheckoutRequest = {
      plan: 'pro',
    };

    const checkoutResponse: CreateBillingCheckoutResponse = {
      url: 'https://billing.stripe.com/session',
    };

    const portalResponse: CreateBillingPortalResponse = {
      url: 'https://billing.stripe.com/portal',
    };

    expect(usage.plan.executionLimit).toBe(100_000);
    expect(usage.usage.failedExecutions).toBe(2_000);
    expect(usage.warnings[0]?.thresholdPercent).toBe(80);
    expect(checkoutRequest.plan).toBe('pro');
    expect(checkoutResponse.url).toContain('billing');
    expect(portalResponse.url).toContain('portal');
  });
});
