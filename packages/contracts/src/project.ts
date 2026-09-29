export const DEFAULT_PROJECT_SLUG = 'default';
export const DEFAULT_PROJECT_NAME = 'default';

export type ProjectId = string;

export type Project = {
  id: ProjectId;
  workspaceId: string;
  name: string;
  slug: string;
  createdAt: string;
};

export type ProjectSummary = {
  id: ProjectId;
  name: string;
  slug: string;
  createdAt: string;
};

export type ListProjectsResponse = {
  projects: ProjectSummary[];
};

export type CreateProjectRequest = {
  name: string;
  slug?: string;
};

export type CreateProjectResponse = {
  project: Project;
};

export type GetProjectResponse = {
  project: Project;
};

export type DeleteProjectRequest = {
  confirm: string;
};

export type DeleteProjectResponse = {
  deletedProjectId: ProjectId;
  slug: string;
};

export type ProjectSecret = {
  name: string;
  createdAt: string;
  updatedAt: string;
};

export type ListProjectSecretsResponse = {
  secrets: ProjectSecret[];
};

export type PutProjectSecretRequest = {
  value: string;
};

export type PutProjectSecretResponse = ProjectSecret;
