import type { DeploymentManifest } from '@trigora/contracts';
import { buildDeploymentArtifact } from '../lib/buildDeploymentArtifact';
import { buildDeploymentManifest } from '../lib/buildDeploymentManifest';
import { createDeployApiClient } from '../lib/createDeployApiClient';
import {
  createStatusFailure,
  printDeployProgress,
  printDeploymentSummary,
  printPreparedFlows,
  toApiFailure,
  toArtifactFailure,
  toTokenFailure,
  toValidationFailure,
} from '../lib/deployOutput';

type DeployOptions = {
  filePath?: string;
};

function getDeployToken(): string {
  const token = process.env.TRIGORA_DEPLOY_TOKEN?.trim();

  if (!token) {
    throw toTokenFailure();
  }

  return token;
}

export async function deployCommand(options: DeployOptions): Promise<DeploymentManifest> {
  printDeployProgress('Validating flow modules...');

  let manifest: DeploymentManifest;

  try {
    manifest = await buildDeploymentManifest(options);
  } catch (error) {
    throw toValidationFailure(error);
  }

  printPreparedFlows(manifest.flows.length);

  printDeployProgress('Building deployment artifact...');

  const artifact = await buildDeploymentArtifact(manifest).catch((error) => {
    throw toArtifactFailure(error);
  });
  const apiClient = createDeployApiClient({
    token: getDeployToken(),
  });

  printDeployProgress('Uploading deployment package...');

  const deployment = await apiClient.createDeployment({ manifest, artifact }).catch((error) => {
    throw toApiFailure(error);
  });

  printDeployProgress('Activating deployment...');

  if (deployment.status === 'failed') {
    throw createStatusFailure(deployment.status);
  }

  printDeploymentSummary(manifest, deployment);

  return manifest;
}
