import type { WebhookTrigger } from './trigger';

export type DeploymentManifestFlow = {
  entrypoint: string;
  routePath: string;
  id: string;
  trigger: WebhookTrigger;
};

export type DeploymentManifest = {
  version: 1;
  flows: DeploymentManifestFlow[];
};

export type DeploymentArtifactFile = {
  contents: string;
  entrypoint: string;
  path: string;
};

export type DeploymentArtifact = {
  files: DeploymentArtifactFile[];
  format: 'esm';
  target: 'node20';
  version: 1;
};

export type CreateDeploymentRequest = {
  artifact: DeploymentArtifact;
  manifest: DeploymentManifest;
};

export type DeploymentStatus = 'pending' | 'active' | 'failed';

export type DeploymentManifestSnapshot = {
  version: number;
  flows: DeploymentManifestFlow[];
};

export type DeployedFlowResponse = {
  /**
   * Public identifier for the deployed flow instance.
   * This is distinct from the source flow id in the manifest.
   */
  id: string;
  /**
   * Source flow id from the original deployment manifest.
   */
  flowId: string;
  routePath: string;
  status: string;
  /**
   * Public deployed URL for this flow when available.
   */
  url: string | null;
};

/**
 * @deprecated Prefer `DeployedFlowResponse` for clearer semantics in new code.
 */
export type DeploymentFlowResponse = DeployedFlowResponse;

export type CreateDeploymentResponse = {
  id: string;
  status: DeploymentStatus;
  manifestVersion: number;
  manifestJson: DeploymentManifestSnapshot;
  flowCount: number;
  baseUrl: string | null;
  /**
   * Public deployed URL for single-flow deployments.
   * Multi-flow deployments should return null and use `flows[*].url`.
   */
  url: string | null;
  flows: DeployedFlowResponse[];
  createdAt: string;
  updatedAt: string;
};
