import type { FlowStatusResponse, FlowRecord } from '@trigora/contracts';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  createDeployApiClient,
  type DeployApiClient,
  DeployApiNetworkError,
  DeployApiRequestError,
} from '../lib/createDeployApiClient';
import { promptForTypedConfirmation } from '../lib/interactive';
import {
  deleteFlowCommand,
  disableFlowCommand,
  enableFlowCommand,
  inspectFlowCommand,
  listFlowsCommand,
} from './flows';

vi.mock('../lib/createDeployApiClient', async () => {
  const actual = await vi.importActual<typeof import('../lib/createDeployApiClient')>(
    '../lib/createDeployApiClient',
  );

  return {
    ...actual,
    createDeployApiClient: vi.fn(),
  };
});

vi.mock('../lib/interactive', () => ({
  promptForTypedConfirmation: vi.fn(),
}));

const originalConsoleLog = console.log;
const originalConsoleWarn = console.warn;
const originalEnv = { ...process.env };
const mockedCreateDeployApiClient = vi.mocked(createDeployApiClient);
const mockedPromptForTypedConfirmation = vi.mocked(promptForTypedConfirmation);

const helloFlow = {
  id: 'hello',
  slug: 'hello',
  status: 'ready',
  trigger: 'webhook' as const,
  routePath: '/hello',
  endpoint: 'https://acme.trigora.dev/hello',
  createdAt: '2026-04-21T10:00:00.000Z',
} satisfies FlowRecord;

const cronFlow = {
  id: 'nightly-sync',
  slug: 'nightly-sync',
  status: 'ready',
  trigger: 'cron' as const,
  schedule: '0 2 * * *',
  timezone: 'UTC' as const,
  createdAt: '2026-04-21T11:00:00.000Z',
} satisfies FlowRecord;

function createMockApiClient(overrides: Partial<DeployApiClient> = {}): DeployApiClient {
  return {
    createDeployment: vi.fn(),
    deleteFlow: vi.fn().mockResolvedValue({
      deleted: true,
    }),
    deleteFlowSecret: vi.fn(),
    listFlows: vi.fn().mockResolvedValue([helloFlow]),
    getFlow: vi.fn().mockResolvedValue(helloFlow),
    getInvocation: vi.fn(),
    listInvocations: vi.fn(),
    listFlowSecrets: vi.fn(),
    setFlowSecret: vi.fn(),
    whoAmI: vi.fn(),
    disableFlow: vi.fn().mockResolvedValue({
      id: helloFlow.id,
      slug: helloFlow.slug,
      trigger: 'webhook',
      status: 'disabled',
      routePath: helloFlow.routePath,
      endpoint: helloFlow.endpoint,
    } satisfies FlowStatusResponse['flow']),
    enableFlow: vi.fn().mockResolvedValue({
      id: helloFlow.id,
      slug: helloFlow.slug,
      trigger: 'webhook',
      status: 'ready',
      routePath: helloFlow.routePath,
      endpoint: helloFlow.endpoint,
    } satisfies FlowStatusResponse['flow']),
    ...overrides,
  };
}

beforeEach(() => {
  console.log = vi.fn();
  console.warn = vi.fn();
  process.env = {
    ...originalEnv,
    TRIGORA_DEPLOY_TOKEN: 'secret-token',
  };
  mockedCreateDeployApiClient.mockReset();
  mockedCreateDeployApiClient.mockReturnValue(createMockApiClient());
  mockedPromptForTypedConfirmation.mockReset();
});

afterEach(() => {
  console.log = originalConsoleLog;
  console.warn = originalConsoleWarn;
  process.env = originalEnv;
});

