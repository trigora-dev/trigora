import type {
  FlowRecord,
  GetInvocationResponse,
  ListInvocationsResponse,
} from '@trigora/contracts';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  createDeployApiClient,
  type DeployApiClient,
  DeployApiNetworkError,
  DeployApiRequestError,
} from '../lib/createDeployApiClient';
import { inspectInvocationCommand, listInvocationsCommand } from './invocations';
import { getLogCommand } from './logs';

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
  id: 'stripe-checkout',
  slug: 'stripe-checkout',
  status: 'ready',
  trigger: 'webhook' as const,
  routePath: '/stripe-checkout',
  endpoint: 'https://acme.trigora.dev/stripe-checkout',
  createdAt: '2026-05-03T10:00:00.000Z',
} satisfies FlowRecord;

const failedInvocation = {
  id: 'inv_123',
  flowSlug: stripeFlow.slug,
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
  triggerType: 'webhook',
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
} satisfies GetInvocationResponse['invocation'];

function createMockApiClient(overrides: Partial<DeployApiClient> = {}): DeployApiClient {
  return {
    createDeployment: vi.fn(),
    deleteFlow: vi.fn(),
    deleteFlowSecret: vi.fn(),
    disableFlow: vi.fn(),
    enableFlow: vi.fn(),
    getFlow: vi.fn().mockResolvedValue(stripeFlow),
    getInvocation: vi.fn().mockResolvedValue(invocationDetail),
    listInvocations: vi.fn().mockResolvedValue([
      failedInvocation,
      {
        id: 'inv_124',
        flowSlug: stripeFlow.slug,
        status: 'succeeded',
        startedAt: '2026-05-05T09:00:00.000Z',
        completedAt: '2026-05-05T09:00:00.210Z',
        durationMs: 210,
        httpStatus: 200,
        errorCode: null,
        errorMessage: null,
      },
    ] satisfies ListInvocationsResponse['invocations']),
    listSecrets: vi.fn(),
    listFlows: vi.fn(),
    setFlowSecret: vi.fn(),
    whoAmI: vi.fn(),
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

describe('invocation commands', () => {
  it('lists recent invocations with applied filters', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-05-05T10:02:00.000Z'));

    const apiClient = createMockApiClient();
    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(
      listInvocationsCommand({
        flow: stripeFlow.slug,
        range: '7d',
        status: 'failed',
      }),
    ).resolves.toEqual([
      failedInvocation,
      {
        id: 'inv_124',
        flowSlug: stripeFlow.slug,
        status: 'succeeded',
        startedAt: '2026-05-05T09:00:00.000Z',
        completedAt: '2026-05-05T09:00:00.210Z',
        durationMs: 210,
        httpStatus: 200,
        errorCode: null,
        errorMessage: null,
      },
    ]);

    expect(apiClient.listInvocations).toHaveBeenCalledWith({
      flow: stripeFlow.slug,
      range: '7d',
      status: 'failed',
    });
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/✔ Found 2 invocations/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Flow\s+stripe-checkout/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Status\s+failed/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Range\s+7d/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/1\.\s+inv_123\s+2m ago/));

    vi.useRealTimers();
  });

  it('shows an empty state when no invocations match the filters', async () => {
    const apiClient = createMockApiClient({
      listInvocations: vi.fn().mockResolvedValue([]),
    });

    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(
      listInvocationsCommand({
        flow: stripeFlow.slug,
      }),
    ).resolves.toEqual([]);

    expect(console.log).toHaveBeenCalledWith('No invocations found.');
  });

  it('prints invocation details for inspect', async () => {
    const apiClient = createMockApiClient();
    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(
      inspectInvocationCommand({
        invocationId: failedInvocation.id,
      }),
    ).resolves.toEqual(invocationDetail);

    expect(apiClient.getInvocation).toHaveBeenCalledWith(failedInvocation.id);
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Invocation inv_123/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Flow\s+.*stripe-checkout/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Trigger\s+webhook/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Status\s+failed/));
    expect(console.log).not.toHaveBeenCalledWith(expect.stringMatching(/^Logs$/));
  });

  it('prints logs for a single invocation', async () => {
    const apiClient = createMockApiClient();
    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(getLogCommand(failedInvocation.id)).resolves.toEqual(invocationDetail);

    expect(apiClient.getInvocation).toHaveBeenCalledWith(failedInvocation.id);
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Logs for invocation inv_123/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Flow\s+.*stripe-checkout/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Trigger\s+webhook/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Logs/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/1\.\s+2026-05-05T10:00:00.100Z\s+INFO\s+Received webhook event/),
    );
  });

  it('throws a polished error when the deploy token is missing', async () => {
    delete process.env.TRIGORA_DEPLOY_TOKEN;

    await expect(listInvocationsCommand({})).rejects.toThrow('TRIGORA_DEPLOY_TOKEN is not set.');
  });

  it('maps invocation not found errors', async () => {
    const apiClient = createMockApiClient({
      getInvocation: vi.fn().mockRejectedValue(
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
      inspectInvocationCommand({
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
      listInvocations: vi.fn().mockRejectedValue(new DeployApiNetworkError('connect ECONNREFUSED')),
    });

    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(listInvocationsCommand({})).rejects.toMatchObject({
      details: expect.arrayContaining([
        expect.objectContaining({ label: 'Step', value: 'Fetching invocations' }),
        expect.objectContaining({ label: 'Reason', value: 'Network request failed.' }),
      ]),
    });
  });
});
