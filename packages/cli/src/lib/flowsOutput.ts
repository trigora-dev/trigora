import type { DisableFlowResponse, FlowRecord } from '@trigora/contracts';
import {
  DeployApiNetworkError,
  DeployApiRequestError,
  DeployApiResponseError,
} from './createDeployApiClient';
import {
  CliDetail,
  CliDisplayError,
  pluralize,
  printProgress,
  printSuccessSummary,
} from './cliOutput';
import { colors } from './colors';

const FLOWS_SCOPE = 'flows';

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

function formatDetailLines(
  items: Array<{ label: string; value: string | undefined }>,
  indent = '   ',
): string[] {
  const visibleItems = items.filter((item): item is { label: string; value: string } =>
    Boolean(item.value),
  );

  if (visibleItems.length === 0) {
    return [];
  }

  const labelWidth = visibleItems.reduce((width, item) => Math.max(width, item.label.length), 0);

  return visibleItems.map((item) => `${indent}${item.label.padEnd(labelWidth)}  ${item.value}`);
}

function createDetails(items: Array<{ label: string; value: string | undefined }>): CliDetail[] {
  return items.filter((item): item is CliDetail => Boolean(item.value));
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

export function printFlowsProgress(message: string): void {
  printProgress(FLOWS_SCOPE, message);
}

export function printFlowList(flows: FlowRecord[]): void {
  const flowCount = flows.length;

  console.log('');
  console.log(colors.success(`✔ Found ${flowCount} ${pluralize(flowCount, 'flow')}`));
  console.log('');

  for (const [index, flow] of flows.entries()) {
    const lines = [
      `  ${index + 1}. ${getFlowName(flow)}`,
      ...formatDetailLines([
        { label: 'ID', value: flow.id },
        { label: 'Trigger', value: formatTriggerLabel(flow.trigger) },
        { label: 'Status', value: flow.status },
        { label: 'Endpoint', value: flow.trigger === 'webhook' ? flow.endpoint : undefined },
        { label: 'Schedule', value: flow.trigger === 'cron' ? flow.schedule : undefined },
        { label: 'Queue', value: flow.trigger === 'queue' ? flow.queue : undefined },
      ]),
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
  printSuccessSummary('No deployed flows found', []);
}

export function printFlowSummary(flow: FlowRecord): void {
  printSuccessSummary(
    'Flow details',
    createDetails([
      { label: 'Name', value: getFlowName(flow) },
      { label: 'ID', value: flow.id },
      { label: 'Trigger', value: formatTriggerLabel(flow.trigger) },
      { label: 'Status', value: flow.status },
      { label: 'Created', value: flow.createdAt },
      { label: 'Endpoint', value: flow.trigger === 'webhook' ? flow.endpoint : undefined },
      { label: 'Route', value: flow.trigger === 'webhook' ? flow.route : undefined },
      { label: 'Schedule', value: flow.trigger === 'cron' ? flow.schedule : undefined },
      { label: 'Queue', value: flow.trigger === 'queue' ? flow.queue : undefined },
    ]),
  );
}

function printFlowStatusChange(title: string, flow: DisableFlowResponse['flow']): void {
  printSuccessSummary(title, [
    { label: 'ID', value: flow.id },
    { label: 'Status', value: flow.status },
  ]);
}

export function printFlowDisabled(flow: DisableFlowResponse['flow']): void {
  printFlowStatusChange('Flow disabled', flow);
}

export function printFlowEnabled(flow: DisableFlowResponse['flow']): void {
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
