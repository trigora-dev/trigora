import type { WhoAmIResponse } from '@trigora/contracts';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  createDeployApiClient,
  type DeployApiClient,
  DeployApiNetworkError,
  DeployApiRequestError,
} from '../lib/createDeployApiClient';
import { whoAmICommand } from './whoami';

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

const identity = {
  actorType: 'deploy_token' as const,
  workspace: {
    id: 'ws_123',
    name: 'Acme',
    plan: 'pro',
    planStatus: 'active',
    slug: 'acme',
  },
  token: {
    id: 'tok_123',
    label: 'local-dev',
    status: 'active',
    createdAt: '2026-05-17T00:00:00.000Z',
  },
} satisfies WhoAmIResponse;

function createMockApiClient(overrides: Partial<DeployApiClient> = {}): DeployApiClient {
  return {
    createDeployment: vi.fn(),
    deleteFlow: vi.fn(),
    deleteFlowSecret: vi.fn(),
    disableFlow: vi.fn(),
    enableFlow: vi.fn(),
    getFlow: vi.fn(),
    getInvocation: vi.fn(),
    listInvocations: vi.fn(),
    listSecrets: vi.fn(),
    listFlows: vi.fn(),
    setFlowSecret: vi.fn(),
    whoAmI: vi.fn().mockResolvedValue(identity),
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

describe('whoAmICommand', () => {
  it('prints the authenticated workspace and token summary', async () => {
    await expect(whoAmICommand()).resolves.toEqual(identity);

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Workspace\s+acme/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Token\s+local-dev/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Status\s+active/));
  });

  it('throws a polished error when the deploy token is missing', async () => {
    delete process.env.TRIGORA_DEPLOY_TOKEN;

    await expect(whoAmICommand()).rejects.toThrow('TRIGORA_DEPLOY_TOKEN is not set.');
  });

  it('maps invalid token errors to the canonical token reason', async () => {
    mockedCreateDeployApiClient.mockReturnValue(
      createMockApiClient({
        whoAmI: vi.fn().mockRejectedValue(
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

    await expect(whoAmICommand()).rejects.toMatchObject({
      title: 'Request failed',
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Fetching identity' }),
        expect.objectContaining({
          label: 'Reason',
          value: 'Deploy token is invalid or no longer active.',
        }),
      ]),
      hint: 'Check your deploy token and try again.',
    });
  });

  it('maps network failures to a concise request error', async () => {
    mockedCreateDeployApiClient.mockReturnValue(
      createMockApiClient({
        whoAmI: vi.fn().mockRejectedValue(new DeployApiNetworkError('connect ECONNREFUSED')),
      }),
    );

    await expect(whoAmICommand()).rejects.toMatchObject({
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Fetching identity' }),
        expect.objectContaining({ label: 'Reason', value: 'Network request failed.' }),
      ]),
    });
  });
});
