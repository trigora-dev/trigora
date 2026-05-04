import type { FlowStatusResponse, FlowRecord } from '@trigora/contracts';
import { createDeployApiClient } from '../lib/createDeployApiClient';
import {
  flowSteps,
  printFlowDisabled,
  printFlowEnabled,
  printFlowList,
  printFlowSummary,
  printNoFlowsFound,
  toFlowsApiFailure,
  toFlowsTokenFailure,
} from '../lib/flowsOutput';
import { getDeployToken } from '../lib/getDeployToken';

function requireDeployToken(): string {
  const token = getDeployToken();

  if (!token) {
    throw toFlowsTokenFailure();
  }

  return token;
}

function createFlowsApiClient() {
  return createDeployApiClient({
    token: requireDeployToken(),
  });
}

export async function listFlowsCommand(): Promise<FlowRecord[]> {
  const flows = await createFlowsApiClient()
    .listFlows()
    .catch((error) => {
      throw toFlowsApiFailure(error, flowSteps.fetchingFlows);
    });

  if (flows.length === 0) {
    printNoFlowsFound();
    return flows;
  }

  printFlowList(flows);

  return flows;
}

export async function inspectFlowCommand(flowId: string): Promise<FlowRecord> {
  const flow = await createFlowsApiClient()
    .getFlow(flowId)
    .catch((error) => {
      throw toFlowsApiFailure(error, flowSteps.fetchingFlow);
    });

  printFlowSummary(flow);

  return flow;
}

export async function disableFlowCommand(flowId: string): Promise<FlowStatusResponse['flow']> {
  const flow = await createFlowsApiClient()
    .disableFlow(flowId)
    .catch((error) => {
      throw toFlowsApiFailure(error, flowSteps.disablingFlow);
    });

  printFlowDisabled(flow);

  return flow;
}

export async function enableFlowCommand(flowId: string): Promise<FlowStatusResponse['flow']> {
  const flow = await createFlowsApiClient()
    .enableFlow(flowId)
    .catch((error) => {
      throw toFlowsApiFailure(error, flowSteps.enablingFlow);
    });

  printFlowEnabled(flow);

  return flow;
}
