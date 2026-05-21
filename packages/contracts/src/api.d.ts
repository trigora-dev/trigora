export type ApiErrorCode =
  | 'bad_request'
  | 'conflict'
  | 'deployment_not_found'
  | 'forbidden'
  | 'internal_error'
  | 'invalid_cron_expression'
  | 'not_found'
  | 'rate_limited'
  | 'unauthorized';
export type ApiErrorStep =
  | 'uploading_package'
  | 'worker_creation'
  | 'dispatch_setup'
  | 'activating';
export type ApiErrorResponse = {
  error: {
    code: ApiErrorCode;
    message: string;
    step?: ApiErrorStep;
    details?: unknown;
  };
};
export type FlowTriggerType = 'webhook' | 'cron' | 'queue';
export type FlowStatus = 'ready' | 'disabled' | 'failed';
export type Plan = 'free' | 'pro' | 'scale' | 'internal';
type BaseFlowRecord = {
  createdAt: string;
  id: string;
  slug: string;
  status: FlowStatus;
};
export type WebhookFlowRecord = BaseFlowRecord & {
  endpoint: string;
  routePath: string;
  trigger: 'webhook';
};
export type CronFlowRecord = BaseFlowRecord & {
  trigger: 'cron';
  schedule: string;
  timezone: 'UTC';
};
export type QueueFlowRecord = BaseFlowRecord & {
  queue?: string;
  trigger: 'queue';
};
export type FlowRecord = WebhookFlowRecord | CronFlowRecord | QueueFlowRecord;
export type ListFlowsResponse = {
  flows: FlowRecord[];
};
export type GetFlowResponse = {
  flow: FlowRecord;
};
export type WorkspaceRecord = {
  id: string;
  name: string;
  plan: Plan;
  slug: string;
};
export type UserRecord = {
  id: string;
  email: string;
  emailVerified: boolean;
  image: string | null;
  name: string;
};
export type CurrentWorkspaceRecord = WorkspaceRecord & {
  role: string;
};
export type UserWorkspaceRecord = CurrentWorkspaceRecord & {
  isCurrent: boolean;
};
export type WorkspaceActivity = {
  createdAt: string;
  updatedAt: string;
  flowCount: number;
  tokenCount: number;
};
export type DeployTokenRecord = {
  id: string;
  label: string;
  status: string;
  createdAt: string;
};
export type WorkspaceDeployTokenRecord = DeployTokenRecord & {
  lastUsedAt: string | null;
};
export type WhoAmIResponse =
  | {
      actorType: 'deploy_token';
      workspace: WorkspaceRecord;
      token: DeployTokenRecord;
    }
  | {
      actorType: 'user';
      user: UserRecord;
      workspace: CurrentWorkspaceRecord;
    };
export type CurrentWorkspaceResponse = {
  user: UserRecord;
  workspace: CurrentWorkspaceRecord | null;
  activity: WorkspaceActivity | null;
  stats: {
    recentInvocationCount: number;
  };
  flows: FlowRecord[];
};
export type ListWorkspacesResponse = {
  workspaces: UserWorkspaceRecord[];
};
export type CreateWorkspaceRequest = {
  name: string;
  slug?: string;
};
export type CreateWorkspaceResponse = {
  workspace: UserWorkspaceRecord;
};
export type SelectCurrentWorkspaceRequest = {
  workspaceSlug: string;
};
export type SelectCurrentWorkspaceResponse = {
  workspace: UserWorkspaceRecord;
};
export type UpdateWorkspaceRequest = {
  name: string;
  slug: string;
};
export type UpdateWorkspaceResponse = {
  workspace: UserWorkspaceRecord;
};
export type DeleteWorkspaceResponse = {
  deleted: true;
  currentWorkspace: UserWorkspaceRecord;
};
export type ListWorkspaceDeployTokensResponse = {
  tokens: WorkspaceDeployTokenRecord[];
};
export type CreateWorkspaceDeployTokenRequest = {
  label: string;
};
export type CreateWorkspaceDeployTokenResponse = {
  ok: true;
  rawToken: string;
  token: WorkspaceDeployTokenRecord;
  workspace: CurrentWorkspaceRecord;
};
type BaseFlowStatusRecord = {
  id: string;
  slug: string;
  status: FlowStatus;
};
export type WebhookFlowStatusRecord = BaseFlowStatusRecord & {
  endpoint: string;
  routePath: string;
  trigger: 'webhook';
};
export type CronFlowStatusRecord = BaseFlowStatusRecord & {
  schedule: string;
  timezone: 'UTC';
  trigger: 'cron';
};
export type QueueFlowStatusRecord = BaseFlowStatusRecord & {
  queue?: string;
  trigger: 'queue';
};
export type FlowStatusRecord =
  | WebhookFlowStatusRecord
  | CronFlowStatusRecord
  | QueueFlowStatusRecord;
export type FlowStatusResponse = {
  ok: true;
  flow: FlowStatusRecord;
};
export type DeleteFlowResponse = {
  deleted: true;
};
export type FlowSecretRecord = {
  createdAt: string;
  name: string;
  updatedAt: string;
};
export type ListSecretsResponse = {
  secrets: Array<
    FlowSecretRecord & {
      flowSlug: string;
    }
  >;
};
export type SetFlowSecretRequest = {
  flow: string;
  name: string;
  value: string;
};
export type SetFlowSecretResponse = {
  ok: true;
  secret: FlowSecretRecord;
};
export type DeleteFlowSecretResponse = {
  ok: true;
  deleted: true;
  name: string;
};
export type FlowInvocationStatus = 'running' | 'succeeded' | 'failed';
export type FlowInvocationLogLevel = 'info' | 'warn' | 'error';
export type FlowInvocationRecord = {
  id: string;
  status: FlowInvocationStatus;
  startedAt: string;
  completedAt: string | null;
  durationMs: number | null;
  httpStatus: number | null;
  errorCode: string | null;
  errorMessage: string | null;
};
export type FlowInvocationLogRecord = {
  sequence: number;
  level: FlowInvocationLogLevel;
  message: string;
  timestamp: string;
  metadata?: Record<string, unknown> | null;
};
export type ListInvocationsResponse = {
  invocations: Array<
    FlowInvocationRecord & {
      flowSlug: string;
    }
  >;
};
export type GetInvocationResponse = {
  invocation: FlowInvocationRecord & {
    flowSlug: string;
    triggerType: string;
    logs: FlowInvocationLogRecord[];
  };
};
export type ListFlowInvocationsQuery = {
  flow?: string;
  limit?: number;
  range?: string;
  status?: FlowInvocationStatus;
};
export {};
