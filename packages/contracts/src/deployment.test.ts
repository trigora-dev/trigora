import { describe, expect, it } from 'vitest';

import type {
  CreateDeploymentRequest,
  CreateDeploymentResponse,
  DeployedFlowResponse,
  DeploymentArtifact,
  DeploymentManifest,
} from './deployment';

describe('Deployment types', () => {
  it('accepts a deployment manifest', () => {
    const manifest: DeploymentManifest = {
      version: 1,
      flows: [
        {
          id: 'hello',
          entrypoint: 'flows/hello.ts',
          routePath: '/hello',
          trigger: { type: 'webhook', event: 'orders.created' },
        },
      ],
    };

    expect(manifest.version).toBe(1);
    expect(manifest.flows[0]?.routePath).toBe('/hello');
  });

  it('accepts cron deployment manifests without route paths', () => {
    const manifest: DeploymentManifest = {
      version: 1,
      flows: [
        {
          id: 'nightly-sync',
          entrypoint: 'flows/nightly-sync.ts',
          trigger: { type: 'cron', cron: '0 2 * * *' },
        },
      ],
    };

    expect(manifest.flows[0]?.trigger.type).toBe('cron');
    if (manifest.flows[0]?.trigger.type !== 'cron') {
      throw new Error('Expected cron manifest flow');
    }
    expect(manifest.flows[0].trigger.cron).toBe('0 2 * * *');
  });

  it('accepts a bundled deployment artifact', () => {
    const artifact: DeploymentArtifact = {
      version: 1,
      format: 'esm',
      target: 'node20',
      files: [
        {
          entrypoint: 'flows/hello.ts',
          path: 'flows/hello.mjs',
          contents: 'export default {};',
        },
      ],
    };

    expect(artifact.format).toBe('esm');
    expect(artifact.files[0]?.path).toBe('flows/hello.mjs');
  });

  it('accepts a create deployment request and response', () => {
    const request: CreateDeploymentRequest = {
      manifest: {
        version: 1,
        flows: [
          {
            id: 'hello',
            entrypoint: 'flows/hello.ts',
            routePath: '/hello',
            trigger: { type: 'webhook' },
          },
        ],
      },
      artifact: {
        version: 1,
        format: 'esm',
        target: 'node20',
        files: [
          {
            entrypoint: 'flows/hello.ts',
            path: 'flows/hello.mjs',
            contents: 'export default {};',
          },
        ],
      },
    };

    const deployedFlows: DeployedFlowResponse[] = [
      {
        id: 'df_123',
        flowId: 'hello',
        trigger: 'webhook',
        routePath: '/hello',
        status: 'active',
        url: 'https://trigora.dev/f/df_123',
      },
    ];

    const response: CreateDeploymentResponse = {
      id: 'dep_123',
      status: 'pending',
      manifestVersion: 1,
      manifestJson: {
        version: 1,
        flows: [
          {
            id: 'hello',
            entrypoint: 'flows/hello.ts',
            routePath: '/hello',
            trigger: { type: 'webhook' },
          },
        ],
      },
      flowCount: 1,
      baseUrl: 'https://deploy.trigora.dev',
      url: 'https://trigora.dev/f/df_123',
      flows: deployedFlows,
      createdAt: '2026-04-12T00:00:00.000Z',
      updatedAt: '2026-04-12T00:00:00.000Z',
    };

    expect(request.artifact.files).toHaveLength(1);
    expect(response.status).toBe('pending');
    expect(response.flows[0]?.url).toBe('https://trigora.dev/f/df_123');
  });

  it('accepts the canonical deployed flow response type', () => {
    const deployedFlow: DeployedFlowResponse = {
      id: 'df_123',
      flowId: 'hello',
      trigger: 'webhook',
      routePath: '/hello',
      status: 'active',
      url: 'https://trigora.dev/f/df_123',
    };

    expect(deployedFlow.id).toBe('df_123');
  });

  it('accepts cron deployment responses without webhook fields', () => {
    const response: CreateDeploymentResponse = {
      id: 'dep_456',
      status: 'active',
      manifestVersion: 1,
      manifestJson: {
        version: 1,
        flows: [
          {
            id: 'nightly-sync',
            entrypoint: 'flows/nightly-sync.ts',
            trigger: { type: 'cron', cron: '0 2 * * *' },
          },
        ],
      },
      flowCount: 1,
      baseUrl: 'https://deploy.trigora.dev',
      url: null,
      flows: [
        {
          id: 'df_456',
          flowId: 'nightly-sync',
          trigger: 'cron',
          schedule: '0 2 * * *',
          timezone: 'UTC',
          status: 'active',
          url: null,
        },
      ],
      createdAt: '2026-04-12T00:00:00.000Z',
      updatedAt: '2026-04-12T00:00:00.000Z',
    };

    expect(response.flows[0]?.trigger).toBe('cron');
    if (response.flows[0]?.trigger !== 'cron') {
      throw new Error('Expected cron deployed flow');
    }
    expect(response.flows[0].schedule).toBe('0 2 * * *');
    expect(response.flows[0].timezone).toBe('UTC');
  });
});
