import type {
  DeleteQueueResponse,
  EnqueueQueueMessageResponse,
  JsonValue,
  PurgeFailedQueueMessagesResponse,
  WorkspaceQueueRecord,
} from '@trigora/contracts';
import { createDeployApiClient } from '../lib/createDeployApiClient';
import { confirmAction, promptForTypedConfirmation } from '../lib/interactive';
import { loadJsonFile } from '../lib/loadJsonFile';
import { getDeployToken } from '../lib/getDeployToken';
import {
  printDeletingQueue,
  printEnqueuingMessage,
  printFailedMessagesPurged,
  printMessageEnqueued,
  printNoQueuesFound,
  printPurgingFailedMessages,
  printPurgeCanceled,
  printQueueDeleted,
  printQueueDeletionCanceled,
  printQueuesList,
  queueSteps,
  toQueuesApiFailure,
  toQueuesTokenFailure,
} from '../lib/queuesOutput';

type EnqueueQueueOptions = {
  payloadPath?: string;
  queue: string;
};

type PurgeFailedQueueOptions = {
  queue: string;
  yes?: boolean;
};

type DeleteQueueOptions = {
  queue: string;
  yes?: boolean;
};

function requireDeployToken(): string {
  const token = getDeployToken();

  if (!token) {
    throw toQueuesTokenFailure();
  }

  return token;
}

function createQueuesApiClient() {
  return createDeployApiClient({
    token: requireDeployToken(),
  });
}

async function loadPayload(filePath?: string): Promise<JsonValue> {
  if (!filePath) {
    return {};
  }

  return loadJsonFile(filePath);
}

export async function listQueuesCommand(): Promise<WorkspaceQueueRecord[]> {
  const queues = await createQueuesApiClient()
    .listQueues()
    .catch((error) => {
      throw toQueuesApiFailure(error, queueSteps.fetchingQueues);
    });

  if (queues.length === 0) {
    printNoQueuesFound();
    return queues;
  }

  printQueuesList(queues);

  return queues;
}

export async function enqueueQueueCommand(
  options: EnqueueQueueOptions,
): Promise<EnqueueQueueMessageResponse> {
  const apiClient = createQueuesApiClient();
  const payload = await loadPayload(options.payloadPath);

  printEnqueuingMessage(options.queue);

  const response = await apiClient
    .enqueueQueueMessage(options.queue, { payload })
    .catch((error) => {
      throw toQueuesApiFailure(error, queueSteps.enqueuingMessage);
    });

  printMessageEnqueued(response);

  return response;
}

export async function purgeFailedQueueCommand(
  options: PurgeFailedQueueOptions,
): Promise<PurgeFailedQueueMessagesResponse | null> {
  const apiClient = createQueuesApiClient();

  if (!options.yes) {
    const confirmed = await confirmAction(`Purge failed messages from queue "${options.queue}"?`);

    if (!confirmed) {
      printPurgeCanceled(options.queue);
      return null;
    }
  }

  printPurgingFailedMessages(options.queue);

  const response = await apiClient.purgeFailedQueueMessages(options.queue).catch((error) => {
    throw toQueuesApiFailure(error, queueSteps.purgingFailedMessages);
  });

  printFailedMessagesPurged(options.queue, response.purged);

  return response;
}

export async function deleteQueueCommand(
  options: DeleteQueueOptions,
): Promise<DeleteQueueResponse | null> {
  const apiClient = createQueuesApiClient();

  if (!options.yes) {
    const confirmed = await promptForTypedConfirmation({
      expectedValue: options.queue,
      message: `This will delete queue "${options.queue}". Queues with an active consumer or pending messages cannot be deleted.`,
      nonInteractiveHint: 'Re-run with --yes to confirm in non-interactive environments.',
      nonInteractiveReason: `Confirmation is required before deleting queue "${options.queue}".`,
    });

    if (!confirmed) {
      printQueueDeletionCanceled(options.queue);
      return null;
    }
  }

  printDeletingQueue(options.queue);

  const response = await apiClient.deleteQueue(options.queue).catch((error) => {
    throw toQueuesApiFailure(error, queueSteps.deletingQueue);
  });

  printQueueDeleted(response.name);

  return response;
}
