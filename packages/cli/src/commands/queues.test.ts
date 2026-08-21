import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { DeployApiClient } from '../lib/createDeployApiClient';
import { createDeployApiClient } from '../lib/createDeployApiClient';
import { confirmAction, promptForTypedConfirmation } from '../lib/interactive';
import {
  deleteQueueCommand,
  enqueueQueueCommand,
  listQueuesCommand,
  purgeFailedQueueCommand,
} from './queues';

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
  confirmAction: vi.fn(),
  promptForTypedConfirmation: vi.fn(),
}));

const mockedCreateDeployApiClient = vi.mocked(createDeployApiClient);
const mockedConfirmAction = vi.mocked(confirmAction);
const mockedPromptForTypedConfirmation = vi.mocked(promptForTypedConfirmation);

const originalConsoleLog = console.log;
const originalEnv = process.env;

const ordersQueue = {
  id: 'q_123',
  name: 'orders',
  consumerFlowSlug: 'orders-processor',
  concurrency: 5,
  pendingCount: 2,
  processingCount: 1,
  failedCount: 3,
  createdAt: '2026-05-10T00:00:00.000Z',
  updatedAt: '2026-05-10T01:00:00.000Z',
};

function createMockApiClient(overrides: Partial<DeployApiClient> = {}): DeployApiClient {
  return {
    createDeployment: vi.fn(),
    deleteFlow: vi.fn(),
    deleteFlowSecret: vi.fn(),
    deleteQueue: vi.fn().mockResolvedValue({
      deleted: true,
      name: 'orders',
    }),
    disableFlow: vi.fn(),
    enableFlow: vi.fn(),
    enqueueQueueMessage: vi.fn().mockResolvedValue({
      id: 'msg_123',
      queue: 'orders',
      enqueuedAt: '2026-05-10T01:30:00.000Z',
    }),
    getFlow: vi.fn(),
    getInvocation: vi.fn(),
    listInvocations: vi.fn(),
    listQueues: vi.fn().mockResolvedValue([ordersQueue]),
    listSecrets: vi.fn(),
    listFlows: vi.fn(),
    purgeFailedQueueMessages: vi.fn().mockResolvedValue({
      purged: 3,
    }),
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
  mockedConfirmAction.mockReset();
  mockedPromptForTypedConfirmation.mockReset();
});

afterEach(() => {
  console.log = originalConsoleLog;
  process.env = originalEnv;
});

describe('queues commands', () => {
  it('lists workspace queues', async () => {
    await expect(listQueuesCommand()).resolves.toEqual([ordersQueue]);

    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Found 1 queue/));
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/orders/));
    expect(console.log).toHaveBeenCalledWith(
      expect.stringMatching(/Consumer\s+.*orders-processor/),
    );
  });

  it('enqueues a message with an empty payload by default', async () => {
    const apiClient = createMockApiClient();
    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(enqueueQueueCommand({ queue: 'orders' })).resolves.toEqual({
      id: 'msg_123',
      queue: 'orders',
      enqueuedAt: '2026-05-10T01:30:00.000Z',
    });

    expect(apiClient.enqueueQueueMessage).toHaveBeenCalledWith('orders', { payload: {} });
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Message enqueued/));
  });

  it('purges failed messages without prompting when --yes is used', async () => {
    const apiClient = createMockApiClient();
    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(purgeFailedQueueCommand({ queue: 'orders', yes: true })).resolves.toEqual({
      purged: 3,
    });

    expect(mockedConfirmAction).not.toHaveBeenCalled();
    expect(apiClient.purgeFailedQueueMessages).toHaveBeenCalledWith('orders');
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Purged 3 failed messages/));
  });

  it('deletes a queue without prompting when --yes is used', async () => {
    const apiClient = createMockApiClient();
    mockedCreateDeployApiClient.mockReturnValue(apiClient);

    await expect(deleteQueueCommand({ queue: 'orders', yes: true })).resolves.toEqual({
      deleted: true,
      name: 'orders',
    });

    expect(mockedPromptForTypedConfirmation).not.toHaveBeenCalled();
    expect(apiClient.deleteQueue).toHaveBeenCalledWith('orders');
    expect(console.log).toHaveBeenCalledWith(expect.stringMatching(/Deleted queue .*orders/));
  });

  it('requires typed confirmation before deleting a queue', async () => {
    mockedPromptForTypedConfirmation.mockResolvedValue(true);

    await deleteQueueCommand({ queue: 'orders' });

    expect(mockedPromptForTypedConfirmation).toHaveBeenCalledWith(
      expect.objectContaining({
        expectedValue: 'orders',
      }),
    );
  });
});
