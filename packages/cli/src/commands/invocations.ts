import type {
  FlowInvocationRecord,
  GetInvocationResponse,
  ListFlowInvocationsQuery,
} from '@trigora/contracts';
import { createDeployApiClient } from '../lib/createDeployApiClient';
import { getDeployToken } from '../lib/getDeployToken';
import {
  invocationSteps,
  printInvocationDetail,
  printInvocationList,
  printNoInvocationsFound,
  toInvocationApiFailure,
  toInvocationTokenFailure,
} from '../lib/logsOutput';

type ListInvocationsOptions = Pick<ListFlowInvocationsQuery, 'flow' | 'range' | 'status'>;

type InspectInvocationOptions = {
  invocationId: string;
};

function requireDeployToken(): string {
  const token = getDeployToken();

  if (!token) {
    throw toInvocationTokenFailure();
  }

  return token;
}

function createInvocationsApiClient() {
  return createDeployApiClient({
    token: requireDeployToken(),
  });
}

export async function listInvocationsCommand(
  options: ListInvocationsOptions,
): Promise<FlowInvocationRecord[]> {
  const invocations = await createInvocationsApiClient()
    .listInvocations(options)
    .catch((error) => {
      throw toInvocationApiFailure(error, invocationSteps.fetchingInvocations, 'list');
    });

  if (invocations.length === 0) {
    printNoInvocationsFound(options);
    return invocations;
  }

  printInvocationList(invocations, options);

  return invocations;
}

export async function inspectInvocationCommand(
  options: InspectInvocationOptions,
): Promise<GetInvocationResponse['invocation']> {
  const invocation = await createInvocationsApiClient()
    .getInvocation(options.invocationId)
    .catch((error) => {
      throw toInvocationApiFailure(error, invocationSteps.fetchingInvocation, 'invocation');
    });

  printInvocationDetail(invocation);

  return invocation;
}
