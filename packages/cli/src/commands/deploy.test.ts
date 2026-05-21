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
      plan: 'pro',
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
    }),
    deleteFlow: vi.fn(),
    deleteFlowSecret: vi.fn(),
    disableFlow: vi.fn(),
    enableFlow: vi.fn(),
    getFlow: vi.fn(),
    getInvocation: vi.fn(),
    listInvocations: vi.fn(),
    listFlowSecrets: vi.fn(),
    listFlows: vi.fn(),
    setFlowSecret: vi.fn(),
    whoAmI: vi.fn(),
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
      flow: {
        id: 'hello',
        entrypoint: 'flows/hello.ts',
        trigger: { type: 'webhook' },
      },
    });

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Deploying flow .*"hello".*\.\.\./),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Deployment complete/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/ID\s+hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Route\s+\/hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Trigger\s+webhook/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Endpoint/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/https:\/\/acme\.trigora\.dev\/hello/),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Ready to receive events/));
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Validating flow modules/));
    expect(console.log).not.toHaveBeenCalledWith(
      expect.stringMatching(/Prepared 1 flow for deployment/),
    );
    expect(console.log).not.toHaveBeenCalledWith(
      expect.stringMatching(/Building deployment artifact/),
    );
    expect(console.log).not.toHaveBeenCalledWith(
      expect.stringMatching(/Uploading deployment package/),
    );
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Activating deployment/));
  });

  it('throws a helpful error when multiple flows exist and none is selected', async () => {
    const tempDir = await makeTempDir();
    const helloPath = path.join(tempDir, 'flows', 'hello.ts');
    const ordersPath = path.join(tempDir, 'flows', 'nested', 'orders.ts');

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

    await expect(deployCommand({})).rejects.toThrow(
      'Multiple flows found. Pass a flow name or file path to deploy one flow.',
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
      'Flow "hello" in "flows/hello.ts" uses unsupported trigger "manual". trigora deploy currently supports only webhook- and cron-triggered flows.',
    );
  });

  it('loads and summarizes a cron flow without an endpoint', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'nightly.ts');
    const createDeployment = vi.fn().mockResolvedValue({
      plan: 'pro',
      id: 'dep_789',
      status: 'active',
      manifestVersion: 1,
      manifestJson: {
        version: 1,
        flow: {
          id: 'nightly',
          entrypoint: 'flows/nightly.ts',
          trigger: { type: 'cron', cron: '0 2 * * *' },
        },
      },
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

    mockedCreateDeployApiClient.mockReturnValue(createMockApiClient({ createDeployment }));

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'nightly',
          trigger: { type: 'cron', cron: '0 2 * * *' },
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
      flow: {
        id: 'nightly',
        entrypoint: 'flows/nightly.ts',
        trigger: { type: 'cron', cron: '0 2 * * *' },
      },
    });

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/ID\s+nightly/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Trigger\s+cron/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Schedule\s+0 2 \* \* \*/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Timezone\s+UTC/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Scheduled and active/));
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Endpoint/));
  });

  it('uses the polished ready message when deployed flows return ready status', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');
    const createDeployment = vi.fn().mockResolvedValue({
      plan: 'pro',
      id: 'dep_ready',
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
      flow: {
        id: 'df_ready',
        slug: 'hello',
        trigger: 'webhook',
        routePath: '/hello',
        status: 'ready',
        url: 'https://acme.trigora.dev/hello',
      },
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

    await deployCommand({
      filePath: flowPath,
    });

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Ready to receive events/));
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/^ready$/));
  });

  it('requires an explicit selection even when multiple flows share the same id', async () => {
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
      'Multiple flows found. Pass a flow name or file path to deploy one flow.',
    );
  });

  it('sends the deployment package to the Trigora deploy api', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'hello.ts');
    const createDeployment = vi.fn().mockResolvedValue({
      plan: 'pro',
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

  it('surfaces invalid cron validation errors clearly', async () => {
    const tempDir = await makeTempDir();
    const flowPath = path.join(tempDir, 'flows', 'nightly.ts');
    const createDeployment = vi.fn().mockRejectedValue(
      new DeployApiRequestError(
        {
          code: 'invalid_cron_expression',
          details: {
            hint: 'Use 5 fields: minute hour day-of-month month day-of-week',
            message:
              'Invalid cron expression for flow "nightly": Day-of-week field value "13" must be between 0 and 7.',
          },
          message:
            'Invalid cron expression for flow "nightly": Day-of-week field value "13" must be between 0 and 7.',
        },
        400,
      ),
    );

    mockedCreateDeployApiClient.mockReturnValue(createMockApiClient({ createDeployment }));

    await fs.mkdir(path.dirname(flowPath), { recursive: true });
    await fs.writeFile(
      flowPath,
      `
        export default {
          id: 'nightly',
          trigger: { type: 'cron', cron: '0 2 * *' },
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
      title: 'Deployment failed',
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Flow', value: 'nightly' }),
        expect.objectContaining({ label: 'Error', value: 'Invalid cron expression' }),
        expect.objectContaining({
          label: 'Reason',
          value: 'Day-of-week field value "13" must be between 0 and 7',
        }),
        expect.objectContaining({
          label: 'Hint',
          value: 'Use 5 fields: minute hour day-of-month month day-of-week',
        }),
      ]),
      message: 'Day-of-week field value "13" must be between 0 and 7',
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
