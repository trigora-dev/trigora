import type { DisableFlowResponse } from '@trigora/contracts';
import { describe, expect, it, vi } from 'vitest';

import {
  createDeployApiClient,
  DeployApiNetworkError,
  DeployApiRequestError,
  DeployApiResponseError,
  TRIGORA_API_BASE_URL,
} from './createDeployApiClient';

describe('createDeployApiClient', () => {
  const manifest = {
    version: 1 as const,
    flows: [
      {
        id: 'hello',
        entrypoint: 'flows/hello.ts',
        routePath: '/hello',
        trigger: { type: 'webhook' as const },
      },
    ],
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
    id: '402c04b0-62c8-4d0b-942f-0ee2329436a8',
    name: 'hello',
    status: 'ready',
    trigger: 'webhook' as const,
    route: '/hello',
    endpoint: 'https://trigora.dev/f/402c04b0-62c8-4d0b-942f-0ee2329436a8',
    createdAt: '2026-04-21T10:00:00.000Z',
  };

  it('posts the deployment manifest to the default deploy API', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          id: 'dep_123',
          status: 'pending',
          manifestVersion: 1,
          manifestJson: manifest,
          flowCount: 1,
          baseUrl: 'https://deploy.trigora.dev',
          url: 'https://trigora.dev/f/df_123',
          flows: [
            {
              id: 'df_123',
              flowId: 'hello',
              routePath: '/hello',
              status: 'pending',
              url: 'https://trigora.dev/f/df_123',
            },
          ],
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
      status: 'pending',
      manifestVersion: 1,
      manifestJson: manifest,
      flowCount: 1,
      baseUrl: 'https://deploy.trigora.dev',
      url: 'https://trigora.dev/f/df_123',
      flows: [
        {
          id: 'df_123',
          flowId: 'hello',
          routePath: '/hello',
          status: 'pending',
          url: 'https://trigora.dev/f/df_123',
        },
      ],
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
          status: 'pending',
          manifestVersion: 1,
          manifestJson: manifest,
          flowCount: 1,
          baseUrl: null,
          url: null,
          flows: [
            {
              id: 'df_123',
              flowId: 'hello',
              routePath: '/hello',
              status: 'pending',
              url: null,
            },
          ],
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
              name: 'nightly-sync',
              status: 'ready',
              trigger: 'cron',
              createdAt: '2026-04-21T11:00:00.000Z',
              schedule: '0 2 * * *',
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
        name: 'nightly-sync',
        status: 'ready',
        trigger: 'cron',
        createdAt: '2026-04-21T11:00:00.000Z',
        schedule: '0 2 * * *',
      },
    ]);

    expect(fetch).toHaveBeenCalledWith(`${TRIGORA_API_BASE_URL}/v1/flows`, {
      method: 'GET',
      headers: {
        Authorization: 'Bearer secret-token',
      },
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

    await expect(client.getFlow(managedFlow.id)).resolves.toEqual(managedFlow);

    expect(fetch).toHaveBeenCalledWith(`${TRIGORA_API_BASE_URL}/v1/flows/${managedFlow.id}`, {
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
            status: 'disabled',
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

    await expect(client.disableFlow(managedFlow.id)).resolves.toEqual({
      id: managedFlow.id,
      status: 'disabled',
    } satisfies DisableFlowResponse['flow']);

    expect(fetch).toHaveBeenCalledWith(
      `${TRIGORA_API_BASE_URL}/v1/flows/${managedFlow.id}/disable`,
      {
        method: 'POST',
        headers: {
          Authorization: 'Bearer secret-token',
        },
      },
    );
  });
});
