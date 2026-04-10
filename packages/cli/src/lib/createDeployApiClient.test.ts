import { describe, expect, it, vi } from 'vitest';

import { createDeployApiClient } from './createDeployApiClient';

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
          deploymentId: 'dep_123',
          status: 'pending',
          dashboardUrl: 'https://app.trigora.dev/deployments/dep_123',
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
      deploymentId: 'dep_123',
      status: 'pending',
      dashboardUrl: 'https://app.trigora.dev/deployments/dep_123',
    });

    expect(fetch).toHaveBeenCalledWith('https://api.trigora.dev/v1/deployments', {
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
      'Trigora deploy API returned an invalid deployment response.',
    );
  });

  it('allows overriding the api base url when needed', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      async json() {
        return {
          deploymentId: 'dep_123',
          status: 'pending',
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
