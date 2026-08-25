import type { EnqueueQueueMessageResponse, WorkspaceQueueRecord } from '@trigora/contracts';
import {
  DeployApiNetworkError,
  DeployApiRequestError,
  DeployApiResponseError,
} from './createDeployApiClient';
import { CliDisplayError, pluralize } from './cliOutput';
import { colors } from './colors';

export const queueSteps = {
  deletingQueue: 'Deleting queue',
  enqueuingMessage: 'Enqueuing message',
  fetchingQueues: 'Fetching queues',
  purgingFailedMessages: 'Purging failed messages',
  retryingFailedMessages: 'Retrying failed messages',
} as const;

function formatQueueName(name: string): string {
  return colors.flow(colors.heading(name));
}

function createRequestFailure(reason: string, step?: string, hint?: string): CliDisplayError {
  const details = step
    ? [
        { label: 'Step', value: step },
        { label: 'Reason', value: reason },
      ]
    : [{ label: 'Reason', value: reason }];

  return new CliDisplayError({
    title: 'Request failed',
    details,
    hint,
    message: reason,
  });
}

function formatQueueListDetailLines(queue: WorkspaceQueueRecord, indent = '   '): string[] {
  const items = [
    {
      label: 'Consumer',
      value: queue.consumerFlowSlug ? colors.flow(queue.consumerFlowSlug) : colors.label('none'),
    },
    { label: 'Concurrency', value: String(queue.concurrency) },
    { label: 'Pending', value: String(queue.pendingCount) },
    { label: 'Processing', value: String(queue.processingCount) },
    { label: 'Failed', value: String(queue.failedCount) },
  ];

  const labelWidth = items.reduce((width, item) => Math.max(width, item.label.length), 0);

  return items.map(
    (item) => `${indent}${colors.label(item.label.padEnd(labelWidth))}  ${item.value}`,
  );
}

export function printQueuesList(queues: WorkspaceQueueRecord[]): void {
  const queueCount = queues.length;

  console.log(`${colors.success('✔')} Found ${queueCount} ${pluralize(queueCount, 'queue')}:`);
  console.log('');

  for (const [index, queue] of queues.entries()) {
    const itemPrefix = `  ${index + 1}. `;
    const lines = [
      `${itemPrefix}${formatQueueName(queue.name)}`,
      ...formatQueueListDetailLines(queue, ' '.repeat(itemPrefix.length)),
    ];

    for (const line of lines) {
      console.log(line);
    }

    if (index < queues.length - 1) {
      console.log('');
    }
  }
}

export function printNoQueuesFound(): void {
  console.log('No queues found.');
}

export function printEnqueuingMessage(queue: string): void {
  console.log(colors.label(`Enqueuing message to ${formatQueueName(queue)}...`));
}

export function printMessageEnqueued(response: EnqueueQueueMessageResponse): void {
  console.log('');
  console.log(`${colors.success('✔')} Message enqueued`);
  console.log('');
  console.log(`${colors.label('Queue'.padEnd(10))} ${formatQueueName(response.queue)}`);
  console.log(`${colors.label('Message'.padEnd(10))} ${response.id}`);
  console.log(`${colors.label('Enqueued'.padEnd(10))} ${colors.label(response.enqueuedAt)}`);
}

export function printPurgingFailedMessages(queue: string): void {
  console.log(colors.label(`Purging failed messages from ${formatQueueName(queue)}...`));
}

export function printFailedMessagesPurged(queue: string, purged: number): void {
  console.log('');
  console.log(
    `${colors.success('✔')} Purged ${purged} failed ${pluralize(purged, 'message')} from ${formatQueueName(queue)}`,
  );
}

export function printRetryingFailedMessages(queue: string): void {
  console.log(colors.label(`Retrying failed messages in ${formatQueueName(queue)}...`));
}

export function printFailedMessagesRetried(queue: string, retried: number): void {
  console.log('');
  console.log(
    `${colors.success('✔')} Retried ${retried} failed ${pluralize(retried, 'message')} in ${formatQueueName(queue)}`,
  );
}

export function printQueueDeletionCanceled(queue: string): void {
  console.log(colors.label(`Skipped deleting queue "${queue}".`));
}

export function printPurgeCanceled(queue: string): void {
  console.log(colors.label(`Skipped purging failed messages from queue "${queue}".`));
}

export function printRetryCanceled(queue: string): void {
  console.log(colors.label(`Skipped retrying failed messages in queue "${queue}".`));
}

export function printDeletingQueue(queue: string): void {
  console.log(colors.label(`Deleting queue ${formatQueueName(queue)}...`));
}

export function printQueueDeleted(queue: string): void {
  console.log('');
  console.log(`${colors.success('✔')} Deleted queue ${formatQueueName(queue)}`);
}

export function toQueuesTokenFailure(): CliDisplayError {
  return createRequestFailure(
    'TRIGORA_DEPLOY_TOKEN is not set.',
    queueSteps.fetchingQueues,
    'Set TRIGORA_DEPLOY_TOKEN in your environment and try again.',
  );
}

export function toQueuesApiFailure(error: unknown, step: string): CliDisplayError {
  if (error instanceof DeployApiRequestError) {
    if (error.code === 'unauthorized' || error.code === 'forbidden') {
      return createRequestFailure(
        'Deploy token is invalid or no longer active.',
        step,
        'Check your deploy token and try again.',
      );
    }

    if (error.code === 'queue_not_found' || error.code === 'not_found') {
      return createRequestFailure('Queue not found.', step);
    }

    if (error.code === 'queue_has_consumer') {
      return createRequestFailure(
        error.message.trim() || 'Queue still has a consumer flow bound.',
        step,
        'Unbind the consumer by deploying a non-queue flow or deleting the consumer flow, then try again.',
      );
    }

    if (error.code === 'conflict' || error.code === 'bad_request') {
      return createRequestFailure(
        error.message.trim() || 'Trigora Cloud rejected the request.',
        step,
      );
    }

    return createRequestFailure(
      error.message.trim() || 'Trigora Cloud rejected the request.',
      step,
    );
  }

  if (error instanceof DeployApiNetworkError) {
    return createRequestFailure(error.message || 'Network request failed.', step);
  }

  if (error instanceof DeployApiResponseError) {
    return createRequestFailure('Trigora Cloud returned an unexpected response.', step);
  }

  if (!(error instanceof Error)) {
    return createRequestFailure('Trigora Cloud could not process the request.', step);
  }

  return createRequestFailure(error.message, step);
}
