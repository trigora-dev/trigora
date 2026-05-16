import type { FlowInvocationRecord, GetFlowInvocationResponse } from '@trigora/contracts';
import { createDeployApiClient } from '../lib/createDeployApiClient';
import { getDeployToken } from '../lib/getDeployToken';
import {
  logSteps,
  printInvocationDetail,
  printInvocationList,
  printNoInvocationsFound,
  toLogsApiFailure,
  toLogsTokenFailure,
} from '../lib/logsOutput';

type ListLogsOptions = {
  flow: string;
};

type GetLogOptions = {
  flow: string;
  invocationId: string;
};

function requireDeployToken(): string {
  const token = getDeployToken();

  if (!token) {
    throw toLogsTokenFailure();
  }

  return token;
}

function createLogsApiClient() {
  return createDeployApiClient({
    token: requireDeployToken(),
  });
}

export async function listLogsCommand(options: ListLogsOptions): Promise<FlowInvocationRecord[]> {
  const apiClient = createLogsApiClient();
  const flow = await apiClient.getFlow(options.flow).catch((error) => {
    throw toLogsApiFailure(error, logSteps.resolvingFlow, 'flow');
  });

  const invocations = await apiClient.listFlowInvocations(flow.slug).catch((error) => {
    throw toLogsApiFailure(error, logSteps.fetchingInvocations, 'invocation');
  });

  if (invocations.length === 0) {
    printNoInvocationsFound(flow);
    return invocations;
  }

  printInvocationList(flow, invocations);

  return invocations;
}

export async function getLogCommand(
  options: GetLogOptions,
): Promise<GetFlowInvocationResponse['invocation']> {
  const apiClient = createLogsApiClient();
  const flow = await apiClient.getFlow(options.flow).catch((error) => {
    throw toLogsApiFailure(error, logSteps.resolvingFlow, 'flow');
  });

  const invocation = await apiClient
    .getFlowInvocation(flow.slug, options.invocationId)
    .catch((error) => {
      throw toLogsApiFailure(error, logSteps.fetchingInvocation, 'invocation');
    });

  printInvocationDetail(flow, invocation);

  return invocation;
}
