import type {
  CreateDeploymentResponse,
  DeployedFlowResponse,
  DeploymentManifest,
  DeploymentManifestFlow,
} from '@trigora/contracts';
import {
  DeployApiNetworkError,
  DeployApiRequestError,
  DeployApiResponseError,
} from './createDeployApiClient';
import { CliDisplayError, printWarning } from './cliOutput';
import { colors } from './colors';

export const deploySteps = {
  activating: 'Activating deployment',
  buildingArtifact: 'Building deployment artifact',
  uploadingPackage: 'Uploading deployment package',
  validatingFlows: 'Validating flow modules',
  workerCreation: 'Creating worker runtime',
  dispatchSetup: 'Configuring dispatch',
} as const;

function getApiDetailsMessage(details: unknown): string | undefined {
  if (typeof details === 'string' && details.trim()) {
    return details.trim();
  }

  if (Array.isArray(details)) {
    const items = details.filter(
      (item): item is string => typeof item === 'string' && item.trim().length > 0,
    );

    return items.length > 0 ? items.join('; ') : undefined;
  }

  if (!details || typeof details !== 'object') {
    return undefined;
  }

  if ('message' in details && typeof details.message === 'string' && details.message.trim()) {
    return details.message.trim();
  }

  if ('error' in details && typeof details.error === 'string' && details.error.trim()) {
    return details.error.trim();
  }

  if ('errors' in details && Array.isArray(details.errors)) {
    const items = details.errors.filter(
      (item): item is string => typeof item === 'string' && item.trim().length > 0,
    );

    return items.length > 0 ? items.join('; ') : undefined;
  }

  return undefined;
}

function getApiDetailsHint(details: unknown): string | undefined {
  if (!details || typeof details !== 'object') {
    return undefined;
  }

  if ('hint' in details && typeof details.hint === 'string' && details.hint.trim()) {
    return details.hint.trim();
  }

  return undefined;
}

