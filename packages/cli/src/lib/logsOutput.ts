import type {
  FlowInvocationLogLevel,
  FlowInvocationLogRecord,
  FlowInvocationRecord,
} from '@trigora/contracts';
import {
  DeployApiNetworkError,
  DeployApiRequestError,
  DeployApiResponseError,
} from './createDeployApiClient';
import { CliDisplayError, pluralize } from './cliOutput';
import { colors } from './colors';
import { formatFlowTarget } from './secretsOutput';

export const logSteps = {
  fetchingInvocation: 'Fetching invocation',
  fetchingInvocations: 'Fetching invocations',
  resolvingFlow: 'Resolving flow',
} as const;

type InvocationWithLogs = FlowInvocationRecord & {
  logs: FlowInvocationLogRecord[];
};

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

function formatRelativeTime(timestamp: string, now = Date.now()): string {
  const time = Date.parse(timestamp);

  if (Number.isNaN(time)) {
    return timestamp;
  }

  const diffMs = Math.max(0, now - time);
  const diffMinutes = Math.floor(diffMs / (60 * 1000));

  if (diffMinutes < 1) {
    return 'just now';
  }

  if (diffMinutes < 60) {
    return `${diffMinutes}m ago`;
  }

  const diffHours = Math.floor(diffMinutes / 60);

  if (diffHours < 24) {
    return `${diffHours}h ago`;
  }

  const diffDays = Math.floor(diffHours / 24);

  if (diffDays < 30) {
    return `${diffDays}d ago`;
  }

  return new Date(time).toISOString().slice(0, 10);
}

function formatInvocationStatus(status: FlowInvocationRecord['status']): string {
  switch (status) {
    case 'succeeded':
      return colors.success(status);
    case 'failed':
      return colors.error(status);
    case 'running':
      return colors.info(status);
  }
}

function formatLogLevel(level: FlowInvocationLogLevel): string {
  const label = level.toUpperCase().padEnd(5);

  switch (level) {
    case 'info':
      return colors.info(label);
    case 'warn':
      return colors.warn(label);
    case 'error':
      return colors.error(label);
  }
}

function formatDuration(durationMs: number | null): string {
  return durationMs === null ? '-' : `${durationMs}ms`;
}

function formatHttpStatus(httpStatus: number | null): string {
  return httpStatus === null ? '-' : String(httpStatus);
}

function formatValue(value: string | null): string {
  return value ?? '-';
}

function formatMetadata(metadata: Record<string, unknown> | null | undefined): string | null {
  if (!metadata || Object.keys(metadata).length === 0) {
    return null;
  }

  try {
    return JSON.stringify(metadata);
  } catch {
    return '[unserializable metadata]';
  }
}

function formatInvocationSummaryDetailLines(
  invocation: FlowInvocationRecord,
  indent: string,
): string[] {
  const details = [
    { label: 'Status', value: formatInvocationStatus(invocation.status) },
    { label: 'Started', value: colors.label(invocation.startedAt) },
    { label: 'Duration', value: formatDuration(invocation.durationMs) },
    { label: 'HTTP', value: formatHttpStatus(invocation.httpStatus) },
    { label: 'Error', value: invocation.errorCode ?? undefined },
    { label: 'Message', value: invocation.errorMessage ?? undefined },
  ];
  const visibleDetails = details.filter((detail): detail is { label: string; value: string } =>
    Boolean(detail.value),
  );

  const labelWidth = visibleDetails.reduce(
    (width, detail) => Math.max(width, detail.label.length),
    0,
  );

  return visibleDetails.map(
    (detail) => `${indent}${colors.label(detail.label.padEnd(labelWidth))}  ${detail.value}`,
  );
}

