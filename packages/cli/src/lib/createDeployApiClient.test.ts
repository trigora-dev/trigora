import type { FlowStatusResponse, WhoAmIResponse } from '@trigora/contracts';
import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  createDeployApiClient,
  DeployApiNetworkError,
  DeployApiRequestError,
  DeployApiResponseError,
  TRIGORA_API_BASE_URL,
} from './createDeployApiClient';

describe('createDeployApiClient', () => {
  const originalEnv = { ...process.env };
  const manifest = {
    version: 1 as const,
    flow: {
      id: 'hello',
      entrypoint: 'flows/hello.ts',
      trigger: { type: 'webhook' as const },
    },
  };
  const artifact = {
    version: 1 as const,
    format: 'esm' as const,
    target: 'node20' as const,
    files: [
      {
        entrypoint: 'flows/hello.ts',
        path: 'flows/hello.mjs',
        contents: `const flow = { id: "hello" }; export { flow as default };`,
      },
    ],
  };
  const managedFlow = {
    id: 'hello',
    slug: 'hello',
    status: 'ready',
    trigger: 'webhook' as const,
    routePath: '/hello',
    endpoint: 'https://acme.trigora.dev/hello',
    createdAt: '2026-04-21T10:00:00.000Z',
  };
  const failedInvocation = {
    id: 'inv_123',
    status: 'failed' as const,
    startedAt: '2026-05-05T10:00:00.000Z',
    completedAt: '2026-05-05T10:00:00.842Z',
    durationMs: 842,
    httpStatus: 400,
    errorCode: 'stripe_signature_invalid',
    errorMessage: 'Invalid Stripe signature.',
  };
  const identity = {
    actorType: 'deploy_token' as const,
    workspace: {
      id: 'ws_123',
      slug: 'acme',
      name: 'Acme',
    },
    token: {
      id: 'tok_123',
      label: 'local-dev',
      status: 'active',
      createdAt: '2026-05-17T00:00:00.000Z',
    },
  } satisfies WhoAmIResponse;

  afterEach(() => {
    process.env = { ...originalEnv };
  });

  it('posts the deployment manifest to the default deploy API', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          id: 'dep_123',
          status: 'active',
          manifestVersion: 1,
          manifestJson: manifest,
          flow: {
            id: 'df_123',
            slug: 'hello',
            trigger: 'webhook',
            routePath: '/hello',
            status: 'ready',
            url: 'https://acme.trigora.dev/hello',
          },
          createdAt: '2026-04-12T00:00:00.000Z',
          updatedAt: '2026-04-12T00:00:00.000Z',
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.createDeployment({ manifest, artifact })).resolves.toEqual({
      id: 'dep_123',
      status: 'active',
      manifestVersion: 1,
      manifestJson: manifest,
      flow: {
        id: 'df_123',
        slug: 'hello',
        trigger: 'webhook',
        routePath: '/hello',
        status: 'ready',
        url: 'https://acme.trigora.dev/hello',
      },
      createdAt: '2026-04-12T00:00:00.000Z',
      updatedAt: '2026-04-12T00:00:00.000Z',
    });

    expect(fetch).toHaveBeenCalledWith(`${TRIGORA_API_BASE_URL}/v1/deployments`, {
      method: 'POST',
      headers: {
        Authorization: 'Bearer secret-token',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ manifest, artifact }),
    });
  });

  it('surfaces API error messages', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 401,
      async json() {
        return {
          message: 'Deploy token is invalid or no longer active.',
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'bad-token',
      fetch,
    });

    await expect(client.createDeployment({ manifest, artifact })).rejects.toMatchObject({
      name: 'DeployApiRequestError',
      code: 'unauthorized',
      message: 'Deploy token is invalid or no longer active.',
      status: 401,
    } satisfies Partial<DeployApiRequestError>);
  });

  it('reads nested backend error payloads', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 401,
      async json() {
        return {
          error: {
            code: 'unauthorized',
            message: 'A valid deploy token is required.',
          },
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'bad-token',
      fetch,
    });

    await expect(client.createDeployment({ manifest, artifact })).rejects.toMatchObject({
      name: 'DeployApiRequestError',
      code: 'unauthorized',
      message: 'A valid deploy token is required.',
      status: 401,
    } satisfies Partial<DeployApiRequestError>);
  });

  it('reads structured deploy steps from nested backend errors', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 500,
      async json() {
        return {
          error: {
            code: 'internal_error',
            message: 'Failed to create worker runtime.',
            step: 'worker_creation',
          },
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.createDeployment({ manifest, artifact })).rejects.toMatchObject({
      name: 'DeployApiRequestError',
      code: 'internal_error',
      message: 'Failed to create worker runtime.',
      status: 500,
      step: 'worker_creation',
    } satisfies Partial<DeployApiRequestError>);
  });

  it('preserves structured cron validation error details', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 400,
      async json() {
        return {
          error: {
            code: 'invalid_cron_expression',
            message: 'Invalid cron expression.',
            details: {
              message: 'Cron expression must contain five fields.',
            },
          },
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.createDeployment({ manifest, artifact })).rejects.toMatchObject({
      name: 'DeployApiRequestError',
      code: 'invalid_cron_expression',
      message: 'Invalid cron expression.',
      details: {
        message: 'Cron expression must contain five fields.',
      },
      status: 400,
    } satisfies Partial<DeployApiRequestError>);
  });

  it('maps empty unauthorized responses to the v1 deploy token message', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 401,
      async json() {
        throw new Error('Unexpected end of JSON input');
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'bad-token',
      fetch,
    });

    await expect(client.createDeployment({ manifest, artifact })).rejects.toMatchObject({
      name: 'DeployApiRequestError',
      code: 'unauthorized',
      message: 'Deploy token is invalid or no longer active.',
      status: 401,
    } satisfies Partial<DeployApiRequestError>);
  });

  it('wraps network failures with a helpful message', async () => {
    const fetch = vi.fn().mockRejectedValue(new Error('connect ECONNREFUSED'));

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.createDeployment({ manifest, artifact })).rejects.toMatchObject({
      name: 'DeployApiNetworkError',
      message: 'connect ECONNREFUSED',
    } satisfies Partial<DeployApiNetworkError>);
  });

  it('normalizes generic fetch failures into a clearer network error', async () => {
    const fetch = vi.fn().mockRejectedValue(new TypeError('Failed to fetch'));

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.createDeployment({ manifest, artifact })).rejects.toMatchObject({
      name: 'DeployApiNetworkError',
      message:
        'Could not reach the Trigora API. Check your network connection or TRIGORA_API_BASE_URL.',
    } satisfies Partial<DeployApiNetworkError>);
  });

  it('throws when the API returns an invalid success payload', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          id: 'dep_123',
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.createDeployment({ manifest, artifact })).rejects.toMatchObject({
      name: 'DeployApiResponseError',
      message: 'Trigora Cloud returned an unexpected response.',
    } satisfies Partial<DeployApiResponseError>);
  });

  it('allows overriding the api base url when needed', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          id: 'dep_123',
          status: 'active',
          manifestVersion: 1,
          manifestJson: manifest,
          flow: {
            id: 'df_123',
            slug: 'hello',
            trigger: 'webhook',
            routePath: '/hello',
            status: 'ready',
            url: 'https://acme.trigora.dev/hello',
          },
          createdAt: '2026-04-12T00:00:00.000Z',
          updatedAt: '2026-04-12T00:00:00.000Z',
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      baseUrl: 'http://localhost:3000/',
      token: 'secret-token',
      fetch,
    });

    await client.createDeployment({ manifest, artifact });

    expect(fetch).toHaveBeenCalledWith('http://localhost:3000/v1/deployments', {
      method: 'POST',
      headers: {
        Authorization: 'Bearer secret-token',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ manifest, artifact }),
    });
  });

  it('uses TRIGORA_API_BASE_URL from the environment when no base url override is provided', async () => {
    process.env.TRIGORA_API_BASE_URL = 'http://localhost:8787/';

    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          flows: [managedFlow],
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.listFlows()).resolves.toEqual([managedFlow]);

    expect(fetch).toHaveBeenCalledWith('http://localhost:8787/v1/flows', {
      method: 'GET',
      headers: {
        Authorization: 'Bearer secret-token',
      },
    });
  });

  it('lists deployed flows from the flows endpoint', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          flows: [
            managedFlow,
            {
              id: '8a4c04b0-62c8-4d0b-942f-0ee2329436b9',
              slug: 'nightly-sync',
              status: 'ready',
              trigger: 'cron',
              createdAt: '2026-04-21T11:00:00.000Z',
              schedule: '0 2 * * *',
              timezone: 'UTC',
            },
          ],
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.listFlows()).resolves.toEqual([
      managedFlow,
      {
        id: '8a4c04b0-62c8-4d0b-942f-0ee2329436b9',
        slug: 'nightly-sync',
        status: 'ready',
        trigger: 'cron',
        createdAt: '2026-04-21T11:00:00.000Z',
        schedule: '0 2 * * *',
        timezone: 'UTC',
      },
    ]);

    expect(fetch).toHaveBeenCalledWith(`${TRIGORA_API_BASE_URL}/v1/flows`, {
      method: 'GET',
      headers: {
        Authorization: 'Bearer secret-token',
      },
    });
  });

  it('accepts cron deployment responses without route paths', async () => {
    const cronManifest = {
      version: 1 as const,
      flow: {
        id: 'nightly',
        entrypoint: 'flows/nightly.ts',
        trigger: { type: 'cron' as const, cron: '0 2 * * *' },
      },
    };
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          id: 'dep_789',
          status: 'active',
          manifestVersion: 1,
          manifestJson: cronManifest,
          flow: {
            id: 'df_789',
            slug: 'nightly',
            trigger: 'cron',
            schedule: '0 2 * * *',
            timezone: 'UTC',
            status: 'ready',
            url: null,
          },
          createdAt: '2026-04-12T00:00:00.000Z',
          updatedAt: '2026-04-12T00:00:00.000Z',
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.createDeployment({ manifest: cronManifest, artifact })).resolves.toEqual({
      id: 'dep_789',
      status: 'active',
      manifestVersion: 1,
      manifestJson: cronManifest,
      flow: {
        id: 'df_789',
        slug: 'nightly',
        trigger: 'cron',
        schedule: '0 2 * * *',
        timezone: 'UTC',
        status: 'ready',
        url: null,
      },
      createdAt: '2026-04-12T00:00:00.000Z',
      updatedAt: '2026-04-12T00:00:00.000Z',
    });
  });

  it('reads a single flow from the inspect endpoint', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          flow: managedFlow,
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.getFlow(managedFlow.slug)).resolves.toEqual(managedFlow);

    expect(fetch).toHaveBeenCalledWith(`${TRIGORA_API_BASE_URL}/v1/flows/${managedFlow.slug}`, {
      method: 'GET',
      headers: {
        Authorization: 'Bearer secret-token',
      },
    });
  });

  it('deletes a hosted flow from the delete endpoint', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          ok: true,
          deleted: true,
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.deleteFlow(managedFlow.slug)).resolves.toEqual({
      deleted: true,
    });

    expect(fetch).toHaveBeenCalledWith(`${TRIGORA_API_BASE_URL}/v1/flows/${managedFlow.slug}`, {
      method: 'DELETE',
      headers: {
        Authorization: 'Bearer secret-token',
      },
    });
  });

  it('reads workspace and token metadata from the whoami endpoint', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return identity;
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.whoAmI()).resolves.toEqual(identity);

    expect(fetch).toHaveBeenCalledWith(`${TRIGORA_API_BASE_URL}/v1/whoami`, {
      method: 'GET',
      headers: {
        Authorization: 'Bearer secret-token',
      },
    });
  });

  it('disables a flow from the disable endpoint', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          ok: true,
          flow: {
            id: managedFlow.id,
            slug: managedFlow.slug,
            trigger: 'webhook',
            status: 'disabled',
            routePath: managedFlow.routePath,
            endpoint: managedFlow.endpoint,
          },
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.disableFlow(managedFlow.slug)).resolves.toEqual({
      id: managedFlow.id,
      slug: managedFlow.slug,
      trigger: 'webhook',
      status: 'disabled',
      routePath: managedFlow.routePath,
      endpoint: managedFlow.endpoint,
    } satisfies FlowStatusResponse['flow']);

    expect(fetch).toHaveBeenCalledWith(
      `${TRIGORA_API_BASE_URL}/v1/flows/${managedFlow.slug}/disable`,
      {
        method: 'POST',
        headers: {
          Authorization: 'Bearer secret-token',
        },
      },
    );
  });

  it('enables a flow from the enable endpoint', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          ok: true,
          flow: {
            id: managedFlow.id,
            slug: managedFlow.slug,
            trigger: 'webhook',
            status: 'ready',
            routePath: managedFlow.routePath,
            endpoint: managedFlow.endpoint,
          },
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.enableFlow(managedFlow.slug)).resolves.toEqual({
      id: managedFlow.id,
      slug: managedFlow.slug,
      trigger: 'webhook',
      status: 'ready',
      routePath: managedFlow.routePath,
      endpoint: managedFlow.endpoint,
    } satisfies FlowStatusResponse['flow']);

    expect(fetch).toHaveBeenCalledWith(
      `${TRIGORA_API_BASE_URL}/v1/flows/${managedFlow.slug}/enable`,
      {
        method: 'POST',
        headers: {
          Authorization: 'Bearer secret-token',
        },
      },
    );
  });

  it('sets a hosted flow secret without leaking the value in the response shape', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          ok: true,
          secret: {
            name: 'STRIPE_WEBHOOK_SECRET',
            createdAt: '2026-05-03T12:00:00.000Z',
            updatedAt: '2026-05-03T12:00:00.000Z',
          },
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(
      client.setFlowSecret(managedFlow.slug, {
        name: 'STRIPE_WEBHOOK_SECRET',
        value: 'super-secret',
      }),
    ).resolves.toEqual({
      name: 'STRIPE_WEBHOOK_SECRET',
      createdAt: '2026-05-03T12:00:00.000Z',
      updatedAt: '2026-05-03T12:00:00.000Z',
    });

    expect(fetch).toHaveBeenCalledWith(`${TRIGORA_API_BASE_URL}/v1/secrets`, {
      method: 'POST',
      headers: {
        Authorization: 'Bearer secret-token',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        name: 'STRIPE_WEBHOOK_SECRET',
        value: 'super-secret',
        flow: managedFlow.slug,
      }),
    });
  });

  it('lists hosted flow secret metadata without returning values', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          secrets: [
            {
              flowSlug: managedFlow.slug,
              name: 'STRIPE_WEBHOOK_SECRET',
              createdAt: '2026-05-03T12:00:00.000Z',
              updatedAt: '2026-05-03T12:00:00.000Z',
            },
            {
              flowSlug: managedFlow.slug,
              name: 'RESEND_API_KEY',
              createdAt: '2026-05-02T12:00:00.000Z',
              updatedAt: '2026-05-02T15:00:00.000Z',
            },
          ],
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.listFlowSecrets(managedFlow.slug)).resolves.toEqual([
      {
        flowSlug: managedFlow.slug,
        name: 'STRIPE_WEBHOOK_SECRET',
        createdAt: '2026-05-03T12:00:00.000Z',
        updatedAt: '2026-05-03T12:00:00.000Z',
      },
      {
        flowSlug: managedFlow.slug,
        name: 'RESEND_API_KEY',
        createdAt: '2026-05-02T12:00:00.000Z',
        updatedAt: '2026-05-02T15:00:00.000Z',
      },
    ]);

    expect(fetch).toHaveBeenCalledWith(
      `${TRIGORA_API_BASE_URL}/v1/secrets?flow=${managedFlow.slug}`,
      {
        method: 'GET',
        headers: {
          Authorization: 'Bearer secret-token',
        },
      },
    );
  });

  it('lists invocations from the global invocations endpoint for a flow filter', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          invocations: [
            {
              ...failedInvocation,
              flowSlug: managedFlow.slug,
            },
            {
              flowSlug: managedFlow.slug,
              id: 'inv_124',
              status: 'succeeded',
              startedAt: '2026-05-05T09:00:00.000Z',
              completedAt: '2026-05-05T09:00:00.210Z',
              durationMs: 210,
              httpStatus: 200,
              errorCode: null,
              errorMessage: null,
            },
          ],
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.listInvocations({ flow: managedFlow.slug })).resolves.toEqual([
      {
        ...failedInvocation,
        flowSlug: managedFlow.slug,
      },
      {
        flowSlug: managedFlow.slug,
        id: 'inv_124',
        status: 'succeeded',
        startedAt: '2026-05-05T09:00:00.000Z',
        completedAt: '2026-05-05T09:00:00.210Z',
        durationMs: 210,
        httpStatus: 200,
        errorCode: null,
        errorMessage: null,
      },
    ]);

    expect(fetch).toHaveBeenCalledWith(
      `${TRIGORA_API_BASE_URL}/v1/invocations?flow=${managedFlow.slug}`,
      {
        method: 'GET',
        headers: {
          Authorization: 'Bearer secret-token',
        },
      },
    );
  });

  it('lists invocations from the global invocations endpoint with filters', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          invocations: [
            {
              ...failedInvocation,
              flowSlug: managedFlow.slug,
            },
          ],
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(
      client.listInvocations({
        flow: managedFlow.slug,
        limit: 20,
        range: '7d',
        status: 'failed',
      }),
    ).resolves.toEqual([
      {
        ...failedInvocation,
        flowSlug: managedFlow.slug,
      },
    ]);

    expect(fetch).toHaveBeenCalledWith(
      `${TRIGORA_API_BASE_URL}/v1/invocations?flow=${managedFlow.slug}&limit=20&range=7d&status=failed`,
      {
        method: 'GET',
        headers: {
          Authorization: 'Bearer secret-token',
        },
      },
    );
  });

  it('reads a single invocation with ordered logs', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          invocation: {
            ...failedInvocation,
            flowSlug: managedFlow.slug,
            triggerType: 'webhook',
            logs: [
              {
                sequence: 1,
                level: 'info',
                message: 'Received webhook event',
                timestamp: '2026-05-05T10:00:00.100Z',
                metadata: {
                  eventType: 'checkout.session.completed',
                },
              },
              {
                sequence: 2,
                level: 'error',
                message: 'Signature verification failed',
                timestamp: '2026-05-05T10:00:00.200Z',
                metadata: null,
              },
            ],
          },
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.getInvocation(failedInvocation.id)).resolves.toEqual({
      ...failedInvocation,
      flowSlug: managedFlow.slug,
      triggerType: 'webhook',
      logs: [
        {
          sequence: 1,
          level: 'info',
          message: 'Received webhook event',
          timestamp: '2026-05-05T10:00:00.100Z',
          metadata: {
            eventType: 'checkout.session.completed',
          },
        },
        {
          sequence: 2,
          level: 'error',
          message: 'Signature verification failed',
          timestamp: '2026-05-05T10:00:00.200Z',
          metadata: null,
        },
      ],
    });

    expect(fetch).toHaveBeenCalledWith(
      `${TRIGORA_API_BASE_URL}/v1/invocations/${failedInvocation.id}`,
      {
        method: 'GET',
        headers: {
          Authorization: 'Bearer secret-token',
        },
      },
    );
  });

  it('deletes a hosted flow secret by name', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          ok: true,
          deleted: true,
          name: 'STRIPE_WEBHOOK_SECRET',
        };
      },
      async text() {
        return '';
      },
    });

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(
      client.deleteFlowSecret(managedFlow.slug, 'STRIPE_WEBHOOK_SECRET'),
    ).resolves.toEqual({
      ok: true,
      deleted: true,
      name: 'STRIPE_WEBHOOK_SECRET',
    });

    expect(fetch).toHaveBeenCalledWith(
      `${TRIGORA_API_BASE_URL}/v1/secrets/STRIPE_WEBHOOK_SECRET?flow=${managedFlow.slug}`,
      {
        method: 'DELETE',
        headers: {
          Authorization: 'Bearer secret-token',
        },
      },
    );
  });
});
