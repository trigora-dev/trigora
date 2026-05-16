import type { CronTrigger, WebhookTrigger } from './trigger';

export type DeploymentManifestFlow = {
  entrypoint: string;
  id: string;
  trigger: WebhookTrigger | CronTrigger;
};

export type DeploymentManifest = {
  version: 1;
  flow: DeploymentManifestFlow;
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

export type DeploymentStatus = 'active' | 'failed';

export type DeploymentManifestSnapshot = DeploymentManifest;

type BaseDeployedFlowResponse = {
  id: string;
  slug: string;
  status: 'ready' | 'disabled' | 'failed';
};

export type WebhookDeployedFlowResponse = BaseDeployedFlowResponse & {
  trigger: 'webhook';
  url: string;
};

export type CronDeployedFlowResponse = BaseDeployedFlowResponse & {
  trigger: 'cron';
  schedule: string;
  timezone: 'UTC';
  url: null;
};

export type DeployedFlowResponse = WebhookDeployedFlowResponse | CronDeployedFlowResponse;

export type CreateDeploymentResponse = {
  id: string;
  status: DeploymentStatus;
  manifestVersion: number;
  manifestJson: DeploymentManifestSnapshot;
  flow: DeployedFlowResponse;
  createdAt: string;
  updatedAt: string;
};
