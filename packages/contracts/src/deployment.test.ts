import { describe, expect, it } from 'vitest';

import type {
  CreateDeploymentRequest,
  CreateDeploymentResponse,
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

    const response: CreateDeploymentResponse = {
      deploymentId: 'dep_123',
      status: 'pending',
      dashboardUrl: 'https://app.trigora.dev/deployments/dep_123',
    };

    expect(request.artifact.files).toHaveLength(1);
    expect(response.status).toBe('pending');
  });
});
