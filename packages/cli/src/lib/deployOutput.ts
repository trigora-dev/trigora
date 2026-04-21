import type {
  CreateDeploymentResponse,
  DeploymentManifest,
  DeploymentManifestFlow,
} from '@trigora/contracts';
import {
  DeployApiNetworkError,
  DeployApiRequestError,
  DeployApiResponseError,
} from './createDeployApiClient';
import {
  CliDisplayError,
  pluralize,
  printProgress,
  printSuccessSummary,
  printWarning,
} from './cliOutput';

const DEPLOY_SCOPE = 'deploy';

export const deploySteps = {
  activating: 'Activating deployment',
  buildingArtifact: 'Building deployment artifact',
  uploadingPackage: 'Uploading deployment package',
  validatingFlows: 'Validating flow modules',
  workerCreation: 'Creating worker runtime',
  dispatchSetup: 'Configuring dispatch',
} as const;

function getApiFailureDisplayReason(error: DeployApiRequestError): string {
  if (error.code === 'unauthorized' || error.code === 'forbidden') {
    return 'Deploy token is invalid or no longer active.';
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

function formatWebhookTrigger(flow: DeploymentManifestFlow): string {
  return flow.trigger.event ? `webhook:${flow.trigger.event}` : 'webhook';
}

type TriggerLike = {
  type: string;
  cron?: string;
  event?: string;
  queue?: string;
  topic?: string;
};

function getTriggerLike(flow: DeploymentManifestFlow): TriggerLike {
  return flow.trigger as TriggerLike;
}

function formatTriggerLabel(flow: DeploymentManifestFlow): string {
  const trigger = getTriggerLike(flow);

  switch (trigger.type) {
    case 'cron':
      return 'cron';
    case 'queue':
      return 'queue';
    case 'webhook':
      return formatWebhookTrigger(flow);
    default:
      return trigger.type;
  }
}

function getFlowStatus(
  flow: DeploymentManifestFlow,
  status: CreateDeploymentResponse['status'],
): string {
  if (status !== 'active') {
    return 'Activation is in progress';
  }

  const trigger = getTriggerLike(flow);

  switch (trigger.type) {
    case 'cron':
      return 'Scheduled and active';
    case 'queue':
      return 'Ready to consume messages';
    case 'webhook':
    default:
      return 'Ready to receive events';
  }
}

function getOptionalEndpoint(flow: CreateDeploymentResponse['flows'][number]): string | undefined {
  return flow.url ?? undefined;
}

function getDeployedFlowLookup(
  deployment: CreateDeploymentResponse,
): Map<string, CreateDeploymentResponse['flows'][number]> {
  return new Map(deployment.flows.map((flow) => [`${flow.flowId}:${flow.routePath}`, flow]));
}

function formatFlowDetailLines(
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

  return visibleItems.map((item) => {
    return `${indent}${item.label.padEnd(labelWidth)}  ${item.value}`;
  });
}

function formatActivatedFlows(
  flows: DeploymentManifestFlow[],
  deployment: CreateDeploymentResponse,
): string[] {
  const deployedFlows = getDeployedFlowLookup(deployment);

  return flows.flatMap((flow, index) => {
    const trigger = getTriggerLike(flow);
    const deployedFlow = deployedFlows.get(`${flow.id}:${flow.routePath}`);
    const lines = [
      `${index + 1}. ${flow.id}`,
      ...formatFlowDetailLines([
        { label: 'Trigger', value: formatTriggerLabel(flow) },
        { label: 'Route', value: trigger.type === 'webhook' ? flow.routePath : undefined },
        { label: 'Endpoint', value: deployedFlow ? getOptionalEndpoint(deployedFlow) : undefined },
        { label: 'Schedule', value: trigger.type === 'cron' ? trigger.cron : undefined },
        {
          label: 'Queue',
          value: trigger.type === 'queue' ? (trigger.queue ?? trigger.topic) : undefined,
        },
        {
          label: 'Status',
          value:
            deployedFlow?.status === 'active'
              ? getFlowStatus(flow, deployment.status)
              : (deployedFlow?.status ?? getFlowStatus(flow, deployment.status)),
        },
      ]),
    ];

    return index === flows.length - 1 ? lines : [...lines, ''];
  });
}

function getSummaryTitle(status: CreateDeploymentResponse['status']): string {
  return status === 'active' ? 'Deployment complete' : 'Deployment submitted';
}

function getSummaryFooter(status: CreateDeploymentResponse['status'], flowCount: number): string {
  if (status === 'active') {
    return flowCount === 1 ? 'Ready to receive events' : '';
  }

  return 'Activation is in progress';
}

export function printDeployProgress(message: string): void {
  printProgress(DEPLOY_SCOPE, message);
}

export function printDeployWarning(message: string): void {
  printWarning(message);
}

export function printPreparedFlows(flowCount: number): void {
  printDeployProgress(`Prepared ${flowCount} ${pluralize(flowCount, 'flow')} for deployment`);
}

export function printDeploymentSummary(
  manifest: DeploymentManifest,
  deployment: CreateDeploymentResponse,
): void {
  const flows = manifest.flows;
  const flowCount = flows.length;
  const details =
    flowCount === 1
      ? [
          { label: 'Flow', value: flows[0]!.id },
          { label: 'Trigger', value: formatTriggerLabel(flows[0]!) },
          { label: 'Route', value: flows[0]!.routePath },
        ]
      : [
          { label: 'Deployment', value: deployment.id },
          { label: 'Flows', value: String(flowCount) },
        ];

  const sections =
    flowCount === 1
      ? [
          ...(deployment.url
            ? [
                {
                  title: 'Endpoint',
                  lines: [deployment.url],
                },
              ]
            : []),
        ]
      : [
          {
            title: 'Activated flows',
            lines: formatActivatedFlows(flows, deployment),
          },
        ];

  printSuccessSummary(
    getSummaryTitle(deployment.status),
    details,
    sections,
    getSummaryFooter(deployment.status, manifest.flows.length) || undefined,
  );
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

export function toApiFailure(error: unknown): CliDisplayError {
  if (error instanceof DeployApiRequestError) {
    const reason = getApiFailureDisplayReason(error);

    if (error.code === 'unauthorized' || error.code === 'forbidden') {
      return createDeployFailure(
        deploySteps.uploadingPackage,
        reason,
        'Check your deploy token and try again.',
      );
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
