import type { CronTrigger, WebhookTrigger } from './trigger';

type BaseDeploymentManifestFlow = {
  entrypoint: string;
  id: string;
};

export type WebhookDeploymentManifestFlow = BaseDeploymentManifestFlow & {
  routePath: string;
  trigger: WebhookTrigger;
};

export type CronDeploymentManifestFlow = BaseDeploymentManifestFlow & {
  routePath?: never;
  trigger: CronTrigger;
};

export type DeploymentManifestFlow = WebhookDeploymentManifestFlow | CronDeploymentManifestFlow;

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

type BaseDeployedFlowResponse = {
  id: string;
  flowId: string;
  status: string;
};

export type WebhookDeployedFlowResponse = BaseDeployedFlowResponse & {
  trigger: 'webhook';
  routePath: string;
  url: string | null;
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
  flowCount: number;
  baseUrl: string | null;
  url: string | null;
  flows: DeployedFlowResponse[];
  createdAt: string;
  updatedAt: string;
};
