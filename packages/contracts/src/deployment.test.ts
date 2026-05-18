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
      flow: {
        id: 'hello',
        entrypoint: 'flows/hello.ts',
        trigger: { type: 'webhook', event: 'orders.created' },
      },
    };

    expect(manifest.version).toBe(1);
    expect(manifest.flow.id).toBe('hello');
  });

  it('accepts cron deployment manifests without route paths', () => {
    const manifest: DeploymentManifest = {
      version: 1,
      flow: {
        id: 'nightly-sync',
        entrypoint: 'flows/nightly-sync.ts',
        trigger: { type: 'cron', cron: '0 2 * * *' },
      },
    };

    expect(manifest.flow.trigger.type).toBe('cron');
    if (manifest.flow.trigger.type !== 'cron') {
      throw new Error('Expected cron manifest flow');
    }
    expect(manifest.flow.trigger.cron).toBe('0 2 * * *');
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
        flow: {
          id: 'hello',
          entrypoint: 'flows/hello.ts',
          trigger: { type: 'webhook' },
        },
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
        slug: 'hello',
        trigger: 'webhook',
        status: 'ready',
        url: 'https://acme.trigora.dev/hello',
      },
    ];

    const response: CreateDeploymentResponse = {
      id: 'dep_123',
      status: 'active',
      manifestVersion: 1,
      manifestJson: {
        version: 1,
        flow: {
          id: 'hello',
          entrypoint: 'flows/hello.ts',
          trigger: { type: 'webhook' },
        },
      },
      flow: deployedFlows[0]!,
      createdAt: '2026-04-12T00:00:00.000Z',
      updatedAt: '2026-04-12T00:00:00.000Z',
    };

    expect(request.artifact.files).toHaveLength(1);
    expect(response.status).toBe('active');
    expect(response.flow.url).toBe('https://acme.trigora.dev/hello');
  });

  it('accepts the canonical deployed flow response type', () => {
    const deployedFlow: DeployedFlowResponse = {
      id: 'df_123',
      slug: 'hello',
      trigger: 'webhook',
      status: 'ready',
      url: 'https://acme.trigora.dev/hello',
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
        flow: {
          id: 'nightly-sync',
          entrypoint: 'flows/nightly-sync.ts',
          trigger: { type: 'cron', cron: '0 2 * * *' },
        },
      },
      flow: {
        id: 'df_456',
        slug: 'nightly-sync',
        trigger: 'cron',
        schedule: '0 2 * * *',
        timezone: 'UTC',
        status: 'ready',
        url: null,
      },
      createdAt: '2026-04-12T00:00:00.000Z',
      updatedAt: '2026-04-12T00:00:00.000Z',
    };

    expect(response.flow.trigger).toBe('cron');
    if (response.flow.trigger !== 'cron') {
      throw new Error('Expected cron deployed flow');
    }
    expect(response.flow.schedule).toBe('0 2 * * *');
    expect(response.flow.timezone).toBe('UTC');
  });
});
