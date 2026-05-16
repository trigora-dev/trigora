import type { DeleteFlowSecretResponse, FlowSecretRecord } from '@trigora/contracts';
import { createDeployApiClient } from '../lib/createDeployApiClient';
import { confirmAction, promptForSecretValue } from '../lib/interactive';
import {
  printDeletingSecret,
  isValidSecretName,
  printNoSecretsFound,
  printSecretDeleted,
  printSecretDeletionCanceled,
  printSettingSecret,
  printSecretSet,
  printSecretsList,
  secretSteps,
  toEmptySecretValueFailure,
  toSecretNameFailure,
  toSecretsApiFailure,
  toSecretsTokenFailure,
} from '../lib/secretsOutput';
import { getDeployToken } from '../lib/getDeployToken';

type SetSecretOptions = {
  flowId: string;
  name: string;
  value?: string;
};

type ListSecretsOptions = {
  flowId: string;
};

type DeleteSecretOptions = {
  flowId: string;
  name: string;
  yes?: boolean;
};

function requireDeployToken(): string {
  const token = getDeployToken();

  if (!token) {
    throw toSecretsTokenFailure();
  }

  return token;
}

function createSecretsApiClient() {
  return createDeployApiClient({
    token: requireDeployToken(),
  });
}

function validateSecretName(name: string): void {
  if (!isValidSecretName(name)) {
    throw toSecretNameFailure(name);
  }
}

async function getSecretValue(name: string, providedValue?: string): Promise<string> {
  if (providedValue !== undefined) {
    if (providedValue.length === 0) {
      throw toEmptySecretValueFailure(name);
    }

    return providedValue;
  }

  const value = await promptForSecretValue(name);

  if (value.length === 0) {
    throw toEmptySecretValueFailure(name);
  }

  return value;
}

export async function setSecretCommand(options: SetSecretOptions): Promise<FlowSecretRecord> {
  validateSecretName(options.name);
  const apiClient = createSecretsApiClient();

  const flow = await apiClient.getFlow(options.flowId).catch((error) => {
    throw toSecretsApiFailure(error, secretSteps.resolvingFlow, 'flow');
  });

  printSettingSecret(options.name, flow);
  const value = await getSecretValue(options.name, options.value);

  const secret = await apiClient
    .setFlowSecret(flow.id, {
      name: options.name,
      value,
    })
    .catch((error) => {
      throw toSecretsApiFailure(error, secretSteps.settingSecret, 'secret');
    });

  printSecretSet(options.name);

  return secret;
}

export async function listSecretsCommand(options: ListSecretsOptions): Promise<FlowSecretRecord[]> {
  const apiClient = createSecretsApiClient();
  const flow = await apiClient.getFlow(options.flowId).catch((error) => {
    throw toSecretsApiFailure(error, secretSteps.resolvingFlow, 'flow');
  });

  const secrets = await apiClient.listFlowSecrets(flow.id).catch((error) => {
    throw toSecretsApiFailure(error, secretSteps.fetchingSecrets, 'secret');
  });

  if (secrets.length === 0) {
    printNoSecretsFound(flow);
    return secrets;
  }

  printSecretsList(flow, secrets);

  return secrets;
}

export async function deleteSecretCommand(
  options: DeleteSecretOptions,
): Promise<DeleteFlowSecretResponse | null> {
  validateSecretName(options.name);
  const apiClient = createSecretsApiClient();

  const flow = await apiClient.getFlow(options.flowId).catch((error) => {
    throw toSecretsApiFailure(error, secretSteps.resolvingFlow, 'flow');
  });

  if (!options.yes) {
    const confirmed = await confirmAction(
      `Delete secret "${options.name}" for flow "${flow.slug}"?`,
    );

    if (!confirmed) {
      printSecretDeletionCanceled(options.name);
      return null;
    }
  }

  printDeletingSecret(options.name, flow);

  const response = await apiClient.deleteFlowSecret(flow.id, options.name).catch((error) => {
    throw toSecretsApiFailure(error, secretSteps.deletingSecret, 'secret');
  });

  printSecretDeleted(options.name);

  return response;
}
