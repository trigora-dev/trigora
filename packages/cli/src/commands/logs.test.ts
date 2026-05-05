import type {
  FlowRecord,
  GetFlowInvocationResponse,
  ListFlowInvocationsResponse,
} from '@trigora/contracts';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  createDeployApiClient,
  type DeployApiClient,
  DeployApiNetworkError,
  DeployApiRequestError,
} from '../lib/createDeployApiClient';
import { getLogCommand, listLogsCommand } from './logs';

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

const stripeFlow = {
  id: '402c04b0-62c8-4d0b-942f-0ee2329436a8',
  name: 'stripe-checkout',
  status: 'ready',
  trigger: 'webhook' as const,
  endpoint: 'https://trigora.dev/f/402c04b0-62c8-4d0b-942f-0ee2329436a8',
  route: '/stripe-checkout',
  createdAt: '2026-05-03T10:00:00.000Z',
} satisfies FlowRecord;

const failedInvocation = {
  id: 'inv_123',
  status: 'failed' as const,
  startedAt: '2026-05-05T10:00:00.000Z',
  completedAt: '2026-05-05T10:00:00.842Z',
  durationMs: 842,
  httpStatus: 400,
  errorCode: 'stripe_signature_invalid',
  errorMessage: 'Invalid Stripe signature.',
};

const invocationDetail = {
  ...failedInvocation,
  logs: [
    {
      sequence: 1,
      level: 'info' as const,
      message: 'Received webhook event',
      timestamp: '2026-05-05T10:00:00.100Z',
      metadata: {
        eventType: 'checkout.session.completed',
      },
    },
    {
      sequence: 2,
      level: 'error' as const,
      message: 'Signature verification failed',
      timestamp: '2026-05-05T10:00:00.200Z',
      metadata: null,
    },
  ],
} satisfies GetFlowInvocationResponse['invocation'];

function createMockApiClient(overrides: Partial<DeployApiClient> = {}): DeployApiClient {
  return {
    createDeployment: vi.fn(),
    deleteFlowSecret: vi.fn(),
    disableFlow: vi.fn(),
    enableFlow: vi.fn(),
    getFlow: vi.fn().mockResolvedValue(stripeFlow),
    getFlowInvocation: vi.fn().mockResolvedValue(invocationDetail),
    listFlowInvocations: vi.fn().mockResolvedValue([
      failedInvocation,
      {
        id: 'inv_124',
        status: 'succeeded',
        startedAt: '2026-05-05T09:00:00.000Z',
        completedAt: '2026-05-05T09:00:00.210Z',
        durationMs: 210,
        httpStatus: 200,
        errorCode: null,
        errorMessage: null,
      },
    ] satisfies ListFlowInvocationsResponse['invocations']),
    listFlowSecrets: vi.fn(),
    listFlows: vi.fn(),
    setFlowSecret: vi.fn(),
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

describe('logs commands', () => {
  it('lists recent invocations for a hosted flow', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-05-05T10:02:00.000Z'));

    const apiClient = createMockApiClient();
    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(
      listLogsCommand({
        flowId: stripeFlow.id,
      }),
    ).resolves.toEqual([
      failedInvocation,
      {
        id: 'inv_124',
        status: 'succeeded',
        startedAt: '2026-05-05T09:00:00.000Z',
        completedAt: '2026-05-05T09:00:00.210Z',
        durationMs: 210,
        httpStatus: 200,
        errorCode: null,
        errorMessage: null,
      },
    ]);

    expect(apiClient.getFlow).toHaveBeenCalledWith(stripeFlow.id);
    expect(apiClient.listFlowInvocations).toHaveBeenCalledWith(stripeFlow.id);
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(
        /✔ Found 2 invocations for flow .*stripe-checkout.*402c04b0-62c8-4d0b-942f-0ee2329436a8.*:/,
      ),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/1\.\s+inv_123\s+2m ago/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Status\s+failed/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/HTTP\s+400/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Error\s+stripe_signature_invalid/),
    );
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/Fetching invocations/));

    vi.useRealTimers();
  });

  it('shows an empty state when a hosted flow has no invocations', async () => {
    const apiClient = createMockApiClient({
      listFlowInvocations: vi.fn().mockResolvedValue([]),
    });

    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(
      listLogsCommand({
        flowId: stripeFlow.id,
      }),
    ).resolves.toEqual([]);

    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(
        /No invocations found for flow .*stripe-checkout.*402c04b0-62c8-4d0b-942f-0ee2329436a8.*\./,
      ),
    );
  });

  it('prints a detailed invocation view with ordered log lines', async () => {
    const apiClient = createMockApiClient();
    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(
      getLogCommand({
        flowId: stripeFlow.id,
        invocationId: failedInvocation.id,
      }),
    ).resolves.toEqual(invocationDetail);

    expect(apiClient.getFlowInvocation).toHaveBeenCalledWith(stripeFlow.id, failedInvocation.id);
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Invocation inv_123/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Flow\s+.*stripe-checkout.*402c04b0-62c8-4d0b-942f-0ee2329436a8/),
    );
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Status\s+failed/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Logs/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/1\.\s+2026-05-05T10:00:00.100Z\s+INFO\s+Received webhook event/),
    );
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Metadata\s+\{"eventType":"checkout\.session\.completed"\}/),
    );
  });

  it('throws a polished error when the deploy token is missing', async () => {
    delete process.env.TRIGORA_DEPLOY_TOKEN;

    await expect(
      listLogsCommand({
        flowId: stripeFlow.id,
      }),
    ).rejects.toThrow('TRIGORA_DEPLOY_TOKEN is not set.');
  });

  it('maps invocation not found to a concise hosted error', async () => {
    const apiClient = createMockApiClient({
      getFlowInvocation: vi.fn().mockRejectedValue(
        new DeployApiRequestError(
          {
            code: 'not_found',
            message: 'Invocation not found.',
          },
          404,
        ),
      ),
    });

    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(
      getLogCommand({
        flowId: stripeFlow.id,
        invocationId: failedInvocation.id,
      }),
    ).rejects.toMatchObject({
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Fetching invocation' }),
        expect.objectContaining({ label: 'Reason', value: 'Invocation not found.' }),
      ]),
    });
  });

  it('maps network failures while listing invocations', async () => {
    const apiClient = createMockApiClient({
      listFlowInvocations: vi
        .fn()
        .mockRejectedValue(new DeployApiNetworkError('connect ECONNREFUSED')),
    });

    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(
      listLogsCommand({
        flowId: stripeFlow.id,
      }),
    ).rejects.toMatchObject({
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Fetching invocations' }),
        expect.objectContaining({ label: 'Reason', value: 'Network request failed.' }),
      ]),
    });
  });
});
