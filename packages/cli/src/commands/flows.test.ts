import type { FlowStatusResponse, FlowRecord } from '@trigora/contracts';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  createDeployApiClient,
  type DeployApiClient,
  DeployApiNetworkError,
  DeployApiRequestError,
} from '../lib/createDeployApiClient';
import {
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

const originalConsoleLog = console.log;
const originalEnv = { ...process.env };
const mockedCreateDeployApiClient = vi.mocked(createDeployApiClient);

const helloFlow = {
  id: '402c04b0-62c8-4d0b-942f-0ee2329436a8',
  name: 'hello',
  status: 'ready',
  trigger: 'webhook' as const,
  endpoint: 'https://trigora.dev/f/402c04b0-62c8-4d0b-942f-0ee2329436a8',
  route: '/hello',
  createdAt: '2026-04-21T10:00:00.000Z',
} satisfies FlowRecord;

function createMockApiClient(overrides: Partial<DeployApiClient> = {}): DeployApiClient {
  return {
    createDeployment: vi.fn(),
    listFlows: vi.fn().mockResolvedValue([helloFlow]),
    getFlow: vi.fn().mockResolvedValue(helloFlow),
    disableFlow: vi.fn().mockResolvedValue({
      id: helloFlow.id,
      status: 'disabled',
    } satisfies FlowStatusResponse['flow']),
    enableFlow: vi.fn().mockResolvedValue({
      id: helloFlow.id,
      status: 'ready',
    } satisfies FlowStatusResponse['flow']),
    ...overrides,
  };
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

afterEach(() => {
  console.log = originalConsoleLog;
  process.env = originalEnv;
});

describe('flows commands', () => {
  it('lists deployed flows', async () => {
    await expect(listFlowsCommand()).resolves.toEqual([helloFlow]);

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Fetching deployed flows/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Found 1 flow/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/1\. hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/ID\s+402c04b0/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Trigger\s+webhook/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Status\s+ready/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Endpoint\s+https:\/\/trigora\.dev\/f\/402c04b0/),
    );
  });

  it('shows an empty state when no flows are deployed', async () => {
    mockedCreateDeployApiClient.mockReturnValue(
      createMockApiClient({
        listFlows: vi.fn().mockResolvedValue([]),
      }),
    );

    await expect(listFlowsCommand()).resolves.toEqual([]);

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ No deployed flows found/));
  });

  it('prints flow details for inspect', async () => {
    await expect(inspectFlowCommand(helloFlow.id)).resolves.toEqual(helloFlow);

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Fetching flow/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Flow details/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Name\s+hello/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/ID\s+402c04b0/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Route\s+\/hello/));
  });

  it('prints a success summary when a flow is disabled', async () => {
    await expect(disableFlowCommand(helloFlow.id)).resolves.toEqual({
      id: helloFlow.id,
      status: 'disabled',
    });

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Disabling flow/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Flow disabled/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Status\s+disabled/));
  });

  it('prints a success summary when a flow is enabled', async () => {
    await expect(enableFlowCommand(helloFlow.id)).resolves.toEqual({
      id: helloFlow.id,
      status: 'ready',
    });

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Enabling flow/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Flow enabled/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Status\s+ready/));
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

    await expect(inspectFlowCommand(helloFlow.id)).rejects.toMatchObject({
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

    await expect(disableFlowCommand(helloFlow.id)).rejects.toMatchObject({
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

    await expect(enableFlowCommand(helloFlow.id)).rejects.toMatchObject({
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Enabling flow' }),
        expect.objectContaining({ label: 'Reason', value: 'Network request failed.' }),
      ]),
    });
  });
});
