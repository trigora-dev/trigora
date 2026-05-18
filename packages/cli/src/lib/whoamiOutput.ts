import type { WhoAmIResponse } from '@trigora/contracts';
import {
  DeployApiNetworkError,
  DeployApiRequestError,
  DeployApiResponseError,
} from './createDeployApiClient';
import { CliDisplayError } from './cliOutput';
import { colors } from './colors';

export const whoAmISteps = {
  fetchingIdentity: 'Fetching identity',
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

function formatTokenStatus(status: string): string {
  switch (status) {
    case 'active':
      return colors.success(status);
    case 'disabled':
      return colors.warn(status);
    case 'revoked':
      return colors.error(status);
    default:
      return status;
  }
}

export function printWhoAmI(identity: WhoAmIResponse): void {
  const details =
    identity.actorType === 'deploy_token'
      ? [
          {
            label: 'Workspace',
            value: colors.flow(colors.heading(identity.workspace.slug)),
          },
          {
            label: 'Token',
            value: colors.heading(identity.token.label),
          },
          {
            label: 'Status',
            value: formatTokenStatus(identity.token.status),
          },
        ]
      : [
          {
            label: 'Workspace',
            value: colors.flow(colors.heading(identity.workspace.slug)),
          },
          {
            label: 'User',
            value: colors.heading(identity.user.email),
          },
          {
            label: 'Role',
            value: identity.workspace.role,
          },
        ];
  const labelWidth = details.reduce((width, detail) => Math.max(width, detail.label.length), 0);

  console.log('');

  for (const detail of details) {
    console.log(`${colors.label(detail.label.padEnd(labelWidth))}  ${detail.value}`);
  }
}

export function toWhoAmITokenFailure(): CliDisplayError {
  return createRequestFailure(
    'TRIGORA_DEPLOY_TOKEN is not set.',
    undefined,
    'Set your deploy token and try again.',
  );
}

export function toWhoAmIApiFailure(error: unknown, step: string): CliDisplayError {
  if (error instanceof DeployApiRequestError) {
    if (error.code === 'unauthorized' || error.code === 'forbidden') {
      return createRequestFailure(
        'Deploy token is invalid or no longer active.',
        step,
        'Check your deploy token and try again.',
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
