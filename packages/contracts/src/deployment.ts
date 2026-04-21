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

export type DeploymentFlowResponse = {
  id: string;
  flowId: string;
  routePath: string;
  status: string;
  url: string | null;
};

export type CreateDeploymentResponse = {
  id: string;
  status: DeploymentStatus;
  manifestVersion: number;
  manifestJson: DeploymentManifestSnapshot;
  flowCount: number;
  baseUrl: string | null;
  url: string | null;
  flows: DeploymentFlowResponse[];
  createdAt: string;
  updatedAt: string;
};
