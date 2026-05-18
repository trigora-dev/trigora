import type { FlowStatusResponse, FlowRecord } from '@trigora/contracts';
import { createDeployApiClient } from '../lib/createDeployApiClient';
import { printWarning } from '../lib/cliOutput';
import { promptForTypedConfirmation } from '../lib/interactive';
import {
  printFlowDeleted,
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

export async function inspectFlowCommand(flow: string): Promise<FlowRecord> {
  const flowRecord = await createFlowsApiClient()
    .getFlow(flow)
    .catch((error) => {
      throw toFlowsApiFailure(error, flowSteps.fetchingFlow);
    });

  printFlowSummary(flowRecord);

  return flowRecord;
}

export async function disableFlowCommand(flow: string): Promise<FlowStatusResponse['flow']> {
  const flowRecord = await createFlowsApiClient()
    .disableFlow(flow)
    .catch((error) => {
      throw toFlowsApiFailure(error, flowSteps.disablingFlow);
    });

  printFlowDisabled(flowRecord);

  return flowRecord;
}

export async function enableFlowCommand(flow: string): Promise<FlowStatusResponse['flow']> {
  const flowRecord = await createFlowsApiClient()
    .enableFlow(flow)
    .catch((error) => {
      throw toFlowsApiFailure(error, flowSteps.enablingFlow);
    });

  printFlowEnabled(flowRecord);

  return flowRecord;
}

export async function deleteFlowCommand(
  flow: string,
  options: { yes?: boolean } = {},
): Promise<void | null> {
  if (!options.yes) {
    const confirmed = await promptForTypedConfirmation({
      expectedValue: flow,
      message: `This will delete flow "${flow}", including deployments, invocations, logs, schedules, secrets, and hosted workers.`,
      nonInteractiveHint: 'Re-run with --yes to confirm in non-interactive environments.',
      nonInteractiveReason: `Confirmation is required before deleting flow "${flow}".`,
    });

    if (!confirmed) {
      printWarning(`Skipped deleting flow "${flow}".`);
      return null;
    }
  }

  await createFlowsApiClient()
    .deleteFlow(flow)
    .catch((error) => {
      throw toFlowsApiFailure(error, flowSteps.deletingFlow);
    });

  printFlowDeleted(flow);
}
