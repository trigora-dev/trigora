import { describe, expect, it, vi } from 'vitest';

import { createDeployApiClient, TRIGORA_API_BASE_URL } from './createDeployApiClient';

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
          message: 'Invalid deploy token.',
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

    await expect(client.createDeployment({ manifest, artifact })).rejects.toThrow(
      'Trigora deploy API request failed: Invalid deploy token.',
    );
  });

  it('wraps network failures with a helpful message', async () => {
    const fetch = vi.fn().mockRejectedValue(new Error('connect ECONNREFUSED'));

    const client = createDeployApiClient({
      token: 'secret-token',
      fetch,
    });

    await expect(client.createDeployment({ manifest, artifact })).rejects.toThrow(
      'Failed to reach the Trigora deploy API: connect ECONNREFUSED',
    );
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

    await expect(client.createDeployment({ manifest, artifact })).rejects.toThrow(
      'Internal server error.',
    );
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
});
