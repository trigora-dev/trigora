import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  createDeployApiClient,
  type DeployApiClient,
  DeployApiNetworkError,
  DeployApiRequestError,
  DeployApiResponseError,
} from '../lib/createDeployApiClient';
import { deployCommand } from './deploy';

vi.mock('../lib/createDeployApiClient', async () => {
  const actual = await vi.importActual<typeof import('../lib/createDeployApiClient')>(
    '../lib/createDeployApiClient',
  );

  return {
    ...actual,
    createDeployApiClient: vi.fn(),
  };
});

const originalCwd = process.cwd();
const originalConsoleLog = console.log;
const originalEnv = { ...process.env };

const tempDirs: string[] = [];
const mockedCreateDeployApiClient = vi.mocked(createDeployApiClient);

function createMockApiClient(overrides: Partial<DeployApiClient> = {}): DeployApiClient {
  return {
    createDeployment: vi.fn().mockResolvedValue({
      id: 'dep_123',
      status: 'active',
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
      flows: [
        {
          id: 'df_123',
          flowId: 'hello',
          routePath: '/hello',
          status: 'active',
          url: 'https://trigora.dev/f/df_123',
        },
      ],
      createdAt: '2026-04-12T00:00:00.000Z',
      updatedAt: '2026-04-12T00:00:00.000Z',
    }),
    disableFlow: vi.fn(),
    getFlow: vi.fn(),
    listFlows: vi.fn(),
    ...overrides,
  };
}

async function makeTempDir() {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'trigora-deploy-command-'));
  tempDirs.push(dir);
  return dir;
}

beforeEach(() => {
  console.log = vi.fn();
  process.env = {
    ...originalEnv,
    TRIGORA_DEPLOY_TOKEN: 'secret-token',
  };
  mockedCreateDeployApiClient.mockReset();
  mockedCreateDeployApiClient.mockReturnValue(createMockApiClient());
});

afterEach(async () => {
  process.chdir(originalCwd);
  console.log = originalConsoleLog;
  process.env = originalEnv;

  await Promise.all(
    tempDirs.splice(0).map(async (dir) => {
      await fs.rm(dir, { recursive: true, force: true });
    }),
  );
});

