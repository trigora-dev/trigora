import type { FlowSecretRecord } from '@trigora/contracts';
import {
  DeployApiNetworkError,
  DeployApiRequestError,
  DeployApiResponseError,
} from './createDeployApiClient';
import { CliDisplayError, printWarning } from './cliOutput';
import { colors } from './colors';

const SECRET_NAME_PATTERN = /^[A-Za-z_][A-Za-z0-9_]*$/;

export const secretSteps = {
  deletingSecret: 'Deleting secret',
  fetchingSecrets: 'Fetching flow secrets',
  resolvingFlow: 'Resolving flow',
  settingSecret: 'Setting secret',
} as const;

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

export function isValidSecretName(name: string): boolean {
  return SECRET_NAME_PATTERN.test(name);
}

function formatSecretName(secretName: string): string {
  return colors.heading(secretName);
}

export function formatFlowTarget(flow: { id: string; slug?: string }): string {
  if (flow.slug) {
    return `${colors.flow(colors.heading(`"${flow.slug}"`))} (${colors.label(flow.id)})`;
  }

  return colors.label(flow.id);
}

export function printSettingSecret(secretName: string, flow: { id: string; slug?: string }): void {
  const flowLabel = formatFlowTarget(flow);

  console.log('');
  console.log(`Setting secret ${formatSecretName(secretName)} for flow ${flowLabel}...`);
}

export function printDeletingSecret(secretName: string, flow: { id: string; slug?: string }): void {
  const flowLabel = formatFlowTarget(flow);

  console.log('');
  console.log(`Deleting secret ${formatSecretName(secretName)} for flow ${flowLabel}...`);
}

export function printSecretSet(secretName: string): void {
  console.log('');
  console.log(colors.success('✔ Secret set'));
  console.log('');
  console.log('Available in code as:');
  console.log(`ctx.env.${formatSecretName(secretName)}`);
}

export function printSecretsList(
  flow: { id: string; slug?: string },
  secrets: FlowSecretRecord[],
): void {
  console.log('');
  console.log(`Secrets for flow ${formatFlowTarget(flow)}:`);
  console.log('');

  const nameWidth = secrets.reduce((width, secret) => Math.max(width, secret.name.length), 0);

  for (const secret of secrets) {
    console.log(
      `${formatSecretName(secret.name.padEnd(nameWidth))}   ${colors.label(`set ${formatRelativeTime(secret.updatedAt)}`)}`,
    );
  }
}

export function printNoSecretsFound(flow: { id: string; slug?: string }): void {
  console.log('');
  console.log(`No secrets set for flow ${formatFlowTarget(flow)}.`);
}

export function printSecretDeleted(secretName: string): void {
  console.log('');
  console.log(colors.success(`✔ Secret deleted: ${formatSecretName(secretName)}`));
}

export function printSecretDeletionCanceled(secretName: string): void {
  printWarning(`Skipped deleting secret "${formatSecretName(secretName)}".`);
}

export function toSecretsTokenFailure(): CliDisplayError {
  return createRequestFailure(
    'TRIGORA_DEPLOY_TOKEN is not set.',
    undefined,
    'Set your deploy token and try again.',
  );
}

export function toSecretNameFailure(name: string): CliDisplayError {
  return createRequestFailure(
    `Invalid secret name "${name}".`,
    undefined,
    'Secret names must start with a letter or underscore and contain only letters, numbers, and underscores.',
  );
}

export function toEmptySecretValueFailure(name: string): CliDisplayError {
  return createRequestFailure(
    `Secret "${name}" cannot be empty.`,
    undefined,
    'Enter a non-empty value or pass --value for automation.',
  );
}

export function toFlowResolutionFailure(flowId: string): CliDisplayError {
  return createRequestFailure(
    `Hosted flow "${flowId}" was not found.`,
    secretSteps.resolvingFlow,
    'Run "trigora flows" to find the correct hosted flow slug and try again.',
  );
}

export function toSecretsApiFailure(
  error: unknown,
  step: string,
  target: 'flow' | 'secret',
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
        target === 'secret' ? 'Secret not found.' : 'Flow not found.',
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