function normalizeInvalidCronReason(reason: string): string {
  return reason
    .trim()
    .replace(/^Invalid cron expression(?: for flow "[^"]+")?:\s*/i, '')
    .replace(/\.$/, '');
}

function getApiFailureDisplayReason(error: DeployApiRequestError): string {
  if (error.code === 'unauthorized' || error.code === 'forbidden') {
    return 'Deploy token is invalid or no longer active.';
  }

  if (error.code === 'bad_request' || error.code === 'invalid_cron_expression') {
    return (
      getApiDetailsMessage(error.details) ??
      error.message.trim() ??
      'Trigora Cloud rejected the deployment request.'
    );
  }

  return error.message.trim() || 'Trigora Cloud rejected the deployment request.';
}

function getDeployFailureStep(error: DeployApiRequestError): string {
  switch (error.step) {
    case 'worker_creation':
      return deploySteps.workerCreation;
    case 'dispatch_setup':
      return deploySteps.dispatchSetup;
    case 'activating':
      return deploySteps.activating;
    case 'uploading_package':
    default:
      return deploySteps.uploadingPackage;
  }
}

function createDeployRequestFailure(error: DeployApiRequestError, reason: string): CliDisplayError {
  const normalizedReason = reason.trim() || 'Trigora Cloud rejected the deployment request.';

  return createDeployFailure(
    getDeployFailureStep(error),
    normalizedReason,
    'Try again in a moment.',
  );
}

function createInvalidCronFailure(
  manifest: DeploymentManifest | undefined,
  error: DeployApiRequestError,
  reason: string,
): CliDisplayError {
  const normalizedReason =
    normalizeInvalidCronReason(reason) || 'Trigora Cloud rejected the cron expression.';
  const details = [
    ...(manifest?.flows.length === 1 ? [{ label: 'Flow', value: manifest.flows[0]?.id }] : []),
    { label: 'Error', value: 'Invalid cron expression' },
    { label: 'Reason', value: normalizedReason },
    {
      label: 'Hint',
      value:
        getApiDetailsHint(error.details) ??
        'Use 5 fields: minute hour day-of-month month day-of-week',
    },
  ].filter((detail): detail is { label: string; value: string } => Boolean(detail.value));

  return new CliDisplayError({
    title: 'Deployment failed',
    details,
    message: normalizedReason,
  });
}

function formatTriggerLabel(flow: DeploymentManifestFlow): string {
  switch (flow.trigger.type) {
    case 'cron':
      return 'cron';
    case 'webhook':
      return flow.trigger.event ? `webhook:${flow.trigger.event}` : 'webhook';
  }
}

function formatFlowName(flowId: string): string {
  return colors.flow(colors.heading(flowId));
}

function formatStatusMessage(status: CreateDeploymentResponse['status'], message: string): string {
  if (status === 'active') {
    return message;
  }

  return colors.label(message);
}

function getFlowStatus(
  flow: DeploymentManifestFlow,
  status: CreateDeploymentResponse['status'],
): string {
  if (status !== 'active') {
    return 'Activation is in progress';
  }

  switch (flow.trigger.type) {
    case 'cron':
      return 'Scheduled and active';
    case 'webhook':
      return 'Ready to receive events';
  }
}

function getOptionalEndpoint(flow: DeployedFlowResponse | undefined): string | undefined {
  return flow?.trigger === 'webhook' ? (flow.url ?? undefined) : undefined;
}

function getDeployedFlowLookup(
  deployment: CreateDeploymentResponse,
): Map<string, DeployedFlowResponse> {
  return new Map(deployment.flows.map((flow) => [flow.flowId, flow]));
}

function formatDetailLines(
  items: ReadonlyArray<{ label: string; value: string | undefined }>,
  indent = '   ',
  labelWidth?: number,
): string[] {
  const visibleItems = items.filter((item): item is { label: string; value: string } =>
    Boolean(item.value),
  );

  if (visibleItems.length === 0) {
    return [];
  }

  const effectiveLabelWidth =
    labelWidth ?? visibleItems.reduce((width, item) => Math.max(width, item.label.length), 0);

  return visibleItems.map(
    (item) => `${indent}${colors.label(item.label.padEnd(effectiveLabelWidth))}  ${item.value}`,
  );
}

function formatEndpointLines(endpoint: string | undefined, indent = ''): string[] {
  if (!endpoint) {
    return [];
  }

  return [`${indent}${colors.label('Endpoint')}`, `${indent}${colors.link(endpoint)}`];
}

function formatDeploymentBlock(
  flow: DeploymentManifestFlow,
  deployment: CreateDeploymentResponse,
  index?: number,
): string[] {
  const deployedFlows = getDeployedFlowLookup(deployment);
  const deployedFlow = deployedFlows.get(flow.id);
  const prefix = index === undefined ? '' : `  ${index + 1}. `;
  const detailItems = [
    { label: 'Trigger', value: formatTriggerLabel(flow) },
    { label: 'Route', value: flow.trigger.type === 'webhook' ? flow.routePath : undefined },
    { label: 'Schedule', value: flow.trigger.type === 'cron' ? flow.trigger.cron : undefined },
    {
      label: 'Timezone',
      value:
        flow.trigger.type === 'cron'
          ? deployedFlow?.trigger === 'cron'
            ? deployedFlow.timezone
            : 'UTC'
          : undefined,
    },
  ] as const;
  const visibleDetailLabels = detailItems
    .filter((item) => Boolean(item.value))
    .map((item) => item.label);
  const singleFlowLabelWidth = Math.max(
    'Flow'.length,
    ...visibleDetailLabels.map((label) => label.length),
  );
  const nameLine =
    index === undefined
      ? `${colors.label('Flow'.padEnd(singleFlowLabelWidth))}  ${formatFlowName(flow.id)}`
      : `${prefix}${formatFlowName(flow.id)}`;
  const detailIndent = index === undefined ? '' : ' '.repeat(prefix.length);
  const details = formatDetailLines(detailItems, detailIndent, singleFlowLabelWidth);
  const endpointLines = formatEndpointLines(getOptionalEndpoint(deployedFlow), detailIndent);
  const isReadyLikeStatus = deployedFlow?.status === 'active' || deployedFlow?.status === 'ready';
  const statusMessage = formatStatusMessage(
    deployment.status,
    isReadyLikeStatus
      ? getFlowStatus(flow, deployment.status)
      : (deployedFlow?.status ?? getFlowStatus(flow, deployment.status)),
  );

  return [
    nameLine,
    ...details,
    ...(endpointLines.length > 0 ? ['', ...endpointLines] : []),
    '',
    `${detailIndent}${statusMessage}`,
  ];
}

function formatDeploymentBlocks(
  flows: DeploymentManifestFlow[],
  deployment: CreateDeploymentResponse,
): string[] {
  return flows.flatMap((flow, index) => {
    const lines = formatDeploymentBlock(flow, deployment, index);

    return index === flows.length - 1 ? lines : [...lines, ''];
  });
}

export function printDeployStart(manifest: DeploymentManifest): void {
  if (manifest.flows.length === 1) {
    const [flow] = manifest.flows;

    if (!flow) {
      return;
    }

    console.log(
      `${colors.dev('Deploying flow')} ${formatFlowName(`"${flow.id}"`)}${colors.dev('...')}`,
    );
    return;
  }

  console.log(colors.dev(`Deploying ${manifest.flows.length} flows...`));
}

export function printDeployWarning(message: string): void {
  printWarning(message);
}

export function printDeploymentSummary(
  manifest: DeploymentManifest,
  deployment: CreateDeploymentResponse,
): void {
  const flows = manifest.flows;
  const flowCount = flows.length;

  console.log('');
  console.log(
    `${colors.success('✔')} ${deployment.status === 'active' ? 'Deployment complete' : 'Deployment submitted'}`,
  );
  console.log('');

  if (flowCount === 1) {
    const [flow] = flows;

    if (!flow) {
      return;
    }

    for (const line of formatDeploymentBlock(flow, deployment)) {
      console.log(line);
    }

    return;
  }

  for (const line of formatDeploymentBlocks(flows, deployment)) {
    console.log(line);
  }
}

export function createDeployFailure(step: string, reason: string, hint?: string): CliDisplayError {
  return new CliDisplayError({
    title: 'Deployment failed',
    details: [
      { label: 'Step', value: step },
      { label: 'Reason', value: reason },
    ],
    hint,
    message: reason,
  });
}

export function createStatusFailure(status: CreateDeploymentResponse['status']): CliDisplayError {
  if (status === 'failed') {
    return createDeployFailure(
      deploySteps.activating,
      'Trigora Cloud reported that the deployment could not be activated.',
      'Try again in a moment.',
    );
  }

  return createDeployFailure(deploySteps.activating, 'Deployment did not reach an active state.');
}

export function toValidationFailure(error: unknown): CliDisplayError {
  const reason = error instanceof Error ? error.message : 'Could not validate the selected flows.';

  return createDeployFailure(deploySteps.validatingFlows, reason);
}

export function toArtifactFailure(error: unknown): CliDisplayError {
  const reason =
    error instanceof Error ? error.message : 'Could not build the deployment artifact.';

  return createDeployFailure(deploySteps.buildingArtifact, reason);
}

export function toTokenFailure(): CliDisplayError {
  return createDeployFailure(
    deploySteps.uploadingPackage,
    'TRIGORA_DEPLOY_TOKEN is not set.',
    'Set TRIGORA_DEPLOY_TOKEN in your environment and try again.',
  );
}

export function toApiFailure(error: unknown, manifest?: DeploymentManifest): CliDisplayError {
  if (error instanceof DeployApiRequestError) {
    const reason = getApiFailureDisplayReason(error);

    if (error.code === 'unauthorized' || error.code === 'forbidden') {
      return createDeployFailure(
        deploySteps.uploadingPackage,
        reason,
        'Check your deploy token and try again.',
      );
    }

    if (error.code === 'invalid_cron_expression') {
      return createInvalidCronFailure(manifest, error, reason);
    }

    return createDeployRequestFailure(error, reason);
  }

  if (error instanceof DeployApiNetworkError) {
    return createDeployFailure(
      deploySteps.uploadingPackage,
      error.message,
      'Check your network connection and try again.',
    );
  }

  if (error instanceof DeployApiResponseError) {
    return createDeployFailure(
      deploySteps.activating,
      'Trigora Cloud returned an unexpected response.',
      'Try again in a moment.',
    );
  }

  if (!(error instanceof Error)) {
    return createDeployFailure(
      deploySteps.uploadingPackage,
      'Trigora Cloud could not process the deployment request.',
      'Try again in a moment.',
    );
  }

  return createDeployFailure(deploySteps.uploadingPackage, error.message, 'Try again in a moment.');
}