describe('deployCommand', () => {
  it('loads and summarizes a specific webhook flow', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    const manifest = await deployCommand({
      filePath: flowPath,
    });

    expect(manifest).toEqual({
      version: 1,
      flows: [
        {
          id: 'hello',
          entrypoint: 'flows/hello.ts',
          routePath: '/hello',
          trigger: { type: 'webhook' },
        },
      ],
    });

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Validating flow modules/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Prepared 1 flow for deployment/),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Building deployment artifact/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Uploading deployment package/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Activating deployment/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Deployment complete/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Flow\s+hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Trigger\s+webhook/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Route\s+\/hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Endpoint/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/https:\/\/trigora\.dev\/f\/df_123/),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Ready to receive events/));
  });

  it('discovers and summarizes all webhook flows in the flows directory', async () => {
    const tempDir = await makeTempDir();
    const helloPath = path.join(tempDir, 'flows', 'hello.ts');
    const ordersPath = path.join(tempDir, 'flows', 'nested', 'orders.ts');
    const createDeployment = vi.fn().mockResolvedValue({
      id: 'dep_123',
      status: 'active',
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
          {
            id: 'orders',
            entrypoint: 'flows/nested/orders.ts',
            routePath: '/orders',
            trigger: { type: 'webhook', event: 'orders.created' },
          },
        ],
      },
      flowCount: 2,
      baseUrl: 'https://deploy.trigora.dev',
      url: null,
      flows: [
        {
          id: 'df_123',
          flowId: 'hello',
          routePath: '/hello',
          status: 'active',
          url: 'https://trigora.dev/f/df_123',
        },
        {
          id: 'df_456',
          flowId: 'orders',
          routePath: '/orders',
          status: 'active',
          url: 'https://trigora.dev/f/df_456',
        },
      ],
      createdAt: '2026-04-12T00:00:00.000Z',
      updatedAt: '2026-04-12T00:00:00.000Z',
    });

    mockedCreateDeployApiClient.mockReturnValue(createMockApiClient({ createDeployment }));

    await fs.mkdir(path.dirname(ordersPath), { recursive: true });
    await fs.writeFile(
      helloPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );
    await fs.writeFile(
      ordersPath,
      `
        export default {
          id: 'orders',
          trigger: { type: 'webhook', event: 'orders.created' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    const manifest = await deployCommand({});

    expect(manifest).toEqual({
      version: 1,
      flows: [
        {
          id: 'hello',
          entrypoint: 'flows/hello.ts',
          routePath: '/hello',
          trigger: { type: 'webhook' },
        },
        {
          id: 'orders',
          entrypoint: 'flows/nested/orders.ts',
          routePath: '/orders',
          trigger: { type: 'webhook', event: 'orders.created' },
        },
      ],
    });

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Prepared 2 flows for deployment/),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Deployment complete/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Deployment\s+dep_123/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Flows\s+2/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Activated flows/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/1\. hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Trigger\s+webhook/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Route\s+\/hello/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Endpoint\s+https:\/\/trigora\.dev\/f\/df_123/),
    );
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Status\s+Ready to receive events/),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/2\. orders/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Trigger\s+webhook:orders\.created/),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Route\s+\/orders/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Endpoint\s+https:\/\/trigora\.dev\/f\/df_456/),
    );
  });

  it('throws a helpful error when no flows exist', async () => {
    const tempDir = await makeTempDir();
    process.chdir(tempDir);

    await expect(deployCommand({})).rejects.toThrow(
      'No flow files found in "flows". Create one with "trigora init" or pass a specific flow to "trigora deploy <flow>".',
    );
  });

  it('throws a helpful error when a flow uses an unsupported trigger', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'manual' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(
      deployCommand({
        filePath: flowPath,
      }),
    ).rejects.toThrow(
      'Flow "hello" in "flows/hello.ts" uses unsupported trigger "manual". trigora deploy currently supports only webhook-triggered flows.',
    );
  });

  it('throws a helpful error when duplicate flow ids are found', async () => {
    const tempDir = await makeTempDir();
    const firstPath = path.join(tempDir, 'flows', 'hello.ts');
    const secondPath = path.join(tempDir, 'flows', 'nested', 'hello-copy.ts');

    await fs.mkdir(path.dirname(secondPath), { recursive: true });
    await fs.writeFile(
      firstPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );
    await fs.writeFile(
      secondPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(deployCommand({})).rejects.toThrow(
      'Duplicate flow id "hello" found in "flows/hello.ts" and "flows/nested/hello-copy.ts". Flow ids must be unique for deployment.',
    );
  });

  it('sends the deployment package to the Trigora deploy api', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');
    const createDeployment = vi.fn().mockResolvedValue({
      id: 'dep_123',
      status: 'active',
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
      flows: [
        {
          id: 'df_123',
          flowId: 'hello',
          routePath: '/hello',
          status: 'active',
          url: 'https://trigora.dev/f/df_123',
        },
      ],
      createdAt: '2026-04-12T00:00:00.000Z',
      updatedAt: '2026-04-12T00:00:00.000Z',
    });

    mockedCreateDeployApiClient.mockReturnValue(createMockApiClient({ createDeployment }));

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    const manifest = await deployCommand({
      filePath: flowPath,
    });

    expect(mockedCreateDeployApiClient).toHaveBeenCalledWith({
      token: 'secret-token',
    });
    expect(createDeployment).toHaveBeenCalledWith({
      manifest,
      artifact: {
        version: 1,
        format: 'esm',
        target: 'node20',
        files: [
          {
            entrypoint: 'flows/hello.ts',
            path: 'flows/hello.mjs',
            contents: expect.any(String),
          },
        ],
      },
    });
  });

  it('throws a helpful error when the deploy token is missing', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );

    delete process.env.TRIGORA_DEPLOY_TOKEN;
    process.chdir(tempDir);

    await expect(
      deployCommand({
        filePath: flowPath,
      }),
    ).rejects.toThrow('TRIGORA_DEPLOY_TOKEN is not set.');
  });

  it('uses the v1 token error reason for invalid or revoked deploy tokens', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');
    const createDeployment = vi.fn().mockRejectedValue(
      new DeployApiRequestError(
        {
          code: 'forbidden',
          message: 'This deploy token is no longer active.',
        },
        403,
      ),
    );

    mockedCreateDeployApiClient.mockReturnValue(createMockApiClient({ createDeployment }));

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(
      deployCommand({
        filePath: flowPath,
      }),
    ).rejects.toThrow('Deploy token is invalid or no longer active.');
  });

  it('normalizes nested unauthorized token messages to the v1 reason', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');
    const createDeployment = vi.fn().mockRejectedValue(
      new DeployApiRequestError(
        {
          code: 'unauthorized',
          message: 'A valid deploy token is required.',
        },
        401,
      ),
    );

    mockedCreateDeployApiClient.mockReturnValue(createMockApiClient({ createDeployment }));

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(
      deployCommand({
        filePath: flowPath,
      }),
    ).rejects.toThrow('Deploy token is invalid or no longer active.');
  });

  it('uses the structured worker_creation step for deploy failures', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');
    const createDeployment = vi.fn().mockRejectedValue(
      new DeployApiRequestError(
        {
          code: 'internal_error',
          message: 'Failed to create worker runtime.',
          step: 'worker_creation',
        },
        500,
      ),
    );

    mockedCreateDeployApiClient.mockReturnValue(createMockApiClient({ createDeployment }));

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(
      deployCommand({
        filePath: flowPath,
      }),
    ).rejects.toMatchObject({
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Creating worker runtime' }),
        expect.objectContaining({ label: 'Reason', value: 'Failed to create worker runtime.' }),
      ]),
    });
  });

  it('uses the structured activating step for deploy failures', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');
    const createDeployment = vi.fn().mockRejectedValue(
      new DeployApiRequestError(
        {
          code: 'internal_error',
          message: 'Failed to activate deployment.',
          step: 'activating',
        },
        500,
      ),
    );

    mockedCreateDeployApiClient.mockReturnValue(createMockApiClient({ createDeployment }));

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(
      deployCommand({
        filePath: flowPath,
      }),
    ).rejects.toMatchObject({
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Activating deployment' }),
        expect.objectContaining({ label: 'Reason', value: 'Failed to activate deployment.' }),
      ]),
    });
  });

  it('shows a network-specific deploy error when the API cannot be reached', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');
    const createDeployment = vi
      .fn()
      .mockRejectedValue(new DeployApiNetworkError('connect ECONNREFUSED'));

    mockedCreateDeployApiClient.mockReturnValue(createMockApiClient({ createDeployment }));

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(
      deployCommand({
        filePath: flowPath,
      }),
    ).rejects.toThrow('connect ECONNREFUSED');
  });

  it('shows an unexpected-response error when the deploy API returns an invalid success payload', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');
    const createDeployment = vi.fn().mockRejectedValue(new DeployApiResponseError());

    mockedCreateDeployApiClient.mockReturnValue(createMockApiClient({ createDeployment }));

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'hello',
          trigger: { type: 'webhook' },
          async run() {}
        };
      `,
      'utf-8',
    );

    process.chdir(tempDir);

    await expect(
      deployCommand({
        filePath: flowPath,
      }),
    ).rejects.toThrow('Trigora Cloud returned an unexpected response.');
  });
});
