export type WorkspacePlan = 'free' | 'pro' | 'scale' | 'internal';

export type WorkspacePlanStatus = 'active' | 'past_due';

export type WorkspaceId = string;

export type Workspace = {
  id: WorkspaceId;
  name: string;
  plan: WorkspacePlan;
  planStatus: WorkspacePlanStatus;
  slug: string;
};

export type User = {
  id: string;
  email: string;
  emailVerified: boolean;
  image: string | null;
  name: string;
};

export type CurrentWorkspace = Workspace & {
  role: string;
};

export type UserWorkspace = CurrentWorkspace & {
  isCurrent: boolean;
};

export type WorkspaceActivity = {
  createdAt: string;
  updatedAt: string;
  programCount: number;
  tokenCount: number;
};

export type ApiToken = {
  id: string;
  label: string;
  status: string;
  createdAt: string;
};

export type WorkspaceApiToken = ApiToken & {
  lastUsedAt: string | null;
};

export type WhoAmIResponse =
  | {
      actorType: 'api_token';
      workspace: Workspace;
      token: ApiToken;
    }
  | {
      actorType: 'user';
      user: User;
      workspace: CurrentWorkspace;
    };

export type CurrentWorkspaceResponse = {
  user: User;
  workspace: CurrentWorkspace | null;
  activity: WorkspaceActivity | null;
  stats: {
    recentExecutionCount: number;
  };
};

export type ListWorkspacesResponse = {
  workspaces: UserWorkspace[];
};

export type CreateWorkspaceRequest = {
  name: string;
  slug?: string;
};

export type CreateWorkspaceResponse = {
  workspace: UserWorkspace;
};

export type SelectCurrentWorkspaceRequest = {
  workspaceSlug: string;
};

export type SelectCurrentWorkspaceResponse = {
  workspace: UserWorkspace;
};

export type UpdateWorkspaceRequest = {
  name: string;
  slug: string;
};

export type UpdateWorkspaceResponse = {
  workspace: UserWorkspace;
};

export type DeleteWorkspaceResponse = {
  deleted: true;
  currentWorkspace: UserWorkspace;
};

export type ListWorkspaceApiTokensResponse = {
  tokens: WorkspaceApiToken[];
};

export type CreateWorkspaceApiTokenRequest = {
  label: string;
};

export type CreateWorkspaceApiTokenResponse = {
  ok: true;
  rawToken: string;
  token: WorkspaceApiToken;
  workspace: CurrentWorkspace;
};

export type UsageWarningThreshold = 80 | 90 | 100;

export type UsageMetric = 'executions' | 'activeApiTokens';

export type UsageWarning = {
  metric: UsageMetric;
  thresholdPercent: UsageWarningThreshold;
  current: number;
  limit: number;
};

export type GetUsageResponse = {
  period: {
    month: string;
    startsAt: string;
    endsAt: string;
  };
  plan: {
    name: WorkspacePlan;
    status: WorkspacePlanStatus;
    executionLimit: number | null;
    enforcement: 'hard' | 'soft' | 'none';
  };
  usage: {
    executions: number;
    succeededExecutions: number;
    failedExecutions: number;
    activeApiTokens: number;
  };
  warnings: UsageWarning[];
};

export type CreateBillingCheckoutRequest = {
  plan: 'pro';
};

export type CreateBillingCheckoutResponse = {
  url: string;
};

export type CreateBillingPortalResponse = {
  url: string;
};
