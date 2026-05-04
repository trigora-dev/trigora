import type { FlowRecord, FlowStatusResponse } from '@trigora/contracts';
import {
  DeployApiNetworkError,
  DeployApiRequestError,
  DeployApiResponseError,
} from './createDeployApiClient';
import { CliDetail, CliDisplayError, pluralize } from './cliOutput';
import { colors } from './colors';

export const flowSteps = {
  disablingFlow: 'Disabling flow',
  enablingFlow: 'Enabling flow',
  fetchingFlow: 'Fetching flow',
  fetchingFlows: 'Fetching deployed flows',
} as const;

function getFlowName(flow: FlowRecord): string {
  return flow.name;
}

function formatTriggerLabel(trigger: FlowRecord['trigger']): string {
  return trigger;
}

function formatFlowName(flow: FlowRecord): string {
  return colors.flow(colors.heading(getFlowName(flow)));
}

function formatFlowStatus(status: FlowRecord['status']): string {
  switch (status) {
    case 'ready':
      return colors.success(status);
    case 'disabled':
      return colors.warn(status);
    case 'failed':
      return colors.error(status);
  }
}

function formatFlowListValue(label: string, flow: FlowRecord): string | undefined {
  switch (label) {
    case 'ID':
      return colors.label(flow.id);
    case 'Trigger':
      return formatTriggerLabel(flow.trigger);
    case 'Status':
      return formatFlowStatus(flow.status);
    case 'Endpoint':
      return flow.trigger === 'webhook' ? colors.link(flow.endpoint) : undefined;
    case 'Schedule':
      return flow.trigger === 'cron' ? flow.schedule : undefined;
    case 'Queue':
      return flow.trigger === 'queue' ? flow.queue : undefined;
    default:
      return undefined;
  }
}

function formatFlowSummaryValue(label: string, flow: FlowRecord): string | undefined {
  switch (label) {
    case 'ID':
      return colors.label(flow.id);
    case 'Trigger':
      return formatTriggerLabel(flow.trigger);
    case 'Status':
      return formatFlowStatus(flow.status);
    case 'Created':
      return colors.label(flow.createdAt);
    case 'Endpoint':
      return flow.trigger === 'webhook' ? colors.link(flow.endpoint) : undefined;
    case 'Route':
      return flow.trigger === 'webhook' ? flow.route : undefined;
    case 'Schedule':
      return flow.trigger === 'cron' ? flow.schedule : undefined;
    case 'Queue':
      return flow.trigger === 'queue' ? flow.queue : undefined;
    default:
      return undefined;
  }
}

function formatFlowListDetailLines(flow: FlowRecord, indent = '   '): string[] {
  const labels = ['ID', 'Trigger', 'Status', 'Endpoint', 'Schedule', 'Queue'] as const;
  const visibleItems = labels
    .map((label) => ({ label, value: formatFlowListValue(label, flow) }))
    .filter((item): item is { label: (typeof labels)[number]; value: string } =>
      Boolean(item.value),
    );

  if (visibleItems.length === 0) {
    return [];
  }

  const labelWidth = visibleItems.reduce((width, item) => Math.max(width, item.label.length), 0);

  return visibleItems.map(
    (item) => `${indent}${colors.label(item.label.padEnd(labelWidth))}  ${item.value}`,
  );
}

function formatFlowSummaryDetailLines(flow: FlowRecord, indent = ''): string[] {
  const labels = [
    'ID',
    'Trigger',
    'Status',
    'Created',
    'Endpoint',
    'Route',
    'Schedule',
    'Queue',
  ] as const;
  const visibleItems = labels
    .map((label) => ({ label, value: formatFlowSummaryValue(label, flow) }))
    .filter((item): item is { label: (typeof labels)[number]; value: string } =>
      Boolean(item.value),
    );

  if (visibleItems.length === 0) {
    return [];
  }

  const labelWidth = visibleItems.reduce((width, item) => Math.max(width, item.label.length), 0);

  return visibleItems.map(
    (item) => `${indent}${colors.label(item.label.padEnd(labelWidth))}  ${item.value}`,
  );
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

export function printFlowList(flows: FlowRecord[]): void {
  const flowCount = flows.length;

  console.log(`${colors.success('✔')} Found ${flowCount} ${pluralize(flowCount, 'flow')}:`);
  console.log('');

  for (const [index, flow] of flows.entries()) {
    const itemPrefix = `  ${index + 1}. `;
    const lines = [
      `${itemPrefix}${formatFlowName(flow)}`,
      ...formatFlowListDetailLines(flow, ' '.repeat(itemPrefix.length)),
    ];

    for (const line of lines) {
      console.log(line);
    }

    if (index < flows.length - 1) {
      console.log('');
    }
  }
}

export function printNoFlowsFound(): void {
  console.log('No deployed flows found.');
}

export function printFlowSummary(flow: FlowRecord): void {
  console.log(formatFlowName(flow));
  console.log('');

  for (const line of formatFlowSummaryDetailLines(flow)) {
    console.log(line);
  }
}

function printFlowStatusChange(title: string, flow: FlowStatusResponse['flow']): void {
  const formattedStatus =
    flow.status === 'ready'
      ? colors.success(flow.status)
      : flow.status === 'disabled'
        ? colors.warn(flow.status)
        : colors.error(flow.status);

  console.log('');
  console.log(`${colors.success('✔')} ${title}`);
  console.log('');
  console.log(`${colors.label('ID'.padEnd(6))}  ${colors.label(flow.id)}`);
  console.log(`${colors.label('Status'.padEnd(6))}  ${formattedStatus}`);
}

export function printFlowDisabled(flow: FlowStatusResponse['flow']): void {
  printFlowStatusChange('Flow disabled', flow);
}

export function printFlowEnabled(flow: FlowStatusResponse['flow']): void {
  printFlowStatusChange('Flow enabled', flow);
}

export function toFlowsTokenFailure(): CliDisplayError {
  return createRequestFailure(
    'TRIGORA_DEPLOY_TOKEN is not set.',
    undefined,
    'Set your deploy token and try again.',
  );
}

export function toFlowsApiFailure(error: unknown, step: string): CliDisplayError {
  if (error instanceof DeployApiRequestError) {
    if (error.code === 'unauthorized' || error.code === 'forbidden') {
      return createRequestFailure(
        'Deploy token is invalid or no longer active.',
        step,
        'Check your deploy token and try again.',
      );
    }

    if (error.code === 'not_found' || error.code === 'deployment_not_found') {
      return createRequestFailure('Flow not found.', step);
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
