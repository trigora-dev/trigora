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
  flow: string;
  name: string;
  value?: string;
};

type ListSecretsOptions = {
  flow?: string;
};

type DeleteSecretOptions = {
  flow: string;
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
  printSettingSecret(options.name, options.flow);
  const value = await getSecretValue(options.name, options.value);

  const secret = await apiClient
    .setFlowSecret(options.flow, {
      name: options.name,
      value,
    })
    .catch((error) => {
      throw toSecretsApiFailure(error, secretSteps.settingSecret, 'secret');
    });

  printSecretSet(options.name);

  return secret;
}

export async function listSecretsCommand(
  options: ListSecretsOptions = {},
): Promise<FlowSecretRecord[]> {
  const apiClient = createSecretsApiClient();
  const secrets = await apiClient
    .listSecrets(options.flow ? { flow: options.flow } : {})
    .catch((error) => {
      throw toSecretsApiFailure(error, secretSteps.fetchingSecrets, 'secret');
    });

  if (secrets.length === 0) {
    printNoSecretsFound(options);
    return secrets;
  }

  printSecretsList(secrets, options);

  return secrets;
}

export async function deleteSecretCommand(
  options: DeleteSecretOptions,
): Promise<DeleteFlowSecretResponse | null> {
  validateSecretName(options.name);
  const apiClient = createSecretsApiClient();

  if (!options.yes) {
    const confirmed = await confirmAction(
      `Delete secret "${options.name}" for flow "${options.flow}"?`,
    );

    if (!confirmed) {
      printSecretDeletionCanceled(options.name);
      return null;
    }
  }

  printDeletingSecret(options.name, options.flow);

  const response = await apiClient.deleteFlowSecret(options.flow, options.name).catch((error) => {
    throw toSecretsApiFailure(error, secretSteps.deletingSecret, 'secret');
  });

  printSecretDeleted(options.name);

  return response;
}
