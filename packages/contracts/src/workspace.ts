export type WorkspacePlan = 'free' | 'pro' | 'internal';

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

export type GetUsageResponse = {
  period: {
    month: string;
    startsAt: string;
    endsAt: string;
  };
  plan: {
    name: WorkspacePlan;
    status: WorkspacePlanStatus;
  };
  meters: {
    durableOperations: number;
    durableOperationsCents: number;
    cpuMs: number;
    cpuCents: number;
    storageByteSeconds: number;
    storageCents: number;
  };
  credit: {
    includedCents: number | null;
    grossCents: number;
    remainingCents: number | null;
    overageCents: number;
    invoiceCents: number | null;
  };
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

export type WorkspaceMemberRole = 'owner' | 'admin' | 'member';

export type WorkspaceMember = {
  createdAt: string;
  email: string;
  name: string;
  role: WorkspaceMemberRole;
  userId: string;
};

export type WorkspaceInvite = {
  createdAt: string;
  email: string;
  expiresAt: string;
  id: string;
  role: WorkspaceMemberRole;
};

export type ListWorkspaceMembersResponse = {
  invites: WorkspaceInvite[];
  members: WorkspaceMember[];
};

export type CreateWorkspaceInviteRequest = {
  email: string;
  role: WorkspaceMemberRole;
};

export type WorkspaceInviteResponse = {
  emailSent: boolean;
  invite: WorkspaceInvite;
  inviteUrl: string;
};

export type UpdateWorkspaceMemberRoleRequest = {
  role: WorkspaceMemberRole;
};

export type AcceptWorkspaceInviteRequest = {
  token: string;
};

export type AcceptWorkspaceInviteResponse = {
  workspaceSlug: string;
};

export type WorkspaceInvitePreviewStatus = 'pending' | 'expired' | 'accepted';

export type PreviewWorkspaceInviteRequest = {
  token: string;
};

export type PreviewWorkspaceInviteResponse = {
  email: string;
  role: WorkspaceMemberRole;
  status: WorkspaceInvitePreviewStatus;
  workspaceName: string;
};

export type InviteReturnPathResponse = {
  path: string | null;
};
