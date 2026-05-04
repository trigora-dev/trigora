import type { DeploymentManifest } from '@trigora/contracts';
import { buildDeploymentArtifact } from '../lib/buildDeploymentArtifact';
import { buildDeploymentManifest } from '../lib/buildDeploymentManifest';
import { createDeployApiClient } from '../lib/createDeployApiClient';
import {
  createStatusFailure,
  printDeployStart,
  printDeploymentSummary,
  toApiFailure,
  toArtifactFailure,
  toTokenFailure,
  toValidationFailure,
} from '../lib/deployOutput';
import { getDeployToken } from '../lib/getDeployToken';

type DeployOptions = {
  filePath?: string;
};

function requireDeployToken(): string {
  const token = getDeployToken();

  if (!token) {
    throw toTokenFailure();
  }

  return token;
}

export async function deployCommand(options: DeployOptions): Promise<DeploymentManifest> {
  let manifest: DeploymentManifest;

  try {
    manifest = await buildDeploymentManifest(options);
  } catch (error) {
    throw toValidationFailure(error);
  }
  printDeployStart(manifest);

  const artifact = await buildDeploymentArtifact(manifest).catch((error) => {
    throw toArtifactFailure(error);
  });
  const apiClient = createDeployApiClient({
    token: requireDeployToken(),
  });

  const deployment = await apiClient.createDeployment({ manifest, artifact }).catch((error) => {
    throw toApiFailure(error);
  });

  if (deployment.status === 'failed') {
    throw createStatusFailure(deployment.status);
  }

  printDeploymentSummary(manifest, deployment);

  return manifest;
}
