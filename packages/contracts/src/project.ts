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