describe('flows commands', () => {
  it('lists deployed flows', async () => {
    await expect(listFlowsCommand()).resolves.toEqual([helloFlow]);

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Found 1 flow/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/1\. hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/^\s{5}Trigger\s+webhook/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/^\s{5}Route\s+\/hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/^\s{5}Status\s+ready/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/^\s{5}Endpoint\s+https:\/\/acme\.trigora\.dev\/hello/),
    );
  });

  it('shows an empty state when no flows are deployed', async () => {
    mockedCreateDeployApiClient.mockReturnValue(
      createMockApiClient({
        listFlows: vi.fn().mockResolvedValue([]),
      }),
    );

    await expect(listFlowsCommand()).resolves.toEqual([]);

    expect(console.log).toHaveBeenCalledWith('No deployed flows found.');
  });

  it('prints cron metadata in flow listings and inspect output', async () => {
    mockedCreateDeployApiClient.mockReturnValue(
      createMockApiClient({
        listFlows: vi.fn().mockResolvedValue([cronFlow]),
        getFlow: vi.fn().mockResolvedValue(cronFlow),
      }),
    );

    await expect(listFlowsCommand()).resolves.toEqual([cronFlow]);
    await expect(inspectFlowCommand(cronFlow.slug)).resolves.toEqual(cronFlow);

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/1\. nightly-sync/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/^\s{5}Trigger\s+cron/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/^\s{5}Schedule\s+0 2 \* \* \*/),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/^\s{5}Timezone\s+UTC/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Schedule\s+0 2 \* \* \*/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Timezone\s+UTC/));
  });

  it('prints flow details for inspect', async () => {
    await expect(inspectFlowCommand(helloFlow.slug)).resolves.toEqual(helloFlow);

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/ID\s+hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Trigger\s+webhook/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Route\s+\/hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Status\s+ready/));
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Fetching flow/));
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Flow details/));
  });

  it('prints a success summary when a flow is disabled', async () => {
    await expect(disableFlowCommand(helloFlow.slug)).resolves.toEqual({
      id: helloFlow.id,
      slug: helloFlow.slug,
      trigger: 'webhook',
      status: 'disabled',
      routePath: helloFlow.routePath,
      endpoint: helloFlow.endpoint,
    });

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Flow disabled/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/ID\s+hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Status\s+disabled/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Route\s+\/hello/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Endpoint\s+https:\/\/acme\.trigora\.dev\/hello/),
    );
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Disabling flow/));
  });

  it('prints a success summary when a flow is enabled', async () => {
    await expect(enableFlowCommand(helloFlow.slug)).resolves.toEqual({
      id: helloFlow.id,
      slug: helloFlow.slug,
      trigger: 'webhook',
      status: 'ready',
      routePath: helloFlow.routePath,
      endpoint: helloFlow.endpoint,
    });

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Flow enabled/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/ID\s+hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Status\s+ready/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Route\s+\/hello/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Endpoint\s+https:\/\/acme\.trigora\.dev\/hello/),
    );
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Enabling flow/));
  });

  it('deletes a flow without prompting when --yes is used', async () => {
    const apiClient = createMockApiClient();

    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(deleteFlowCommand(helloFlow.slug, { yes: true })).resolves.toBeUndefined();

    expect(mockedPromptForTypedConfirmation).not.toHaveBeenCalled();
    expect(apiClient.deleteFlow).toHaveBeenCalledWith(helloFlow.slug);
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Flow deleted/));
  });

  it('requires typed confirmation before deleting a flow', async () => {
    const apiClient = createMockApiClient();

    mockedCreateDeployApiClient.mockReturnValue(apiClient);
    mockedPromptForTypedConfirmation.mockResolvedValue(true);

    await deleteFlowCommand(helloFlow.slug);

    expect(mockedPromptForTypedConfirmation).toHaveBeenCalledWith({
      expectedValue: helloFlow.slug,
      message:
        'This will delete flow "hello", including deployments, invocations, logs, schedules, secrets, and hosted workers.',
      nonInteractiveHint: 'Re-run with --yes to confirm in non-interactive environments.',
      nonInteractiveReason: 'Confirmation is required before deleting flow "hello".',
    });
    expect(apiClient.deleteFlow).toHaveBeenCalledWith(helloFlow.slug);
  });

  it('skips flow deletion when typed confirmation does not match', async () => {
    const apiClient = createMockApiClient();

    mockedCreateDeployApiClient.mockReturnValue(apiClient);
    mockedPromptForTypedConfirmation.mockResolvedValue(false);

    await expect(deleteFlowCommand(helloFlow.slug)).resolves.toBeNull();

    expect(apiClient.deleteFlow).not.toHaveBeenCalled();
    expect(console.warn).toHaveBeenCalledWith(
      expect.stringMatching(/Skipped deleting flow "hello"/),
    );
  });

  it('throws a polished error when the deploy token is missing', async () => {
    delete process.env.TRIGORA_DEPLOY_TOKEN;

    await expect(listFlowsCommand()).rejects.toThrow('TRIGORA_DEPLOY_TOKEN is not set.');
  });

  it('maps invalid token errors to the canonical token reason', async () => {
    mockedCreateDeployApiClient.mockReturnValue(
      createMockApiClient({
        listFlows: vi.fn().mockRejectedValue(
          new DeployApiRequestError(
            {
              code: 'unauthorized',
              message: 'A valid deploy token is required.',
            },
            401,
          ),
        ),
      }),
    );

    await expect(listFlowsCommand()).rejects.toMatchObject({
      title: 'Request failed',
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Fetching deployed flows' }),
        expect.objectContaining({
          label: 'Reason',
          value: 'Deploy token is invalid or no longer active.',
        }),
      ]),
      hint: 'Check your deploy token and try again.',
    });
  });

  it('maps missing flows to a concise not found error', async () => {
    mockedCreateDeployApiClient.mockReturnValue(
      createMockApiClient({
        getFlow: vi.fn().mockRejectedValue(
          new DeployApiRequestError(
            {
              code: 'deployment_not_found',
              message: 'Deployment not found',
            },
            404,
          ),
        ),
      }),
    );

    await expect(inspectFlowCommand(helloFlow.slug)).rejects.toMatchObject({
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Fetching flow' }),
        expect.objectContaining({ label: 'Reason', value: 'Flow not found.' }),
      ]),
    });
  });

  it('maps network failures to a concise request error', async () => {
    mockedCreateDeployApiClient.mockReturnValue(
      createMockApiClient({
        disableFlow: vi.fn().mockRejectedValue(new DeployApiNetworkError('connect ECONNREFUSED')),
      }),
    );

    await expect(disableFlowCommand(helloFlow.slug)).rejects.toMatchObject({
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Disabling flow' }),
        expect.objectContaining({ label: 'Reason', value: 'Network request failed.' }),
      ]),
    });
  });

  it('maps enable failures to the matching enable step', async () => {
    mockedCreateDeployApiClient.mockReturnValue(
      createMockApiClient({
        enableFlow: vi.fn().mockRejectedValue(new DeployApiNetworkError('connect ECONNREFUSED')),
      }),
    );

    await expect(enableFlowCommand(helloFlow.slug)).rejects.toMatchObject({
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Enabling flow' }),
        expect.objectContaining({ label: 'Reason', value: 'Network request failed.' }),
      ]),
    });
  });

  it('maps delete failures to the matching delete step', async () => {
    mockedCreateDeployApiClient.mockReturnValue(
      createMockApiClient({
        deleteFlow: vi.fn().mockRejectedValue(new DeployApiNetworkError('connect ECONNREFUSED')),
      }),
    );

    await expect(deleteFlowCommand(helloFlow.slug, { yes: true })).rejects.toMatchObject({
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Deleting flow' }),
        expect.objectContaining({ label: 'Reason', value: 'Network request failed.' }),
      ]),
    });
  });
});