function formatInvocationDetailLines(invocation: FlowInvocationRecord): string[] {
  const details = [
    { label: 'Status', value: formatInvocationStatus(invocation.status) },
    { label: 'Started', value: colors.label(invocation.startedAt) },
    { label: 'Completed', value: colors.label(formatValue(invocation.completedAt)) },
    { label: 'Duration', value: formatDuration(invocation.durationMs) },
    { label: 'HTTP', value: formatHttpStatus(invocation.httpStatus) },
    { label: 'Error', value: formatValue(invocation.errorCode) },
    { label: 'Message', value: formatValue(invocation.errorMessage) },
  ];
  const labelWidth = details.reduce((width, detail) => Math.max(width, detail.label.length), 0);

  return details.map(
    (detail) => `${colors.label(detail.label.padEnd(labelWidth))}  ${detail.value}`,
  );
}

function printInvocationLogs(logs: FlowInvocationLogRecord[]): void {
  console.log(colors.heading('Logs'));
  console.log('');

  for (const log of logs) {
    console.log(
      `  ${colors.label(String(log.sequence).padStart(2, ' '))}. ${colors.label(log.timestamp)}  ${formatLogLevel(log.level)}  ${log.message}`,
    );

    const metadata = formatMetadata(log.metadata);

    if (metadata) {
      console.log(`      ${colors.label('Metadata'.padEnd(8))}  ${metadata}`);
    }
  }
}

export function printInvocationList(
  flow: { id: string; slug?: string },
  invocations: FlowInvocationRecord[],
): void {
  console.log('');
  console.log(
    `${colors.success('✔')} Found ${invocations.length} ${pluralize(invocations.length, 'invocation')} for flow ${formatFlowTarget(flow)}:`,
  );
  console.log('');

  for (const [index, invocation] of invocations.entries()) {
    const itemPrefix = `  ${index + 1}. `;
    console.log(
      `${itemPrefix}${colors.heading(invocation.id)}   ${colors.label(formatRelativeTime(invocation.startedAt))}`,
    );

    for (const line of formatInvocationSummaryDetailLines(
      invocation,
      ' '.repeat(itemPrefix.length),
    )) {
      console.log(line);
    }

    if (index < invocations.length - 1) {
      console.log('');
    }
  }
}

export function printNoInvocationsFound(flow: { id: string; slug?: string }): void {
  console.log('');
  console.log(`No invocations found for flow ${formatFlowTarget(flow)}.`);
}

export function printInvocationDetail(
  flow: { id: string; slug?: string },
  invocation: InvocationWithLogs,
): void {
  console.log('');
  console.log(`Invocation ${colors.heading(invocation.id)}`);
  console.log('');
  console.log(`${colors.label('Flow'.padEnd(9))}  ${formatFlowTarget(flow)}`);

  for (const line of formatInvocationDetailLines(invocation)) {
    console.log(line);
  }

  console.log('');

  if (invocation.logs.length === 0) {
    console.log('No logs captured for this invocation.');
    return;
  }

  printInvocationLogs(invocation.logs);
}

export function toLogsTokenFailure(): CliDisplayError {
  return createRequestFailure(
    'TRIGORA_DEPLOY_TOKEN is not set.',
    undefined,
    'Set your deploy token and try again.',
  );
}

export function toLogsApiFailure(
  error: unknown,
  step: string,
  target: 'flow' | 'invocation',
): CliDisplayError {
  if (error instanceof DeployApiRequestError) {
    if (error.code === 'unauthorized' || error.code === 'forbidden') {
      return createRequestFailure(
        'Deploy token is invalid or no longer active.',
        step,
        'Check your deploy token and try again.',
      );
    }

    if (error.code === 'not_found' || error.code === 'deployment_not_found') {
      return createRequestFailure(
        target === 'invocation' ? 'Invocation not found.' : 'Flow not found.',
        step,
      );
    }

    return createRequestFailure(
      error.message.trim() || 'Trigora Cloud rejected the request.',
      step,
    );
  }

  if (error instanceof DeployApiNetworkError) {
    return createRequestFailure('Network request failed.', step);
  }

  if (error instanceof DeployApiResponseError) {
    return createRequestFailure('Trigora Cloud returned an unexpected response.', step);
  }

  if (!(error instanceof Error)) {
    return createRequestFailure('Trigora Cloud could not process the request.', step);
  }

  return createRequestFailure(error.message, step);
}
