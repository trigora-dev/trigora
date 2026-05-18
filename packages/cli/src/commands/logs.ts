import type { GetInvocationResponse } from '@trigora/contracts';
import { createDeployApiClient } from '../lib/createDeployApiClient';
import { getDeployToken } from '../lib/getDeployToken';
import {
  invocationSteps,
  printInvocationLogsOutput,
  toInvocationApiFailure,
  toInvocationTokenFailure,
} from '../lib/logsOutput';

export async function getLogCommand(
  invocationId: string,
): Promise<GetInvocationResponse['invocation']> {
  const token = getDeployToken();

  if (!token) {
    throw toInvocationTokenFailure();
  }

  const invocation = await createDeployApiClient({ token })
    .getInvocation(invocationId)
    .catch((error) => {
      throw toInvocationApiFailure(error, invocationSteps.fetchingInvocation, 'invocation');
    });

  printInvocationLogsOutput(invocation);

  return invocation;
}
