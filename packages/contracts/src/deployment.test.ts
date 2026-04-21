import { describe, expect, it } from 'vitest';

import type {
  CreateDeploymentRequest,
  CreateDeploymentResponse,
  DeploymentArtifact,
  DeploymentFlowResponse,
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

    const deployedFlows: DeploymentFlowResponse[] = [
      {
        id: 'df_123',
        flowId: 'hello',
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
});
