import type { TrigoraClient } from '@trigora/client';
import { TrigoraRuntimeError } from '@trigora/client';
import type { WhoAmIResponse } from '@trigora/contracts';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { whoAmICommand } from './whoami';

vi.mock('@trigora/client', async () => {
  const actual = await vi.importActual<typeof import('@trigora/client')>('@trigora/client');
  return {
    ...actual,
    createClient: vi.fn(),
  };
});

import { createClient } from '@trigora/client';

const originalConsoleLog = console.log;
const originalEnv = { ...process.env };
const mockedCreateClient = vi.mocked(createClient);

const identity = {
  actorType: 'api_token' as const,
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

function createMockClient(overrides: Partial<TrigoraClient> = {}): TrigoraClient {
  return {
    whoAmI: vi.fn().mockResolvedValue(identity),
    listProjects: vi.fn(),
    createProject: vi.fn(),
    deployProgram: vi.fn(),
    listPrograms: vi.fn(),
    getProgram: vi.fn(),
    listProgramVersions: vi.fn(),
    start: vi.fn(),
    startExecution: vi.fn(),
    get: vi.fn(),
    getExecution: vi.fn(),
    listExecutions: vi.fn(),
    sendEvent: vi.fn(),
    cancelExecution: vi.fn(),
    getResult: vi.fn(),
    programs: vi.fn(),
    executions: vi.fn(),
    ...overrides,
  };
}

beforeEach(() => {
  console.log = vi.fn();
  process.env = {
    ...originalEnv,
    TRIGORA_TOKEN: 'secret-token',
  };
  mockedCreateClient.mockReset();
  mockedCreateClient.mockReturnValue(createMockClient());
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

  it('throws a polished error when the API token is missing', async () => {
    delete process.env.TRIGORA_TOKEN;

    await expect(whoAmICommand()).rejects.toThrow('TRIGORA_TOKEN is not set.');
  });

  it('maps invalid token errors to the canonical token reason', async () => {
    mockedCreateClient.mockReturnValue(
      createMockClient({
        whoAmI: vi.fn().mockRejectedValue(
          new TrigoraRuntimeError('A valid API token is required.', {
            status: 401,
            code: 'unauthorized',
          }),
        ),
      }),
    );

    await expect(whoAmICommand()).rejects.toMatchObject({
      title: 'Request failed',
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Fetching identity' }),
        expect.objectContaining({
          label: 'Reason',
          value: 'API token is invalid or no longer active.',
        }),
      ]),
      hint: 'Check your API token and try again.',
    });
  });

  it('maps network failures to a concise request error', async () => {
    mockedCreateClient.mockReturnValue(
      createMockClient({
        whoAmI: vi
          .fn()
          .mockRejectedValue(new TrigoraRuntimeError('connect ECONNREFUSED', { status: 0 })),
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
